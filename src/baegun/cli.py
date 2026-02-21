from __future__ import annotations

from pathlib import Path
from typing import Any

import typer
from rich.console import Console

from baegun.cache import OcrCache, compute_cache_key
from baegun.config import ConvertConfig, build_convert_config
from baegun.cover import extract_pdf_cover_asset
from baegun.epub_builder import build_epub
from baegun.mistral_client import infer_metadata_from_ocr_payload, run_ocr
from baegun.models import AssetIR, InferredMetadata
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
    slugify,
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
    output_from_metadata: bool = typer.Option(
        False,
        "--output-from-metadata/--no-output-from-metadata",
        help="Name output file from inferred book title when --output is not set.",
    ),
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
    comic: bool = typer.Option(False, "--comic/--no-comic", help="Enable comic mode (render PDF pages as images, skips OCR)."),
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
    infer_metadata: bool = typer.Option(
        True,
        "--infer-metadata/--no-infer-metadata",
        help="Use Mistral chat to infer missing title/author/publisher.",
    ),
    metadata_model: str = typer.Option(
        "mistral-small-latest",
        "--metadata-model",
        help="Model used for metadata inference.",
    ),
    metadata_max_pages: int = typer.Option(
        3,
        "--metadata-max-pages",
        help="Number of OCR pages sampled for metadata inference.",
    ),
    metadata_max_chars: int = typer.Option(
        12000,
        "--metadata-max-chars",
        help="Max characters sampled for metadata inference.",
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
            infer_metadata=infer_metadata,
            metadata_model=metadata_model,
            metadata_max_chars=metadata_max_chars,
            output_from_metadata=output_from_metadata,
            comic_mode=comic,
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

    source_hash = sha256_file(cfg.input_pdf)
    resolved_title = cfg.epub.title or cfg.input_pdf.stem
    resolved_author = cfg.epub.author
    resolved_publisher = cfg.epub.publisher
    metadata_title = cfg.epub.title

    if cfg.comic_mode:
        if cfg.verbose and not cfg.quiet:
            console.print("[cyan]Comic Mode:[/cyan] Rendering PDF pages as images")
        from baegun.comic import build_comic_document
        document = build_comic_document(cfg, source_hash)
        _apply_output_name_from_metadata(cfg, cfg.epub.title)
        cover_asset = _extract_cover_asset(cfg)
        if cover_asset is not None:
            document.assets[cover_asset.asset_id] = cover_asset
    else:
        payload = _load_or_run_ocr(cfg)
        if cfg.debug_dir:
            write_json(cfg.debug_dir / "ocr_payload.json", payload)

        if cfg.metadata.enabled and (cfg.epub.title is None or cfg.epub.author is None or cfg.epub.publisher is None):
            inferred = _infer_metadata(cfg, payload)
            if inferred is not None:
                if cfg.epub.title is None and inferred.title:
                    resolved_title = inferred.title
                    metadata_title = inferred.title
                if cfg.epub.author is None and inferred.author:
                    resolved_author = inferred.author
                if cfg.epub.publisher is None and inferred.publisher:
                    resolved_publisher = inferred.publisher

        _apply_output_name_from_metadata(cfg, metadata_title)

        document = normalize_ocr_payload(
            payload,
            cfg.normalize,
            source_pdf_sha256=source_hash,
            title=resolved_title,
            author=resolved_author,
            language=cfg.epub.language,
            publisher=resolved_publisher,
        )
        document = build_structure(document, cfg.structure)

        cover_asset = _extract_cover_asset(cfg)
        if cover_asset is not None:
            document.assets[cover_asset.asset_id] = cover_asset

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


def _extract_cover_asset(cfg: ConvertConfig) -> AssetIR | None:
    try:
        cover_asset = extract_pdf_cover_asset(cfg.input_pdf)
        if cover_asset is not None and cfg.verbose and not cfg.quiet:
            console.print("[cyan]Cover:[/cyan] Extracted first page as cover image")
        return cover_asset
    except Exception as exc:  # pragma: no cover - non-critical fallback path
        if cfg.verbose and not cfg.quiet:
            console.print(f"[yellow]Cover skipped:[/yellow] {exc}")
        return None


def _infer_metadata(cfg: ConvertConfig, payload: dict[str, Any]) -> InferredMetadata | None:
    try:
        inferred = infer_metadata_from_ocr_payload(
            payload,
            api_key=cfg.ocr.api_key,
            model=cfg.metadata.model,
            max_pages=cfg.metadata.max_pages,
            max_chars=cfg.metadata.max_chars,
        )
    except Exception as exc:  # pragma: no cover - non-critical fallback path
        if cfg.verbose and not cfg.quiet:
            console.print(f"[yellow]Metadata inference skipped:[/yellow] {exc}")
        return None

    if inferred is not None and cfg.verbose and not cfg.quiet:
        bits: list[str] = []
        if inferred.title:
            bits.append(f"title='{inferred.title}'")
        if inferred.author:
            bits.append(f"author='{inferred.author}'")
        if inferred.publisher:
            bits.append(f"publisher='{inferred.publisher}'")
        if bits:
            console.print(f"[cyan]Metadata:[/cyan] inferred {', '.join(bits)}")
    return inferred


def _apply_output_name_from_metadata(cfg: ConvertConfig, metadata_title: str | None) -> None:
    if not cfg.output_from_metadata:
        return
    if cfg.output_was_explicit:
        if cfg.verbose and not cfg.quiet:
            console.print("[yellow]Output name kept:[/yellow] --output was explicitly provided")
        return
    if not metadata_title:
        if cfg.verbose and not cfg.quiet:
            console.print("[yellow]Output name kept:[/yellow] no metadata title available")
        return

    base_name = slugify(metadata_title, fallback="")
    if not base_name:
        if cfg.verbose and not cfg.quiet:
            console.print("[yellow]Output name kept:[/yellow] metadata title could not be normalized")
        return

    output_path = (cfg.output_path.parent / f"{base_name}.epub").resolve()
    cfg.output_path = output_path
    cfg.epub.output_path = output_path

    if cfg.verbose and not cfg.quiet:
        console.print(f"[cyan]Output name:[/cyan] using metadata-derived filename {output_path.name}")


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
