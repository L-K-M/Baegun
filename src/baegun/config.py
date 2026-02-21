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


class MetadataConfig(BaseModel):
    enabled: bool = True
    model: str = "mistral-small-latest"
    max_pages: int = 3
    max_chars: int = 12000


class ConvertConfig(BaseModel):
    input_pdf: Path
    output_path: Path
    output_was_explicit: bool = False
    output_from_metadata: bool = False
    comic_mode: bool = False
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
    metadata: MetadataConfig = Field(default_factory=MetadataConfig)


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
    infer_metadata: bool = True,
    metadata_model: str = "mistral-small-latest",
    metadata_max_pages: int = 3,
    metadata_max_chars: int = 12000,
    output_from_metadata: bool = False,
    comic_mode: bool = False,
) -> ConvertConfig:
    if quiet and verbose:
        raise ConfigError("Use either --quiet or --verbose, not both.")

    if table_format not in {"html", "markdown"}:
        raise ConfigError("--table-format must be either 'html' or 'markdown'.")

    if metadata_max_pages < 1:
        raise ConfigError("--metadata-max-pages must be >= 1.")
    if metadata_max_chars < 1000:
        raise ConfigError("--metadata-max-chars must be >= 1000.")

    normalized_input = input_pdf.expanduser().resolve()
    if not normalized_input.exists() or not normalized_input.is_file():
        raise ConfigError(f"Input PDF not found: {normalized_input}")
    if normalized_input.suffix.lower() != ".pdf":
        raise ConfigError("Input file must be a .pdf")

    output_path = derive_output_path(normalized_input, output)
    cache_path = cache_dir.expanduser().resolve()
    debug_path = debug_dir.expanduser().resolve() if debug_dir else None

    try:
        resolved_api_key = resolve_api_key(api_key)
    except ConfigError as e:
        if comic_mode:
            resolved_api_key = "dummy"
        else:
            raise e

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
    metadata_cfg = MetadataConfig(
        enabled=infer_metadata,
        model=metadata_model,
        max_pages=metadata_max_pages,
        max_chars=metadata_max_chars,
    )

    return ConvertConfig(
        input_pdf=normalized_input,
        output_path=output_path,
        output_was_explicit=output is not None,
        output_from_metadata=output_from_metadata,
        comic_mode=comic_mode,
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
        metadata=metadata_cfg,
    )
