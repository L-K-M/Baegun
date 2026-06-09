# awesome.md — Baegun code review

A thorough review of the current codebase (core crate, CLI, Tauri bridge, Svelte frontend),
with bugs, general issues, missing features, and ideas. Entries marked **✅ implemented**
have a companion PR; everything else is documented here for future work.

---

## Bugs

### 1. CLI boolean options can never be turned off ✅ implemented
`--extract-header`, `--extract-footer`, and `--include-images` are declared as
`bool` fields with `default_value_t = true` in `crates/baegun-cli/src/main.rs`. With
clap's derive API, a plain `bool` field gets `ArgAction::SetTrue` — so these flags take
**no value** and can only set the option to `true`, which it already is. The README
documents `--include-images true|false`, but:

```
$ baegun convert in.pdf --include-images false
error: unexpected argument 'false' found
```

There is no way at all to disable header/footer extraction or body images from the CLI.
Fix: use `ArgAction::Set` with an explicit boolean value (`--include-images false`),
matching the documented contract.

### 2. Toggling comic mode silently re-buys OCR ✅ implemented
`compute_cache_key` in `crates/baegun-core/src/cache.rs` hashes `cfg.comic_mode` into the
OCR cache key, but comic mode does not change the OCR request payload in any way
(`include_image_base64` is always `true`, and the other request fields are unaffected).
Converting a PDF normally and then re-converting with `--comic` therefore misses the
cache and issues a fresh **paid** Mistral OCR call for byte-identical results.
Comic mode is a pure post-processing decision and should not participate in the key.
(Note: fixing this rolls all existing cache keys once, same as any version bump, since
the key also hashes `CARGO_PKG_VERSION`.)

### 3. Desktop: "Clear Finished" can't clear failed jobs ✅ implemented
In `src/routes/+page.svelte`, `clearFinished()` removes both `done` and `error` jobs,
but the button is `disabled={doneCount === 0}`. If every conversion in the queue failed,
the button stays disabled and the errored rows can never be cleared (except by removing
them one by one). The disabled condition should count `done` **and** `error` jobs.

### 4. Desktop: duplicate PDF basenames silently overwrite each other's output ✅ implemented
`deriveOutputPath()` maps every queued PDF to `<outputDir>/<basename>.epub`. Queueing
`a/report.pdf` and `b/report.pdf` (allowed — job identity is the full path) makes the
second conversion silently overwrite the first EPUB. The conversion loop should
uniquify output paths within a run (e.g. `report-2.epub`).

### 5. Image extraction assumes OCR image ids are globally unique
`extract_images` in `normalize.rs` keeps a document-wide `seen_placeholders` set and a
document-wide `id → path` map. If the OCR backend ever emits per-page image ids that
repeat across pages (e.g. `img-0.jpeg` on every page), images after the first page are
silently dropped, and every page's markdown placeholder resolves to page 1's image.
Defensive fix: key the map per `(page_index, id)` and replace placeholders per page.

### 6. One undecodable image aborts the whole conversion
A single corrupt base64 payload in the OCR response makes `extract_images` return an
error and the entire conversion fails — after the user has already paid for OCR. A bad
image should be skipped (leaving its placeholder stripped) rather than sinking an
otherwise fine 400-page conversion.

### 7. epubcheck issue counts are substring counts
`validate.rs` counts `raw_output.matches("WARNING")` / `matches("ERROR")`. Any message
that *mentions* those words (e.g. a quoted attribute value or a file named `ERROR.png`)
inflates the count. Parsing epubcheck's `--json` output (or its final
`Check finished with N errors` summary line) would be robust.

---

## General issues

### 1. API key stored in plaintext `localStorage`
The desktop app persists the Mistral key in `localStorage` under
`baegun.desktop.settings.v1`. That's a plaintext file in the WebView profile dir.
The OS keychain (e.g. `keyring` crate behind a small Tauri command) would be a better
home, and would also stop the key from round-tripping through frontend state on every
conversion.

### 2. Explicit `--language en` is indistinguishable from the default
`missing_metadata`/`resolve_language` treat `language == "en"` as "user didn't say".
A user who *explicitly* passes `--language en` for an English book with French PDF
metadata gets `fr` (PDF wins over the configured value). An `Option<String>` config
field (defaulting to `None` → "en") would preserve user intent.

### 3. Retry handling is partial
- Only the OCR call retries; the file upload (`POST /v1/files`) and the metadata chat
  call have no retry at all, though they hit the same rate limits.
- `Retry-After` headers from 429 responses are ignored in favor of fixed backoff.

