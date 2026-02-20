from __future__ import annotations

import json
import re
from pathlib import Path
from typing import Any, Callable

from tenacity import Retrying, retry_if_exception_type, stop_after_attempt, wait_exponential

from baegun.config import OcrConfig
from baegun.models import InferredMetadata
from baegun.utils import OcrApiError, OcrAuthError, OcrSchemaError

try:
    from mistralai import Mistral
except Exception:  # pragma: no cover - import failure path depends on runtime env
    Mistral = None  # type: ignore[assignment]


class OcrRetryableError(OcrApiError):
    """Error type used for retryable OCR failures."""


def _object_to_dict(value: Any) -> dict[str, Any]:
    if isinstance(value, dict):
        return value
    if hasattr(value, "model_dump"):
        return value.model_dump()  # type: ignore[no-any-return]
    if hasattr(value, "dict"):
        return value.dict()  # type: ignore[no-any-return]
    if hasattr(value, "__dict__"):
        return dict(value.__dict__)
    raise OcrSchemaError("Unable to normalize SDK response object into dict.")


class MistralOcrClient:
    def __init__(self, *, api_key: str, model: str = "mistral-ocr-latest", client: Any | None = None) -> None:
        if client is not None:
            self._client = client
        else:
            if Mistral is None:
                raise OcrApiError("mistralai package is not available.")
            self._client = Mistral(api_key=api_key)
        self.model = model

    def run_ocr(
        self,
        pdf_path: Path,
        *,
        table_format: str = "html",
        extract_header: bool = True,
        extract_footer: bool = True,
        include_images: bool = True,
        keep_remote_file: bool = False,
    ) -> dict[str, Any]:
        file_id = self._upload_file(pdf_path)
        try:
            payload = {
                "model": self.model,
                "document": {"type": "file", "file_id": file_id},
                "table_format": table_format,
                "extract_header": extract_header,
                "extract_footer": extract_footer,
                "include_image_base64": include_images,
            }

            ocr_api = getattr(self._client, "ocr", None)
            if ocr_api is None:
                raise OcrSchemaError("Mistral SDK client has no 'ocr' API namespace.")

            if hasattr(ocr_api, "process"):
                response = self._call_with_retry(lambda: ocr_api.process(**payload))
            elif hasattr(ocr_api, "process_file"):
                response = self._call_with_retry(lambda: ocr_api.process_file(**payload))
            else:
                raise OcrSchemaError("Mistral SDK OCR endpoint method was not found.")

            response_dict = _object_to_dict(response)
            if "pages" not in response_dict or not isinstance(response_dict.get("pages"), list):
                raise OcrSchemaError("OCR response is missing a 'pages' list.")
            return response_dict
        finally:
            if not keep_remote_file:
                self._delete_file(file_id)

    def _upload_file(self, pdf_path: Path) -> str:
        files_api = getattr(self._client, "files", None)
        if files_api is None or not hasattr(files_api, "upload"):
            raise OcrSchemaError("Mistral SDK client has no 'files.upload' method.")

        file_bytes = pdf_path.read_bytes()

        def _upload() -> Any:
            try:
                return files_api.upload(
                    purpose="ocr",
                    file={"file_name": pdf_path.name, "content": file_bytes},
                )
            except TypeError:
                return files_api.upload(purpose="ocr", file=str(pdf_path))

        response = self._call_with_retry(_upload)
        response_dict = _object_to_dict(response)
        file_id = response_dict.get("id") or response_dict.get("file_id")
        if not isinstance(file_id, str) or not file_id:
            raise OcrSchemaError("File upload response does not contain a valid file id.")
        return file_id

    def _delete_file(self, file_id: str) -> None:
        files_api = getattr(self._client, "files", None)
        if files_api is None or not hasattr(files_api, "delete"):
            return
        try:
            files_api.delete(file_id=file_id)
        except TypeError:
            files_api.delete(file_id)
        except Exception:
            return

    def infer_metadata(self, text: str, *, model: str = "mistral-small-latest") -> InferredMetadata | None:
        normalized = text.strip()
        if not normalized:
            return None

        chat_api = getattr(self._client, "chat", None)
        if chat_api is None or not hasattr(chat_api, "complete"):
            raise OcrSchemaError("Mistral SDK client has no 'chat.complete' method.")

        system_prompt = (
            "You extract bibliographic metadata from OCR text of a book. "
            "Return strict JSON with keys: title, author, publisher. "
            "Use null for unknown values. Do not include extra keys."
        )
        user_prompt = (
            "Extract the book metadata from the OCR text below. "
            "Use the most likely full book title and author.\n\n"
            f"OCR text:\n{normalized}"
        )

        response = self._call_with_retry(
            lambda: chat_api.complete(
                model=model,
                messages=[
                    {"role": "system", "content": system_prompt},
                    {"role": "user", "content": user_prompt},
                ],
                temperature=0,
                response_format={"type": "json_object"},
            )
        )
        content = _extract_chat_content(response)
        if not content:
            return None

        parsed = _parse_inferred_metadata(content)
        if parsed.title is None and parsed.author is None and parsed.publisher is None:
            return None
        return parsed

    def _call_with_retry(self, callback: Callable[[], Any]) -> Any:
        retrying = Retrying(
            reraise=True,
            stop=stop_after_attempt(4),
            wait=wait_exponential(multiplier=1, min=1, max=8),
            retry=retry_if_exception_type(OcrRetryableError),
        )
        for attempt in retrying:
            with attempt:
                try:
                    return callback()
                except Exception as exc:
                    raise self._map_exception(exc) from exc
        raise OcrApiError("Retry loop terminated unexpectedly.")

    @staticmethod
    def _map_exception(exc: Exception) -> Exception:
        status_code = getattr(exc, "status_code", None)
        if status_code is None:
            response = getattr(exc, "response", None)
            status_code = getattr(response, "status_code", None)

        message = str(exc)
        message_lower = message.lower()

        if status_code in (401, 403):
            return OcrAuthError(message or "OCR authentication failed.")
        if status_code == 429 or (isinstance(status_code, int) and status_code >= 500):
            return OcrRetryableError(message or "OCR request failed with a retryable server error.")
        if isinstance(status_code, int):
            return OcrApiError(message or f"OCR API request failed with status {status_code}.")

        if "rate limit" in message_lower or "quota" in message_lower:
            return OcrRetryableError(message)
        if "unauthorized" in message_lower or "forbidden" in message_lower:
            return OcrAuthError(message)

        return OcrApiError(message or "OCR request failed.")


