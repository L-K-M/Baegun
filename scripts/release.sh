#!/usr/bin/env bash
#
# Cuts a release by pushing a "v<version>" tag, which triggers
# .github/workflows/release.yml to build the Tauri bundles and publish the GitHub
# Release.
#
# IMPORTANT: tauri-action builds the app from the *committed* version (it reads
# src-tauri/tauri.conf.json) and only *names* the GitHub Release from the tag — it does
# NOT derive the bundle version from the tag. So the committed version and the tag must
# agree, or you'd ship a release named "v1.5.0" containing a 0.1.0 app. This script
# bumps the version everywhere it's declared (package.json + lock, tauri.conf.json,
# Cargo.toml), updates the README, commits, and tags — so they always match.
#
#   scripts/release.sh 1.3.0          # bump version everywhere + README, commit, tag v1.3.0
#   scripts/release.sh 1.3.0 --push   # …also push the commit + tag (CI then publishes)
#   scripts/release.sh                # tag the current version as-is
#
# Usage: scripts/release.sh [X.Y.Z] [--push]
set -euo pipefail

cd "$(dirname "$0")/.."

APP_NAME="Baegun"
TAURI_CONF="src-tauri/tauri.conf.json"
CARGO_TOMLS=("Cargo.toml")   # the crate(s) whose [package]/[workspace.package] version to bump

# --- Parse args (an optional version, and/or --push, in any order) ----------------
NEW_VERSION=""
PUSH=false
for arg in "$@"; do
  case "$arg" in
    --push) PUSH=true ;;
    -*)     echo "error: unknown option '$arg'" >&2; exit 1 ;;
    *)
      if [[ -n "$NEW_VERSION" ]]; then echo "error: version given twice" >&2; exit 1; fi
      NEW_VERSION="$arg"
      ;;
  esac
done

if [[ -n "$NEW_VERSION" && ! "$NEW_VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "error: version must be X.Y.Z (got '$NEW_VERSION')" >&2
  exit 1
fi

# --- Read the current version (package.json is the canonical source) --------------
CURRENT=$(node -e "console.log(JSON.parse(require('fs').readFileSync('package.json','utf8')).version)" 2>/dev/null || true)
if [[ -z "$CURRENT" ]]; then
  echo "error: could not read version from package.json" >&2
  exit 1
fi

TARGET="${NEW_VERSION:-$CURRENT}"
TAG="v${TARGET}"

# --- Pre-flight checks (before mutating anything) ---------------------------------
if [[ -n "$(git status --porcelain)" ]]; then
  echo "error: working tree has uncommitted changes — commit or stash them first." >&2
  exit 1
fi
if git rev-parse -q --verify "refs/tags/${TAG}" >/dev/null; then
  echo "error: tag ${TAG} already exists." >&2
  echo "       Pass a newer version, e.g. scripts/release.sh 1.3.0" >&2
  exit 1
fi

# --- Bump version everywhere + README, then commit --------------------------------
DID_COMMIT=false
if [[ -n "$NEW_VERSION" ]]; then
  [[ "$NEW_VERSION" != "$CURRENT" ]] && echo "Bumping version ${CURRENT} → ${NEW_VERSION}…"

  # package.json (+ package-lock.json). --allow-same-version makes re-runs safe.
  npm version "$NEW_VERSION" --no-git-tag-version --allow-same-version --ignore-scripts >/dev/null

  # src-tauri/tauri.conf.json — replace only the first "version" key, then verify via
  # Node (this is the value tauri-action ships as the bundle version).
  if [[ -f "$TAURI_CONF" ]]; then
    sed -i '' -E '1,/"version"/ s/("version"[[:space:]]*:[[:space:]]*")[^"]*"/\1'"${NEW_VERSION}"'"/' "$TAURI_CONF"
    GOT=$(node -e "console.log(JSON.parse(require('fs').readFileSync('${TAURI_CONF}','utf8')).version)" 2>/dev/null || true)
    if [[ "$GOT" != "$NEW_VERSION" ]]; then
      echo "error: failed to set version in ${TAURI_CONF} (got '${GOT}')." >&2
      git checkout -- . 2>/dev/null || true
      exit 1
    fi
  fi

  # Cargo.toml(s) — replace only the first (package/workspace) version line, never a
  # dependency's version.
  for cargo in "${CARGO_TOMLS[@]}"; do
    [[ -f "$cargo" ]] && sed -i '' -E '1,/^version = "/ s/^version = "[^"]*"/version = "'"${NEW_VERSION}"'"/' "$cargo"
  done

  # README version line (between the <!-- version --> markers).
  if [[ -f README.md ]]; then
    sed -i '' -E "s|(<!-- version -->)[^<]*(<!-- /version -->)|\1${NEW_VERSION}\2|" README.md
    if ! grep -qF "<!-- version -->${NEW_VERSION}<!-- /version -->" README.md; then
      echo "note: README.md has no <!-- version --> marker — left unchanged." >&2
    fi
  fi

  # Commit whatever the version change touched.
  if [[ -n "$(git status --porcelain)" ]]; then
    git commit -am "Bump version to ${NEW_VERSION}" >/dev/null
    DID_COMMIT=true
    echo "Committed version bump (package.json, ${TAURI_CONF}, Cargo.toml, README)."
  else
    echo "Version is already ${NEW_VERSION}; nothing to bump."
  fi
fi

# --- Tag --------------------------------------------------------------------------
git tag -a "${TAG}" -m "${APP_NAME} ${TARGET}"
echo "Created tag ${TAG}."

# --- Push (optional) — pushing the tag triggers the release workflow ---------------
if $PUSH; then
  git push origin HEAD
  git push origin "${TAG}"
  echo "Pushed branch + ${TAG}."
  echo "CI (release.yml) will now build the Tauri bundles and publish the GitHub Release for ${TAG}."
else
  echo "Local tag ${TAG} created (not pushed)."
  echo "Push it to trigger the release:  git push origin HEAD && git push origin ${TAG}"
  echo "Or undo:                         git tag -d ${TAG}$( $DID_COMMIT && echo " && git reset --hard HEAD~1" )"
fi
