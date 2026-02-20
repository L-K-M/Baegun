from __future__ import annotations

from baegun.config import NormalizeConfig, RenderConfig, StructureConfig
from baegun.normalize import normalize_ocr_payload
from baegun.render import render_chapters
from baegun.structure import build_structure


def test_render_outputs_xhtml_with_callouts(sample_payload: dict) -> None:
    doc = normalize_ocr_payload(
        sample_payload,
        NormalizeConfig(),
        source_pdf_sha256="hash",
        title="Fixture Title",
        author=None,
        language="en",
        publisher=None,
    )
    doc = build_structure(doc, StructureConfig(min_chapter_chars=50))
    rendered = render_chapters(doc, RenderConfig(language="en"))

    assert rendered.chapters
    first = rendered.chapters[0].xhtml
    assert first.startswith("<?xml version=\"1.0\"")
    assert "<aside class=\"callout note\">" in first
    assert "id=\"sample-ocr-book\"" in first
    assert "../images/image-p001-001.jpg" in first
