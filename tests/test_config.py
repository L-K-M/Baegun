from __future__ import annotations

from pathlib import Path

import pytest

from baegun.config import build_convert_config
from baegun.utils import ConfigError


def test_build_convert_config_defaults(sample_pdf_path: Path) -> None:
    cfg = build_convert_config(
        input_pdf=sample_pdf_path,
        output=None,
        api_key="dummy-key",
        model="mistral-ocr-latest",
        title=None,
        author=None,
        language="en",
        publisher=None,
        table_format="html",
        extract_header=True,
        extract_footer=True,
        include_images=True,
        cache_dir=Path(".baegun-cache"),
        no_cache=False,
        validate=False,
        epubcheck_bin="epubcheck",
        debug_dir=None,
        keep_remote_file=False,
        fail_on_warn=False,
        quiet=False,
        verbose=False,
    )

    assert cfg.input_pdf == sample_pdf_path.resolve()
    assert cfg.output_path.suffix == ".epub"
    assert cfg.ocr.api_key == "dummy-key"
    assert cfg.cache.enabled is True


def test_build_convert_config_requires_api_key(sample_pdf_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.delenv("MISTRAL_API_KEY", raising=False)
    with pytest.raises(ConfigError):
        build_convert_config(
            input_pdf=sample_pdf_path,
            output=None,
            api_key=None,
            model="mistral-ocr-latest",
            title=None,
            author=None,
            language="en",
            publisher=None,
            table_format="html",
            extract_header=True,
            extract_footer=True,
            include_images=True,
            cache_dir=Path(".baegun-cache"),
            no_cache=False,
            validate=False,
            epubcheck_bin="epubcheck",
            debug_dir=None,
            keep_remote_file=False,
            fail_on_warn=False,
            quiet=False,
            verbose=False,
        )


def test_build_convert_config_rejects_invalid_metadata_limits(sample_pdf_path: Path) -> None:
    with pytest.raises(ConfigError):
        build_convert_config(
            input_pdf=sample_pdf_path,
            output=None,
            api_key="dummy-key",
            model="mistral-ocr-latest",
            title=None,
            author=None,
            language="en",
            publisher=None,
            table_format="html",
            extract_header=True,
            extract_footer=True,
            include_images=True,
            cache_dir=Path(".baegun-cache"),
            no_cache=False,
            validate=False,
            epubcheck_bin="epubcheck",
            debug_dir=None,
            keep_remote_file=False,
            fail_on_warn=False,
            quiet=False,
            verbose=False,
            metadata_max_pages=0,
        )

    with pytest.raises(ConfigError):
        build_convert_config(
            input_pdf=sample_pdf_path,
            output=None,
            api_key="dummy-key",
            model="mistral-ocr-latest",
            title=None,
            author=None,
            language="en",
            publisher=None,
            table_format="html",
            extract_header=True,
            extract_footer=True,
            include_images=True,
            cache_dir=Path(".baegun-cache"),
            no_cache=False,
            validate=False,
            epubcheck_bin="epubcheck",
            debug_dir=None,
            keep_remote_file=False,
            fail_on_warn=False,
            quiet=False,
            verbose=False,
            metadata_max_chars=999,
        )
