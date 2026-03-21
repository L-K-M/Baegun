# Baegun

Baegun is a command-line and GUI Python tool that converts PDFs into high-fidelity EPUBs that retain images, formatting, tables, and structure, using MIstral's OCR service.

By default, Baegun also renders page 1 of the source PDF, uses it as the EPUB cover image, and attempts to set metadata, such as the author and book title.

## Install

```bash
pipx install -e .
```

Optional GUI dependencies:

```bash
pipx install -e ".[gui]"
```

or

```bash
brew install python-tk@3.13
/opt/homebrew/bin/python3.12 -m tkinter
pipx install --python /opt/homebrew/bin/python3.13 --editable '.[gui]'
```

Update:

```bash
pipx uninstall baegun
pipx install --editable '.[gui]'
```

or: 

```bash
pipx install --force --editable '.[gui]'
```

If drag-and-drop to the file list doesn't work:

```bash
pipx uninstall baegun
pipx install --python /opt/homebrew/bin/python3.12 --force --editable '.[gui]'
~/.local/pipx/venvs/baegun/bin/python -m tkinter
baegun-gui
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

### Comic Book Conversion

You can rapidly convert graphic PDFs to EPUBs without OCR using the `--comic` flag. Each page is rendered as an image.

```bash
baegun convert ./my_comic.pdf -o ./my_comic.epub --comic
```

**Bulk Comic Conversion**

Use the helper script `bulk_convert_and_delete_comic.sh` to batch process comics without needing an API key.

**This script will delete source PDFs.**

```bash
./bulk_convert_and_delete_comic.sh /input_dir /output_dir
```

### Desktop GUI (Optional)

Launch the desktop app with drag-and-drop queue support:

```bash
baegun-gui
```

GUI notes:

- Drop one or many PDFs into the queue panel, then click `Convert All`.
- Settings are saved to `~/.baegun_gui_settings.json` between sessions.
- `Comic Mode` disables API key requirements and uses image-render mode.

### Build a macOS App Bundle (Optional)

You can build a clickable `.app` bundle for the GUI:

```bash
./build_macos_app.sh
```

Useful options:

```bash
./build_macos_app.sh --icon ./assets/Baegun.icns --bundle-id com.example.baegun
./build_macos_app.sh --python /Library/Frameworks/Python.framework/Versions/3.12/bin/python3.12
```

The app is created at `dist/Baegun.app`.

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
- `--comic/--no-comic`
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
