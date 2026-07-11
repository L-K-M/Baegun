# AGENTS.md

Baegun is now a Rust/Tauri codebase.

This document is the implementation handoff and must be kept in sync with the code.

When behavior changes, also update `README.md`.

## Product Direction

Build one shared conversion engine and expose it through two frontends:

- CLI: `baegun`
- Desktop app: Tauri (`src-tauri`) + SvelteKit (`src`) + `system7-ui`

No Python runtime or Tk GUI remains in the main architecture.

## Notes

- PDF content is uploaded to Mistral OCR when cache is missed.
- CBZ content is converted locally without Mistral, OCR, extraction, or cache use.
- OCR payloads are cached under `.baegun-cache` by default.
- Use `--no-cache` for sensitive documents.
- Uploaded OCR files are deleted by default unless `--keep-remote-file` is set.
- In the desktop app, API key entry and conversion toggles (`Include images`, `PDF comic mode`, `Run epubcheck`) live in `Settings...`; a key is required only when pending PDFs exist.
- The desktop app Settings dialog includes a shortcut link to the Mistral API key page.
- After at least one successful conversion, the desktop app can open the selected target output folder.
- During desktop conversions, backend stage progress events are emitted and shown in the progress modal. PDF uses input, OCR, normalize, package, optional validate, and complete; CBZ omits OCR/cache work.
- The desktop queue supports per-file removal, and the progress modal includes a cancel button that stops after the current in-flight file.
- EPUB output marks the first extracted image from the first PDF page as the cover image.
- EPUB metadata is resolved from explicit config, cover/title-page OCR text, PDF metadata, and best-effort Mistral LLM generation from OCR content when needed.
- CBZ metadata is parsed locally from one bounded root `ComicInfo.xml`; explicit config wins. Deleted page records are filtered, and only `Manga=YesAndRightToLeft` enables RTL.
- Desktop validation resolves `epubcheck` from `PATH`, bundled resources, common Homebrew/MacPorts locations, or `EPUBCHECK_BIN`.

## Quality Gates

Automatic checks are wired into both commits and builds:

- `npm run build` runs `npm run verify` first, which runs:
    - `npm run check`
    - `npm run test` (`cargo test --workspace`)
- Git pre-commit hook runs `npm run verify` automatically.

If you commit from IntelliJ, keep **Run Git hooks** enabled in the commit dialog.

## Architecture

```text
PDF -> Mistral OCR -> normalization -> chapter split -> reflowable EPUB
CBZ -> safe local ZIP adapter -> ordered image pages -> fixed-layout EPUB
```

Workspace layout:

```text
crates/
  baegun-core/   shared conversion logic used by CLI and Tauri
  baegun-cli/    `baegun` binary
src/             SvelteKit frontend
src-tauri/       Tauri host + Rust command bridge
```

## Current Architecture

```text
PDF
 -> Mistral OCR (files upload + OCR endpoint)
 -> normalization (headers/footers, placeholders, images, tables)
 -> chapter segmentation (H1 boundaries)
 -> markdown -> HTML -> XHTML
 -> EPUB 3 zip packaging
 -> optional epubcheck validation

CBZ
 -> extension/signature validation
 -> bounded in-place ZIP inspection and CRC-checked reads
 -> JPEG/PNG sniffing, validation, dimensions, and natural path ordering
 -> bounded local ComicInfo.xml metadata
 -> viewport XHTML page per image
 -> fixed-layout EPUB 3 zip packaging
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

- `convert_to_epub(cfg: &ConvertConfig) -> Result<ConvertSummary>` dispatches PDF/CBZ
- `convert_to_epub_with_progress(cfg, on_progress) -> Result<ConvertSummary>` for generic stage callbacks
- `convert_pdf_to_epub(cfg: &ConvertConfig) -> Result<ConvertSummary>`
- `convert_pdf_to_epub_with_progress(cfg, on_progress) -> Result<ConvertSummary>` compatibility wrapper
- `detect_source_format(path) -> Result<SourceFormat>` validates extension and signature

Important types:

- `ConvertConfig`
- `ConvertProgress`
- `ConvertStage`
- `TableFormat`
- `SourceFormat`
- `ConvertSummary`
- `ValidationResult`
- `BaegunError` (`ErrorKind` includes CLI-friendly exit mapping)

Key modules:

- `mistral.rs`: file upload + OCR request + retry + cleanup
- `metadata.rs`: PDF metadata extraction + metadata merge + optional LLM enrichment
- `cache.rs`: `.baegun-cache` SHA256-keyed OCR payload cache
- `cbz.rs`: safe local CBZ inspection, ComicInfo parsing, image validation/order, and fixed-layout rendering
- `normalize.rs`: placeholder replacement, chapterization, XHTML rendering
- `epub.rs`: EPUB packaging (zip + `content.opf` + `nav.xhtml`)
- `validate.rs`: optional `epubcheck` execution

## CLI Contract

Command:

```bash
baegun convert INPUT [OPTIONS]
baegun convert-batch INPUT_DIR [OPTIONS]
```

Notable options:

- `-o, --output`
- `-o, --output-dir` (batch)
- `--recursive` (batch)
- `--api-key` (fallback `MISTRAL_API_KEY`)
- `--model` (default `mistral-ocr-latest`)
- `--table-format html|markdown`
- `--extract-header true|false`
- `--extract-footer true|false`
- `--include-images true|false`
- `--comic`
- `--cache-dir`
- `--no-cache`
- `--validate`
- `--epubcheck-bin`
- `--debug-dir`
- `--keep-remote-file`
- `--delete-source`
- `--fail-on-warn`
- `--quiet`
- `--verbose`

`INPUT` accepts PDF or CBZ. Batch mode discovers both. CBZ requires no API key and does not use PDF OCR/cache options. `--comic` is PDF-only and is rejected for CBZ.

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

- `convert_book(request: ConvertRequest) -> ConvertResponse`
- `convert_pdf(request: ConvertRequest) -> ConvertResponse` compatibility alias

Progress event:

- `baegun://convert-progress` with stage payload (`reading_input`, `ocr`, `normalize`, `package_epub`, optional `validate`, `complete`)

