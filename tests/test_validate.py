from __future__ import annotations

from pathlib import Path

import pytest

from baegun.utils import ValidationFailedError
from baegun.validate import run_epubcheck


def test_run_epubcheck_parses_errors(tmp_path: Path) -> None:
    binary = tmp_path / "fake-epubcheck"
    binary.write_text("#!/bin/sh\necho 'ERROR: bad file'\nexit 1\n", encoding="utf-8")
    binary.chmod(0o755)

    epub = tmp_path / "sample.epub"
    epub.write_bytes(b"fake")

    result = run_epubcheck(epub, str(binary))
    assert result.ok is False
    assert result.errors >= 1


def test_run_epubcheck_missing_binary(tmp_path: Path) -> None:
    epub = tmp_path / "sample.epub"
    epub.write_bytes(b"fake")

    with pytest.raises(ValidationFailedError):
        run_epubcheck(epub, str(tmp_path / "not-found"))
