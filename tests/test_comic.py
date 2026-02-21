from __future__ import annotations

from pathlib import Path
from typer.testing import CliRunner
from baegun.cli import app

runner = CliRunner()

def test_comic_mode_invokes_comic_builder(monkeypatch, tmp_path: Path, sample_pdf_path: Path) -> None:
    output = tmp_path / "comic_out.epub"
    
    comic_builder_called = False
    
    def mock_build_comic_document(cfg, source_hash):
        nonlocal comic_builder_called
        comic_builder_called = True
        
        # Return a dummy DocumentIR
        from baegun.models import DocumentIR, MetadataIR
        return DocumentIR(
            metadata=MetadataIR(title="Comic Test", source_pdf_sha256="dummy"),
            pages=[],
            chapters=[],
            toc=[],
            assets={},
            full_markdown=""
        )

    monkeypatch.setattr("baegun.comic.build_comic_document", mock_build_comic_document)
    monkeypatch.setattr("baegun.cli._extract_cover_asset", lambda _pdf: None)

    result = runner.invoke(
        app,
        [
            "convert",
            str(sample_pdf_path),
            "-o",
            str(output),
            "--comic",
        ],
    )

    assert comic_builder_called, "comic mode should bypass OCR and use build_comic_document"
    assert result.exit_code == 0
    assert output.exists()
