# CI/CD

Baegun ships GitHub Actions workflows for continuous integration and for cutting
desktop releases. CI runs on every pull request and on pushes to `main`; releases
are produced by pushing a version tag. Everything works with **no secrets
configured** — code signing and notarization are optional and only kick in when
the relevant secrets are present.

## Workflows
| Workflow | Trigger | Purpose |
| --- | --- | --- |
| `.github/workflows/ci.yml` | PRs + pushes to `main` | Type-check and build the SvelteKit frontend, then run `cargo fmt`/`clippy`/`test` across the workspace. |
| `.github/workflows/release.yml` | Pushing a `v*.*.*` tag | Build the Tauri desktop bundles for macOS, Linux, and Windows and attach them to a GitHub Release. |

## Continuous integration (`ci.yml`)

The CI workflow has two parallel jobs:

- **Frontend (check & build)** — installs npm dependencies with `npm ci`, runs
  `npm run check` (`svelte-kit sync` + `svelte-check`), then builds the frontend.
  Note that `npm run build` in this repo chains `npm run verify` (which runs
  `svelte-check` *and* `cargo test --workspace`), so the job calls `npx vite build`
  directly to keep this job to a pure frontend build — the Rust suite is covered by
  the `rust` job.
- **Rust (fmt, clippy, test)** — installs the Tauri Linux system dependencies
  (webview + GTK), builds the frontend into `build/` first (because
  `tauri::generate_context!` embeds it at compile time), then runs
  `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
  and `cargo test --workspace`. `Swatinem/rust-cache` caches the cargo build between
  runs.

Baegun is a Cargo workspace (`crates/baegun-core`, `crates/baegun-cli`,
`src-tauri`), so the cargo steps use `--workspace` to cover every member.

Since Baegun is built on **Tauri v2**, the Linux build needs the
`libwebkit2gtk-4.1-dev` package (the 4.1 series). Tauri v1 projects use `-4.0-dev`
instead.

### Running CI checks locally

```bash
# Frontend
npm ci
npm run check
npx vite build        # pure frontend build (npm run build also runs cargo test)

# Rust (run from the repository root)
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

On Linux you also need the Tauri system dependencies listed in `ci.yml`
(`libwebkit2gtk-4.1-dev`, `build-essential`, `libxdo-dev`, `libssl-dev`,
`libayatana-appindicator3-dev`, `librsvg2-dev`, etc.).

## Releases (`release.yml`)

To cut a release:

```
git tag v1.2.3
git push origin v1.2.3
```

Or use the helper, which bumps the version everywhere it's declared (`package.json`, `src-tauri/tauri.conf.json`, `Cargo.toml`) and the README, then creates and pushes the matching tag:

```
scripts/release.sh 1.2.3 --push
```

The workflow:

1. **Creates a draft GitHub Release** named `Baegun v1.2.3` with auto-generated
   release notes. Tags containing `-` (e.g. `v1.2.3-rc.1`) are marked as
   pre-releases.
2. **Builds the desktop bundles** with `tauri-apps/tauri-action@v0` across a
   four-way matrix and uploads each artifact to the draft release:
   - macOS Apple Silicon (`aarch64-apple-darwin`) — `.dmg` / `.app`
   - macOS Intel (`x86_64-apple-darwin`) — `.dmg` / `.app`
   - Linux (`ubuntu-22.04`) — `.deb`, `.rpm`, `.AppImage`
   - Windows (`windows-latest`) — `.msi` / `.exe`

   The `bundle.targets` in `src-tauri/tauri.conf.json` is `"all"`, so each runner
   produces every installer format native to its platform.
3. **Publishes the release** (flips it from draft to published) once all build
   jobs succeed. If a build fails, the release stays a draft so nothing
   half-built is published.

Builds are **unsigned** unless the optional signing secrets below are configured.
An unsigned macOS app still runs, but users will see Gatekeeper warnings;
add the Apple secrets later to enable notarization without editing the workflow.

## Secrets

All secrets are **optional** — the workflows build and release successfully
without any of them. They only enable code signing / notarization and Tauri
updater signing.

| Secret | Used for |
| --- | --- |
| `APPLE_CERTIFICATE` | Base64 of the Apple Developer ID signing certificate (.p12). |
| `APPLE_CERTIFICATE_PASSWORD` | Password for the .p12 certificate. |
| `APPLE_SIGNING_IDENTITY` | Signing identity name (e.g. `Developer ID Application: …`). |
| `APPLE_ID` | Apple ID used for notarization. |
| `APPLE_PASSWORD` | App-specific password for that Apple ID. |
| `APPLE_TEAM_ID` | Apple Developer Team ID. |
| `TAURI_SIGNING_PRIVATE_KEY` | Tauri updater private signing key. |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | Password for the Tauri updater signing key. |

`GITHUB_TOKEN` is provided automatically by GitHub Actions; you do not need to
create it.
