from __future__ import annotations

import zipfile
from pathlib import Path

from baegun.cli import convert_pdf_to_epub
from baegun.config import build_convert_config
from baegun.models import AssetIR, AssetType, InferredMetadata


def test_pipeline_smoke_uses_cache(
    monkeypatch,
    tmp_path: Path,
    sample_payload: dict,
    sample_pdf_path: Path,
) -> None:
    calls = {"count": 0}

    def fake_run_ocr(_pdf: Path, _cfg: object) -> dict:
        calls["count"] += 1
        return sample_payload

    monkeypatch.setattr("baegun.cli.run_ocr", fake_run_ocr)
    monkeypatch.setattr(
        "baegun.cli.infer_metadata_from_ocr_payload",
        lambda *_args, **_kwargs: InferredMetadata(
            title="Inferred Smoke Book",
            author="Inferred Author",
            publisher="Inferred Publisher",
        ),
    )
    monkeypatch.setattr(
        "baegun.cli.extract_pdf_cover_asset",
        lambda _pdf: AssetIR(
            asset_id="cover-image",
            type=AssetType.IMAGE,
            content=bytes.fromhex("ffd8ffe000104a464946"),
            mime_type="image/jpeg",
            source_page=0,
            file_name="cover.jpg",
            alt_text="Cover",
        ),
    )

    cfg = build_convert_config(
        input_pdf=sample_pdf_path,
        output=tmp_path / "first.epub",
        api_key="dummy",
        model="mistral-ocr-latest",
        title=None,
        author=None,
        language="en",
        publisher=None,
        table_format="html",
        extract_header=True,
        extract_footer=True,
        include_images=True,
        cache_dir=tmp_path / "cache",
        no_cache=False,
        validate=False,
        epubcheck_bin="epubcheck",
        debug_dir=tmp_path / "debug",
        keep_remote_file=False,
        fail_on_warn=False,
        quiet=True,
        verbose=False,
    )

    first_output = convert_pdf_to_epub(cfg)
    assert first_output.exists()
    assert calls["count"] == 1
    with zipfile.ZipFile(first_output, "r") as archive:
        names = archive.namelist()
        assert any(name.endswith("cover.jpg") for name in names)
        assert any(name.endswith("cover.xhtml") for name in names)
        opf = archive.read("EPUB/content.opf").decode("utf-8")
        assert "Inferred Smoke Book" in opf
        assert "Inferred Author" in opf

    monkeypatch.setattr(
        "baegun.cli.run_ocr",
        lambda _pdf, _cfg: (_ for _ in ()).throw(RuntimeError("OCR should not be called on cache hit")),
    )

    cfg_2 = build_convert_config(
        input_pdf=sample_pdf_path,
        output=tmp_path / "second.epub",
        api_key="dummy",
        model="mistral-ocr-latest",
        title=None,
        author=None,
        language="en",
        publisher=None,
        table_format="html",
        extract_header=True,
        extract_footer=True,
        include_images=True,
        cache_dir=tmp_path / "cache",
        no_cache=False,
        validate=False,
        epubcheck_bin="epubcheck",
        debug_dir=tmp_path / "debug2",
        keep_remote_file=False,
        fail_on_warn=False,
        quiet=True,
        verbose=False,
    )

    second_output = convert_pdf_to_epub(cfg_2)
    assert second_output.exists()
