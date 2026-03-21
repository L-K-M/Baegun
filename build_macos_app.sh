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
      --venv PATH         Build virtualenv path (default: .baegun-build-venv)
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
BUILD_VENV_DIR=".baegun-build-venv"
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
    --venv)
      BUILD_VENV_DIR="${2:-}"
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
PYTHON_RESOLVED="$(command -v "$PYTHON_BIN")"

if [[ -n "$ICON_PATH" ]]; then
  if [[ "$ICON_PATH" != /* ]]; then
    ICON_PATH="$PROJECT_ROOT/$ICON_PATH"
  fi
  if [[ ! -f "$ICON_PATH" ]]; then
    echo "Error: icon file not found: $ICON_PATH" >&2
    exit 1
  fi
fi

has_tkinter() {
  local python_bin="$1"
  "$python_bin" - <<'PY' >/dev/null 2>&1
import _tkinter  # noqa: F401
import tkinter   # noqa: F401
PY
}

is_externally_managed_python() {
  local python_bin="$1"
  "$python_bin" - <<'PY' >/dev/null 2>&1
import pathlib
import sys
import sysconfig

if sys.prefix != sys.base_prefix:
    raise SystemExit(1)

marker = pathlib.Path(sysconfig.get_path("stdlib")) / "EXTERNALLY-MANAGED"
raise SystemExit(0 if marker.exists() else 1)
PY
}

discover_tk_python() {
  local candidate
  local brew_prefix=""

  if command -v brew >/dev/null 2>&1; then
    brew_prefix="$(brew --prefix 2>/dev/null || true)"
    for candidate in "$brew_prefix"/bin/python3 "$brew_prefix"/bin/python3.[0-9] "$brew_prefix"/bin/python3.[0-9][0-9]; do
      [[ -x "$candidate" ]] || continue
      [[ "$candidate" == "$PYTHON_RESOLVED" ]] && continue
      if has_tkinter "$candidate"; then
        echo "$candidate"
        return 0
      fi
    done
  fi

  for candidate in /Library/Frameworks/Python.framework/Versions/*/bin/python3 /Library/Frameworks/Python.framework/Versions/*/bin/python3.[0-9] /Library/Frameworks/Python.framework/Versions/*/bin/python3.[0-9][0-9]; do
    [[ -x "$candidate" ]] || continue
    [[ "$candidate" == "$PYTHON_RESOLVED" ]] && continue
    if has_tkinter "$candidate"; then
      echo "$candidate"
      return 0
    fi
  done

  return 1
}

echo "Checking tkinter availability in $PYTHON_BIN ..."
if ! has_tkinter "$PYTHON_BIN"; then
  PYTHON_MM="$($PYTHON_BIN - <<'PY' 2>/dev/null || true
import sys
print(f"{sys.version_info.major}.{sys.version_info.minor}")
PY
)"
  [[ -n "$PYTHON_MM" ]] || PYTHON_MM="<major.minor>"

  BREW_PREFIX=""
  if command -v brew >/dev/null 2>&1; then
    BREW_PREFIX="$(brew --prefix 2>/dev/null || true)"
  fi

  ALT_TK_PYTHON="$(discover_tk_python || true)"

  cat >&2 <<EOF
Error: tkinter is missing for the selected Python: $PYTHON_BIN

Tkinter is required to package the Baegun GUI app.
EOF

  if [[ -n "$ALT_TK_PYTHON" ]]; then
    cat >&2 <<EOF

Detected another Python with tkinter:
  $ALT_TK_PYTHON

Try:
  ./build_macos_app.sh --python "$ALT_TK_PYTHON"
EOF
  fi

  if [[ -n "$BREW_PREFIX" ]]; then
    cat >&2 <<EOF

Homebrew fix (Python and python-tk minor versions must match):
  brew install "python@$PYTHON_MM" "python-tk@$PYTHON_MM"
  "$BREW_PREFIX/bin/python$PYTHON_MM" -m tkinter
  ./build_macos_app.sh --python "$BREW_PREFIX/bin/python$PYTHON_MM"
EOF
  fi

  cat >&2 <<'EOF'

Python.org fix:
  /Library/Frameworks/Python.framework/Versions/<version>/bin/python3 -m tkinter
  ./build_macos_app.sh --python /Library/Frameworks/Python.framework/Versions/<version>/bin/python3
EOF
  exit 1
fi

if [[ "$BUILD_VENV_DIR" != /* ]]; then
  BUILD_VENV_DIR="$PROJECT_ROOT/$BUILD_VENV_DIR"
fi

BUILD_PYTHON="$PYTHON_BIN"
EXTERNALLY_MANAGED=0
if is_externally_managed_python "$PYTHON_BIN"; then
  EXTERNALLY_MANAGED=1
  if [[ -x "$BUILD_VENV_DIR/bin/python" ]]; then
    echo "Detected externally-managed Python; reusing build virtualenv."
    BUILD_PYTHON="$BUILD_VENV_DIR/bin/python"
  elif [[ "$INSTALL_DEPS" == "1" ]]; then
    echo "Detected externally-managed Python; using isolated build virtualenv."
    if [[ ! -x "$BUILD_VENV_DIR/bin/python" ]]; then
      "$PYTHON_BIN" -m venv "$BUILD_VENV_DIR"
    fi
    BUILD_PYTHON="$BUILD_VENV_DIR/bin/python"
  fi
fi

echo "Using Python for build steps: $BUILD_PYTHON"

if [[ "$INSTALL_DEPS" == "1" ]]; then
  echo "Installing build dependencies (editable Baegun + GUI extras + PyInstaller) ..."
  "$BUILD_PYTHON" -m pip install -e "${PROJECT_ROOT}[gui]" pyinstaller
fi

if ! "$BUILD_PYTHON" -m PyInstaller --version >/dev/null 2>&1; then
  echo "Error: PyInstaller is not available for $BUILD_PYTHON" >&2
  if [[ "$EXTERNALLY_MANAGED" == "1" && "$INSTALL_DEPS" == "0" ]]; then
    cat >&2 <<EOF
Tip: the selected Python is externally managed and --no-install is set.
Run without --no-install so this script can create and populate:
  $BUILD_VENV_DIR
EOF
  else
    echo "Tip: run with install step enabled, or install manually:" >&2
    echo "  $BUILD_PYTHON -m pip install pyinstaller" >&2
  fi
  exit 1
fi

TKDND_PATH="$($BUILD_PYTHON - <<'PY'
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
"$BUILD_PYTHON" -m PyInstaller "${ARGS[@]}"

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
