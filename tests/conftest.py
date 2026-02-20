from __future__ import annotations

import json
from pathlib import Path

import pytest


@pytest.fixture
def sample_payload() -> dict:
    fixture_path = Path(__file__).resolve().parent / "fixtures" / "sample_ocr_payload.json"
    return json.loads(fixture_path.read_text(encoding="utf-8"))


@pytest.fixture
def sample_pdf_path() -> Path:
    pdf_root = Path(__file__).resolve().parents[1] / "test"
    candidates = sorted(pdf_root.glob("*.pdf"))
    if not candidates:
        raise RuntimeError("No PDFs found in ./test")
    return candidates[0]