### 4. Integration test re-implements the cache key
`tests/convert_integration.rs` contains a byte-for-byte copy of `compute_cache_key`.
Any change to the real key silently breaks the copy (the tests then fail on a cache
miss with a confusing "missing API key" error). Exposing the function (or a
test-only constructor) from `baegun-core` removes the drift hazard.

### 5. Doc drift: dependency release-age policy
`AGENTS.md` documents `min-release-age=3`, but `.npmrc` says `min-release-age=10`.
*(Fixed alongside the CLI flag PR.)* ✅ implemented

### 6. Metadata generation truncation
The chat request uses `max_tokens: 500`; a long description plus 8 subjects can
truncate the JSON mid-string. `parse_generated_metadata` then errors, the error is
swallowed (`ok()?`), and the result is not cached — so the cost is re-incurred on
every conversion of that book. Either raise the limit, or fall back to
partial-JSON salvage, and cache the empty result to stop repeat spend.

### 7. Unbounded cache growth
`.baegun-cache` (and the desktop app-cache dir) grows forever; OCR payloads with
base64 images can be tens of MB per book. There's no eviction, no size cap, no
tooling (see "cache subcommand" below).

### 8. `OcrPage.hyperlinks` is parsed and dropped
The model deserializes hyperlinks from the OCR payload but nothing consumes them, so
links present in the source PDF are lost in the EPUB.

---

## Missing features

1. **`--skip-existing` / overwrite control for batch** — re-running a batch over a
   folder currently re-converts (and rewrites) everything; cached OCR makes it cheap
   but not free (normalize/package/validate still run, file mtimes churn).
2. **Parallel batch conversion (`--jobs N`)** — conversions are independent;
   OCR latency dominates, so even modest parallelism would be a big win.
3. **Page ranges (`--pages 1-50`)** — useful for sampling a book before paying to
   OCR all of it.
4. **`baegun cache` subcommand** — `ls`/`stats`/`clear`, with sizes and source
   filenames; pairs with issue 7 above.
5. **Desktop: drop a folder of PDFs** — dropping a folder onto the queue currently
   does nothing (paths are filtered by `.pdf$`). The backend already has
   `is_directory`; a small `list_pdfs` command would let folder drops enqueue
   their contents recursively.
6. **Desktop: expose more engine options** — table format, header/footer toggles,
   title/author overrides, and output language all exist in the engine but have no UI.
7. **Cover override (`--cover image.png` / `--no-cover`)** — the first-image
   heuristic is good but sometimes wrong (publisher logo on page 1).
8. **Nested ToC** — `nav.xhtml` is flat H1-level entries; chapters' H2 headings
   already get anchor ids (`add_heading_anchors`), so a two-level ToC is nearly free.
9. **`dc:date` and series metadata** — publication year is often on the title page /
   in PDF metadata; Calibre-style series tags would delight series readers.
10. **`--version` for the CLI** — clap only emits it when asked; it isn't enabled. ✅ implemented

---

## Novel / cool / delightful / quirky ideas

1. **Fixed-layout comic EPUBs** — the OCR payload already includes per-page
   `dimensions` (parsed, unused!). Comic mode could emit a proper pre-paginated EPUB
   (`rendition:layout: pre-paginated`, per-page viewport from the real page size),
   which is what comic readers actually expect. Double-page spreads could be detected
   from aspect ratio and given `page-spread-left/right` properties.
2. **EPUB page-list from OCR page boundaries** — the pipeline knows exactly where
   each PDF page begins. Emitting an EPUB 3 `page-list` nav plus `epub:type="pagebreak"`
   markers would let reading systems show *original print page numbers* — wonderful for
   citing scanned books, and almost no converter does it.
3. **Reading-time summary** — after conversion, print
   `Done: 142k words, ≈ 8.1 hours of reading` (word count is a `split_whitespace`
   away). Cheap, charming, informative.
4. **Cost transparency** — `--dry-run` prints page count (from cache or a quick local
   PDF page scan) and estimated Mistral OCR cost before uploading anything.
5. **Cover thumbnails in the desktop queue** — the cover bytes are already extracted;
   a tiny base64 `<img>` next to each done row would make the queue feel alive.
6. **Preserve OCR hyperlinks** — wire `OcrPage.hyperlinks` into the markdown so
   URLs in the source survive into the EPUB (see issue 8).
7. **"Open in Books"** — on macOS, a per-row button that hands the finished EPUB to
   Apple Books (`open -a Books file.epub`) for instant gratification.
8. **epubcheck auto-fetch** — when validation is requested and no binary is found,
   offer to download the epubcheck release jar into the app's data dir (it's a single
   jar + `java -jar`), instead of failing.

---

*Review performed across `crates/baegun-core`, `crates/baegun-cli`, `src-tauri`, and
`src/`. The sibling repo Alan has its own `awesome.md` from the same review pass.*