The desktop app accepts PDF and CBZ books and should remain a thin orchestrator over the shared `baegun-core` conversion logic.

Drag-and-drop is handled through Tauri window drag-drop events, while file/folder picking uses `@tauri-apps/plugin-dialog`.

## system7-ui Integration

Frontend imports:

- `@lkmc/system7-ui/styles.css` in `src/routes/+layout.svelte`
- Components from `@lkmc/system7-ui` in page/UI components

Dependency source is the npm registry:

- `@lkmc/system7-ui`: `^0.2.1` (published from the `system7-ui` repo; for local
  library development use `npm link ../system7-ui` or a temporary `file:` override)

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
Image payloads can arrive as raw base64 or `data:*;base64,...` data URIs and should be decoded in either shape.
OCR image payloads are requested for all conversions so the first page image can be used as the EPUB cover, even when body image embedding is disabled.
When title/author/description/subjects are missing, cached OCR text can be sent to Mistral chat completions for best-effort EPUB metadata generation. Explicit config values take precedence, and cover/title-page OCR is preferred for title and author.

## EPUB Packaging Rules

Generated archive includes:

- `mimetype` (stored/uncompressed)
- `META-INF/container.xml`
- `OEBPS/content.opf`
- `OEBPS/nav.xhtml`
- `OEBPS/styles/book.css`
- `OEBPS/text/cover.xhtml` when a reflowable PDF cover image is available
- `OEBPS/text/chapter-*.xhtml`
- `OEBPS/text/page-*.xhtml` for fixed-layout CBZ pages
- `OEBPS/images/*`

When a cover image is available, mark its manifest item with `properties="cover-image"`.
For CBZ, use generated image/page names, store image ZIP entries without recompressing, emit `rendition:layout=pre-paginated`, `rendition:orientation=auto`, `rendition:spread=none`, `ltr`/`rtl` spine direction, viewport dimensions, and page-list navigation. The selected cover image remains its normal CBZ page and must not get a duplicate cover spine document.

## CBZ Safety Contract

- Open with `ZipArchive` and never extract.
- Limit archives to 10,000 entries and 2,000 pages. Enforce 100 MiB per actually expanded entry, 2 GiB cumulative actual expanded bytes, and 1000:1 observed expansion ratio during bounded reads, in addition to metadata preflight checks.
- Reject encrypted entries, symlinks/non-regular entries, absolute paths, traversal paths, malformed JPEG/PNG pages, and unsupported content presented with a supported image extension.
- Ignore directories, `__MACOSX`, `.DS_Store`, `Thumbs.db`, and `._` resource forks.
- Fully read accepted regular entries through EOF so ZIP CRC validation completes; rejected over-limit entries may stop at the detection byte.
- Sniff JPEG/PNG bytes rather than trusting archive extensions, then fully decode with 100,000-pixel per-axis, 100-million-pixel total, and 512-MiB decoded-allocation limits; use generated EPUB asset names only.
- Natural-sort relative archive paths deterministically, component by component, with numeric runs ordered by numeric value.
- Bound root `ComicInfo.xml` to 1 MiB, reject duplicates and DTDs/external entities, and honor supported declared legacy encodings.
- Hash the complete CBZ source stream for the EPUB identifier without loading another full source copy.

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

- Tune chapter merge/split thresholds with a broader OCR fixture corpus
- Add additional table-placeholder fixture variants from real OCR edge cases

## Dependency release-age policy

- npm installs in this repo are protected by `min-release-age=10` in `.npmrc`.
- If `npm install` fails because a package is too new, do not repeatedly retry.
- Use this order:
  1. wait until the package ages past 10 days,
  2. pin to an older known-good version,
  3. temporarily bypass with `npm install --min-release-age=0` for urgent fixes, then restore the policy.
