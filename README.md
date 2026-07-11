# Baegun

Baegun is a Rust PDF/CBZ to EPUB converter with two frontends that share the same core conversion engine:

- `baegun` CLI (Rust binary)
- `Baegun` desktop app (Tauri)

**Latest release:** v<!-- version -->1.0.0<!-- /version --> · [Download](https://github.com/L-K-M/Baegun/releases/latest)

PDF conversion uses Mistral OCR to extract structured markdown, images, and tables, then builds chapterized EPUB3 output. The first extracted image from the first PDF page is marked as the EPUB cover image. PDF EPUB metadata is populated from explicit settings, cover/title-page OCR text, PDF metadata, and best-effort Mistral LLM generation from OCR content when needed.

CBZ conversion is fully local and does not require a Mistral API key. JPEG and PNG pages are byte-sniffed, fully decoded under pixel/allocation limits, naturally sorted (including nested paths and numeric names), and packaged as a fixed-layout EPUB with one viewport-sized XHTML spine item per image. `ComicInfo.xml` supplies title, writer, publisher, summary, language, deleted-page filtering, right-to-left reading direction, and front-cover selection when present; explicit metadata options take precedence. Only `Manga=YesAndRightToLeft` enables RTL. Source archive paths never become EPUB paths, and the EPUB identifier hashes the complete source archive.

OCR image payloads are accepted as either raw base64 strings or `data:*;base64,...` data URIs.
Metadata generation uses the configured Mistral API key and is skipped when enough metadata is already present or no API key is available.

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
baegun convert INPUT [OPTIONS]
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
| `--api-key TEXT` | Mistral API key for PDF OCR. Falls back to `MISTRAL_API_KEY`; not needed for CBZ. |
| `--model TEXT` | PDF-only Mistral OCR model name. Default: `mistral-ocr-latest`. |
| `--table-format html\|markdown` | PDF-only extracted table format. Default: `html`. |
| `--extract-header true\|false` | PDF-only OCR header extraction. Default: `true`. |
| `--extract-footer true\|false` | PDF-only OCR footer extraction. Default: `true`. |
| `--include-images true\|false` | Embed PDF OCR images. CBZ pages are always embedded. Default: `true`. |
| `--comic` | PDF-only comic mode. CBZ is already fixed-layout and rejects this option. |
| `--cache-dir PATH` | Directory for caching PDF OCR payloads. Default: `.baegun-cache`. |
| `--no-cache` | Skip the PDF OCR cache; always call the Mistral API for PDFs. |
| `--validate` | Run `epubcheck` on the output EPUB after packaging. |
| `--epubcheck-bin TEXT` | Path or command name for the epubcheck executable. Default: `epubcheck`. |
| `--debug-dir PATH` | Write intermediate PDF pipeline artifacts (OCR JSON and markdown) to this directory. |
| `--keep-remote-file` | PDF-only: do not delete the uploaded file from the Mistral files API after OCR. |
| `--delete-source` | Delete the source book after a successful conversion. Skipped for files that fail. |
| `--fail-on-warn` | Treat epubcheck warnings as errors (exit code 6). |
| `--quiet` | Suppress all non-error output. |
| `--verbose` | Print extra diagnostic information during conversion. |

Input type is selected from a case-insensitive `.pdf` or `.cbz` extension and validated against the file signature. `convert-batch` discovers both formats, preserves relative folder structure for recursive runs (for example `input/nested/a.cbz` -> `output/nested/a.epub`), and suffixes case-insensitive output-name collisions. A PDF only needs an API key on an OCR cache miss; CBZ never uses OCR or the cache.

CBZ archives are read in place without extraction. Baegun rejects encrypted entries, symlinks and other non-regular entries, unsafe paths, duplicate root `ComicInfo.xml`, malformed supported images, excessive entry/page counts, entries over 100 MiB actually expanded, archives over 2 GiB actually expanded, and observed expansion ratios over 1000:1. Reads are bounded independently of declared expanded sizes and accepted entries are consumed through EOF so ZIP CRC checks run. Images are capped at 100,000 pixels per axis, 100 million total pixels, and 512 MiB decoded output. Directories, `__MACOSX`, `.DS_Store`, `Thumbs.db`, and Apple resource forks are ignored.

## Desktop App Notes

- Use **Add Books** or drag and drop to queue PDF and CBZ files.
- Open `Settings...` to set your Mistral API key and conversion toggles (`Include images`, `PDF comic mode`, `Run epubcheck`). The API key is required only while pending PDFs exist.
- Desktop PDF conversions store OCR cache files in the operating system's app cache directory. CBZ conversion is local and emits no OCR/cache progress stage.
- When `Run epubcheck` is enabled, the desktop app resolves `epubcheck` from `PATH`, bundled resources, common Homebrew/MacPorts locations, or `EPUBCHECK_BIN`.
- `PDF comic mode` emits one image-first EPUB chapter per PDF page using OCR image payloads. CBZ always emits fixed-layout image pages.
- The queue supports per-file removal, and the conversion progress modal supports canceling the remaining queue after the current file finishes.

# Development

Run Tauri desktop app in dev mode:

```bash
npm run tauri dev
```
CLI:

```bash
cargo run -p baegun-cli -- convert ./input.pdf -o ./output.epub --api-key "$MISTRAL_API_KEY"
cargo run -p baegun-cli -- convert ./comic.cbz -o ./comic.epub
```

See [AGENTS.md](AGENTS.md) for more details.
