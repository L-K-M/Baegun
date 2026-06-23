use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TableFormat {
    Html,
    Markdown,
}

impl Default for TableFormat {
    fn default() -> Self {
        Self::Html
    }
}

impl TableFormat {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Html => "html",
            Self::Markdown => "markdown",
        }
    }
}

impl FromStr for TableFormat {
    type Err = String;

    fn from_str(value: &str) -> std::result::Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "html" => Ok(Self::Html),
            "markdown" => Ok(Self::Markdown),
            other => Err(format!(
                "Unsupported table format '{other}'. Expected one of: html, markdown"
            )),
        }
    }
}

/// Selects which OCR backend produces the document payload.
///
/// Today only the hosted Mistral OCR API is implemented; the enum exists so
/// future providers (see `docs/ocr-providers.md`) can be slotted in behind the
/// `OcrProvider` trait without touching the conversion pipeline.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum OcrBackend {
    /// Mistral hosted OCR API (`POST /v1/ocr`).
    #[default]
    Mistral,
}

impl OcrBackend {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Mistral => "mistral",
        }
    }
}

impl FromStr for OcrBackend {
    type Err = String;

    fn from_str(value: &str) -> std::result::Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "mistral" => Ok(Self::Mistral),
            other => Err(format!(
                "Unsupported OCR provider '{other}'. Expected one of: mistral"
            )),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConvertConfig {
    pub input_pdf: PathBuf,
    pub output_epub: PathBuf,
    pub api_key: Option<String>,
    pub provider: OcrBackend,
    pub model: String,
    pub title: Option<String>,
    pub author: Option<String>,
    pub language: String,
    pub publisher: Option<String>,
    pub table_format: TableFormat,
    pub extract_header: bool,
    pub extract_footer: bool,
    pub include_images: bool,
    pub comic_mode: bool,
    pub cache_dir: PathBuf,
    pub no_cache: bool,
    pub validate: bool,
    pub epubcheck_bin: String,
    pub keep_remote_file: bool,
    pub fail_on_warn: bool,
    pub debug_dir: Option<PathBuf>,
    pub quiet: bool,
    pub verbose: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MistralOcrResponse {
    #[serde(default)]
    pub pages: Vec<OcrPage>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub usage_info: Option<Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BookMetadata {
    pub title: Option<String>,
    pub author: Option<String>,
    pub language: Option<String>,
    pub publisher: Option<String>,
    pub description: Option<String>,
    #[serde(default)]
    pub subjects: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OcrPage {
    pub index: usize,
    #[serde(default)]
    pub markdown: String,
    #[serde(default)]
    pub images: Vec<OcrImage>,
    #[serde(default)]
    pub tables: Vec<OcrTable>,
    #[serde(default)]
    pub hyperlinks: Vec<Value>,
    #[serde(default)]
    pub header: Option<String>,
    #[serde(default)]
    pub footer: Option<String>,
    #[serde(default)]
    pub dimensions: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OcrImage {
    pub id: String,
    #[serde(default)]
    pub image_base64: Option<String>,
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OcrTable {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub html: Option<String>,
    #[serde(default)]
    pub markdown: Option<String>,
    #[serde(default)]
    pub content: Option<String>,
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

#[derive(Debug, Clone)]
pub struct RenderedBook {
    pub title: String,
    pub author: Option<String>,
    pub language: String,
    pub publisher: Option<String>,
    pub description: Option<String>,
    pub subjects: Vec<String>,
    pub source_hash: String,
    pub chapters: Vec<RenderedChapter>,
    pub images: Vec<ImageAsset>,
    pub cover_image: Option<String>,
}

#[derive(Debug, Clone)]
pub struct RenderedChapter {
    pub id: String,
    pub title: String,
    pub file_name: String,
    pub markdown: String,
    pub xhtml: String,
}

#[derive(Debug, Clone)]
pub struct ImageAsset {
    pub file_name: String,
    pub media_type: String,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationResult {
    pub warnings: usize,
    pub errors: usize,
    pub passed: bool,
    pub raw_output: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConvertSummary {
    pub output_path: PathBuf,
    pub pages_processed: usize,
    pub chapters: usize,
    pub images: usize,
    pub cache_hit: bool,
    pub validation: Option<ValidationResult>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ConvertStage {
    ReadingInput,
    Ocr,
    Normalize,
    PackageEpub,
    Validate,
    Complete,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConvertProgress {
    pub stage: ConvertStage,
    pub step: usize,
    pub total_steps: usize,
    pub message: String,
    pub cache_hit: Option<bool>,
}
