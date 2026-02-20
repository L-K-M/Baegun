from __future__ import annotations

import hashlib
import json
import mimetypes
import re
import unicodedata
from pathlib import Path
from typing import Any


class BaegunError(Exception):
    """Base exception for Baegun."""


class ConfigError(BaegunError):
    """Raised for invalid CLI/config inputs."""


class OcrApiError(BaegunError):
    """Raised when OCR API interaction fails."""


class OcrAuthError(OcrApiError):
    """Raised for OCR auth or permission failures."""


class OcrSchemaError(BaegunError):
    """Raised when OCR payload shape is invalid."""


class EpubBuildError(BaegunError):
    """Raised for EPUB build failures."""


class ValidationFailedError(BaegunError):
    """Raised when EPUB validation fails."""


def ensure_dir(path: Path) -> Path:
    path.mkdir(parents=True, exist_ok=True)
    return path


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(8192), b""):
            digest.update(chunk)
    return digest.hexdigest()


def stable_json_dumps(data: Any) -> str:
    return json.dumps(data, sort_keys=True, separators=(",", ":"), ensure_ascii=True)


def slugify(text: str, fallback: str = "chapter") -> str:
    normalized = unicodedata.normalize("NFKD", text)
    ascii_text = normalized.encode("ascii", "ignore").decode("ascii")
    slug = re.sub(r"[^a-zA-Z0-9]+", "-", ascii_text).strip("-").lower()
    return slug or fallback


def unique_slug(base_slug: str, used: set[str]) -> str:
    if base_slug not in used:
        used.add(base_slug)
        return base_slug
    counter = 2
    while True:
        candidate = f"{base_slug}-{counter}"
        if candidate not in used:
            used.add(candidate)
            return candidate
        counter += 1


def guess_mime_type(file_name: str, default: str = "application/octet-stream") -> str:
    mime_type, _ = mimetypes.guess_type(file_name)
    return mime_type or default


def write_json(path: Path, payload: Any) -> None:
    ensure_dir(path.parent)
    path.write_text(json.dumps(payload, indent=2, sort_keys=True), encoding="utf-8")
