mod cache;
mod epub;
mod errors;
mod metadata;
mod mistral;
mod models;
mod normalize;
mod provider;
mod validate;

pub use errors::{BaegunError, ErrorKind, Result};
pub use models::{
    BookMetadata, ConvertConfig, ConvertProgress, ConvertStage, ConvertSummary, MistralOcrResponse,
    OcrBackend, OcrImage, OcrPage, OcrTable, TableFormat, ValidationResult,
};
pub use provider::OcrProvider;

use sha2::{Digest, Sha256};
use std::fs;

pub fn convert_pdf_to_epub(cfg: &ConvertConfig) -> Result<ConvertSummary> {
    convert_pdf_to_epub_with_progress(cfg, |_| {})
}

pub fn convert_pdf_to_epub_with_progress<F>(
    cfg: &ConvertConfig,
    mut on_progress: F,
) -> Result<ConvertSummary>
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

    if !cfg.input_pdf.exists() {
        return Err(BaegunError::bad_args(format!(
            "Input PDF does not exist: {}",
            cfg.input_pdf.display()
        )));
    }

    if !cfg.input_pdf.is_file() {
        return Err(BaegunError::bad_args(format!(
            "Input path is not a file: {}",
            cfg.input_pdf.display()
        )));
    }

    let pdf_bytes = fs::read(&cfg.input_pdf).map_err(|error| {
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
            "Uploading PDF and running OCR",
            Some(false),
        );

        let fresh = provider::provider_for(cfg.provider).run(cfg, &pdf_bytes, source_filename)?;
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

    epub::write_epub(&rendered, &cfg.output_epub)?;

    let validation = if cfg.validate {
        emit_progress(
            &mut on_progress,
            ConvertStage::Validate,
            5,
            total_steps,
            "Running epubcheck validation",
            Some(cache_hit),
        );

        Some(validate::run_epubcheck(
            &cfg.epubcheck_bin,
            &cfg.output_epub,
            cfg.fail_on_warn,
        )?)
    } else {
        None
    };

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
