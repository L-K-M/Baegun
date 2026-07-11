mod cache;
mod cbz;
mod epub;
mod errors;
mod metadata;
mod mistral;
mod models;
mod normalize;
mod output;
mod validate;

pub use errors::{BaegunError, ErrorKind, Result};
pub use models::{
    BookMetadata, ConvertConfig, ConvertProgress, ConvertStage, ConvertSummary, MistralOcrResponse,
    OcrImage, OcrPage, OcrTable, PageProgressionDirection, SourceFormat, TableFormat,
    ValidationResult,
};

use sha2::{Digest, Sha256};
use std::fs;
use std::io::Read;
use std::path::Path;

pub fn detect_source_format(path: &Path) -> Result<SourceFormat> {
    if !path.exists() {
        return Err(BaegunError::bad_args(format!(
            "Input file does not exist: {}",
            path.display()
        )));
    }
    if !path.is_file() {
        return Err(BaegunError::bad_args(format!(
            "Input path is not a file: {}",
            path.display()
        )));
    }

    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase)
        .ok_or_else(|| BaegunError::bad_args("Input must have a .pdf or .cbz extension."))?;
    let expected = match extension.as_str() {
        "pdf" => SourceFormat::Pdf,
        "cbz" => SourceFormat::Cbz,
        _ => {
            return Err(BaegunError::bad_args(format!(
                "Unsupported input extension '.{extension}'; expected .pdf or .cbz."
            )))
        }
    };

    let mut file = fs::File::open(path).map_err(|error| {
        BaegunError::bad_args(format!(
            "Failed opening input '{}': {error}",
            path.display()
        ))
    })?;
    let mut signature = [0_u8; 8];
    let read = file.read(&mut signature).map_err(|error| {
        BaegunError::bad_args(format!(
            "Failed reading input '{}': {error}",
            path.display()
        ))
    })?;
    let signature = &signature[..read];
    let valid = match expected {
        SourceFormat::Pdf => signature.starts_with(b"%PDF-"),
        SourceFormat::Cbz => {
            signature.starts_with(b"PK\x03\x04")
                || signature.starts_with(b"PK\x05\x06")
                || signature.starts_with(b"PK\x07\x08")
        }
    };
    if !valid {
        return Err(BaegunError::bad_args(format!(
            "Input '{}' does not have a valid {} signature.",
            path.display(),
            extension.to_ascii_uppercase()
        )));
    }
    Ok(expected)
}

pub fn convert_to_epub(cfg: &ConvertConfig) -> Result<ConvertSummary> {
    convert_to_epub_with_progress(cfg, |_| {})
}

pub fn convert_to_epub_with_progress<F>(
    cfg: &ConvertConfig,
    on_progress: F,
) -> Result<ConvertSummary>
where
    F: FnMut(&ConvertProgress),
{
    match detect_source_format(&cfg.input_pdf)? {
        SourceFormat::Pdf => convert_pdf_impl(cfg, on_progress),
        SourceFormat::Cbz => convert_cbz_impl(cfg, on_progress),
    }
}

pub fn convert_pdf_to_epub(cfg: &ConvertConfig) -> Result<ConvertSummary> {
    convert_pdf_to_epub_with_progress(cfg, |_| {})
}

pub fn convert_pdf_to_epub_with_progress<F>(
    cfg: &ConvertConfig,
    on_progress: F,
) -> Result<ConvertSummary>
where
    F: FnMut(&ConvertProgress),
{
    if detect_source_format(&cfg.input_pdf)? != SourceFormat::Pdf {
        return Err(BaegunError::bad_args(
            "convert_pdf_to_epub requires a PDF input.",
        ));
    }
    convert_pdf_impl(cfg, on_progress)
}