def run_ocr(pdf_path: Path, cfg: OcrConfig) -> dict[str, Any]:
    client = MistralOcrClient(api_key=cfg.api_key, model=cfg.model)
    return client.run_ocr(
        pdf_path,
        table_format=cfg.table_format,
        extract_header=cfg.extract_header,
        extract_footer=cfg.extract_footer,
        include_images=cfg.include_images,
        keep_remote_file=cfg.keep_remote_file,
    )


def infer_metadata_from_ocr_payload(
    payload: dict[str, Any],
    *,
    api_key: str,
    model: str = "mistral-small-latest",
    max_pages: int = 3,
    max_chars: int = 12000,
) -> InferredMetadata | None:
    sample_text = _sample_ocr_text(payload, max_pages=max_pages, max_chars=max_chars)
    if not sample_text:
        return None

    client = MistralOcrClient(api_key=api_key)
    return client.infer_metadata(sample_text, model=model)


def _sample_ocr_text(payload: dict[str, Any], *, max_pages: int, max_chars: int) -> str:
    pages = payload.get("pages")
    if not isinstance(pages, list) or not pages:
        return ""

    sorted_pages = sorted(
        [page for page in pages if isinstance(page, dict)],
        key=lambda page: int(page.get("index", 0)),
    )

    chunks: list[str] = []
    for page in sorted_pages[:max_pages]:
        markdown = str(page.get("markdown") or "").strip()
        if markdown:
            chunks.append(markdown)

    text = "\n\n".join(chunks)
    if len(text) > max_chars:
        return text[:max_chars]
    return text


def _extract_chat_content(response: Any) -> str:
    if hasattr(response, "choices"):
        choices = response.choices
    else:
        choices = _object_to_dict(response).get("choices")
    if not choices:
        return ""

    first_choice = choices[0]
    message = getattr(first_choice, "message", None)
    if message is None and isinstance(first_choice, dict):
        message = first_choice.get("message")
    if message is None:
        return ""

    content = getattr(message, "content", None)
    if content is None and isinstance(message, dict):
        content = message.get("content")

    if isinstance(content, str):
        return content.strip()
    if isinstance(content, list):
        parts: list[str] = []
        for item in content:
            if isinstance(item, str):
                parts.append(item)
                continue
            if isinstance(item, dict):
                text_part = item.get("text")
                if isinstance(text_part, str):
                    parts.append(text_part)
                    continue
            text_attr = getattr(item, "text", None)
            if isinstance(text_attr, str):
                parts.append(text_attr)
        return "\n".join(part for part in parts if part).strip()
    return ""


def _parse_inferred_metadata(content: str) -> InferredMetadata:
    cleaned = _strip_code_fence(content)
    try:
        parsed = json.loads(cleaned)
    except json.JSONDecodeError:
        return InferredMetadata()

    if not isinstance(parsed, dict):
        return InferredMetadata()

    return InferredMetadata(
        title=_normalize_meta_field(parsed.get("title")),
        author=_normalize_meta_field(parsed.get("author")),
        publisher=_normalize_meta_field(parsed.get("publisher")),
    )


def _normalize_meta_field(value: Any) -> str | None:
    if value is None:
        return None
    if not isinstance(value, str):
        value = str(value)
    text = re.sub(r"\s+", " ", value).strip()
    if not text:
        return None
    if text.lower() in {"unknown", "n/a", "na", "null", "none"}:
        return None
    return text


def _strip_code_fence(text: str) -> str:
    stripped = text.strip()
    if not stripped.startswith("```"):
        return stripped

    lines = stripped.splitlines()
    if not lines:
        return stripped

    if lines[0].startswith("```"):
        lines = lines[1:]
    if lines and lines[-1].strip() == "```":
        lines = lines[:-1]
    return "\n".join(lines).strip()
