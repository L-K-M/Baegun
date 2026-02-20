# AGENTS.md

This file is the full implementation handoff for building the `baegun` PDF -> EPUB CLI using Mistral OCR.

## Goal

Build a CLI tool that takes a local PDF and produces a high-quality EPUB with:

- OCR-backed extraction
- preserved headings and chapter structure
- extracted images
- retained tables
- reasonable preservation of callouts/boxes and reading order

Primary command target:

```bash
baegun convert input.pdf -o output.epub --api-key <KEY>
```

## Scope and Non-goals (MVP)

In scope:

- Single-PDF conversion via CLI
- Mistral OCR integration
- Chapterized EPUB3 output
- TOC generation
- Image extraction and embedding
- Table preservation
- Optional EPUB validation with epubcheck

Out of scope for MVP:

- GUI
- distributed processing
- perfect page-faithful print reproduction
- handwritten annotation reconstruction
- footnote cross-link perfection for all edge cases

## Chosen Approach (Option 2)

LLM OCR-first parser + deterministic EPUB post-processor.

Pipeline:

```text
PDF -> Mistral OCR JSON -> normalization -> structure inference -> XHTML/CSS -> EPUB package -> optional epubcheck
```

## Technical Stack

Python 3.11+

Recommended libraries:

- CLI: `typer`, `rich`
- OCR client: `mistralai`
- Models/config: `pydantic`
- Markdown to HTML: `markdown` (Python-Markdown)
- HTML post-processing: `beautifulsoup4`, `lxml`
- EPUB build: `ebooklib`
- Retry: `tenacity`
- Testing: `pytest`, `pytest-cov`

Optional:

- `orjson` for faster JSON
- `python-dotenv` if `.env` loading is desired

## Proposed Project Layout

```text
src/
  baegun/
    __init__.py
    cli.py
    config.py
    models.py
    mistral_client.py
    cache.py
    normalize.py
    structure.py
    render.py
    epub_builder.py
    validate.py
    utils.py
tests/
  fixtures/
  test_cli.py
  test_normalize.py
  test_structure.py
  test_render.py
  test_epub_builder.py
  test_pipeline_smoke.py
pyproject.toml
README.md
```

## CLI Contract

Command:

```bash
baegun convert INPUT_PDF [OPTIONS]
```

Options (MVP defaults):

- `-o, --output PATH` output epub path (default: same name as input with `.epub`)
- `--api-key TEXT` (fallback `MISTRAL_API_KEY`)
- `--model TEXT` default `mistral-ocr-latest`
- `--title TEXT` optional metadata override
- `--author TEXT` optional metadata
- `--language TEXT` default `en`
- `--publisher TEXT` optional metadata
- `--table-format [html|markdown]` default `html`
- `--extract-header/--no-extract-header` default `true`
- `--extract-footer/--no-extract-footer` default `true`
- `--include-images/--no-images` default `true`
- `--cache-dir PATH` default `.baegun-cache`
- `--no-cache` disable cache
- `--validate` run epubcheck if present
- `--epubcheck-bin TEXT` default `epubcheck`
- `--debug-dir PATH` dump intermediate artifacts
- `--keep-remote-file` do not delete uploaded file from Mistral
- `--fail-on-warn` treat validation warnings as error
- `--quiet` minimal output
- `--verbose` detailed output

Exit codes:

- `0` success
- `2` bad CLI arguments or missing input
- `3` API auth or quota error
- `4` OCR response/schema error
- `5` EPUB build error
- `6` validation failed (if `--validate`)
- `1` unexpected internal error

## Mistral API Notes (critical)

Use OCR endpoint with model `mistral-ocr-latest`.

Prefer local file upload flow:

1. Upload PDF via Files API with purpose `ocr`
2. Call OCR with `document={"type":"file", "file_id": <id>}` (or equivalent SDK shape)
3. Optional cleanup: delete uploaded file

OCR request fields to use:

- `table_format="html"` (best for EPUB fidelity)
- `extract_header=true`
- `extract_footer=true`
- `include_image_base64=true` (for embedded image assets)

OCR response essentials:

- `pages[]`
  - `index`
  - `markdown`
  - `images[]` (image id, bbox, base64 when enabled)
  - `tables[]` (when table format requested)
  - `header`, `footer` (if requested)
  - `dimensions`
- `usage_info`

Placeholder behavior:

- image placeholder example: `![img-0.jpeg](img-0.jpeg)`
- table placeholder example: `[tbl-3.html](tbl-3.html)`

Implementation must map placeholders to real extracted assets/content.

## Internal Data Model (IR)

