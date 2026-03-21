use crate::errors::{BaegunError, Result};
use crate::models::{ConvertConfig, MistralOcrResponse};
use reqwest::blocking::{multipart, Client};
use reqwest::StatusCode;
use serde::Deserialize;
use serde_json::{json, Value};
use std::thread;
use std::time::Duration;

const FILES_API_URL: &str = "https://api.mistral.ai/v1/files";
const OCR_API_URL: &str = "https://api.mistral.ai/v1/ocr";

#[derive(Debug, Deserialize)]
struct UploadFileResponse {
    id: String,
}

pub fn run_mistral_ocr(
    cfg: &ConvertConfig,
    pdf_bytes: &[u8],
    source_filename: &str,
) -> Result<MistralOcrResponse> {
    let api_key = cfg
        .api_key
        .as_deref()
        .ok_or_else(|| BaegunError::bad_args("Missing API key. Pass --api-key or set MISTRAL_API_KEY."))?;

    let client = Client::builder()
        .timeout(Duration::from_secs(300))
        .build()
        .map_err(|error| BaegunError::api(format!("Failed creating HTTP client: {error}")))?;

    let file_id = upload_pdf(&client, api_key, pdf_bytes, source_filename)?;
    let ocr_result = perform_ocr_with_retries(&client, api_key, cfg, &file_id);

    if !cfg.keep_remote_file {
        let _ = delete_remote_file(&client, api_key, &file_id);
    }

    ocr_result
}

fn upload_pdf(client: &Client, api_key: &str, pdf_bytes: &[u8], source_filename: &str) -> Result<String> {
    let part = multipart::Part::bytes(pdf_bytes.to_vec())
        .file_name(source_filename.to_owned())
        .mime_str("application/pdf")
        .map_err(|error| BaegunError::api(format!("Failed preparing PDF upload body: {error}")))?;

    let form = multipart::Form::new()
        .text("purpose", "ocr")
        .part("file", part);

    let response = client
        .post(FILES_API_URL)
        .bearer_auth(api_key)
        .multipart(form)
        .send()
        .map_err(|error| BaegunError::api(format!("Failed uploading PDF to Mistral: {error}")))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().unwrap_or_default();
        return Err(BaegunError::api(format!(
            "Mistral file upload failed ({status}): {}",
            decode_api_error_message(&body)
        )));
    }

    let upload = response
        .json::<UploadFileResponse>()
        .map_err(|error| BaegunError::api(format!("Failed parsing Mistral file upload response: {error}")))?;

    Ok(upload.id)
}

fn perform_ocr_with_retries(
    client: &Client,
    api_key: &str,
    cfg: &ConvertConfig,
    file_id: &str,
) -> Result<MistralOcrResponse> {
    let request_payload = json!({
        "model": cfg.model,
        "document": {
            "type": "file",
            "file_id": file_id,
        },
        "table_format": cfg.table_format.as_str(),
        "extract_header": cfg.extract_header,
        "extract_footer": cfg.extract_footer,
        "include_image_base64": cfg.include_images,
    });

    let mut attempt = 0_u32;
    let max_attempts = 4_u32;
    let mut sleep_duration = Duration::from_millis(800);

    loop {
        attempt += 1;

        let response = client
            .post(OCR_API_URL)
            .bearer_auth(api_key)
            .json(&request_payload)
            .send();

        match response {
            Ok(resp) if resp.status().is_success() => {
                let payload = resp.json::<MistralOcrResponse>().map_err(|error| {
                    BaegunError::ocr_schema(format!(
                        "Failed parsing Mistral OCR response payload: {error}"
                    ))
                })?;

                if payload.pages.is_empty() {
                    return Err(BaegunError::ocr_schema(
                        "OCR response contains no pages."
                    ));
                }

                return Ok(payload);
            }
            Ok(resp) => {
                let status = resp.status();
                let body = resp.text().unwrap_or_default();
                if is_retryable_status(status) && attempt < max_attempts {
                    thread::sleep(sleep_duration);
                    sleep_duration = sleep_duration.saturating_mul(2);
                    continue;
                }

                return Err(BaegunError::api(format!(
                    "Mistral OCR failed ({status}): {}",
                    decode_api_error_message(&body)
                )));
            }
            Err(error) => {
                if attempt < max_attempts {
                    thread::sleep(sleep_duration);
                    sleep_duration = sleep_duration.saturating_mul(2);
                    continue;
                }

                return Err(BaegunError::api(format!(
                    "Mistral OCR request failed after retries: {error}"
                )));
            }
        }
    }
}

fn delete_remote_file(client: &Client, api_key: &str, file_id: &str) -> Result<()> {
    let response = client
        .delete(format!("{FILES_API_URL}/{file_id}"))
        .bearer_auth(api_key)
        .send()
        .map_err(|error| BaegunError::api(format!("Failed deleting remote OCR file: {error}")))?;

    if !response.status().is_success() {
        return Err(BaegunError::api(format!(
            "Mistral remote file cleanup failed ({})",
            response.status()
        )));
    }

    Ok(())
}

fn is_retryable_status(status: StatusCode) -> bool {
    status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error()
}

fn decode_api_error_message(body: &str) -> String {
    if let Ok(value) = serde_json::from_str::<Value>(body) {
        if let Some(message) = value
            .get("message")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
        {
            return message.to_owned();
        }

        if let Some(message) = value
            .get("error")
            .and_then(|err| err.get("message"))
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
        {
            return message.to_owned();
        }
    }

    body.trim().to_owned()
}
