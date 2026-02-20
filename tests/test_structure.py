from __future__ import annotations

from baegun.config import NormalizeConfig, StructureConfig
from baegun.normalize import normalize_ocr_payload
from baegun.structure import build_structure, normalize_heading_levels


def test_heading_level_jump_is_compressed() -> None:
    markdown = "# A\n\n#### B\n\nText"
    normalized, headings = normalize_heading_levels(markdown)
    assert "#### B" not in normalized
    assert "## B" in normalized
    assert headings[1].level == 2


def test_structure_builds_chapters_and_toc(sample_payload: dict) -> None:
    doc = normalize_ocr_payload(
        sample_payload,
        NormalizeConfig(),
        source_pdf_sha256="hash",
        title="Fixture Title",
        author=None,
        language="en",
        publisher=None,
    )

    structured = build_structure(doc, StructureConfig(min_chapter_chars=50))
    assert structured.chapters
    assert structured.toc
    assert structured.chapters[0].file_name.endswith(".xhtml")
    assert structured.toc[0].href.startswith("text/")
