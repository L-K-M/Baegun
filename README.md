# Baegun

Baegun is a CLI that converts local PDF files into EPUB3 books using Mistral OCR.

By default, Baegun also renders page 1 of the source PDF and uses it as the EPUB cover image.

## Install

```bash
pip install -e .
```

## Quickstart

```bash
export MISTRAL_API_KEY="your-key"
baegun convert ./input.pdf -o ./output.epub --validate
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
- `--fail-on-warn`
- `--quiet`
- `--verbose`

## Notes

- PDF content is sent to the Mistral API for OCR.
- Cache files may contain extracted text and image data.
- Use `--no-cache` for sensitive documents.
- Uploaded OCR files are deleted by default unless `--keep-remote-file` is set.
