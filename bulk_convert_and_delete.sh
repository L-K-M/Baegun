#!/usr/bin/env bash
set -euo pipefail

# Usage:
#   ./bulk_pdf_to_epub.sh "/path/to/pdf-folder" "/path/to/epub-destination" [API_KEY]
#
# API key resolution order:
#   1) 3rd argument
#   2) MISTRAL_API_KEY env var
#
# Optional env vars:
#   BAEGUN_BIN   (default: baegun)
#   DRY_RUN=1    (preview actions; do not convert/delete/copy)

usage() {
  echo "Usage: $0 INPUT_PDF_DIR DEST_EPUB_DIR [API_KEY]"
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

# Lock management functions
acquire_lock() {
  local pdf="$1"
  local lockdir="${pdf}.lock"

  # Try to create lock directory atomically
  if mkdir "$lockdir" 2>/dev/null; then
    echo "$$" > "$lockdir/pid"
    echo "$lockdir"
    return 0
  fi

  # Lock exists - check if process is still alive
  if [[ -f "$lockdir/pid" ]]; then
    local pid
    pid=$(<"$lockdir/pid")
    if ! kill -0 "$pid" 2>/dev/null; then
      # Stale lock - process is dead
      echo "    Removing stale lock (PID $pid no longer exists)" >&2
      rm -rf "$lockdir"
      # Try again
      if mkdir "$lockdir" 2>/dev/null; then
        echo "$$" > "$lockdir/pid"
        echo "$lockdir"
        return 0
      fi
    fi
  fi

  return 1
}

release_lock() {
  local lockdir="$1"
  [[ -n "$lockdir" && -d "$lockdir" ]] && rm -rf "$lockdir"
}

# Track locks for cleanup on exit
declare -a ACTIVE_LOCKS=()

cleanup_locks() {
  for lock in "${ACTIVE_LOCKS[@]}"; do
    release_lock "$lock"
  done
}

trap cleanup_locks EXIT INT TERM

if [[ $# -lt 2 || $# -gt 3 ]]; then
  usage
  exit 1
fi

INPUT_DIR="${1%/}"
DEST_DIR="${2%/}"
API_KEY="${3:-${MISTRAL_API_KEY:-}}"
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

if [[ -z "$API_KEY" ]]; then
  echo "Error: missing API key (3rd arg or MISTRAL_API_KEY env var)."
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
skipped=0

for pdf in "${pdfs[@]}"; do
  [[ -f "$pdf" ]] || continue
  echo "==> Processing: $(basename "$pdf")"

  # Try to acquire lock
  if ! lockdir=$(acquire_lock "$pdf"); then
    echo "    Already being processed by another instance. Skipping."
    skipped=$((skipped + 1))
    continue
  fi
  ACTIVE_LOCKS+=("$lockdir")

  if [[ "$DRY_RUN" == "1" ]]; then
    echo "    [DRY RUN] Would convert with metadata filename, copy EPUB to '$DEST_DIR', then delete PDF."
    release_lock "$lockdir"
    ACTIVE_LOCKS=("${ACTIVE_LOCKS[@]/$lockdir}")
    continue
  fi

  marker="$(mktemp "${TMPDIR:-/tmp}/baegun-marker.XXXXXX")"
  touch "$marker"

  if ! NO_COLOR=1 COLUMNS=300 "$BAEGUN_BIN" convert \
      "$pdf" \
      --api-key "$API_KEY" \
      --output-from-metadata \
      --quiet; then
    echo "    Conversion failed. Keeping PDF."
    rm -f "$marker"
    release_lock "$lockdir"
    ACTIVE_LOCKS=("${ACTIVE_LOCKS[@]/$lockdir}")
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
    release_lock "$lockdir"
    ACTIVE_LOCKS=("${ACTIVE_LOCKS[@]/$lockdir}")
    failed=$((failed + 1))
    continue
  fi

  dest_epub="$(next_available_path "$DEST_DIR/$(basename "$generated_epub")")"

  if cp "$generated_epub" "$dest_epub"; then
    rm -f "$pdf"
    rm -f "$generated_epub"
    echo "    Copied EPUB -> $dest_epub"
    echo "    Deleted PDF  -> $pdf"
    echo "    Deleted EPUB -> $generated_epub"
    success=$((success + 1))
  else
    echo "    Copy failed. Keeping PDF."
    failed=$((failed + 1))
  fi

  release_lock "$lockdir"
  ACTIVE_LOCKS=("${ACTIVE_LOCKS[@]/$lockdir}")
done

echo
echo "Done. Success: $success | Failed: $failed | Skipped: $skipped"
