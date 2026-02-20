from __future__ import annotations

from pathlib import Path

from baegun.cli import convert_pdf_to_epub
from baegun.config import build_convert_config


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

    cfg = build_convert_config(
        input_pdf=sample_pdf_path,
        output=tmp_path / "first.epub",
        api_key="dummy",
        model="mistral-ocr-latest",
        title="Smoke Book",
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

    monkeypatch.setattr(
        "baegun.cli.run_ocr",
        lambda _pdf, _cfg: (_ for _ in ()).throw(RuntimeError("OCR should not be called on cache hit")),
    )

    cfg_2 = build_convert_config(
        input_pdf=sample_pdf_path,
        output=tmp_path / "second.epub",
        api_key="dummy",
        model="mistral-ocr-latest",
        title="Smoke Book",
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
