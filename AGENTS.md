# AGENTS.md

Baegun is now a Rust/Tauri codebase.

This document is the implementation handoff and must be kept in sync with the code.

When behavior changes, also update `README.md`.

## Product Direction

Build one shared conversion engine and expose it through two frontends:

- CLI: `baegun`
- Desktop app: Tauri (`src-tauri`) + SvelteKit (`src`) + `system7-ui`

No Python runtime or Tk GUI remains in the main architecture.

## Current Architecture

```text
PDF
 -> Mistral OCR (files upload + OCR endpoint)
 -> normalization (headers/footers, placeholders, images, tables)
 -> chapter segmentation (H1 boundaries)
 -> markdown -> HTML -> XHTML
 -> EPUB 3 zip packaging
 -> optional epubcheck validation
```

Shared modules live in `crates/baegun-core` and are used by both CLI and Tauri command handlers.

## Workspace Layout

```text
Cargo.toml              # workspace
crates/
  baegun-core/          # shared conversion pipeline
  baegun-cli/           # `baegun` binary
src/                    # SvelteKit app using system7-ui
src-tauri/              # Tauri host and command bridge
```

## Core API (`baegun-core`)

Main entry point:

- `convert_pdf_to_epub(cfg: &ConvertConfig) -> Result<ConvertSummary>`

Important types:

- `ConvertConfig`
- `TableFormat`
- `ConvertSummary`
- `ValidationResult`
- `BaegunError` (`ErrorKind` includes CLI-friendly exit mapping)

Key modules:

- `mistral.rs`: file upload + OCR request + retry + cleanup
- `cache.rs`: `.baegun-cache` SHA256-keyed OCR payload cache
- `normalize.rs`: placeholder replacement, chapterization, XHTML rendering
- `epub.rs`: EPUB packaging (zip + `content.opf` + `nav.xhtml`)
- `validate.rs`: optional `epubcheck` execution

## CLI Contract

Command:

```bash
baegun convert INPUT_PDF [OPTIONS]
```

Notable options:

- `-o, --output`
- `--api-key` (fallback `MISTRAL_API_KEY`)
- `--model` (default `mistral-ocr-latest`)
- `--table-format html|markdown`
- `--extract-header true|false`
- `--extract-footer true|false`
- `--include-images true|false`
- `--cache-dir`
- `--no-cache`
- `--validate`
- `--epubcheck-bin`
- `--debug-dir`
- `--keep-remote-file`
- `--fail-on-warn`
- `--quiet`
- `--verbose`

Exit code mapping:

- `2` bad args/config
- `3` API/auth/quota/network errors
- `4` OCR schema/parsing issues
- `5` EPUB build/write issues
- `6` validation failure
- `1` all other internal failures

## Desktop App Contract

Frontend: `src/routes/+page.svelte`.

Backend command:

- `convert_pdf(request: ConvertRequest) -> ConvertResponse`

The desktop app should remain a thin orchestrator over the shared `baegun-core` conversion logic.

Drag-and-drop is handled through Tauri window drag-drop events, while file/folder picking uses `@tauri-apps/plugin-dialog`.

## system7-ui Integration

Frontend imports:

- `@lkmc/system7-ui/styles.css` in `src/routes/+layout.svelte`
- Components from `@lkmc/system7-ui` in page/UI components

Dependency source is local sibling repo:

- `@lkmc/system7-ui`: `file:../system7-ui`

Reference apps for style/patterns:

- `../Lantenna`
- `../Obtainintosh`

## Mistral OCR Notes

Preferred flow:

1. `POST /v1/files` (`purpose=ocr`) with PDF file
2. `POST /v1/ocr` using uploaded `file_id`
3. Optional `DELETE /v1/files/{id}` cleanup

Request fields used:

- `model`
- `table_format`
- `extract_header`
- `extract_footer`
- `include_image_base64`

OCR payloads are expected to include `pages[]` with markdown + optional images/tables.

## EPUB Packaging Rules

Generated archive includes:

- `mimetype` (stored/uncompressed)
- `META-INF/container.xml`
- `OEBPS/content.opf`
- `OEBPS/nav.xhtml`
- `OEBPS/styles/book.css`
- `OEBPS/text/chapter-*.xhtml`
- `OEBPS/images/*`

## Operational Notes

- Keep core conversion behavior deterministic.
- Keep cache key tied to PDF bytes + OCR-relevant options + pipeline version.
- Keep frontend and CLI behavior aligned for the same config.
- Keep Tauri command payloads serializable and stable.

## Testing and Validation

Preferred checks (when toolchain is available):

```bash
cargo fmt --all
cargo check --workspace
npm run check
```

If `epubcheck` is installed, test one end-to-end conversion with `--validate`.

## Backlog Ideas

- Add progress events from backend to frontend (stream stage updates)
- Better heading normalization and chapter merging heuristics
- Better table placeholder recovery when OCR returns unusual shapes
- Add batch folder conversion command to CLI
- Add robust integration tests with fixed OCR fixtures in Rust