Keep everything in a canonical intermediate representation before rendering EPUB.

Core objects:

- `DocumentIR`
  - metadata (`title`, `author`, `language`, `source_pdf_sha256`)
  - `pages: list[PageIR]`
  - `chapters: list[ChapterIR]`
  - `toc: list[TocEntryIR]`
  - `assets: dict[str, AssetIR]`
- `PageIR`
  - page index
  - normalized markdown
  - normalized html fragment
  - source headers/footers
- `AssetIR`
  - asset id
  - type (`image`, `table_html`)
  - content/path
  - mime type
  - source page
- `ChapterIR`
  - id/slug
  - title
  - html content
  - order
- `TocEntryIR`
  - title
  - href
  - level

## Module Responsibilities

### `config.py`

- Parse CLI options into typed config
- Resolve API key from option/env
- Validate paths and defaults

### `mistral_client.py`

- SDK wrapper around file upload + OCR call + optional remote delete
- Retry transient failures with exponential backoff
- Convert API exceptions into typed internal errors

### `cache.py`

- Cache key = SHA256(input PDF bytes + model + OCR options + tool version)
- Store raw OCR JSON and optional normalized IR snapshot
- Provide cache hit/miss telemetry

### `normalize.py`

- Merge per-page markdown while preserving page boundaries
- Replace image placeholders with local references
- Resolve table placeholders with HTML tables
- Remove duplicated headers/footers heuristically
- Clean OCR artifacts:
  - dehyphenate line-break splits
  - normalize whitespace
  - preserve code fences

### `structure.py`

- Build heading tree from markdown headings
- Normalize heading level jumps (no h1 -> h4 leaps)
- Split chapters by H1 primarily, fallback H2 with min content threshold
- Generate TOC entries from chapter boundaries and internal headings

### `render.py`

- Convert markdown to XHTML-compatible HTML
- Inject anchors for headings
- Preserve inline HTML tables
- Wrap in valid XHTML skeleton
- Apply semantic classes for callouts/boxes/figures/tables

### `epub_builder.py`

- Create EPUB3 package via EbookLib
- Add metadata (`dc:title`, `dc:creator`, language)
- Add stylesheet, chapter docs, images, nav doc
- Build spine and manifest deterministically

### `validate.py`

- Run `epubcheck` subprocess if requested
- Parse output summary
- Fail based on `--fail-on-warn` policy

### `cli.py`

- Orchestrate full pipeline
- Show progress stages
- Map exceptions to exit codes

## Key Algorithms

### 1) Header/footer dedupe

- If `extract_header/footer` used, compare extracted header/footer against nearby body text and remove duplicates.
- Also remove repeated top/bottom lines recurring on >= 60% of pages.

### 2) Heading normalization

- Trust markdown heading markers from OCR first.
- If heading levels jump by > 1, compress (e.g. h4 after h1 -> h2).
- If no headings found, infer candidates from short standalone lines with title-like patterns and spacing heuristics.

### 3) Chapter segmentation

Order of rules:

1. split on H1
2. if no H1, split on H2 where preceding chunk exceeds minimum size
3. if still no split, single chapter document

Safety:

- minimum chapter char count default 1200
- merge tiny trailing chapters into previous

### 4) Image extraction and replacement

- Decode base64 images from OCR response
- Save to `OEBPS/images/` using stable names
- Replace markdown image refs to relative XHTML-safe paths
- preserve alt text where available

### 5) Table retention

- Prefer provided table HTML when available
- Replace table placeholders with raw table HTML block
- fallback to markdown table rendering if table object missing

### 6) Callouts and boxes

- Map markdown blockquotes to `<aside class="callout">`
- Detect OCR patterns like `Note:`/`Warning:` lines and style with callout CSS class

## EPUB Output Design

Output structure in package:

- `mimetype`
- `META-INF/container.xml`
- `OEBPS/content.opf`
- `OEBPS/nav.xhtml`
- `OEBPS/styles/book.css`
- `OEBPS/text/chapter-001.xhtml`, ...
- `OEBPS/images/*`

CSS goals:

- sensible heading scale
- readable body typography
- table styling for overflow and borders
- figure captions
- callout boxes

## Determinism and Caching

- Always sort chapters/assets deterministically.
- Use stable slugs for chapter filenames.
- Cache OCR response by content hash to avoid repeat API cost.
- Include a `pipeline_version` in cache key so format changes invalidate cleanly.

## Error Handling Matrix

- Missing API key -> user-facing message + exit `2`
- 401/403 from Mistral -> exit `3`
- 429/5xx -> retry; final failure exit `3`
- OCR schema mismatch -> dump raw payload in debug dir + exit `4`
- EPUB build failure -> exit `5`
- epubcheck errors -> exit `6`

