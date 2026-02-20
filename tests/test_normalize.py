from __future__ import annotations

import copy

from baegun.config import NormalizeConfig
from baegun.models import AssetType
from baegun.normalize import normalize_ocr_payload


def test_normalize_replaces_placeholders(sample_payload: dict) -> None:
    cfg = NormalizeConfig()
    doc = normalize_ocr_payload(
        sample_payload,
        cfg,
        source_pdf_sha256="abc123",
        title="Fixture Title",
        author="Tester",
        language="en",
        publisher=None,
    )

    assert doc.metadata.title == "Fixture Title"
    assert "hyphenated" in doc.full_markdown
    assert "[tbl-0.html](tbl-0.html)" not in doc.full_markdown
    assert "<table>" in doc.full_markdown
    assert "../images/image-p001-001.jpg" in doc.full_markdown

    image_assets = [asset for asset in doc.assets.values() if asset.type == AssetType.IMAGE]
    table_assets = [asset for asset in doc.assets.values() if asset.type == AssetType.TABLE_HTML]
    assert len(image_assets) == 1
    assert len(table_assets) == 1
    assert len(image_assets[0].binary_content()) > 0


def test_normalize_dedupes_repeated_header(sample_payload: dict) -> None:
    cfg = NormalizeConfig()
    doc = normalize_ocr_payload(
        sample_payload,
        cfg,
        source_pdf_sha256="abc123",
        title="Fixture Title",
        author=None,
        language="en",
        publisher=None,
    )

    first_page_lines = [line for line in doc.pages[0].markdown.splitlines() if line.strip()]
    assert first_page_lines[0].startswith("#")
    assert "Sample OCR Book" not in doc.pages[1].markdown.splitlines()[0]


def test_normalize_decodes_data_uri_images(sample_payload: dict) -> None:
    payload = copy.deepcopy(sample_payload)
    raw_base64 = payload["pages"][0]["images"][0]["image_base64"]
    payload["pages"][0]["images"][0]["image_base64"] = f"data:image/png;base64,{raw_base64}"
    payload["pages"][0]["images"][0].pop("mime_type", None)

    doc = normalize_ocr_payload(
        payload,
        NormalizeConfig(),
        source_pdf_sha256="abc123",
        title="Fixture Title",
        author=None,
        language="en",
        publisher=None,
    )

    image_assets = [asset for asset in doc.assets.values() if asset.type == AssetType.IMAGE]
    assert image_assets
    assert image_assets[0].binary_content().startswith(b"\x89PNG")
    assert image_assets[0].mime_type == "image/png"
