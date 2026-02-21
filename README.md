# Baegun

Baegun is a command-line Python tool that converts PDFs into high-fidelity EPUBs that retain images, formatting, tables, and structure, using MIstral's OCR service.

By default, Baegun also renders page 1 of the source PDF, uses it as the EPUB cover image, and attempts to set metadata, such as the author and book title.

## Install

```bash
pipx install -e .
```

## Quickstart

### Regular Usage

```bash
export MISTRAL_API_KEY="your-key"
baegun convert ./input.pdf -o ./output.epub --validate
```

### Bulk Conversion

Use the helper script ```bulk_convert_and_delete.sh```.

**This script will delete source PDFs**

```bash
export MISTRAL_API_KEY="your-key"
./bulk_convert_and_delete.sh /input_dir /output_dir
```

## Command

```bash
baegun convert INPUT_PDF [OPTIONS]
```

Key options:

- `-o, --output PATH`
- `--api-key TEXT` (or `MISTRAL_API_KEY`)
- `--model TEXT` (default `mistral-ocr-latest`)
- `--table-format [html|markdown]`
- `--extract-header/--no-extract-header`
- `--extract-footer/--no-extract-footer`
- `--include-images/--no-images`
- `--cache-dir PATH`
- `--no-cache`
- `--validate`
- `--epubcheck-bin TEXT`
- `--debug-dir PATH`
- `--keep-remote-file`
- `--infer-metadata/--no-infer-metadata`
- `--metadata-model TEXT`
- `--metadata-max-pages INT`
- `--metadata-max-chars INT`
- `--output-from-metadata/--no-output-from-metadata`
- `--fail-on-warn`
- `--quiet`
- `--verbose`

## Notes

- PDF content is sent to the Mistral API for OCR.
- By default, Baegun makes an additional Mistral chat call to infer missing title/author/publisher metadata (disable with `--no-infer-metadata`).
- Use `--output-from-metadata` to name the output file from inferred title (ignored when `--output` is explicitly provided).
- Cache files may contain extracted text and image data.
- Use `--no-cache` for sensitive documents.
- Uploaded OCR files are deleted by default unless `--keep-remote-file` is set.
