from __future__ import annotations

import re
import uuid
from pathlib import Path

from ebooklib import epub

from baegun.config import EpubConfig
from baegun.models import AssetType, RenderedBook
from baegun.utils import EpubBuildError, ensure_dir


def build_epub(rendered: RenderedBook, cfg: EpubConfig) -> Path:
    try:
        ensure_dir(cfg.output_path.parent)

        book = epub.EpubBook()
        book.set_identifier(
            str(uuid.uuid5(uuid.NAMESPACE_URL, rendered.metadata.source_pdf_sha256))
        )
        book.set_title(cfg.title or rendered.metadata.title)
        book.set_language(cfg.language or rendered.metadata.language)

        author = cfg.author or rendered.metadata.author
        if author:
            book.add_author(author)

        publisher = cfg.publisher or rendered.metadata.publisher
        if publisher:
            book.add_metadata("DC", "publisher", publisher)

        style_item = epub.EpubItem(
            uid="style-book",
            file_name="styles/book.css",
            media_type="text/css",
            content=rendered.stylesheet.encode("utf-8"),
        )
        book.add_item(style_item)

        chapter_items: list[epub.EpubHtml] = []
        for chapter in sorted(rendered.chapters, key=lambda item: item.order):
            chapter_item = epub.EpubHtml(
                title=chapter.title,
                file_name=f"text/{chapter.file_name}",
                lang=cfg.language,
            )
            chapter_item.content = _extract_body_fragment(chapter.xhtml)
            chapter_item.add_link(
                href="../styles/book.css",
                rel="stylesheet",
                type="text/css",
            )
            book.add_item(chapter_item)
            chapter_items.append(chapter_item)

        for asset in sorted(rendered.assets.values(), key=lambda item: item.file_name):
            if asset.type == AssetType.IMAGE:
                item_path = f"images/{asset.file_name}"
            else:
                item_path = f"tables/{asset.file_name}"
            book.add_item(
                epub.EpubItem(
                    uid=asset.asset_id,
                    file_name=item_path,
                    media_type=asset.mime_type,
                    content=asset.binary_content(),
                )
            )

        book.toc = tuple(chapter_items)
        book.spine = ["nav", *chapter_items]

        book.add_item(epub.EpubNcx())
        book.add_item(epub.EpubNav(file_name="nav.xhtml", title="Table of Contents"))

        epub.write_epub(str(cfg.output_path), book, {})
        return cfg.output_path
    except Exception as exc:
        raise EpubBuildError(f"Unable to build EPUB: {exc}") from exc


def _extract_body_fragment(xhtml: str) -> str:
    match = re.search(r"<body[^>]*>(?P<body>.*)</body>", xhtml, flags=re.IGNORECASE | re.DOTALL)
    if match:
        return match.group("body").strip()
    return xhtml
