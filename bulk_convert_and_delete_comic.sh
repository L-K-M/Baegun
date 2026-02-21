#!/usr/bin/env bash
set -euo pipefail

# Usage:
#   ./bulk_convert_and_delete_comic.sh "/path/to/pdf-folder" "/path/to/epub-destination"
#
# Optional env vars:
#   BAEGUN_BIN   (default: baegun)
#   DRY_RUN=1    (preview actions; do not convert/delete/copy)

usage() {
  echo "Usage: $0 INPUT_PDF_DIR DEST_EPUB_DIR"
}

next_available_path() {
  local path="$1"
  if [[ ! -e "$path" ]]; then
    printf '%s\n' "$path"
    return
  fi

  local dir name base ext n candidate
  dir="$(dirname "$path")"
  name="$(basename "$path")"
  base="$name"
  ext=""

  if [[ "$name" == *.* ]]; then
    base="${name%.*}"
    ext=".${name##*.}"
  fi

  n=2
  while true; do
    candidate="$dir/${base}-${n}${ext}"
    if [[ ! -e "$candidate" ]]; then
      printf '%s\n' "$candidate"
      return
    fi
    n=$((n + 1))
  done
}

if [[ $# -lt 2 || $# -gt 3 ]]; then
  usage
  exit 1
fi

INPUT_DIR="${1%/}"
DEST_DIR="${2%/}"
BAEGUN_BIN="${BAEGUN_BIN:-baegun}"
DRY_RUN="${DRY_RUN:-0}"

if [[ ! -d "$INPUT_DIR" ]]; then
  echo "Error: input directory not found: $INPUT_DIR"
  exit 1
fi

mkdir -p "$DEST_DIR"

if ! command -v "$BAEGUN_BIN" >/dev/null 2>&1; then
  echo "Error: baegun binary not found: $BAEGUN_BIN"
  echo "Tip: set BAEGUN_BIN, e.g. BAEGUN_BIN=\"/path/to/baegun\""
  exit 1
fi

shopt -s nullglob
pdfs=( "$INPUT_DIR"/*.pdf "$INPUT_DIR"/*.PDF )

if (( ${#pdfs[@]} == 0 )); then
  echo "No PDFs found in: $INPUT_DIR"
  exit 0
fi

success=0
failed=0

for pdf in "${pdfs[@]}"; do
  [[ -f "$pdf" ]] || continue
  echo "==> Processing: $(basename "$pdf")"

  if [[ "$DRY_RUN" == "1" ]]; then
    echo "    [DRY RUN] Would convert with metallic mode, copy EPUB to '$DEST_DIR', then delete PDF."
    continue
  fi

  marker="$(mktemp "${TMPDIR:-/tmp}/baegun-marker.XXXXXX")"
  touch "$marker"

  if ! NO_COLOR=1 COLUMNS=300 "$BAEGUN_BIN" convert \
      "$pdf" \
      --comic \
      --output-from-metadata \
      --quiet; then
    echo "    Conversion failed. Keeping PDF."
    rm -f "$marker"
    failed=$((failed + 1))
    continue
  fi

  generated_epub=""
  for candidate in "$INPUT_DIR"/*.epub "$INPUT_DIR"/*.EPUB; do
    [[ -f "$candidate" ]] || continue
    if [[ "$candidate" -nt "$marker" ]]; then
      if [[ -z "$generated_epub" || "$candidate" -nt "$generated_epub" ]]; then
        generated_epub="$candidate"
      fi
    fi
  done
  rm -f "$marker"

  if [[ -z "$generated_epub" || ! -f "$generated_epub" ]]; then
    echo "    Could not determine generated EPUB. Keeping PDF."
    failed=$((failed + 1))
    continue
  fi

  dest_epub="$(next_available_path "$DEST_DIR/$(basename "$generated_epub")")"

  if cp "$generated_epub" "$dest_epub"; then
    rm -f "$pdf"
    echo "    Copied EPUB -> $dest_epub"
    echo "    Deleted PDF  -> $pdf"
    success=$((success + 1))
  else
    echo "    Copy failed. Keeping PDF."
    failed=$((failed + 1))
  fi
done

echo
echo "Done. Success: $success | Failed: $failed"
