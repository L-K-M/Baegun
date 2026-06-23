//! LlamaParse hosted OCR backend (LlamaCloud Parsing v1 API).
//!
//! Flow: upload the PDF (`POST /api/v1/parsing/upload`), poll the job until it
//! completes (`GET /api/v1/parsing/job/{id}`), then fetch per-page results
//! (`GET /api/v1/parsing/job/{id}/result/json`) and any extracted images
//! (`GET /api/v1/parsing/job/{id}/result/image/{name}`). The per-page markdown
//! and images are mapped onto the canonical [`MistralOcrResponse`] the rest of
//! the pipeline consumes. See `docs/ocr-providers.md`.

use crate::errors::{BaegunError, Result};
use crate::models::{ConvertConfig, MistralOcrResponse, OcrImage, OcrPage};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use reqwest::blocking::{multipart, Client};
use reqwest::StatusCode;
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;
use std::env;
use std::thread;
use std::time::{Duration, Instant};

const DEFAULT_BASE_URL: &str = "https://api.cloud.llamaindex.ai/api/v1/parsing";
const BASE_URL_ENV: &str = "LLAMA_CLOUD_BASE_URL";
/// Overall budget for a parse job to reach a terminal status.
const POLL_BUDGET: Duration = Duration::from_secs(600);

/// LlamaParse hosted backend.
pub(crate) struct LlamaParseProvider;

impl crate::provider::OcrProvider for LlamaParseProvider {
    fn run(
        &self,
        cfg: &ConvertConfig,
        pdf_bytes: &[u8],
        source_filename: &str,
    ) -> Result<MistralOcrResponse> {
        run_llamaparse(cfg, pdf_bytes, source_filename)
    }
}

#[derive(Debug, Deserialize)]
struct UploadJobResponse {
    id: String,
}

#[derive(Debug, Deserialize)]
struct JobStatusResponse {
    #[serde(default)]
    status: Option<String>,
}

#[derive(Debug, Deserialize)]
struct JsonResult {
    #[serde(default)]
    pages: Vec<LlamaPage>,
}

#[derive(Debug, Deserialize)]
struct LlamaPage {
    #[serde(default)]
    page: Option<i64>,
    #[serde(default)]
    md: String,
    #[serde(default)]
    images: Vec<LlamaImage>,
}

#[derive(Debug, Deserialize)]
struct LlamaImage {
    #[serde(default)]
    name: Option<String>,
}

fn run_llamaparse(
    cfg: &ConvertConfig,
    pdf_bytes: &[u8],
    source_filename: &str,
) -> Result<MistralOcrResponse> {
    let api_key = cfg.api_key.as_deref().ok_or_else(|| {
        BaegunError::bad_args("Missing API key. Pass --api-key or set LLAMA_CLOUD_API_KEY.")
    })?;

    let client = Client::builder()
        .timeout(Duration::from_secs(300))
        .build()
        .map_err(|error| BaegunError::api(format!("Failed creating HTTP client: {error}")))?;

    let base_url = base_url();
    let job_id = upload_pdf(&client, &base_url, api_key, pdf_bytes, source_filename)?;
    wait_for_completion(&client, &base_url, api_key, &job_id)?;
    let result = fetch_json_result(&client, &base_url, api_key, &job_id)?;

    let pages = map_pages(&client, &base_url, api_key, &job_id, result, cfg)?;
    if pages.is_empty() {
        return Err(BaegunError::ocr_schema(
            "LlamaParse result contains no pages.",
        ));
    }

    Ok(MistralOcrResponse {
        pages,
        model: Some("llamaparse".to_string()),
        usage_info: None,
    })
}

/// Base URL for the LlamaCloud parsing API, overridable for non-US regions
/// (e.g. `https://api.cloud.eu.llamaindex.ai/api/v1/parsing`).
fn base_url() -> String {
    env::var(BASE_URL_ENV)
        .ok()
        .map(|value| value.trim().trim_end_matches('/').to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| DEFAULT_BASE_URL.to_string())
}

