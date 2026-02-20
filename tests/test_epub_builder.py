from __future__ import annotations

import zipfile
from pathlib import Path

from baegun.config import EpubConfig, NormalizeConfig, RenderConfig, StructureConfig
from baegun.epub_builder import build_epub
from baegun.models import AssetIR, AssetType
from baegun.normalize import normalize_ocr_payload
from baegun.render import render_chapters
from baegun.structure import build_structure


def test_build_epub_writes_expected_files(tmp_path: Path, sample_payload: dict) -> None:
    doc = normalize_ocr_payload(
        sample_payload,
        NormalizeConfig(),
        source_pdf_sha256="hash",
        title="Fixture Title",
        author="Author",
        language="en",
        publisher="Publisher",
    )
    # Inject a deterministic cover asset to verify EPUB cover packaging.
    doc.assets["cover-image"] = AssetIR(
        asset_id="cover-image",
        type=AssetType.IMAGE,
        content=bytes.fromhex("ffd8ffe000104a464946"),
        mime_type="image/jpeg",
        source_page=0,
        file_name="cover.jpg",
        alt_text="Cover",
    )
    doc = build_structure(doc, StructureConfig(min_chapter_chars=50))
    rendered = render_chapters(doc, RenderConfig(language="en"))

    output = tmp_path / "book.epub"
    result = build_epub(
        rendered,
        EpubConfig(
            output_path=output,
            title="Fixture Title",
            author="Author",
            language="en",
            publisher="Publisher",
        ),
    )

    assert result.exists()

    with zipfile.ZipFile(result, "r") as archive:
        names = archive.namelist()
        assert "mimetype" in names
        assert any(name.endswith("nav.xhtml") for name in names)
        assert any(name.endswith("book.css") for name in names)
        assert any(name.endswith(".xhtml") and "chapter" in name for name in names)
        assert any(name.endswith(".jpg") for name in names)
        assert any(name.endswith("cover.xhtml") for name in names)
        assert any(name.endswith("cover.jpg") for name in names)

        opf = archive.read("EPUB/content.opf").decode("utf-8")
        assert "cover" in opf.lower()
