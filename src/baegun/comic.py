from __future__ import annotations

import io
from typing import TYPE_CHECKING
import uuid

from baegun.models import AssetIR, AssetType, ChapterIR, DocumentIR, MetadataIR, PageIR

try:
    import pypdfium2 as pdfium
except Exception:  # pragma: no cover
    pdfium = None  # type: ignore[assignment]

if TYPE_CHECKING:
    from baegun.config import ConvertConfig

def build_comic_document(cfg: ConvertConfig, source_sha256: str) -> DocumentIR:
    if pdfium is None:
        raise RuntimeError("pypdfium2 is required for comic book mode but could not be imported.")

    document = pdfium.PdfDocument(str(cfg.input_pdf))
    try:
        num_pages = len(document)
        assets: dict[str, AssetIR] = {}
        pages: list[PageIR] = []
        chapters: list[ChapterIR] = []
        
        max_width = 1600
        jpeg_quality = 90
        
        for i in range(num_pages):
            page = document[i]
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
                page_bytes = output.getvalue()
                
                asset_id = f"image-{i+1:03d}"
                file_name = f"page_{i+1:03d}.jpg"
                
                asset = AssetIR(
                    asset_id=asset_id,
                    type=AssetType.IMAGE,
                    content=page_bytes,
                    mime_type="image/jpeg",
                    source_page=i,
                    file_name=file_name,
                    alt_text=f"Page {i+1}",
                )
                assets[asset_id] = asset
                
                markdown_content = f"![Page {i+1}](../images/{file_name})"
                
                pages.append(PageIR(index=i, markdown=markdown_content))
                
                # In comic mode, we make each page a chapter to keep it simple and episodic,
                # or we could make one big chapter. Let's make each page its own chapter 
                # so it paginates well in standard epub readers.
                chapters.append(ChapterIR(
                    id=str(uuid.uuid4()),
                    title=f"Page {i+1}",
                    markdown_content=markdown_content,
                    order=i,
                    file_name=f"page_{i+1:03d}.xhtml",
                ))
            finally:
                page.close()
                
        resolved_title = cfg.epub.title or cfg.input_pdf.stem
        resolved_author = cfg.epub.author
        resolved_publisher = cfg.epub.publisher
        
        metadata = MetadataIR(
            title=resolved_title,
            author=resolved_author,
            language=cfg.epub.language,
            publisher=resolved_publisher,
            source_pdf_sha256=source_sha256,
        )
        
        return DocumentIR(
            metadata=metadata,
            pages=pages,
            chapters=chapters,
            toc=[],
            assets=assets,
            full_markdown="\\n\\n".join(p.markdown for p in pages),
        )
            
    finally:
        document.close()
