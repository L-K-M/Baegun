from __future__ import annotations

import base64
import re
from collections import Counter
from pathlib import Path
from typing import Any

from baegun.config import NormalizeConfig
from baegun.models import AssetIR, AssetType, DocumentIR, MetadataIR, PageIR
from baegun.utils import OcrSchemaError, guess_mime_type


_IMAGE_LINK_RE = re.compile(r"!\[(?P<alt>[^\]]*)\]\((?P<target>[^)]+)\)")
_TABLE_LINK_RE = re.compile(r"\[(?P<label>[^\]]+)\]\((?P<target>[^)]+)\)")


def normalize_ocr_payload(
    payload: dict[str, Any],
    cfg: NormalizeConfig,
    *,
    source_pdf_sha256: str,
    title: str,
    author: str | None,
    language: str,
    publisher: str | None,
) -> DocumentIR:
    pages = payload.get("pages")
    if not isinstance(pages, list) or not pages:
        raise OcrSchemaError("OCR payload does not include any pages.")

    assets: dict[str, AssetIR] = {}
    page_irs: list[PageIR] = []

    sorted_pages = sorted(pages, key=lambda page: int(page.get("index", 0)))

    for fallback_index, page in enumerate(sorted_pages):
        page_index = int(page.get("index", fallback_index))
        markdown = str(page.get("markdown") or "")
        header = str(page.get("header") or "").strip() or None
        footer = str(page.get("footer") or "").strip() or None

        markdown = _replace_table_placeholders(markdown, page.get("tables"), page_index, assets)
        if cfg.include_images:
            markdown = _replace_image_placeholders(markdown, page.get("images"), page_index, assets)

        markdown = _remove_embedded_header_footer(
            markdown,
            header if cfg.extract_header else None,
            footer if cfg.extract_footer else None,
        )
        markdown = clean_markdown(markdown)

        page_irs.append(
            PageIR(index=page_index, markdown=markdown, header=header, footer=footer)
        )

    _remove_repeated_edge_lines(page_irs, threshold=cfg.dedupe_threshold)

    full_markdown = "\n\n".join(page.markdown.strip() for page in page_irs if page.markdown.strip())
    metadata = MetadataIR(
        title=title or infer_title_from_markdown(full_markdown),
        author=author,
        language=language,
        publisher=publisher,
        source_pdf_sha256=source_pdf_sha256,
    )
    return DocumentIR(metadata=metadata, pages=page_irs, assets=assets, full_markdown=full_markdown)


def clean_markdown(markdown: str) -> str:
    chunks = re.split(r"(```.*?```)", markdown, flags=re.DOTALL)
    cleaned: list[str] = []
    for chunk in chunks:
        if chunk.startswith("```"):
            cleaned.append(chunk)
            continue
        fixed = re.sub(r"([A-Za-z])-\n([A-Za-z])", r"\1\2", chunk)
        fixed = re.sub(r"[ \t]+\n", "\n", fixed)
        fixed = re.sub(r"\n{3,}", "\n\n", fixed)
        cleaned.append(fixed)
    return "".join(cleaned).strip()


def infer_title_from_markdown(markdown: str) -> str:
    for line in markdown.splitlines():
        stripped = line.strip()
        if stripped.startswith("# "):
            return stripped[2:].strip()
    return "Untitled"


def _replace_image_placeholders(
    markdown: str,
    images: Any,
    page_index: int,
    assets: dict[str, AssetIR],
) -> str:
    if not isinstance(images, list):
        return markdown

    for img_idx, image in enumerate(images):
        if not isinstance(image, dict):
            continue

        source_ref = str(
            image.get("id") or image.get("file_name") or image.get("name") or f"img-{img_idx}.png"
        )
        mime_type = _normalize_mime_type(
            str(image.get("mime_type") or guess_mime_type(source_ref, default="image/png"))
        )

        encoded = image.get("image_base64") or image.get("base64") or image.get("data")
        if not encoded:
            continue
        decoded = _decode_image_bytes(encoded, mime_type)
        if decoded is None:
            continue
        binary, mime_type = decoded

        source_ext = Path(source_ref).suffix.lower()
        if not source_ext:
            source_ext = _ext_from_mime(mime_type)
        if source_ext in {".jpeg", ".jpg"}:
            source_ext = ".jpg"

        file_name = f"image-p{page_index + 1:03d}-{img_idx + 1:03d}{source_ext}"
        asset_id = f"image-{page_index + 1:03d}-{img_idx + 1:03d}"
        alt_text = str(image.get("alt") or "") or None

        assets[asset_id] = AssetIR(
            asset_id=asset_id,
            type=AssetType.IMAGE,
            content=binary,
            mime_type=mime_type,
            source_page=page_index,
            file_name=file_name,
            alt_text=alt_text,
        )

        def _swap(match: re.Match[str]) -> str:
            current_target = match.group("target")
            if Path(current_target).name != Path(source_ref).name:
                return match.group(0)
            alt = match.group("alt") or alt_text or Path(source_ref).name
            return f"![{alt}](../images/{file_name})"

        markdown = _IMAGE_LINK_RE.sub(_swap, markdown)

        default_refs = {f"img-{img_idx}.jpeg", f"img-{img_idx}.jpg", f"img-{img_idx}.png"}
        for default_ref in default_refs:
            markdown = markdown.replace(
                f"![{default_ref}]({default_ref})",
                f"![{default_ref}](../images/{file_name})",
            )

    return markdown


