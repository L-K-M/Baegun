use crate::errors::{BaegunError, Result};
use crate::models::{ConvertConfig, MistralOcrResponse};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::PathBuf;

pub fn compute_cache_key(cfg: &ConvertConfig, pdf_bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    let include_images_for_ocr = cfg.include_images || cfg.comic_mode;
    hasher.update(pdf_bytes);
    hasher.update(cfg.model.as_bytes());
    hasher.update(cfg.table_format.as_str().as_bytes());
    hasher.update(if cfg.extract_header { b"1" } else { b"0" });
    hasher.update(if cfg.extract_footer { b"1" } else { b"0" });
    hasher.update(if include_images_for_ocr { b"1" } else { b"0" });
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

pub fn store_cached_ocr(cfg: &ConvertConfig, cache_key: &str, payload: &MistralOcrResponse) -> Result<()> {
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
    let json = serde_json::to_string_pretty(payload)
        .map_err(|error| BaegunError::internal(format!("Failed serializing OCR payload: {error}")))?;

    fs::write(&path, json).map_err(|error| {
        BaegunError::internal(format!(
            "Failed writing OCR cache file '{}': {error}",
            path.display()
        ))
    })
}
