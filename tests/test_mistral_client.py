from __future__ import annotations

from pathlib import Path

import pytest

from baegun.mistral_client import MistralOcrClient, infer_metadata_from_ocr_payload
from baegun.utils import OcrAuthError, OcrSchemaError


class FakeHttpError(Exception):
    def __init__(self, status_code: int, message: str) -> None:
        super().__init__(message)
        self.status_code = status_code


def test_mistral_client_success(sample_pdf_path: Path) -> None:
    class FakeFiles:
        def __init__(self) -> None:
            self.deleted: str | None = None

        def upload(self, *, purpose: str, file: object) -> dict:
            assert purpose == "ocr"
            assert file is not None
            return {"id": "file-123"}

        def delete(self, file_id: str | None = None, **_: object) -> None:
            self.deleted = file_id

    class FakeOcr:
        def process(self, **_: object) -> dict:
            return {"pages": [{"index": 0, "markdown": "# Hello"}]}

    class FakeClient:
        def __init__(self) -> None:
            self.files = FakeFiles()
            self.ocr = FakeOcr()

    fake_client = FakeClient()
    client = MistralOcrClient(api_key="x", client=fake_client)
    payload = client.run_ocr(sample_pdf_path)

    assert "pages" in payload
    assert fake_client.files.deleted == "file-123"


def test_mistral_client_auth_error(sample_pdf_path: Path) -> None:
    class FakeFiles:
        def upload(self, *, purpose: str, file: object) -> dict:
            return {"id": "file-123"}

        def delete(self, file_id: str | None = None, **_: object) -> None:
            return

    class FakeOcr:
        def process(self, **_: object) -> dict:
            raise FakeHttpError(401, "unauthorized")

    class FakeClient:
        def __init__(self) -> None:
            self.files = FakeFiles()
            self.ocr = FakeOcr()

    client = MistralOcrClient(api_key="x", client=FakeClient())
    with pytest.raises(OcrAuthError):
        client.run_ocr(sample_pdf_path)


def test_mistral_client_infer_metadata() -> None:
    class FakeChat:
        def complete(self, **_: object) -> dict:
            return {
                "choices": [
                    {
                        "message": {
                            "content": '{"title":"The Test Book","author":"A. Writer","publisher":"Demo Press"}'
                        }
                    }
                ]
            }

    class FakeClient:
        def __init__(self) -> None:
            self.chat = FakeChat()

    client = MistralOcrClient(api_key="x", client=FakeClient())
    metadata = client.infer_metadata("# The Test Book\n\nBy A. Writer")

    assert metadata is not None
    assert metadata.title == "The Test Book"
    assert metadata.author == "A. Writer"
    assert metadata.publisher == "Demo Press"


def test_infer_metadata_from_ocr_payload_sampling(monkeypatch: pytest.MonkeyPatch) -> None:
    payload = {
        "pages": [
            {"index": 0, "markdown": "# First Title\n\nBy First Author"},
            {"index": 1, "markdown": "Second page body"},
        ]
    }

    captured: dict[str, str] = {}

    def fake_infer(self, text: str, *, model: str = "mistral-small-latest"):
        captured["text"] = text
        captured["model"] = model
        from baegun.models import InferredMetadata

        return InferredMetadata(title="Inferred", author="Author", publisher=None)

    monkeypatch.setattr("baegun.mistral_client.MistralOcrClient.infer_metadata", fake_infer)

    metadata = infer_metadata_from_ocr_payload(
        payload,
        api_key="dummy",
        model="mistral-small-latest",
        max_pages=1,
        max_chars=500,
    )

    assert metadata is not None
    assert metadata.title == "Inferred"
    assert "Second page body" not in captured["text"]
    assert captured["model"] == "mistral-small-latest"


def test_mistral_client_infer_metadata_code_fence_and_unknowns() -> None:
    class FakeChat:
        def complete(self, **_: object) -> dict:
            return {
                "choices": [
                    {
                        "message": {
                            "content": "```json\n{\"title\":\"The Test Book\",\"author\":\"unknown\",\"publisher\":null}\n```"
                        }
                    }
                ]
            }

    class FakeClient:
        def __init__(self) -> None:
            self.chat = FakeChat()

    client = MistralOcrClient(api_key="x", client=FakeClient())
    metadata = client.infer_metadata("metadata text")
    assert metadata is not None
    assert metadata.title == "The Test Book"
    assert metadata.author is None
    assert metadata.publisher is None


def test_mistral_client_infer_metadata_requires_chat_api() -> None:
    class FakeClient:
        pass

    client = MistralOcrClient(api_key="x", client=FakeClient())
    with pytest.raises(OcrSchemaError):
        client.infer_metadata("metadata text")


def test_infer_metadata_from_ocr_payload_empty_pages_returns_none() -> None:
    metadata = infer_metadata_from_ocr_payload({"pages": []}, api_key="dummy")
    assert metadata is None
