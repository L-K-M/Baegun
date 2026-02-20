from __future__ import annotations

import os
from pathlib import Path
from typing import Literal

from pydantic import BaseModel, Field

from baegun.utils import ConfigError


class OcrConfig(BaseModel):
    api_key: str
    model: str = "mistral-ocr-latest"
    table_format: Literal["html", "markdown"] = "html"
    extract_header: bool = True
    extract_footer: bool = True
    include_images: bool = True
    keep_remote_file: bool = False


class CacheConfig(BaseModel):
    cache_dir: Path = Path(".baegun-cache")
    enabled: bool = True
    pipeline_version: str = "0.1.0"


class NormalizeConfig(BaseModel):
    table_format: Literal["html", "markdown"] = "html"
    extract_header: bool = True
    extract_footer: bool = True
    include_images: bool = True
    dedupe_threshold: float = 0.6


class StructureConfig(BaseModel):
    min_chapter_chars: int = 1200


class RenderConfig(BaseModel):
    language: str = "en"


class EpubConfig(BaseModel):
    output_path: Path
    title: str | None = None
    author: str | None = None
    language: str = "en"
    publisher: str | None = None


class ConvertConfig(BaseModel):
    input_pdf: Path
    output_path: Path
    quiet: bool = False
    verbose: bool = False
    run_validation: bool = False
    epubcheck_bin: str = "epubcheck"
    debug_dir: Path | None = None
    fail_on_warn: bool = False
    ocr: OcrConfig
    cache: CacheConfig
    normalize: NormalizeConfig
    structure: StructureConfig = Field(default_factory=StructureConfig)
    render: RenderConfig
    epub: EpubConfig


def resolve_api_key(api_key: str | None) -> str:
    if api_key:
        return api_key
    env_key = os.getenv("MISTRAL_API_KEY")
    if env_key:
        return env_key
    raise ConfigError("Missing API key. Use --api-key or set MISTRAL_API_KEY.")


def derive_output_path(input_pdf: Path, output: Path | None) -> Path:
    if output is not None:
        return output.expanduser().resolve()
    return input_pdf.with_suffix(".epub").resolve()


def build_convert_config(
    *,
    input_pdf: Path,
    output: Path | None,
    api_key: str | None,
    model: str,
    title: str | None,
    author: str | None,
    language: str,
    publisher: str | None,
    table_format: Literal["html", "markdown"],
    extract_header: bool,
    extract_footer: bool,
    include_images: bool,
    cache_dir: Path,
    no_cache: bool,
    validate: bool,
    epubcheck_bin: str,
    debug_dir: Path | None,
    keep_remote_file: bool,
    fail_on_warn: bool,
    quiet: bool,
    verbose: bool,
) -> ConvertConfig:
    if quiet and verbose:
        raise ConfigError("Use either --quiet or --verbose, not both.")

    if table_format not in {"html", "markdown"}:
        raise ConfigError("--table-format must be either 'html' or 'markdown'.")

    normalized_input = input_pdf.expanduser().resolve()
    if not normalized_input.exists() or not normalized_input.is_file():
        raise ConfigError(f"Input PDF not found: {normalized_input}")
    if normalized_input.suffix.lower() != ".pdf":
        raise ConfigError("Input file must be a .pdf")

    output_path = derive_output_path(normalized_input, output)
    cache_path = cache_dir.expanduser().resolve()
    debug_path = debug_dir.expanduser().resolve() if debug_dir else None

    resolved_api_key = resolve_api_key(api_key)

    ocr_cfg = OcrConfig(
        api_key=resolved_api_key,
        model=model,
        table_format=table_format,
        extract_header=extract_header,
        extract_footer=extract_footer,
        include_images=include_images,
        keep_remote_file=keep_remote_file,
    )
    cache_cfg = CacheConfig(cache_dir=cache_path, enabled=not no_cache)
    normalize_cfg = NormalizeConfig(
        table_format=table_format,
        extract_header=extract_header,
        extract_footer=extract_footer,
        include_images=include_images,
    )
    render_cfg = RenderConfig(language=language)
    epub_cfg = EpubConfig(
        output_path=output_path,
        title=title,
        author=author,
        language=language,
        publisher=publisher,
    )

    return ConvertConfig(
        input_pdf=normalized_input,
        output_path=output_path,
        quiet=quiet,
        verbose=verbose,
        run_validation=validate,
        epubcheck_bin=epubcheck_bin,
        debug_dir=debug_path,
        fail_on_warn=fail_on_warn,
        ocr=ocr_cfg,
        cache=cache_cfg,
        normalize=normalize_cfg,
        render=render_cfg,
        epub=epub_cfg,
    )
