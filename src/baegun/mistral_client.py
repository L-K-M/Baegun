from __future__ import annotations

from pathlib import Path
from typing import Any, Callable

from tenacity import Retrying, retry_if_exception_type, stop_after_attempt, wait_exponential

from baegun.config import OcrConfig
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
