from __future__ import annotations

import hashlib
import json
from pathlib import Path
from typing import Any

from baegun.config import OcrConfig
from baegun.utils import ensure_dir, sha256_file, stable_json_dumps


def compute_cache_key(pdf_path: Path, cfg: OcrConfig, pipeline_version: str) -> str:
    key_data = {
        "pdf_sha256": sha256_file(pdf_path),
        "model": cfg.model,
        "table_format": cfg.table_format,
        "extract_header": cfg.extract_header,
        "extract_footer": cfg.extract_footer,
        "include_images": cfg.include_images,
        "pipeline_version": pipeline_version,
    }
    return hashlib.sha256(stable_json_dumps(key_data).encode("utf-8")).hexdigest()


class OcrCache:
    def __init__(self, cache_dir: Path, enabled: bool = True) -> None:
        self.cache_dir = cache_dir
        self.enabled = enabled
        if enabled:
            ensure_dir(cache_dir)

    def _ocr_path(self, key: str) -> Path:
        return self.cache_dir / f"{key}.ocr.json"

    def load_ocr_json(self, key: str) -> dict[str, Any] | None:
        if not self.enabled:
            return None
        path = self._ocr_path(key)
        if not path.exists():
            return None
        try:
            return json.loads(path.read_text(encoding="utf-8"))
        except json.JSONDecodeError:
            return None

    def save_ocr_json(self, key: str, payload: dict[str, Any]) -> Path | None:
        if not self.enabled:
            return None
        ensure_dir(self.cache_dir)
        path = self._ocr_path(key)
        path.write_text(json.dumps(payload, indent=2, sort_keys=True), encoding="utf-8")
        return path
