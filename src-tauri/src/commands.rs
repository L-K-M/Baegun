use baegun_core::{
    convert_pdf_to_epub_with_progress, ConvertConfig, ConvertProgress, ConvertStage, TableFormat,
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::env;
use std::path::{Path, PathBuf};
use tauri::{Emitter, Manager};

const CONVERT_PROGRESS_EVENT: &str = "baegun://convert-progress";

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
    pub comic_mode: Option<bool>,
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

#[derive(Debug, Clone, Serialize)]
pub struct ConvertProgressEvent {
    pub input_path: String,
    pub output_path: String,
    pub stage: ConvertStage,
    pub step: usize,
    pub total_steps: usize,
    pub message: String,
    pub cache_hit: Option<bool>,
}

#[tauri::command]
pub async fn convert_pdf(
    app: tauri::AppHandle,
    request: ConvertRequest,
) -> Result<ConvertResponse, String> {
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

    let comic_mode = request.comic_mode.unwrap_or(false);

    let cache_dir = match request.cache_dir.as_deref().map(str::trim) {
        Some(cache_dir) if !cache_dir.is_empty() => PathBuf::from(cache_dir),
        _ => app
            .path()
            .app_cache_dir()
            .map_err(|error| format!("Failed resolving app cache directory: {error}"))?,
    };

    let epubcheck_bin = resolve_epubcheck_bin(&app, request.epubcheck_bin.as_deref());

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
        include_images: request.include_images.unwrap_or(true) || comic_mode,
        comic_mode,
        cache_dir,
        no_cache: request.no_cache.unwrap_or(false),
        validate: request.validate.unwrap_or(false),
        epubcheck_bin,
        keep_remote_file: request.keep_remote_file.unwrap_or(false),
        fail_on_warn: request.fail_on_warn.unwrap_or(false),
        debug_dir: request.debug_dir.map(PathBuf::from),
        quiet: request.quiet.unwrap_or(true),
        verbose: request.verbose.unwrap_or(false),
    };

    let progress_input_path = cfg.input_pdf.to_string_lossy().to_string();
    let progress_output_path = cfg.output_epub.to_string_lossy().to_string();
    let app_handle = app.clone();

    let summary = tauri::async_runtime::spawn_blocking(move || {
        convert_pdf_to_epub_with_progress(&cfg, |progress: &ConvertProgress| {
            let event = ConvertProgressEvent {
                input_path: progress_input_path.clone(),
                output_path: progress_output_path.clone(),
                stage: progress.stage,
                step: progress.step,
                total_steps: progress.total_steps,
                message: progress.message.clone(),
                cache_hit: progress.cache_hit,
            };

            let _ = app_handle.emit(CONVERT_PROGRESS_EVENT, event);
        })
    })
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

#[tauri::command]
pub fn get_system_colors() -> Result<crate::system_colors::SystemColors, String> {
    Ok(crate::system_colors::get_system_colors())
}

fn resolve_epubcheck_bin(app: &tauri::AppHandle, requested: Option<&str>) -> String {
    let command = requested
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| {
            env::var("EPUBCHECK_BIN")
                .ok()
                .filter(|value| !value.trim().is_empty())
        })
        .unwrap_or_else(|| String::from("epubcheck"));

    if path_like_command(&command) {
        return command;
    }

    find_command(app, &command)
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or(command)
}

fn find_command(app: &tauri::AppHandle, command: &str) -> Option<PathBuf> {
    let mut dirs = Vec::new();
    let mut seen = HashSet::new();

    if let Some(path_var) = env::var_os("PATH") {
        for dir in env::split_paths(&path_var) {
            push_unique_dir(&mut dirs, &mut seen, dir);
        }
    }

    if let Ok(resource_dir) = app.path().resource_dir() {
        push_unique_dir(&mut dirs, &mut seen, resource_dir.join("bin"));
        push_unique_dir(&mut dirs, &mut seen, resource_dir);
    }

    for dir in common_executable_dirs() {
        push_unique_dir(&mut dirs, &mut seen, dir);
    }

    let names = command_names(command);
    for dir in dirs {
        for name in &names {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }

    None
}

fn path_like_command(command: &str) -> bool {
    let path = Path::new(command);
    path.is_absolute() || path.components().count() > 1
}

fn push_unique_dir(dirs: &mut Vec<PathBuf>, seen: &mut HashSet<PathBuf>, dir: PathBuf) {
    if seen.insert(dir.clone()) {
        dirs.push(dir);
    }
}

fn common_executable_dirs() -> Vec<PathBuf> {
    [
        "/opt/homebrew/bin",
        "/usr/local/bin",
        "/opt/local/bin",
        "/usr/bin",
        "/bin",
    ]
    .into_iter()
    .map(PathBuf::from)
    .collect()
}

fn command_names(command: &str) -> Vec<String> {
    let path = Path::new(command);
    if path.extension().is_some() {
        return vec![command.to_owned()];
    }

    #[cfg(not(windows))]
    {
        vec![command.to_owned()]
    }

    #[cfg(windows)]
    {
        let mut names = vec![command.to_owned()];
        let extensions =
            env::var("PATHEXT").unwrap_or_else(|_| String::from(".COM;.EXE;.BAT;.CMD"));
        for extension in extensions.split(';') {
            let extension = extension.trim();
            if !extension.is_empty() {
                names.push(format!("{command}{extension}"));
            }
        }
        names
    }
}