def _replace_table_placeholders(
    markdown: str,
    tables: Any,
    page_index: int,
    assets: dict[str, AssetIR],
) -> str:
    if not isinstance(tables, list):
        return markdown

    for table_idx, table in enumerate(tables):
        if not isinstance(table, dict):
            continue
        html = table.get("html") or table.get("content")
        if not html:
            continue

        source_ref = str(table.get("id") or table.get("file_name") or f"tbl-{table_idx}.html")
        html_text = str(html)
        file_name = f"table-p{page_index + 1:03d}-{table_idx + 1:03d}.html"
        asset_id = f"table-{page_index + 1:03d}-{table_idx + 1:03d}"

        assets[asset_id] = AssetIR(
            asset_id=asset_id,
            type=AssetType.TABLE_HTML,
            content=html_text,
            mime_type="text/html",
            source_page=page_index,
            file_name=file_name,
        )

        markdown = markdown.replace(f"[{source_ref}]({source_ref})", f"\n\n{html_text}\n\n")

        def _swap(match: re.Match[str]) -> str:
            target = match.group("target")
            if Path(target).name != Path(source_ref).name:
                return match.group(0)
            return f"\n\n{html_text}\n\n"

        markdown = _TABLE_LINK_RE.sub(_swap, markdown)

    return markdown


def _remove_embedded_header_footer(markdown: str, header: str | None, footer: str | None) -> str:
    lines = markdown.splitlines()
    if header:
        while lines and lines[0].strip() == header.strip():
            lines.pop(0)
    if footer:
        while lines and lines[-1].strip() == footer.strip():
            lines.pop()
    return "\n".join(lines)


def _remove_repeated_edge_lines(pages: list[PageIR], threshold: float) -> None:
    if not pages:
        return

    top_counts: Counter[str] = Counter()
    bottom_counts: Counter[str] = Counter()
    page_lines: list[list[str]] = []

    for page in pages:
        lines = [line.strip() for line in page.markdown.splitlines() if line.strip()]
        page_lines.append(lines)
        if lines:
            top_counts[lines[0]] += 1
            bottom_counts[lines[-1]] += 1

    min_count = max(2, int(len(pages) * threshold + 0.999))
    repeated_top = {line for line, count in top_counts.items() if count >= min_count}
    repeated_bottom = {line for line, count in bottom_counts.items() if count >= min_count}

    for page, lines in zip(pages, page_lines):
        if not lines:
            continue
        first = lines[0]
        last = lines[-1]
        updated = page.markdown.splitlines()
        if first in repeated_top:
            while updated and updated[0].strip() == first:
                updated.pop(0)
        if updated and last in repeated_bottom:
            while updated and updated[-1].strip() == last:
                updated.pop()
        page.markdown = "\n".join(updated).strip()


def _ext_from_mime(mime_type: str) -> str:
    if mime_type == "image/jpeg":
        return ".jpg"
    if mime_type == "image/gif":
        return ".gif"
    if mime_type == "image/webp":
        return ".webp"
    if mime_type == "image/bmp":
        return ".bmp"
    if mime_type == "image/tiff":
        return ".tif"
    if mime_type == "image/svg+xml":
        return ".svg"
    return ".png"


def _normalize_mime_type(mime_type: str) -> str:
    lowered = mime_type.strip().lower()
    if lowered in {"image/jpg", "image/pjpeg"}:
        return "image/jpeg"
    if lowered == "image/x-png":
        return "image/png"
    return lowered or "image/png"


def _decode_image_bytes(encoded: Any, default_mime_type: str) -> tuple[bytes, str] | None:
    if not isinstance(encoded, str):
        return None

    cleaned = encoded.strip()
    mime_type = default_mime_type

    if cleaned.lower().startswith("data:") and "," in cleaned:
        metadata, _, payload = cleaned.partition(",")
        cleaned = payload.strip()

        media_spec = metadata[5:]
        media_type = media_spec.split(";", 1)[0].strip()
        if media_type:
            mime_type = _normalize_mime_type(media_type)

    cleaned = re.sub(r"\s+", "", cleaned)
    if not cleaned:
        return None

    padded = _pad_base64(cleaned)

    for altchars in (None, b"-_"):
        try:
            if altchars is None:
                binary = base64.b64decode(padded, validate=True)
            else:
                binary = base64.b64decode(padded, altchars=altchars, validate=True)
            if binary:
                return binary, mime_type
        except Exception:
            continue

    try:
        binary = base64.b64decode(padded, validate=False)
    except Exception:
        return None

    if not binary:
        return None
    return binary, mime_type


def _pad_base64(encoded: str) -> str:
    padding = (-len(encoded)) % 4
    if padding:
        return f"{encoded}{'=' * padding}"
    return encoded