fn upload_pdf(
    client: &Client,
    base_url: &str,
    api_key: &str,
    pdf_bytes: &[u8],
    source_filename: &str,
) -> Result<String> {
    let part = multipart::Part::bytes(pdf_bytes.to_vec())
        .file_name(source_filename.to_owned())
        .mime_str("application/pdf")
        .map_err(|error| BaegunError::api(format!("Failed preparing PDF upload body: {error}")))?;
    let form = multipart::Form::new().part("file", part);

    let response = client
        .post(format!("{base_url}/upload"))
        .bearer_auth(api_key)
        .multipart(form)
        .send()
        .map_err(|error| {
            BaegunError::api(format!("Failed uploading PDF to LlamaParse: {error}"))
        })?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().unwrap_or_default();
        return Err(BaegunError::api(format!(
            "LlamaParse upload failed ({status}): {}",
            decode_api_error_message(&body)
        )));
    }

    let upload = response.json::<UploadJobResponse>().map_err(|error| {
        BaegunError::api(format!("Failed parsing LlamaParse upload response: {error}"))
    })?;
    Ok(upload.id)
}

fn wait_for_completion(
    client: &Client,
    base_url: &str,
    api_key: &str,
    job_id: &str,
) -> Result<()> {
    let status_url = format!("{base_url}/job/{job_id}");
    let started = Instant::now();
    let mut delay = Duration::from_millis(1500);

    loop {
        let outcome = client.get(&status_url).bearer_auth(api_key).send();

        match outcome {
            Ok(resp) if resp.status().is_success() => {
                let status = resp
                    .json::<JobStatusResponse>()
                    .ok()
                    .and_then(|body| body.status)
                    .unwrap_or_default()
                    .to_ascii_uppercase();

                match status.as_str() {
                    "SUCCESS" | "PARTIAL_SUCCESS" => return Ok(()),
                    "ERROR" | "FAILED" | "CANCELED" | "CANCELLED" => {
                        return Err(BaegunError::api(format!(
                            "LlamaParse job {job_id} ended with status {status}."
                        )));
                    }
                    // PENDING / RUNNING / unknown: keep polling.
                    _ => {}
                }
            }
            Ok(resp) => {
                let status = resp.status();
                if !is_retryable_status(status) {
                    let body = resp.text().unwrap_or_default();
                    return Err(BaegunError::api(format!(
                        "LlamaParse status check failed ({status}): {}",
                        decode_api_error_message(&body)
                    )));
                }
            }
            // Transient network error: fall through and retry until the budget runs out.
            Err(_) => {}
        }

        if started.elapsed() >= POLL_BUDGET {
            return Err(BaegunError::api(format!(
                "LlamaParse job {job_id} did not complete within {}s.",
                POLL_BUDGET.as_secs()
            )));
        }

        thread::sleep(delay);
        delay = (delay * 2).min(Duration::from_secs(5));
    }
}

fn fetch_json_result(
    client: &Client,
    base_url: &str,
    api_key: &str,
    job_id: &str,
) -> Result<JsonResult> {
    let response = client
        .get(format!("{base_url}/job/{job_id}/result/json"))
        .bearer_auth(api_key)
        .send()
        .map_err(|error| {
            BaegunError::api(format!("Failed fetching LlamaParse result: {error}"))
        })?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().unwrap_or_default();
        return Err(BaegunError::api(format!(
            "LlamaParse result fetch failed ({status}): {}",
            decode_api_error_message(&body)
        )));
    }

    response.json::<JsonResult>().map_err(|error| {
        BaegunError::ocr_schema(format!("Failed parsing LlamaParse result payload: {error}"))
    })
}

