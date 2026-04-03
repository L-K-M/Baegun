# Baegun

Baegun is a Rust PDF to EPUB converter with two frontends that share the same core conversion engine:

- `baegun` CLI (Rust binary)
- `Baegun` desktop app (Tauri)

The converter uses Mistral OCR to extract structured markdown, images, and tables, then builds chapterized EPUB3 output.

OCR image payloads are accepted as either raw base64 strings or `data:*;base64,...` data URIs.

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

- `-o, --output PATH` (single-file `convert`)
- `-o, --output-dir PATH` (`convert-batch`; defaults to `INPUT_DIR`)
- `--recursive` (`convert-batch`; scans nested folders)
- `--api-key TEXT` (or `MISTRAL_API_KEY`)
- `--model TEXT` (default `mistral-ocr-latest`)
- `--table-format html|markdown`
- `--extract-header true|false`
- `--extract-footer true|false`
- `--include-images true|false`
- `--comic` (comic mode: one full-bleed image per page)
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

## Desktop App Notes

- Open `Settings...` to set your Mistral API key and conversion toggles (`Include images`, `Comic mode`, `Run epubcheck`).
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
