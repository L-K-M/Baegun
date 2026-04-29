use crate::errors::{BaegunError, Result};
use crate::models::{BookMetadata, ConvertConfig, MistralOcrResponse};
use reqwest::blocking::{multipart, Client};
use reqwest::StatusCode;
use serde::Deserialize;
use serde_json::{json, Value};
use std::thread;
use std::time::Duration;

const FILES_API_URL: &str = "https://api.mistral.ai/v1/files";
const OCR_API_URL: &str = "https://api.mistral.ai/v1/ocr";
const CHAT_API_URL: &str = "https://api.mistral.ai/v1/chat/completions";
const METADATA_MODEL: &str = "mistral-small-latest";

#[derive(Debug, Deserialize)]
struct UploadFileResponse {
    id: String,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    message: ChatMessage,
}

#[derive(Debug, Deserialize)]
struct ChatMessage {
    content: String,
}

pub fn run_mistral_ocr(
    cfg: &ConvertConfig,
    pdf_bytes: &[u8],
    source_filename: &str,
) -> Result<MistralOcrResponse> {
    let api_key = cfg.api_key.as_deref().ok_or_else(|| {
        BaegunError::bad_args("Missing API key. Pass --api-key or set MISTRAL_API_KEY.")
    })?;

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

pub(crate) fn generate_book_metadata(
    cfg: &ConvertConfig,
    ocr_payload: &MistralOcrResponse,
) -> Result<BookMetadata> {
    let api_key = cfg
        .api_key
        .as_deref()
        .ok_or_else(|| BaegunError::bad_args("Missing API key for metadata generation."))?;
    let content_sample = ocr_metadata_sample(ocr_payload);
    if content_sample.is_empty() {
        return Ok(BookMetadata::default());
    }

    let client = Client::builder()
        .timeout(Duration::from_secs(60))
        .build()
        .map_err(|error| BaegunError::api(format!("Failed creating HTTP client: {error}")))?;

    let request_payload = json!({
        "model": METADATA_MODEL,
        "temperature": 0,
        "max_tokens": 500,
        "response_format": { "type": "json_object" },
        "messages": [
            {
                "role": "system",
                "content": "Extract or infer EPUB metadata from OCR text. The first OCR pages are usually the cover or title page; prioritize title and author exactly as printed there. Strip leading labels such as 'by' from author names. Do not use chapter headings, running headers, or table-of-contents entries as the book title when a cover/title-page title exists. Return only compact JSON with keys title, author, publisher, language, description, subjects. Use null for unknown scalar fields. Use a short BCP-47 language code when evident. subjects must be an array of up to 8 concise subject tags. Do not invent a human author when there is no evidence."
            },
            {
                "role": "user",
                "content": content_sample
            }
        ]
    });

    let response = client
        .post(CHAT_API_URL)
        .bearer_auth(api_key)
        .json(&request_payload)
        .send()
        .map_err(|error| {
            BaegunError::api(format!(
                "Failed generating EPUB metadata with Mistral: {error}"
            ))
        })?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().unwrap_or_default();
        return Err(BaegunError::api(format!(
            "Mistral metadata generation failed ({status}): {}",
            decode_api_error_message(&body)
        )));
    }

    let completion = response.json::<ChatCompletionResponse>().map_err(|error| {
        BaegunError::api(format!("Failed parsing Mistral metadata response: {error}"))
    })?;
    let content = completion
        .choices
        .first()
        .map(|choice| choice.message.content.as_str())
        .unwrap_or_default();

    parse_generated_metadata(content)
}

fn upload_pdf(
    client: &Client,
    api_key: &str,
    pdf_bytes: &[u8],
    source_filename: &str,
) -> Result<String> {
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

    let upload = response.json::<UploadFileResponse>().map_err(|error| {
        BaegunError::api(format!(
            "Failed parsing Mistral file upload response: {error}"
        ))
    })?;

    Ok(upload.id)
}

