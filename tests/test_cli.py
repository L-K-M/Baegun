from __future__ import annotations

from pathlib import Path

from typer.testing import CliRunner

from baegun.cli import app

runner = CliRunner()


def test_cli_convert_success(
    monkeypatch,
    tmp_path: Path,
    sample_payload: dict,
    sample_pdf_path: Path,
) -> None:
    output = tmp_path / "out.epub"

    monkeypatch.setattr("baegun.cli.run_ocr", lambda _pdf, _cfg: sample_payload)

    result = runner.invoke(
        app,
        [
            "convert",
            str(sample_pdf_path),
            "-o",
            str(output),
            "--api-key",
            "dummy",
            "--no-cache",
        ],
    )

    assert result.exit_code == 0
    assert output.exists()


def test_cli_missing_api_key(monkeypatch, sample_pdf_path: Path) -> None:
    monkeypatch.delenv("MISTRAL_API_KEY", raising=False)
    result = runner.invoke(app, ["convert", str(sample_pdf_path), "--no-cache"])
    assert result.exit_code == 2
