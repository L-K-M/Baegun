#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Build a clickable macOS app bundle for Baegun GUI.

Usage:
  ./build_macos_app.sh [options]

Options:
  -p, --python PATH      Python interpreter to use (default: python3)
  -n, --name NAME        App name / bundle name (default: Baegun)
  -b, --bundle-id ID     Bundle identifier (default: com.baegun.app)
  -i, --icon PATH        Optional .icns icon path
      --no-install       Skip dependency install step
      --no-clean         Skip PyInstaller --clean
  -h, --help             Show this help

Examples:
  ./build_macos_app.sh
  ./build_macos_app.sh --python /Library/Frameworks/Python.framework/Versions/3.12/bin/python3.12
  ./build_macos_app.sh --icon ./assets/Baegun.icns --bundle-id com.example.baegun
EOF
}

PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

PYTHON_BIN="python3"
APP_NAME="Baegun"
BUNDLE_ID="com.baegun.app"
ICON_PATH=""
INSTALL_DEPS=1
CLEAN_BUILD=1

while (( "$#" )); do
  case "$1" in
    -p|--python)
      PYTHON_BIN="${2:-}"
      shift 2
      ;;
    -n|--name)
      APP_NAME="${2:-}"
      shift 2
      ;;
    -b|--bundle-id)
      BUNDLE_ID="${2:-}"
      shift 2
      ;;
    -i|--icon)
      ICON_PATH="${2:-}"
      shift 2
      ;;
    --no-install)
      INSTALL_DEPS=0
      shift
      ;;
    --no-clean)
      CLEAN_BUILD=0
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown argument: $1" >&2
      usage
      exit 1
      ;;
  esac
done

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "Error: this script only supports macOS." >&2
  exit 1
fi

if ! command -v "$PYTHON_BIN" >/dev/null 2>&1; then
  echo "Error: Python interpreter not found: $PYTHON_BIN" >&2
  exit 1
fi

if [[ -n "$ICON_PATH" ]]; then
  if [[ "$ICON_PATH" != /* ]]; then
    ICON_PATH="$PROJECT_ROOT/$ICON_PATH"
  fi
  if [[ ! -f "$ICON_PATH" ]]; then
    echo "Error: icon file not found: $ICON_PATH" >&2
    exit 1
  fi
fi

echo "Checking tkinter availability in $PYTHON_BIN ..."
if ! "$PYTHON_BIN" - <<'PY' >/dev/null 2>&1
import _tkinter  # noqa: F401
import tkinter   # noqa: F401
PY
then
  cat >&2 <<'EOF'
Error: tkinter is missing for the selected Python.

Use a Python build that includes Tk (for example Python.org Python),
or install matching Tk support for Homebrew Python first.
EOF
  exit 1
fi

if [[ "$INSTALL_DEPS" == "1" ]]; then
  echo "Installing build dependencies (editable Baegun + GUI extras + PyInstaller) ..."
  "$PYTHON_BIN" -m pip install -e "${PROJECT_ROOT}[gui]" pyinstaller
fi

if ! "$PYTHON_BIN" -m PyInstaller --version >/dev/null 2>&1; then
  echo "Error: PyInstaller is not available for $PYTHON_BIN" >&2
  echo "Tip: run with install step enabled, or install manually:" >&2
  echo "  $PYTHON_BIN -m pip install pyinstaller" >&2
  exit 1
fi

TKDND_PATH="$($PYTHON_BIN - <<'PY'
import pathlib

try:
    import tkinterdnd2
except Exception:
    raise SystemExit(0)

pkg = pathlib.Path(tkinterdnd2.__file__).resolve().parent
for candidate in (pkg / "tkdnd", pkg / "tkdnd2.9", pkg / "tkdnd2.8"):
    if candidate.exists():
        print(candidate)
        break
PY
)"
TKDND_PATH="${TKDND_PATH//$'\n'/}"

declare -a ARGS
ARGS=(
  --noconfirm
  --windowed
  --name "$APP_NAME"
  --osx-bundle-identifier "$BUNDLE_ID"
  --paths "$PROJECT_ROOT/src"
  --collect-data customtkinter
  --collect-data tkinterdnd2
  "$PROJECT_ROOT/src/baegun/gui.py"
)

if [[ "$CLEAN_BUILD" == "1" ]]; then
  ARGS+=(--clean)
fi

if [[ -n "$ICON_PATH" ]]; then
  ARGS+=(--icon "$ICON_PATH")
fi

if [[ -n "$TKDND_PATH" && -d "$TKDND_PATH" ]]; then
  ARGS+=(--add-data "${TKDND_PATH}:tkinterdnd2/tkdnd")
fi

echo "Building macOS app bundle ..."
"$PYTHON_BIN" -m PyInstaller "${ARGS[@]}"

APP_PATH="$PROJECT_ROOT/dist/${APP_NAME}.app"
if [[ ! -d "$APP_PATH" ]]; then
  echo "Error: build finished but app bundle not found: $APP_PATH" >&2
  exit 1
fi

echo
echo "Done. App bundle created:"
echo "  $APP_PATH"
echo
echo "Open it with:"
echo "  open \"$APP_PATH\""
