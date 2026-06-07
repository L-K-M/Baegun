use crate::errors::{BaegunError, Result};
use crate::models::{BookMetadata, ConvertConfig, MistralOcrResponse};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::PathBuf;

pub fn compute_cache_key(cfg: &ConvertConfig, pdf_bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(pdf_bytes);
    hasher.update(cfg.model.as_bytes());
    hasher.update(cfg.table_format.as_str().as_bytes());
    hasher.update(if cfg.extract_header { b"1" } else { b"0" });
    hasher.update(if cfg.extract_footer { b"1" } else { b"0" });
    // Image payloads are always requested so the first page image can become the EPUB cover.
    hasher.update(b"1");
    hasher.update(if cfg.comic_mode { b"1" } else { b"0" });
    hasher.update(env!("CARGO_PKG_VERSION").as_bytes());
    format!("{:x}", hasher.finalize())
}

pub fn cache_file_path(cfg: &ConvertConfig, cache_key: &str) -> PathBuf {
    cfg.cache_dir.join(format!("{cache_key}.ocr.json"))
}

pub fn load_cached_ocr(cfg: &ConvertConfig, cache_key: &str) -> Result<Option<MistralOcrResponse>> {
    if cfg.no_cache {
        return Ok(None);
    }

    let path = cache_file_path(cfg, cache_key);
    if !path.exists() {
        return Ok(None);
    }

    let raw = fs::read_to_string(&path).map_err(|error| {
        BaegunError::internal(format!(
            "Failed reading OCR cache file '{}': {error}",
            path.display()
        ))
    })?;

    match serde_json::from_str::<MistralOcrResponse>(&raw) {
        Ok(payload) => Ok(Some(payload)),
        Err(_) => Ok(None),
    }
}

pub fn store_cached_ocr(
    cfg: &ConvertConfig,
    cache_key: &str,
    payload: &MistralOcrResponse,
) -> Result<()> {
    if cfg.no_cache {
        return Ok(());
    }

    fs::create_dir_all(&cfg.cache_dir).map_err(|error| {
        BaegunError::internal(format!(
            "Failed creating cache directory '{}': {error}",
            cfg.cache_dir.display()
        ))
    })?;

    let path = cache_file_path(cfg, cache_key);
    let json = serde_json::to_string_pretty(payload).map_err(|error| {
        BaegunError::internal(format!("Failed serializing OCR payload: {error}"))
    })?;

    fs::write(&path, json).map_err(|error| {
        BaegunError::internal(format!(
            "Failed writing OCR cache file '{}': {error}",
            path.display()
        ))
    })
}

pub fn metadata_cache_file_path(cfg: &ConvertConfig, cache_key: &str) -> PathBuf {
    cfg.cache_dir.join(format!("{cache_key}.metadata.json"))
}

/// Loads previously generated (LLM) metadata for this OCR payload, if cached.
///
/// Shares the OCR cache key so it is invalidated by the same inputs. Corrupt cache
/// files are treated as a miss rather than a hard error.
pub fn load_cached_metadata(cfg: &ConvertConfig, cache_key: &str) -> Option<BookMetadata> {
    if cfg.no_cache {
        return None;
    }

    let path = metadata_cache_file_path(cfg, cache_key);
    let raw = fs::read_to_string(path).ok()?;
    serde_json::from_str::<BookMetadata>(&raw).ok()
}

pub fn store_cached_metadata(
    cfg: &ConvertConfig,
    cache_key: &str,
    metadata: &BookMetadata,
) -> Result<()> {
    if cfg.no_cache {
        return Ok(());
    }

    fs::create_dir_all(&cfg.cache_dir).map_err(|error| {
        BaegunError::internal(format!(
            "Failed creating cache directory '{}': {error}",
            cfg.cache_dir.display()
        ))
    })?;

    let path = metadata_cache_file_path(cfg, cache_key);
    let json = serde_json::to_string_pretty(metadata).map_err(|error| {
        BaegunError::internal(format!("Failed serializing generated metadata: {error}"))
    })?;

    fs::write(&path, json).map_err(|error| {
        BaegunError::internal(format!(
            "Failed writing metadata cache file '{}': {error}",
            path.display()
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::{load_cached_metadata, store_cached_metadata};
    use crate::models::{BookMetadata, ConvertConfig, TableFormat};
    use std::path::PathBuf;

    fn config_with_cache(cache_dir: PathBuf, no_cache: bool) -> ConvertConfig {
        ConvertConfig {
            input_pdf: PathBuf::from("in.pdf"),
            output_epub: PathBuf::from("out.epub"),
            api_key: None,
            model: "mistral-ocr-latest".to_string(),
            title: None,
            author: None,
            language: "en".to_string(),
            publisher: None,
            table_format: TableFormat::Html,
            extract_header: true,
            extract_footer: true,
            include_images: true,
            comic_mode: false,
            cache_dir,
            no_cache,
            validate: false,
            epubcheck_bin: "epubcheck".to_string(),
            keep_remote_file: false,
            fail_on_warn: false,
            debug_dir: None,
            quiet: true,
            verbose: false,
        }
    }

    #[test]
    fn metadata_round_trips_through_cache() {
        let dir = std::env::temp_dir().join(format!(
            "baegun-metadata-cache-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let cfg = config_with_cache(dir.clone(), false);

        assert!(load_cached_metadata(&cfg, "key").is_none());

        let metadata = BookMetadata {
            title: Some("Cached Title".to_string()),
            subjects: vec!["history".to_string()],
            ..BookMetadata::default()
        };
        store_cached_metadata(&cfg, "key", &metadata).expect("store should succeed");

        let loaded = load_cached_metadata(&cfg, "key").expect("metadata should be cached");
        assert_eq!(loaded.title.as_deref(), Some("Cached Title"));
        assert_eq!(loaded.subjects, vec!["history".to_string()]);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn no_cache_disables_metadata_cache() {
        let dir = std::env::temp_dir().join(format!(
            "baegun-metadata-nocache-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let cfg = config_with_cache(dir.clone(), true);

        store_cached_metadata(&cfg, "key", &BookMetadata::default()).expect("store should no-op");
        assert!(load_cached_metadata(&cfg, "key").is_none());
        assert!(!dir.exists());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
