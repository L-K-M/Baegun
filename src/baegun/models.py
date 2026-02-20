from __future__ import annotations

from enum import Enum

from pydantic import BaseModel, Field


class AssetType(str, Enum):
    IMAGE = "image"
    TABLE_HTML = "table_html"


class MetadataIR(BaseModel):
    title: str
    author: str | None = None
    language: str = "en"
    publisher: str | None = None
    source_pdf_sha256: str


class AssetIR(BaseModel):
    asset_id: str
    type: AssetType
    content: bytes | str
    mime_type: str
    source_page: int
    file_name: str
    alt_text: str | None = None

    def binary_content(self) -> bytes:
        if isinstance(self.content, bytes):
            return self.content
        return self.content.encode("utf-8")


class PageIR(BaseModel):
    index: int
    markdown: str
    html_fragment: str | None = None
    header: str | None = None
    footer: str | None = None


class ChapterIR(BaseModel):
    id: str
    title: str
    markdown_content: str
    html_content: str = ""
    order: int
    file_name: str


class TocEntryIR(BaseModel):
    title: str
    href: str
    level: int = 1


class DocumentIR(BaseModel):
    metadata: MetadataIR
    pages: list[PageIR] = Field(default_factory=list)
    chapters: list[ChapterIR] = Field(default_factory=list)
    toc: list[TocEntryIR] = Field(default_factory=list)
    assets: dict[str, AssetIR] = Field(default_factory=dict)
    full_markdown: str = ""


class RenderedChapter(BaseModel):
    id: str
    title: str
    order: int
    file_name: str
    xhtml: str


class RenderedBook(BaseModel):
    metadata: MetadataIR
    chapters: list[RenderedChapter]
    toc: list[TocEntryIR]
    assets: dict[str, AssetIR]
    stylesheet: str


class ValidationResult(BaseModel):
    ok: bool
    errors: int = 0
    warnings: int = 0
    output: str = ""


class InferredMetadata(BaseModel):
    title: str | None = None
    author: str | None = None
    publisher: str | None = None
