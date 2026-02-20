from __future__ import annotations

import io
from pathlib import Path

from baegun.models import AssetIR, AssetType

try:
    import pypdfium2 as pdfium
except Exception:  # pragma: no cover - depends on runtime dependency availability
    pdfium = None  # type: ignore[assignment]


def extract_pdf_cover_asset(
    pdf_path: Path,
    *,
    max_width: int = 1600,
    jpeg_quality: int = 90,
) -> AssetIR | None:
    """Render the first PDF page as a cover image asset.

    Returns None when rendering is unavailable or the document has no pages.
    """
    if pdfium is None:
        return None

    document = pdfium.PdfDocument(str(pdf_path))
    if len(document) == 0:
        document.close()
        return None

    page = document[0]
    try:
        width, _height = page.get_size()
        scale = 1.0
        if width > 0 and width > max_width:
            scale = max_width / width

        bitmap = page.render(scale=scale)
        pil_image = bitmap.to_pil()

        if pil_image.mode not in {"RGB", "L"}:
            pil_image = pil_image.convert("RGB")

        output = io.BytesIO()
        pil_image.save(output, format="JPEG", quality=jpeg_quality, optimize=True)
        cover_bytes = output.getvalue()
    finally:
        page.close()
        document.close()

    if not cover_bytes:
        return None

    return AssetIR(
        asset_id="cover-image",
        type=AssetType.IMAGE,
        content=cover_bytes,
        mime_type="image/jpeg",
        source_page=0,
        file_name="cover.jpg",
        alt_text="Cover",
    )
