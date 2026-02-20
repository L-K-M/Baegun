from __future__ import annotations

from pathlib import Path
from typing import Any

import typer
from rich.console import Console

from baegun.cache import OcrCache, compute_cache_key
from baegun.config import ConvertConfig, build_convert_config
from baegun.epub_builder import build_epub
from baegun.mistral_client import run_ocr
from baegun.normalize import normalize_ocr_payload
from baegun.render import render_chapters
from baegun.structure import build_structure
from baegun.utils import (
    BaegunError,
    ConfigError,
    EpubBuildError,
    OcrApiError,
    OcrAuthError,
    OcrSchemaError,
    ValidationFailedError,
    ensure_dir,
    sha256_file,
    write_json,
)
from baegun.validate import run_epubcheck

app = typer.Typer(help="Convert PDFs to EPUB using Mistral OCR.", no_args_is_help=True)
console = Console(stderr=True)


@app.callback()
def main() -> None:
    """Baegun CLI entrypoint."""


@app.command()
def convert(
    input_pdf: Path = typer.Argument(..., help="Input PDF path."),
    output: Path | None = typer.Option(None, "-o", "--output", help="Output EPUB path."),
    api_key: str | None = typer.Option(None, "--api-key", help="Mistral API key."),
    model: str = typer.Option("mistral-ocr-latest", "--model", help="Mistral OCR model."),
    title: str | None = typer.Option(None, "--title", help="Book title override."),
    author: str | None = typer.Option(None, "--author", help="Book author metadata."),
    language: str = typer.Option("en", "--language", help="Book language metadata."),
    publisher: str | None = typer.Option(None, "--publisher", help="Book publisher metadata."),
    table_format: str = typer.Option("html", "--table-format", help="Table format from OCR."),
    extract_header: bool = typer.Option(True, "--extract-header/--no-extract-header"),
    extract_footer: bool = typer.Option(True, "--extract-footer/--no-extract-footer"),
    include_images: bool = typer.Option(True, "--include-images/--no-images"),
    cache_dir: Path = typer.Option(Path(".baegun-cache"), "--cache-dir", help="Cache directory."),
    no_cache: bool = typer.Option(False, "--no-cache", help="Disable OCR response cache."),
    validate: bool = typer.Option(False, "--validate", help="Run epubcheck validation."),
    epubcheck_bin: str = typer.Option("epubcheck", "--epubcheck-bin", help="Path to epubcheck binary."),
    debug_dir: Path | None = typer.Option(None, "--debug-dir", help="Write debug artifacts into this directory."),
    keep_remote_file: bool = typer.Option(
        False,
        "--keep-remote-file",
        help="Do not delete the uploaded file from Mistral.",
    ),
    fail_on_warn: bool = typer.Option(False, "--fail-on-warn", help="Treat validation warnings as failure."),
    quiet: bool = typer.Option(False, "--quiet", help="Minimal CLI output."),
    verbose: bool = typer.Option(False, "--verbose", help="Verbose CLI output."),
) -> None:
    try:
        cfg = build_convert_config(
            input_pdf=input_pdf,
            output=output,
            api_key=api_key,
            model=model,
            title=title,
            author=author,
            language=language,
            publisher=publisher,
            table_format=table_format,  # type: ignore[arg-type]
            extract_header=extract_header,
            extract_footer=extract_footer,
            include_images=include_images,
            cache_dir=cache_dir,
            no_cache=no_cache,
            validate=validate,
            epubcheck_bin=epubcheck_bin,
            debug_dir=debug_dir,
            keep_remote_file=keep_remote_file,
            fail_on_warn=fail_on_warn,
            quiet=quiet,
            verbose=verbose,
        )
    except ConfigError as exc:
        _error(str(exc))
        raise typer.Exit(code=2) from exc

    try:
        output_path = convert_pdf_to_epub(cfg)
        if not cfg.quiet:
            console.print(f"[green]Created EPUB:[/green] {output_path}")
    except OcrAuthError as exc:
        _error(str(exc))
        raise typer.Exit(code=3) from exc
    except OcrApiError as exc:
        _error(str(exc))
        raise typer.Exit(code=3) from exc
    except OcrSchemaError as exc:
        _error(str(exc))
        raise typer.Exit(code=4) from exc
    except EpubBuildError as exc:
        _error(str(exc))
        raise typer.Exit(code=5) from exc
    except ValidationFailedError as exc:
        _error(str(exc))
        raise typer.Exit(code=6) from exc
    except BaegunError as exc:
        _error(str(exc))
        raise typer.Exit(code=1) from exc
    except Exception as exc:  # pragma: no cover - safety net
        _error(f"Unexpected error: {exc}")
        raise typer.Exit(code=1) from exc


def convert_pdf_to_epub(cfg: ConvertConfig) -> Path:
    if cfg.debug_dir:
        ensure_dir(cfg.debug_dir)

    payload = _load_or_run_ocr(cfg)
    if cfg.debug_dir:
        write_json(cfg.debug_dir / "ocr_payload.json", payload)

    source_hash = sha256_file(cfg.input_pdf)
    document = normalize_ocr_payload(
        payload,
        cfg.normalize,
        source_pdf_sha256=source_hash,
        title=cfg.epub.title or cfg.input_pdf.stem,
        author=cfg.epub.author,
        language=cfg.epub.language,
        publisher=cfg.epub.publisher,
    )
    document = build_structure(document, cfg.structure)

    if cfg.debug_dir:
        write_json(cfg.debug_dir / "document_ir.json", _document_for_debug(document))

    rendered = render_chapters(document, cfg.render)
    if cfg.debug_dir:
        write_json(cfg.debug_dir / "rendered_book.json", _rendered_for_debug(rendered))

    epub_path = build_epub(rendered, cfg.epub)
    if cfg.run_validation:
        result = run_epubcheck(epub_path, cfg.epubcheck_bin, fail_on_warn=cfg.fail_on_warn)
        if not result.ok:
            raise ValidationFailedError(result.output or "EPUB validation failed.")

    return epub_path


def _load_or_run_ocr(cfg: ConvertConfig) -> dict[str, Any]:
    cache = OcrCache(cfg.cache.cache_dir, enabled=cfg.cache.enabled)
    key = compute_cache_key(cfg.input_pdf, cfg.ocr, cfg.cache.pipeline_version)

    cached_payload = cache.load_ocr_json(key)
    if cached_payload is not None:
        if cfg.verbose and not cfg.quiet:
            console.print("[cyan]Cache hit:[/cyan] OCR payload reused")
        return cached_payload

    if cfg.verbose and not cfg.quiet:
        console.print("[cyan]Running OCR:[/cyan] Uploading PDF to Mistral")

    payload = run_ocr(cfg.input_pdf, cfg.ocr)
    cache.save_ocr_json(key, payload)
    return payload


def _document_for_debug(document: Any) -> dict[str, Any]:
    payload = document.model_dump()
    for asset in payload.get("assets", {}).values():
        content = asset.get("content")
        if isinstance(content, str):
            continue
        asset["content"] = f"<bytes:{len(content)}>"
    return payload


def _rendered_for_debug(rendered: Any) -> dict[str, Any]:
    payload = rendered.model_dump()
    for asset in payload.get("assets", {}).values():
        content = asset.get("content")
        if isinstance(content, str):
            continue
        asset["content"] = f"<bytes:{len(content)}>"
    return payload


def _error(message: str) -> None:
    console.print(f"[red]Error:[/red] {message}")


if __name__ == "__main__":
    app()
