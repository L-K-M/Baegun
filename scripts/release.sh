#!/usr/bin/env bash
# Cuts a release: bumps the version, commits, tags "v<version>", and with --push
# pushes branch + tag — which triggers .github/workflows/release.yml to build the
# Tauri bundles and publish the GitHub Release. IMPORTANT: tauri-action builds the
# app from the *committed* version (it reads src-tauri/tauri.conf.json) and only
# *names* the GitHub Release from the tag — it does NOT derive the bundle version
# from the tag. So the committed version and the tag must agree, or you'd ship a
# release named "v1.5.0" containing a 0.1.0 app. The engine bumps the version
# everywhere it's declared (package.json + lock, tauri.conf.json, the workspace
# Cargo.toml + Cargo.lock), updates the README, commits, and tags — so they always
# match.
#
#   scripts/release.sh 1.3.0          # bump version everywhere + README, commit, tag v1.3.0
#   scripts/release.sh 1.3.0 --push   # …also push the commit + tag (CI then publishes)
#   scripts/release.sh                # tag the current version as-is
#
# Usage: scripts/release.sh [X.Y.Z] [--push]
# Shared engine: https://github.com/L-K-M/release-tool (this stub only sets config).
set -euo pipefail

export RELEASE_APP_NAME="Baegun"
export RELEASE_KIND="tauri"
export RELEASE_CARGO_TOMLS="Cargo.toml"
export RELEASE_CI_NOTE="CI (release.yml) will now build the Tauri bundles and publish the GitHub Release."
export RELEASE_INVOKED_AS="scripts/release.sh"

BIN="${LKM_RELEASE_BIN:-lkm-release}"
command -v "$BIN" >/dev/null 2>&1 || {
  echo "error: lkm-release not found — clone https://github.com/L-K-M/release-tool and run ./install.sh" >&2
  exit 1
}
exec "$BIN" "$@"
