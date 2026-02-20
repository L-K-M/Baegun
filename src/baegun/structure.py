from __future__ import annotations

import re
from dataclasses import dataclass

from baegun.config import StructureConfig
from baegun.models import ChapterIR, DocumentIR, TocEntryIR
from baegun.utils import slugify, unique_slug


_HEADING_RE = re.compile(r"^(#{1,6})\s+(.*\S)\s*$")


@dataclass(frozen=True)
class Heading:
    line_index: int
    level: int
    text: str


def build_structure(doc: DocumentIR, cfg: StructureConfig) -> DocumentIR:
    markdown = doc.full_markdown.strip()
    if not markdown:
        chapter = ChapterIR(
            id="chapter-001",
            title=doc.metadata.title,
            markdown_content="",
            order=1,
            file_name="chapter-001.xhtml",
        )
        doc.chapters = [chapter]
        doc.toc = [TocEntryIR(title=chapter.title, href=f"text/{chapter.file_name}", level=1)]
        return doc

    normalized_markdown, headings = normalize_heading_levels(markdown)
    if not headings:
        normalized_markdown = _infer_heading_markdown(normalized_markdown, doc.metadata.title)
        normalized_markdown, headings = normalize_heading_levels(normalized_markdown)

    chapters = _segment_chapters(
        normalized_markdown,
        headings,
        min_chapter_chars=cfg.min_chapter_chars,
        fallback_title=doc.metadata.title,
    )

    if len(chapters) > 1 and len(chapters[-1].markdown_content.strip()) < max(300, cfg.min_chapter_chars // 3):
        previous = chapters[-2]
        previous.markdown_content = (
            f"{previous.markdown_content.strip()}\n\n{chapters[-1].markdown_content.strip()}".strip()
        )
        chapters = chapters[:-1]

    doc.chapters = chapters
    doc.toc = _build_toc(chapters)
    return doc


def normalize_heading_levels(markdown: str) -> tuple[str, list[Heading]]:
    lines = markdown.splitlines()
    discovered: list[Heading] = []

    for index, line in enumerate(lines):
        match = _HEADING_RE.match(line.strip())
        if not match:
            continue
        discovered.append(Heading(line_index=index, level=len(match.group(1)), text=match.group(2).strip()))

    if not discovered:
        return markdown, []

    normalized: list[Heading] = []
    previous_level = 0
    for heading in discovered:
        level = heading.level
        if previous_level == 0:
            normalized_level = 1 if level > 1 else level
        elif level > previous_level + 1:
            normalized_level = previous_level + 1
        else:
            normalized_level = level
        previous_level = normalized_level
        normalized.append(Heading(line_index=heading.line_index, level=normalized_level, text=heading.text))

    for heading in normalized:
        lines[heading.line_index] = f"{'#' * heading.level} {heading.text}"

    return "\n".join(lines), normalized


def _segment_chapters(
    markdown: str,
    headings: list[Heading],
    *,
    min_chapter_chars: int,
    fallback_title: str,
) -> list[ChapterIR]:
    lines = markdown.splitlines()

    h1_indices = [h.line_index for h in headings if h.level == 1]
    split_indices: list[int]

    if h1_indices:
        split_indices = h1_indices
    else:
        h2_indices = [h.line_index for h in headings if h.level == 2]
        split_indices = [0]
        cursor = 0
        for idx in h2_indices:
            if idx <= cursor:
                continue
            current_len = len("\n".join(lines[cursor:idx]).strip())
            if current_len >= min_chapter_chars:
                split_indices.append(idx)
                cursor = idx

    split_indices = sorted(set(split_indices))
    if not split_indices:
        split_indices = [0]
    if split_indices[0] != 0:
        split_indices.insert(0, 0)
    split_indices.append(len(lines))

    used_slugs: set[str] = set()
    chapters: list[ChapterIR] = []

    for order, (start, end) in enumerate(zip(split_indices[:-1], split_indices[1:]), start=1):
        chunk = "\n".join(lines[start:end]).strip()
        if not chunk:
            continue

        chunk_title = _first_heading_title(chunk) or fallback_title or f"Chapter {order}"
        chunk_slug = unique_slug(slugify(chunk_title, fallback=f"chapter-{order:03d}"), used_slugs)
        chapter_id = f"chapter-{order:03d}"
        file_name = f"{chapter_id}-{chunk_slug}.xhtml"

        chapters.append(
            ChapterIR(
                id=chapter_id,
                title=chunk_title,
                markdown_content=chunk,
                order=order,
                file_name=file_name,
            )
        )

    if not chapters:
        chapters = [
            ChapterIR(
                id="chapter-001",
                title=fallback_title,
                markdown_content=markdown,
                order=1,
                file_name="chapter-001.xhtml",
            )
        ]

    return chapters


def _build_toc(chapters: list[ChapterIR]) -> list[TocEntryIR]:
    toc: list[TocEntryIR] = []
    for chapter in chapters:
        chapter_href = f"text/{chapter.file_name}"
        toc.append(TocEntryIR(title=chapter.title, href=chapter_href, level=1))
        headings = _extract_headings(chapter.markdown_content)
        for heading in headings[1:]:
            if heading.level > 3:
                continue
            toc.append(
                TocEntryIR(
                    title=heading.text,
                    href=f"{chapter_href}#{slugify(heading.text, fallback='section')}",
                    level=heading.level,
                )
            )
    return toc


def _extract_headings(markdown: str) -> list[Heading]:
    headings: list[Heading] = []
    for index, line in enumerate(markdown.splitlines()):
        match = _HEADING_RE.match(line.strip())
        if match:
            headings.append(Heading(index, len(match.group(1)), match.group(2).strip()))
    return headings


def _first_heading_title(markdown: str) -> str | None:
    for line in markdown.splitlines():
        match = _HEADING_RE.match(line.strip())
        if match:
            return match.group(2).strip()
    return None


def _infer_heading_markdown(markdown: str, fallback_title: str) -> str:
    lines = markdown.splitlines()
    if not lines:
        return f"# {fallback_title}"

    for index, line in enumerate(lines):
        text = line.strip()
        if not text:
            continue
        looks_like_title = (
            len(text) <= 80
            and text[0].isupper()
            and not text.endswith((".", ";", ":", "?", "!"))
            and (index == 0 or not lines[index - 1].strip())
            and (index == len(lines) - 1 or not lines[index + 1].strip())
        )
        if looks_like_title:
            lines[index] = f"## {text}"
            return "\n".join(lines)

    return f"# {fallback_title}\n\n{markdown}".strip()
