from __future__ import annotations

import pytest

pytest.importorskip("pypdfium2")

from baegun.cover import extract_pdf_cover_asset


def test_extract_pdf_cover_asset(real_pdf_path) -> None:
    cover = extract_pdf_cover_asset(real_pdf_path)
    assert cover is not None
    assert cover.asset_id == "cover-image"
    assert cover.file_name == "cover.jpg"
    assert cover.mime_type == "image/jpeg"
    assert cover.binary_content().startswith(b"\xff\xd8\xff")
