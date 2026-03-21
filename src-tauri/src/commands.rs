use baegun_core::{convert_pdf_to_epub, ConvertConfig, TableFormat};
use serde::{Deserialize, Serialize};
use std::env;
use std::path::PathBuf;

#[derive(Debug, Deserialize)]
pub struct ConvertRequest {
    pub input_path: String,
    pub output_path: Option<String>,
    pub api_key: Option<String>,
    pub model: Option<String>,
    pub title: Option<String>,
    pub author: Option<String>,
    pub language: Option<String>,
    pub publisher: Option<String>,
    pub table_format: Option<String>,
    pub extract_header: Option<bool>,
    pub extract_footer: Option<bool>,
    pub include_images: Option<bool>,
    pub cache_dir: Option<String>,
    pub no_cache: Option<bool>,
    pub validate: Option<bool>,
    pub epubcheck_bin: Option<String>,
    pub keep_remote_file: Option<bool>,
    pub fail_on_warn: Option<bool>,
    pub debug_dir: Option<String>,
    pub quiet: Option<bool>,
    pub verbose: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct ConvertResponse {
    pub output_path: String,
    pub pages_processed: usize,
    pub chapters: usize,
    pub images: usize,
    pub cache_hit: bool,
    pub validation_warnings: usize,
    pub validation_errors: usize,
}

#[tauri::command]
pub async fn convert_pdf(request: ConvertRequest) -> Result<ConvertResponse, String> {
    let input_path = PathBuf::from(request.input_path.clone());
    let output_path = request
        .output_path
        .map(PathBuf::from)
        .unwrap_or_else(|| input_path.with_extension("epub"));

    let api_key = request
        .api_key
        .or_else(|| env::var("MISTRAL_API_KEY").ok())
        .filter(|value| !value.trim().is_empty());

    let table_format = request
        .table_format
        .as_deref()
        .unwrap_or("html")
        .parse::<TableFormat>()
        .map_err(|error| format!("Invalid table format: {error}"))?;

    let cfg = ConvertConfig {
        input_pdf: input_path,
        output_epub: output_path,
        api_key,
        model: request
            .model
            .unwrap_or_else(|| String::from("mistral-ocr-latest")),
        title: request.title,
        author: request.author,
        language: request.language.unwrap_or_else(|| String::from("en")),
        publisher: request.publisher,
        table_format,
        extract_header: request.extract_header.unwrap_or(true),
        extract_footer: request.extract_footer.unwrap_or(true),
        include_images: request.include_images.unwrap_or(true),
        cache_dir: request
            .cache_dir
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(".baegun-cache")),
        no_cache: request.no_cache.unwrap_or(false),
        validate: request.validate.unwrap_or(false),
        epubcheck_bin: request
            .epubcheck_bin
            .unwrap_or_else(|| String::from("epubcheck")),
        keep_remote_file: request.keep_remote_file.unwrap_or(false),
        fail_on_warn: request.fail_on_warn.unwrap_or(false),
        debug_dir: request.debug_dir.map(PathBuf::from),
        quiet: request.quiet.unwrap_or(true),
        verbose: request.verbose.unwrap_or(false),
    };

    let summary = tauri::async_runtime::spawn_blocking(move || convert_pdf_to_epub(&cfg))
        .await
        .map_err(|error| format!("Conversion task failed to join: {error}"))?
        .map_err(|error| error.message)?;

    let (validation_warnings, validation_errors) = summary
        .validation
        .as_ref()
        .map(|validation| (validation.warnings, validation.errors))
        .unwrap_or((0, 0));

    Ok(ConvertResponse {
        output_path: summary.output_path.to_string_lossy().to_string(),
        pages_processed: summary.pages_processed,
        chapters: summary.chapters,
        images: summary.images,
        cache_hit: summary.cache_hit,
        validation_warnings,
        validation_errors,
    })
}

#[tauri::command]
pub async fn is_directory(path: String) -> Result<bool, String> {
    Ok(PathBuf::from(path).is_dir())
}