## Implementation Plan (ordered)

### Phase 0 - Bootstrap

1. Create package skeleton under `src/baegun`.
2. Add `pyproject.toml` with dependencies and console entrypoint `baegun=baegun.cli:app`.
3. Add initial `README.md` with quickstart.

### Phase 1 - CLI and config

1. Implement `convert` command and options.
2. Implement config resolution (CLI > env > defaults).
3. Add path validation and output naming.

### Phase 2 - Mistral integration

1. Implement file upload (purpose `ocr`).
2. Implement OCR request with selected params.
3. Implement optional remote file deletion.
4. Add retries and typed exceptions.

### Phase 3 - Cache layer

1. Implement PDF hash and cache key.
2. Save/load OCR JSON.
3. Respect `--no-cache`.

### Phase 4 - Normalization

1. Parse OCR pages into IR.
2. Extract images to temp assets.
3. Resolve table placeholders.
4. Header/footer dedupe and markdown cleanup.

### Phase 5 - Structure and rendering

1. Heading normalization.
2. Chapter segmentation + TOC creation.
3. Markdown->XHTML rendering with heading anchors.
4. Generate shared CSS.

### Phase 6 - EPUB build and validate

1. Build EPUB package with metadata/spine/nav/assets.
2. Write output file.
3. Optional epubcheck execution and policy handling.

### Phase 7 - Tests and hardening

1. Unit tests for normalization/structure/rendering.
2. API client tests with mocked SDK.
3. End-to-end smoke test using a frozen OCR fixture JSON.
4. Validate deterministic output checksums in test mode.

## Test Plan

Unit tests:

- placeholder replacement for images and tables
- header/footer dedupe logic
- chapter splitting heuristics
- toc generation correctness
- filename slug stability

Integration tests:

- build EPUB from fixture OCR JSON without hitting network
- assert `content.opf`, `nav.xhtml`, chapter files, images exist

Manual QA checklist:

1. Convert sample digital PDF with headings and images.
2. Open EPUB in Calibre and Apple Books.
3. Verify TOC links and chapter ordering.
4. Verify table rendering and image positions.
5. Run epubcheck and inspect warnings/errors.

## Definition of Done

Done when all are true:

- CLI converts a local PDF to `.epub` using Mistral key
- images and tables appear in resulting EPUB
- headings become coherent chapter structure and TOC
- output passes `epubcheck` in default sample test
- tests pass in CI/local
- README documents install and usage

## Security and Privacy Notes

- PDF content is sent to Mistral OCR API.
- Cache may contain extracted text and images; document this clearly.
- Provide `--no-cache` for sensitive workflows.
- By default, delete uploaded remote file after OCR unless `--keep-remote-file` is set.

## Suggested Initial Commands

If starting from empty repo:

```bash
python -m venv .venv
source .venv/bin/activate
pip install -U pip
pip install typer rich mistralai pydantic markdown beautifulsoup4 lxml ebooklib tenacity pytest pytest-cov
```

Run CLI locally:

```bash
export MISTRAL_API_KEY=...
baegun convert ./sample.pdf -o ./sample.epub --validate
```

## Suggested Function Signatures (guide)

```python
def convert_pdf_to_epub(cfg: ConvertConfig) -> Path: ...

def run_ocr(pdf_path: Path, cfg: OcrConfig) -> dict: ...

def normalize_ocr_payload(payload: dict, cfg: NormalizeConfig) -> DocumentIR: ...

def build_structure(doc: DocumentIR, cfg: StructureConfig) -> DocumentIR: ...

def render_chapters(doc: DocumentIR, cfg: RenderConfig) -> RenderedBook: ...

def build_epub(rendered: RenderedBook, cfg: EpubConfig) -> Path: ...

def run_epubcheck(epub_path: Path, bin_path: str) -> ValidationResult: ...
```

## Known Risks and Mitigations

- OCR heading inconsistency -> normalization and fallback splitting
- missing table payload fields -> fallback markdown table rendering
- large PDFs and high cost -> page selection option in future (`--pages`)
- malformed markdown -> sanitize through HTML parser before XHTML output

## Post-MVP Roadmap

- batch mode for folders
- optional parallel per-document processing
- better footnote/endnote linking
- improved math rendering path
- custom CSS themes
- optional open-source OCR fallback

## Immediate Next Task After Context Reset

Start with Phase 0 and Phase 1 only, then run a tiny smoke test with a mocked OCR payload before integrating real API calls.

This keeps feedback loop fast and avoids API cost early.
