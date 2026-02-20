from __future__ import annotations

import json
from pathlib import Path

import pytest


def _pdf_candidates() -> list[Path]:
    repo_root = Path(__file__).resolve().parents[1]
    search_roots = [
        repo_root / "test",
        repo_root / "test-files",
        repo_root / "tests" / "fixtures",
    ]

    candidates: list[Path] = []
    for root in search_roots:
        if not root.exists():
            continue
        candidates.extend(sorted(root.rglob("*.pdf")))
    return candidates


@pytest.fixture
def sample_payload() -> dict:
    fixture_path = Path(__file__).resolve().parent / "fixtures" / "sample_ocr_payload.json"
    return json.loads(fixture_path.read_text(encoding="utf-8"))


@pytest.fixture
def sample_pdf_path(tmp_path_factory: pytest.TempPathFactory) -> Path:
    candidates = _pdf_candidates()
    if candidates:
        return candidates[0]

    fallback_dir = tmp_path_factory.mktemp("baegun-fixtures")
    fallback_pdf = fallback_dir / "placeholder.pdf"
    fallback_pdf.write_bytes(
        b"%PDF-1.4\n% baegun test placeholder\n1 0 obj\n<<>>\nendobj\ntrailer\n<<>>\n%%EOF\n"
    )
    return fallback_pdf


@pytest.fixture
def real_pdf_path() -> Path:
    candidates = _pdf_candidates()
    if not candidates:
        pytest.skip("No real PDFs found for cover rendering test")
    return candidates[0]