fn convert_pdf_impl<F>(cfg: &ConvertConfig, mut on_progress: F) -> Result<ConvertSummary>
where
    F: FnMut(&ConvertProgress),
{
    let total_steps = if cfg.validate { 6 } else { 5 };

    emit_progress(
        &mut on_progress,
        ConvertStage::ReadingInput,
        1,
        total_steps,
        "Reading input PDF",
        None,
    );

    let (mut input_file, source_identity) =
        output::open_source_distinct_from_destination(&cfg.input_pdf, &cfg.output_epub)?;
    let mut pdf_bytes = Vec::new();
    input_file.read_to_end(&mut pdf_bytes).map_err(|error| {
        BaegunError::internal(format!(
            "Failed reading input PDF '{}': {error}",
            cfg.input_pdf.display()
        ))
    })?;

    let mut source_hasher = Sha256::new();
    source_hasher.update(&pdf_bytes);
    let source_hash = format!("{:x}", source_hasher.finalize());

    emit_progress(
        &mut on_progress,
        ConvertStage::Ocr,
        2,
        total_steps,
        "Checking OCR cache",
        None,
    );

    let cache_key = cache::compute_cache_key(cfg, &pdf_bytes);
    let (ocr_payload, cache_hit) = if let Some(cached) = cache::load_cached_ocr(cfg, &cache_key)? {
        emit_progress(
            &mut on_progress,
            ConvertStage::Ocr,
            2,
            total_steps,
            "Using cached OCR payload",
            Some(true),
        );
        (cached, true)
    } else {
        let source_filename = cfg
            .input_pdf
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("document.pdf");

        emit_progress(
            &mut on_progress,
            ConvertStage::Ocr,
            2,
            total_steps,
            "Uploading PDF and running Mistral OCR",
            Some(false),
        );

        let fresh = mistral::run_mistral_ocr(cfg, &pdf_bytes, source_filename)?;
        cache::store_cached_ocr(cfg, &cache_key, &fresh)?;
        (fresh, false)
    };

    if let Some(debug_dir) = &cfg.debug_dir {
        fs::create_dir_all(debug_dir).map_err(|error| {
            BaegunError::internal(format!(
                "Failed creating debug directory '{}': {error}",
                debug_dir.display()
            ))
        })?;

        let ocr_debug_path = debug_dir.join("ocr_payload.json");
        let ocr_debug = serde_json::to_string_pretty(&ocr_payload).map_err(|error| {
            BaegunError::internal(format!("Failed serializing debug OCR payload: {error}"))
        })?;
        fs::write(&ocr_debug_path, ocr_debug).map_err(|error| {
            BaegunError::internal(format!(
                "Failed writing debug OCR payload '{}': {error}",
                ocr_debug_path.display()
            ))
        })?;
    }

    emit_progress(
        &mut on_progress,
        ConvertStage::Normalize,
        3,
        total_steps,
        "Resolving metadata, normalizing OCR output, and chapterizing content",
        Some(cache_hit),
    );

    let book_metadata = metadata::resolve_book_metadata(cfg, &pdf_bytes, &ocr_payload, &cache_key);
    let rendered = normalize::normalize_to_rendered_book(
        &ocr_payload,
        cfg,
        &book_metadata,
        &cfg.input_pdf,
        &source_hash,
    )?;

    if let Some(debug_dir) = &cfg.debug_dir {
        let chapter_dump = rendered
            .chapters
            .iter()
            .map(|chapter| format!("# {}\n\n{}\n", chapter.title, chapter.markdown))
            .collect::<Vec<_>>()
            .join("\n\n");
        let chapter_debug_path = debug_dir.join("normalized_chapters.md");
        fs::write(&chapter_debug_path, chapter_dump).map_err(|error| {
            BaegunError::internal(format!(
                "Failed writing debug chapter dump '{}': {error}",
                chapter_debug_path.display()
            ))
        })?;
    }

    emit_progress(
        &mut on_progress,
        ConvertStage::PackageEpub,
        4,
        total_steps,
        "Packaging EPUB",
        Some(cache_hit),
    );

    output::ensure_destination_is_distinct(&source_identity, &cfg.input_pdf, &cfg.output_epub)?;
    let validation = epub::package_and_publish(
        &rendered,
        &cfg.output_epub,
        |temporary_path| {
            if cfg.validate {
                emit_progress(
                    &mut on_progress,
                    ConvertStage::Validate,
                    5,
                    total_steps,
                    "Running epubcheck validation",
                    Some(cache_hit),
                );

                validate::run_epubcheck(&cfg.epubcheck_bin, temporary_path, cfg.fail_on_warn)
                    .map(Some)
            } else {
                Ok(None)
            }
        },
        || {
            output::ensure_destination_is_distinct(
                &source_identity,
                &cfg.input_pdf,
                &cfg.output_epub,
            )
        },
    )?;

    let summary = ConvertSummary {
        output_path: cfg.output_epub.clone(),
        pages_processed: ocr_payload.pages.len(),
        chapters: rendered.chapters.len(),
        images: rendered.images.len(),
        cache_hit,
        validation,
    };

    emit_progress(
        &mut on_progress,
        ConvertStage::Complete,
        total_steps,
        total_steps,
        "Conversion complete",
        Some(cache_hit),
    );

    Ok(summary)
}

fn convert_cbz_impl<F>(cfg: &ConvertConfig, mut on_progress: F) -> Result<ConvertSummary>
where
    F: FnMut(&ConvertProgress),
{
    let total_steps = if cfg.validate { 5 } else { 4 };
    emit_progress(
        &mut on_progress,
        ConvertStage::ReadingInput,
        1,
        total_steps,
        "Reading CBZ archive",
        Some(false),
    );
    emit_progress(
        &mut on_progress,
        ConvertStage::Normalize,
        2,
        total_steps,
        "Validating and ordering local CBZ pages",
        Some(false),
    );
    let (rendered, pages_processed) = cbz::load_cbz(cfg)?;

    emit_progress(
        &mut on_progress,
        ConvertStage::PackageEpub,
        3,
        total_steps,
        "Packaging fixed-layout EPUB",
        Some(false),
    );
    let validation = epub::package_and_publish(
        &rendered,
        &cfg.output_epub,
        |temporary_path| {
            if cfg.validate {
                emit_progress(
                    &mut on_progress,
                    ConvertStage::Validate,
                    4,
                    total_steps,
                    "Running epubcheck validation",
                    Some(false),
                );
                validate::run_epubcheck(&cfg.epubcheck_bin, temporary_path, cfg.fail_on_warn)
                    .map(Some)
            } else {
                Ok(None)
            }
        },
        || Ok(()),
    )?;
    let summary = ConvertSummary {
        output_path: cfg.output_epub.clone(),
        pages_processed,
        chapters: rendered.chapters.len(),
        images: rendered.images.len(),
        cache_hit: false,
        validation,
    };
    emit_progress(
        &mut on_progress,
        ConvertStage::Complete,
        total_steps,
        total_steps,
        "Conversion complete",
        Some(false),
    );
    Ok(summary)
}

fn emit_progress<F>(
    on_progress: &mut F,
    stage: ConvertStage,
    step: usize,
    total_steps: usize,
    message: &str,
    cache_hit: Option<bool>,
) where
    F: FnMut(&ConvertProgress),
{
    on_progress(&ConvertProgress {
        stage,
        step,
        total_steps,
        message: message.to_string(),
        cache_hit,
    });
}