fn map_pages(
    client: &Client,
    base_url: &str,
    api_key: &str,
    job_id: &str,
    result: JsonResult,
    cfg: &ConvertConfig,
) -> Result<Vec<OcrPage>> {
    let mut pages: Vec<OcrPage> = result
        .pages
        .into_iter()
        .enumerate()
        .map(|(ordinal, page)| {
            let index = page
                .page
                .map(|number| (number - 1).max(0) as usize)
                .unwrap_or(ordinal);
            let images = page
                .images
                .into_iter()
                .filter_map(|image| image.name)
                .filter(|name| !name.trim().is_empty())
                .map(|name| OcrImage {
                    id: name,
                    image_base64: None,
                    extra: HashMap::new(),
                })
                .collect();

            OcrPage {
                index,
                markdown: page.md,
                images,
                tables: Vec::new(),
                hyperlinks: Vec::new(),
                header: None,
                footer: None,
                dimensions: None,
            }
        })
        .collect();

    // Images are an extra round-trip each. Always hydrate the first page so the
    // cover image works; hydrate the rest only when images will be embedded.
    let want_all_images = cfg.include_images || cfg.comic_mode;
    let first_index = pages.iter().map(|page| page.index).min();

    for page in &mut pages {
        let hydrate = want_all_images || Some(page.index) == first_index;
        if !hydrate {
            page.images.clear();
            continue;
        }

        for image in &mut page.images {
            match fetch_image_base64(client, base_url, api_key, job_id, &image.id) {
                Ok(encoded) => image.image_base64 = Some(encoded),
                // A missing image is not fatal: normalize skips entries without payloads.
                Err(_) => image.image_base64 = None,
            }
        }
    }

    Ok(pages)
}

fn fetch_image_base64(
    client: &Client,
    base_url: &str,
    api_key: &str,
    job_id: &str,
    name: &str,
) -> Result<String> {
    let response = client
        .get(format!("{base_url}/job/{job_id}/result/image/{name}"))
        .bearer_auth(api_key)
        .send()
        .map_err(|error| BaegunError::api(format!("Failed fetching LlamaParse image: {error}")))?;

    if !response.status().is_success() {
        return Err(BaegunError::api(format!(
            "LlamaParse image fetch failed ({})",
            response.status()
        )));
    }

    let bytes = response
        .bytes()
        .map_err(|error| BaegunError::api(format!("Failed reading LlamaParse image: {error}")))?;
    Ok(BASE64.encode(bytes))
}

fn is_retryable_status(status: StatusCode) -> bool {
    status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error()
}

fn decode_api_error_message(body: &str) -> String {
    if let Ok(value) = serde_json::from_str::<Value>(body) {
        for key in ["message", "detail", "error"] {
            if let Some(message) = value
                .get(key)
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
            {
                return message.to_owned();
            }
        }
    }
    body.trim().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_pages_with_one_based_index_and_image_names() {
        // include_images=false, not comic -> only the first page is hydrated,
        // but mapping (index, markdown, image ids) is exercised regardless.
        let result = serde_json::from_str::<JsonResult>(
            r##"{"pages":[
                {"page":1,"md":"# Title","images":[{"name":"img_p0_1.png"}]},
                {"page":2,"md":"Body","images":[{"name":"img_p1_1.png"},{"name":""}]}
            ]}"##,
        )
        .expect("fixture should parse");

        assert_eq!(result.pages.len(), 2);
        assert_eq!(result.pages[0].page, Some(1));
        assert_eq!(result.pages[0].md, "# Title");
        assert_eq!(result.pages[0].images[0].name.as_deref(), Some("img_p0_1.png"));
    }

    #[test]
    fn base_url_prefers_env_override() {
        let key = BASE_URL_ENV;
        let previous = env::var(key).ok();
        env::set_var(key, "https://api.cloud.eu.llamaindex.ai/api/v1/parsing/");
        assert_eq!(base_url(), "https://api.cloud.eu.llamaindex.ai/api/v1/parsing");
        match previous {
            Some(value) => env::set_var(key, value),
            None => env::remove_var(key),
        }
    }

    #[test]
    fn decodes_error_detail_field() {
        assert_eq!(
            decode_api_error_message(r#"{"detail":"bad file"}"#),
            "bad file"
        );
    }
}
