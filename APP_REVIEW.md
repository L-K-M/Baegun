# Baegun — App Review

_Review date: 2026-06-06_

A code-level review of the Baegun PDF→EPUB converter (shared `baegun-core` engine,
`baegun` CLI, and Tauri desktop app). Overall this is a clean, well-structured
codebase: the shared-core / thin-frontend split is solid, error handling is
consistent and exit-code mapped, the cache key is sensibly versioned, and there is
a real integration test suite covering the conversion pipeline. The notes below are
ordered by severity, then by improvements and ideas.

---

## Issues (bugs / correctness)

### 1. Generated EPUB 3 is missing the required `dcterms:modified` meta — high
`crates/baegun-core/src/epub.rs` (`build_content_opf`)

EPUB 3 **requires** the package metadata to contain exactly one
`<meta property="dcterms:modified">YYYY-MM-DDThh:mm:ssZ</meta>` element. The
generated `content.opf` has `dc:identifier`, `dc:title`, `dc:language`, etc., but
no `dcterms:modified`. `epubcheck` reports this as a fatal error (RSC-005), so any
run with `--validate` / "Run epubcheck" enabled on otherwise-valid output will
fail. Notably `chrono` is already declared in `Cargo.toml` but is never used by any
crate — it looks like the timestamp was intended but never wired up.

**Fix:** emit `<meta property="dcterms:modified">{utc_now_iso8601}</meta>`. To keep
output deterministic (an explicit operational goal in `AGENTS.md`), derive the
timestamp from a stable source where possible (e.g. PDF mod-date or the source
hash mapped to a fixed value) or document that the modified date is intentionally
"now".

### 2. Generated chapter XHTML is not guaranteed to be well-formed — high
`crates/baegun-core/src/normalize.rs` (`render_markdown_to_html`, `replace_table_placeholders`)

`pulldown-cmark` passes raw inline/block HTML straight through, and OCR table
payloads (`table.html` / `table.content`) are injected verbatim into the markdown
before rendering. EPUB XHTML documents must be well-formed XML. If Mistral returns
table HTML (or any raw HTML in the markdown) with unclosed void elements
(`<br>`, `<img>`), unquoted attributes, or stray `&`, the resulting
`chapter-*.xhtml` becomes malformed and `epubcheck` will reject it. There is
currently no HTML→XHTML tidying/sanitization step.

**Fix:** run rendered fragments through an XML/XHTML sanitizer/normalizer (e.g.
`ammonia` for sanitization, or an HTML5→XHTML serializer) before wrapping. This is
the single biggest risk to producing reliably valid EPUBs.

### 3. LLM metadata generation fires on almost every conversion and is never cached — medium
`crates/baegun-core/src/metadata.rs` (`missing_metadata`, `resolve_book_metadata`)

The `missing_metadata` predicate is an unparenthesized mix of `&&` and `||`. Two
of its `||` arms depend only on PDF info:

```rust
|| pdf_metadata.description.is_none()
|| pdf_metadata.subjects.is_empty()
```

Most PDFs carry no `/Subject` or `/Keywords`, so these are almost always true,
which means the Mistral chat call (`mistral-small-latest`) is triggered on nearly
every conversion — even when title and author were already confidently extracted
from the cover or PDF info. Worse, the generated metadata is **not cached**: a
cached-OCR re-run (`cache_hit == true`) still re-issues the chat request every
time, so repeated conversions of the same file keep spending tokens.

**Fix:** (a) add parentheses / extract a named boolean per field so the intent is
explicit; (b) cache the resolved metadata alongside the OCR payload, keyed like the
OCR cache, so cache hits don't re-call the API; (c) consider only triggering the
LLM for the fields the product actually wants (title/author/description) rather
than any-field-missing.

### 4. `--verbose` is a no-op — low
`crates/baegun-cli/src/main.rs`, `crates/baegun-core/src/models.rs`

`verbose` is parsed by the CLI, plumbed through `ConvertConfig` and the Tauri
request, but `baegun-core` never reads it (no extra diagnostics are ever printed).
Either wire it up to emit diagnostics or drop it from the public surface so it
doesn't imply behavior it doesn't have. (`quiet`, by contrast, is honored by the
CLI.)

### 5. `resolve_language` can override an explicit `en` — low
`crates/baegun-core/src/metadata.rs` (`resolve_language`)

A user who explicitly passes `--language en` is treated identically to the default,
so a detected/inferred PDF or LLM language wins over the explicit choice. Any
non-`en` explicit value is respected. This is a surprising asymmetry; if the user
sets the language it should generally take precedence.

### 6. Validation counts are substring-based and brittle — low
`crates/baegun-core/src/validate.rs`

`warnings`/`errors` are computed via `raw_output.matches("WARNING"/"ERROR").count()`.
This miscounts when those words appear in file paths or message bodies, and
depends on `epubcheck`'s human-readable formatting. Prefer `epubcheck --json`
(or `-q` + machine output) and parse structured results, or at least anchor on
line prefixes.

---

## Improvements (robustness, security, DX)

### Security
- **API key is stored in `localStorage` in plaintext** (`src/routes/+page.svelte`,
  `SETTINGS_STORAGE_KEY`). For a desktop app this is recoverable from disk. Prefer
  the OS keychain via a Tauri secure-storage / stronghold plugin, or at minimum
  document that the key is stored unencrypted.