fn perform_ocr_with_retries(
    client: &Client,
    api_key: &str,
    cfg: &ConvertConfig,
    file_id: &str,
) -> Result<MistralOcrResponse> {
    // Always request image payloads so the first page image can become the EPUB cover.
    let request_payload = json!({
        "model": cfg.model,
        "document": {
            "type": "file",
            "file_id": file_id,
        },
        "table_format": cfg.table_format.as_str(),
        "extract_header": cfg.extract_header,
        "extract_footer": cfg.extract_footer,
        "include_image_base64": true,
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
                    return Err(BaegunError::ocr_schema("OCR response contains no pages."));
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

fn ocr_metadata_sample(ocr_payload: &MistralOcrResponse) -> String {
    let mut pages = ocr_payload.pages.iter().collect::<Vec<_>>();
    pages.sort_by_key(|page| page.index);

    let mut sample = String::new();
    for page in pages.into_iter().take(8) {
        let markdown = page.markdown.trim();
        if markdown.is_empty() {
            continue;
        }

        sample.push_str(&format!("Page {}:\n{}\n\n", page.index + 1, markdown));
        if sample.len() >= 12_000 {
            break;
        }
    }

    truncate_chars(sample.trim(), 12_000)
}

fn parse_generated_metadata(content: &str) -> Result<BookMetadata> {
    let Some(json_text) = extract_json_object(content) else {
        return Ok(BookMetadata::default());
    };
    let value = serde_json::from_str::<Value>(json_text).map_err(|error| {
        BaegunError::api(format!("Failed parsing generated metadata JSON: {error}"))
    })?;

    Ok(BookMetadata {
        title: metadata_string(&value, "title"),
        author: metadata_string(&value, "author"),
        language: metadata_string(&value, "language"),
        publisher: metadata_string(&value, "publisher"),
        description: metadata_string(&value, "description"),
        subjects: metadata_strings(&value, "subjects")
            .into_iter()
            .take(8)
            .collect(),
    })
}

fn extract_json_object(content: &str) -> Option<&str> {
    let start = content.find('{')?;
    let end = content.rfind('}')?;
    (start <= end).then_some(&content[start..=end])
}

fn metadata_string(value: &Value, key: &str) -> Option<String> {
    match value.get(key)? {
        Value::String(raw) => clean_metadata_value(raw),
        Value::Array(items) => {
            let joined = items
                .iter()
                .filter_map(Value::as_str)
                .filter_map(clean_metadata_value)
                .collect::<Vec<_>>()
                .join(", ");
            clean_metadata_value(&joined)
        }
        _ => None,
    }
}

fn metadata_strings(value: &Value, key: &str) -> Vec<String> {
    match value.get(key) {
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(Value::as_str)
            .filter_map(clean_metadata_value)
            .collect(),
        Some(Value::String(raw)) => raw
            .split([',', ';', '\n'])
            .filter_map(clean_metadata_value)
            .collect(),
        _ => Vec::new(),
    }
}

fn clean_metadata_value(value: &str) -> Option<String> {
    let cleaned = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if cleaned.is_empty()
        || cleaned.eq_ignore_ascii_case("unknown")
        || cleaned.eq_ignore_ascii_case("null")
    {
        None
    } else {
        Some(cleaned)
    }
}

fn truncate_chars(value: &str, max_len: usize) -> String {
    if value.len() <= max_len {
        return value.to_owned();
    }

    let end = value
        .char_indices()
        .map(|(index, _)| index)
        .take_while(|index| *index <= max_len)
        .last()
        .unwrap_or(0);
    value[..end].to_owned()
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

#[cfg(test)]
mod tests {
    use super::parse_generated_metadata;

    #[test]
    fn parses_generated_metadata_json() {
        let metadata = parse_generated_metadata(
            r#"```json
{"title":"Generated Title","author":"Generated Author","language":"en","publisher":null,"description":"Short description","subjects":["fiction","history"]}
```"#,
        )
        .expect("generated metadata should parse");

        assert_eq!(metadata.title.as_deref(), Some("Generated Title"));
        assert_eq!(metadata.author.as_deref(), Some("Generated Author"));
        assert_eq!(metadata.description.as_deref(), Some("Short description"));
        assert_eq!(metadata.subjects, vec!["fiction", "history"]);
    }
}
