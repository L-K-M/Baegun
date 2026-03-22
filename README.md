# Baegun

Baegun is a Rust PDF to EPUB converter with two frontends that share the same core conversion engine:

- `baegun` CLI (Rust binary)
- `Baegun` desktop app (Tauri)

The converter uses Mistral OCR to extract structured markdown, images, and tables, then builds chapterized EPUB3 output.

## Architecture

```text
PDF -> Mistral OCR -> normalization -> chapter split -> XHTML/CSS -> EPUB package
```

Workspace layout:

```text
crates/
  baegun-core/   shared conversion logic used by CLI and Tauri
  baegun-cli/    `baegun` binary
src/             SvelteKit frontend
src-tauri/       Tauri host + Rust command bridge
```

## Prerequisites

- Rust toolchain (`cargo`, `rustc`)
- Node 20+
- npm 10+
- Optional: `epubcheck` if you want `--validate`

## Install Dependencies

Frontend dependencies include `system7-ui` from the sibling path `../system7-ui`.

```bash
npm install
```

## Build and Run

CLI:

```bash
cargo run -p baegun-cli -- convert ./input.pdf -o ./output.epub --api-key "$MISTRAL_API_KEY"
```

Tauri desktop app (dev mode):

```bash
npm run tauri dev
```

Build CLI binary:

```bash
cargo build -p baegun-cli --release
```

## Quality Gates

Automatic checks are wired into both commits and builds:

- `npm run build` runs `npm run verify` first, which runs:
  - `npm run check`
  - `npm run test` (`cargo test --workspace`)
- Git pre-commit hook runs `npm run verify` automatically.

If you commit from IntelliJ, keep **Run Git hooks** enabled in the commit dialog.

## CLI Usage

```bash
baegun convert INPUT_PDF [OPTIONS]
```

Batch folder conversion:

```bash
baegun convert-batch INPUT_DIR [OPTIONS]
```

Common options:

- `-o, --output PATH` (single-file `convert`)
- `-o, --output-dir PATH` (`convert-batch`; defaults to `INPUT_DIR`)
- `--recursive` (`convert-batch`; scans nested folders)
- `--api-key TEXT` (or `MISTRAL_API_KEY`)
- `--model TEXT` (default `mistral-ocr-latest`)
- `--table-format html|markdown`
- `--extract-header true|false`
- `--extract-footer true|false`
- `--include-images true|false`
- `--cache-dir PATH`
- `--no-cache`
- `--validate`
- `--epubcheck-bin TEXT`
- `--debug-dir PATH`
- `--keep-remote-file`
- `--fail-on-warn`
- `--quiet`
- `--verbose`

`convert-batch` preserves relative folder structure for recursive runs (for example `input/nested/a.pdf` -> `output/nested/a.epub`).

## Notes

- PDF content is uploaded to Mistral OCR when cache is missed.
- OCR payloads are cached under `.baegun-cache` by default.
- Use `--no-cache` for sensitive documents.
- Uploaded OCR files are deleted by default unless `--keep-remote-file` is set.
- In the desktop app, API key entry and conversion toggles (`Include images`, `Run epubcheck`) live in `Settings...`.
- The desktop app Settings dialog includes a shortcut link to the Mistral API key page.
- After at least one successful conversion, the desktop app can open the selected target output folder.
- During desktop conversions, backend stage progress events are emitted and shown in the progress modal (input, OCR, normalize, package, optional validate, complete).