- The CSP in `tauri.conf.json` includes broad `connect-src` entries
  (`ws:`, `wss:`). Tighten to only what the renderer actually needs (it talks to
  the backend over IPC; the Mistral calls happen in Rust, not the webview).

### Reproducibility / onboarding
- **The build can't be cloned-and-built standalone.** `package.json` depends on
  `@lkmc/system7-ui: file:../system7-ui`, and `AGENTS.md` references sibling repos
  `../Lantenna` / `../Obtainintosh`. A fresh `npm install` fails without the
  sibling checkout. Consider publishing `system7-ui` to a registry (or a git
  dependency, or a vendored copy / submodule) and documenting the expected layout
  in `README.md`.
- **No CI.** Quality gates run only via the Husky pre-commit hook
  (`npm run verify`). A GitHub Actions workflow running `cargo fmt --check`,
  `cargo clippy`, `cargo test --workspace`, and `npm run check` on PRs would catch
  regressions that bypass local hooks. (There is a `session-start-hook` skill in
  this environment that's tailor-made for wiring this up.)
- `chrono` is a declared-but-unused workspace dependency — remove it or use it
  (see issue #1).

### Engine robustness
- **Header/footer stripping is exact-match only** (`strip_header_footer`): it only
  removes the header/footer when the page markdown starts/ends with the exact
  string. Minor OCR whitespace/punctuation differences defeat it. Consider a
  normalized/loose comparison.
- **Everything is in-memory**: the PDF, all decoded base64 images, and the full
  EPUB are held in memory simultaneously. Large/scanned books (especially comic
  mode, which embeds every page image) could spike memory. Streaming images
  directly into the zip writer would help.
- **`extract_pdf_metadata` regex-scans the entire raw PDF** as lossy UTF-8. This
  misses metadata in compressed/object-stream PDFs and can match false positives
  in binary content. A real PDF metadata library would be more reliable (though
  heavier).
- `METADATA_MODEL` (`mistral-small-latest`) is hardcoded; consider making it
  configurable alongside `--model`.

### Frontend / UX
- The desktop UI exposes only API key + output dir + 3 toggles. Title, author,
  publisher, and **language are hardcoded to `'en'`** in the request
  (`+page.svelte` `convertAll`), so the CLI is strictly more capable. Surfacing at
  least language (and optional title/author overrides) would close the gap.
- Errors from individual conversions are shown only as a row "Details" string;
  there's no way to copy/inspect the full error or the `epubcheck` raw output from
  the UI.
- `addPaths` dedupes by exact path string and uses the path as both `id` and
  `path`; re-adding the same file after it finished won't re-queue it. Minor, but
  worth a deliberate decision.

### Tests
- Coverage is good for `baegun-core` (chapterization, table placeholders, image
  decoding, metadata, end-to-end zip structure). Gaps worth filling:
  - a test asserting the OPF contains `dcterms:modified` (once #1 is fixed);
  - a malformed-table-HTML fixture asserting the chapter XHTML stays well-formed
    (once #2 is addressed);
  - comic-mode end-to-end (cover + per-page chapters, no duplicate cover);
  - the Tauri command layer (`epubcheck` resolution, request→config mapping) is
    untested.

---

## Ideas (features / direction)

- **Run `epubcheck` in-process or bundle it** so validation "just works" without a
  separate Java/`epubcheck` install — or surface a clear UI affordance when it's
  not found (the resolution logic already exists in `commands.rs`).
- **Local/offline OCR option** (e.g. Tesseract) as a fallback when no API key is
  set, for non-sensitive documents — the engine is already abstracted enough to
  add an OCR provider trait.
- **Per-file conversion options** in the desktop queue (comic vs. text, table
  format) rather than one global setting.
- **Resume / retry** failed batch items, and a persisted batch report (the CLI
  already aggregates totals — persist it as JSON).
- **Cover selection UI**: let the user pick which extracted image becomes the cover
  instead of always using the first page's first image.
- **TOC depth / nested nav**: `nav.xhtml` is currently flat (one entry per
  chapter). Building a nested TOC from heading levels would improve navigation for
  long books.
- **Cache management**: a command / UI to inspect and clear the OCR cache, show
  cache size, and (per `--no-cache`'s intent) a visible "sensitive document" mode.
- **Configurable metadata model & a `--no-llm-metadata` flag** for users who want
  zero extra API spend.

---

## What's done well
- Clean separation: a single deterministic `baegun-core` engine with both
  frontends as thin orchestrators, exactly as `AGENTS.md` prescribes.
- Thoughtful error taxonomy with CLI exit-code mapping (`ErrorKind::exit_code`).
- Cache key is correctly tied to PDF bytes + OCR-relevant options + pipeline
  version, so option changes and version bumps invalidate stale payloads.
- Robust base64 image decoding (raw, data-URI, and embedded-whitespace variants),
  all covered by tests.
- Sensible OCR retry/backoff with retryable-status detection and best-effort remote
  file cleanup.
- `.npmrc` `min-release-age` supply-chain guard and a documented dependency-age
  policy.
- Progress eventing is consistent between CLI (callback) and desktop (Tauri
  events), and the queue/cancel semantics are clearly documented.
