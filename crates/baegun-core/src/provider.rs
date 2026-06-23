//! OCR provider abstraction.
//!
//! Each provider turns PDF bytes into the canonical OCR payload
//! ([`MistralOcrResponse`]) that the rest of the pipeline consumes. Selecting a
//! provider is the only branch point; `normalize`/`epub` never see which
//! backend produced the payload. See `docs/ocr-providers.md` for the roadmap.

use crate::errors::Result;
use crate::models::{ConvertConfig, MistralOcrResponse, OcrBackend};

/// A backend that produces the canonical OCR payload for a PDF.
pub trait OcrProvider {
    fn run(
        &self,
        cfg: &ConvertConfig,
        pdf_bytes: &[u8],
        source_filename: &str,
    ) -> Result<MistralOcrResponse>;
}

/// Returns the provider implementation for the configured backend.
pub fn provider_for(backend: OcrBackend) -> Box<dyn OcrProvider> {
    match backend {
        OcrBackend::Mistral => Box::new(crate::mistral::MistralProvider),
    }
}
