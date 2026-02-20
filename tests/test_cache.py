from __future__ import annotations

from pathlib import Path

from baegun.cache import OcrCache, compute_cache_key
from baegun.config import OcrConfig


def test_cache_roundtrip(tmp_path: Path, sample_pdf_path: Path, sample_payload: dict) -> None:
    cfg = OcrConfig(api_key="key")
    cache = OcrCache(tmp_path / "cache", enabled=True)
    key = compute_cache_key(sample_pdf_path, cfg, "0.1.0")

    assert cache.load_ocr_json(key) is None
    cache.save_ocr_json(key, sample_payload)

    loaded = cache.load_ocr_json(key)
    assert loaded is not None
    assert loaded["pages"][0]["index"] == 0


def test_cache_disabled(tmp_path: Path, sample_payload: dict) -> None:
    cache = OcrCache(tmp_path / "cache", enabled=False)
    assert cache.save_ocr_json("key", sample_payload) is None
    assert cache.load_ocr_json("key") is None
