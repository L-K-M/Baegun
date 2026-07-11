# Baegun

Baegun is a Rust PDF to EPUB converter with two frontends that share the same core conversion engine:

- `baegun` CLI (Rust binary)
- `Baegun` desktop app (Tauri)

**Latest release:** v<!-- version -->1.0.0<!-- /version --> · [Download](https://github.com/L-K-M/Baegun/releases/latest)

The converter uses Mistral OCR to extract structured markdown, images, and tables, then builds chapterized EPUB3 output. The first extracted image from the first PDF page is marked as the EPUB cover image. EPUB metadata is populated from explicit settings, cover/title-page OCR text, PDF metadata, and best-effort Mistral LLM generation from OCR content when needed.

OCR image payloads are accepted as either raw base64 strings or `data:*;base64,...` data URIs.
Metadata generation uses the configured Mistral API key and is skipped when enough metadata is already present or no API key is available.

Before cache or Mistral work begins, Baegun rejects an output path that identifies the input file, including noncanonical paths, symlinks, and hard links where the platform supports them. The identity check is repeated before temporary-file creation and immediately before publication. EPUBs are packaged into a securely created temporary file in the destination directory, optional `epubcheck` validation runs against that temporary EPUB, and successful conversion atomically replaces the destination on supported Unix and Windows platforms. Packaging, validation, or publication errors leave an existing destination unchanged and clean up the temporary file.

> [!IMPORTANT]
> LLM Disclosure: Much of this code base was written with the help of large language models — AI coding agents working from the [`AGENTS.md`](AGENTS.md) implementation handoff, which is kept in sync with the code.

## Prerequisites

- Rust toolchain (`cargo`, `rustc`)
- Node 20+
- npm 10+
- Optional: `epubcheck` if you want `--validate`

## Build and Run

Build GUI program:

```bash
npm install
npm run tauri build
```

Build CLI binary:

```bash
npm install
cargo build -p baegun-cli --release
cargo install --path crates/baegun-cli --force --bin baegun
```

This installs to `~/.cargo/bin/baegun` by default.

Alternatively, you can build and copy the binary manually:

```bash
cargo build -p baegun-cli --release
sudo install -m 755 target/release/baegun /usr/local/bin/baegun
```

## CLI Usage

```bash
baegun convert INPUT_PDF [OPTIONS]
```

Batch folder conversion:

```bash
baegun convert-batch INPUT_DIR [OPTIONS]
```

Common options:

| Option | Description |
|---|---|
| `-o, --output PATH` | Output EPUB path (single-file `convert`). Defaults to the input path with an `.epub` extension. |
| `-o, --output-dir PATH` | Output directory (`convert-batch`). Defaults to `INPUT_DIR`. |
| `--recursive` | Scan nested folders (`convert-batch` only). |
| `--api-key TEXT` | Mistral API key. Falls back to the `MISTRAL_API_KEY` environment variable. |
| `--model TEXT` | Mistral OCR model name. Default: `mistral-ocr-latest`. |
| `--table-format html\|markdown` | Format for extracted tables in the EPUB. Default: `html`. |
| `--extract-header true\|false` | Include page headers from OCR. Default: `true`. |
| `--extract-footer true\|false` | Include page footers from OCR. Default: `true`. |
| `--include-images true\|false` | Embed OCR images in the EPUB. Default: `true`. |
| `--comic` | Comic mode: render each page as a single full-bleed image chapter. Forces image extraction on. |
| `--cache-dir PATH` | Directory for caching OCR payloads. Default: `.baegun-cache`. |
| `--no-cache` | Skip the OCR cache; always call the Mistral API. |
| `--validate` | Run `epubcheck` on the temporary EPUB after packaging and before atomic publication. |
| `--epubcheck-bin TEXT` | Path or command name for the epubcheck executable. Default: `epubcheck`. |
| `--debug-dir PATH` | Write intermediate pipeline artifacts (OCR JSON, markdown, XHTML) to this directory. |
| `--keep-remote-file` | Do not delete the uploaded PDF from the Mistral files API after OCR. |
| `--delete-source` | Delete the source PDF after a successful conversion. Skipped for files that fail. |
| `--fail-on-warn` | Treat epubcheck warnings as errors (exit code 6). |
| `--quiet` | Suppress all non-error output. |
| `--verbose` | Print extra diagnostic information during conversion. |

`convert-batch` preserves relative folder structure for recursive runs (for example `input/nested/a.pdf` -> `output/nested/a.epub`).

## Desktop App Notes

- Open `Settings...` to set your Mistral API key and conversion toggles (`Include images`, `Comic mode`, `Run epubcheck`).
- Desktop conversions store OCR cache files in the operating system's app cache directory.
- When `Run epubcheck` is enabled, the desktop app resolves `epubcheck` from `PATH`, bundled resources, common Homebrew/MacPorts locations, or `EPUBCHECK_BIN`.
- `Comic mode` emits one image-first EPUB chapter per source page using OCR image payloads.
- The queue supports per-file removal, and the conversion progress modal supports canceling the remaining queue after the current file finishes.

# Development

Run Tauri desktop app in dev mode:

```bash
npm run tauri dev
```
CLI:

```bash
cargo run -p baegun-cli -- convert ./input.pdf -o ./output.epub --api-key "$MISTRAL_API_KEY"
```

See [AGENTS.md](AGENTS.md) for more details.
