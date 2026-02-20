from __future__ import annotations

import html
import re

import markdown as md
from bs4 import BeautifulSoup

from baegun.config import RenderConfig
from baegun.models import DocumentIR, RenderedBook, RenderedChapter
from baegun.utils import slugify, unique_slug


def render_chapters(doc: DocumentIR, cfg: RenderConfig) -> RenderedBook:
    rendered_chapters: list[RenderedChapter] = []

    for chapter in sorted(doc.chapters, key=lambda item: item.order):
        html_fragment = md.markdown(
            chapter.markdown_content,
            extensions=["extra", "tables", "fenced_code", "sane_lists"],
            output_format="xhtml",
        )
        body_html = _postprocess_html(html_fragment)
        xhtml = _wrap_xhtml(
            title=chapter.title,
            language=cfg.language,
            body_html=body_html,
        )
        chapter.html_content = body_html
        rendered_chapters.append(
            RenderedChapter(
                id=chapter.id,
                title=chapter.title,
                order=chapter.order,
                file_name=chapter.file_name,
                xhtml=xhtml,
            )
        )

    return RenderedBook(
        metadata=doc.metadata,
        chapters=rendered_chapters,
        toc=doc.toc,
        assets=doc.assets,
        stylesheet=default_stylesheet(),
    )


def _postprocess_html(html_fragment: str) -> str:
    soup = BeautifulSoup(html_fragment, "lxml")

    _anchor_headings(soup)
    _convert_blockquotes_to_callouts(soup)
    _detect_note_warning_callouts(soup)

    root = soup.body if soup.body else soup
    contents = root.decode_contents(formatter="minimal")
    return contents.strip()


def _anchor_headings(soup: BeautifulSoup) -> None:
    used_slugs: set[str] = set()
    for tag in soup.find_all(re.compile(r"^h[1-6]$")):
        text = tag.get_text(" ", strip=True)
        if not text:
            continue
        base_slug = slugify(text, fallback="section")
        tag["id"] = unique_slug(base_slug, used_slugs)


def _convert_blockquotes_to_callouts(soup: BeautifulSoup) -> None:
    for blockquote in soup.find_all("blockquote"):
        aside = soup.new_tag("aside")
        aside["class"] = "callout quote"
        for child in list(blockquote.children):
            aside.append(child)
        blockquote.replace_with(aside)


def _detect_note_warning_callouts(soup: BeautifulSoup) -> None:
    prefixes = {
        "note": "note",
        "warning": "warning",
        "tip": "tip",
    }
    for para in soup.find_all("p"):
        text = para.get_text(" ", strip=True)
        lowered = text.lower()
        selected_prefix = None
        for prefix in prefixes:
            if lowered.startswith(f"{prefix}:"):
                selected_prefix = prefix
                break
        if not selected_prefix:
            continue

        content_text = text[len(selected_prefix) + 1 :].strip()
        aside = soup.new_tag("aside")
        aside["class"] = f"callout {prefixes[selected_prefix]}"

        label = soup.new_tag("strong")
        label.string = selected_prefix.capitalize()
        aside.append(label)

        if content_text:
            aside.append(" ")
            aside.append(content_text)

        para.replace_with(aside)


def _wrap_xhtml(*, title: str, language: str, body_html: str) -> str:
    escaped_title = html.escape(title)
    return (
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n"
        "<!DOCTYPE html>\n"
        f"<html xmlns=\"http://www.w3.org/1999/xhtml\" xml:lang=\"{language}\" lang=\"{language}\">\n"
        "  <head>\n"
        f"    <title>{escaped_title}</title>\n"
        "    <link rel=\"stylesheet\" type=\"text/css\" href=\"../styles/book.css\"/>\n"
        "  </head>\n"
        "  <body>\n"
        f"{body_html}\n"
        "  </body>\n"
        "</html>\n"
    )


def default_stylesheet() -> str:
    return """
body {
  font-family: "Georgia", "Times New Roman", serif;
  line-height: 1.55;
  margin: 0;
  padding: 0 1rem;
  color: #222;
}

h1, h2, h3, h4, h5, h6 {
  font-family: "Palatino Linotype", "Book Antiqua", serif;
  line-height: 1.2;
  margin: 1.2em 0 0.6em;
}

h1 { font-size: 1.9em; }
h2 { font-size: 1.5em; }
h3 { font-size: 1.3em; }

p {
  margin: 0 0 0.9em;
}

img {
  max-width: 100%;
  height: auto;
}

table {
  width: 100%;
  border-collapse: collapse;
  margin: 1em 0;
  display: block;
  overflow-x: auto;
}

th, td {
  border: 1px solid #999;
  padding: 0.3em 0.45em;
}

aside.callout {
  border-left: 4px solid #7a5c1e;
  background: #faf5e6;
  padding: 0.6em 0.8em;
  margin: 1em 0;
}

aside.callout.warning {
  border-left-color: #9d2b2b;
  background: #f9e8e8;
}

aside.callout.tip {
  border-left-color: #2d7d46;
  background: #e9f6ed;
}
""".strip()
