from __future__ import annotations

from pathlib import Path

import pytest

from baegun.mistral_client import MistralOcrClient
from baegun.utils import OcrAuthError


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
