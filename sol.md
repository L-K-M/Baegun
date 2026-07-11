# Sol Project Review

Review date: 2026-07-11

This is a read-only product and engineering review of Baegun 1.0.0 before any
remediation work. It covers the Rust conversion engine, CLI, Tauri bridge,
Svelte desktop app, CI/release configuration, tests, and product direction.

The project has a good central idea: one Rust conversion engine shared by a CLI
and a small desktop orchestrator. The current code is compact and generally easy
to follow. The largest weaknesses are at trust boundaries and failure boundaries:
source/output path safety, untrusted OCR markup, secret storage, remote cleanup,
archive/image resource limits, cancellation, and representing partial success.

## Review Method and Baseline

- Inspected all first-party Rust conversion, CLI, Tauri command, update, Svelte,
  configuration, CI, release, and current test files.
- Traced PDF bytes from filesystem input through cache/upload/OCR, metadata,
  normalization, EPUB packaging, validation, IPC, and desktop state.
- Reviewed output against EPUB 3/XHTML requirements and designed CBZ as an
  untrusted archive ingestion path.
- `git status` was clean on `main` at `b75fb37` when review began.
- `ANALYSIS.md` did not exist at review time.
- Local `npm run verify` could not start because dependencies were not installed
  (`svelte-kit: not found`). Rust checks could not start because `cargo` is not
  installed in this environment. These are environment limitations, not observed
  test failures. CI currently runs Svelte checks/build plus Rust fmt, clippy, and
  tests on Linux and macOS.

## Priority Summary

### P0: Data Loss and Trust Boundaries

1. Prevent input/output aliasing and package atomically.
2. Sanitize and serialize OCR content as valid, inert XHTML.
3. Stop storing the Mistral API key in WebView `localStorage`.
4. Treat Tauri IPC as untrusted; do not let the renderer choose executables and
   unrestricted internal paths.
5. Surface and retry remote PDF cleanup failures.

### P1: Correctness and Product Credibility

1. Implement CBZ as a local fixed-layout source adapter.
2. Replace the misleading PDF comic mode or rasterize actual PDF pages.
3. Make image identity page-scoped and validate image bytes/media types.
4. Add real in-flight cancellation and structured partial-success results.
5. Fix stale desktop batch summaries and preflight all output conflicts.
6. Add resource limits and reduce full-document memory duplication.

### P2: Quality, UX, and Operations

1. Improve metadata and chapter inference confidence.
2. Make validation bounded, structured, and visible.
3. Add accessible/responsive dialogs, progress, controls, and queue management.
4. Make releases version-safe and distribute the CLI.
5. Add cache management, privacy controls, inspection, and reader-oriented tools.

## Confirmed Core and CLI Findings

### SOL-001: Input and output can identify the same file

Severity: Critical

References: `crates/baegun-core/src/lib.rs:41-60`,
`crates/baegun-core/src/epub.rs:9-24`,
`crates/baegun-cli/src/main.rs:135-167`

`-o book.pdf` reads the source into memory and later truncates that path with
`File::create`. Relative/absolute aliases, symlinks, and hard links create the
same risk. With `--delete-source`, the newly written output can then be deleted.

Recommended change:

- Reject source/output filesystem identity, not merely equal strings.
- Write in the destination directory to a securely created temporary file.
- Finish and, when requested, validate the temporary EPUB before atomic rename.
- Preserve an existing destination after packaging or validation failure.
- Define no-clobber/replace/auto-rename behavior explicitly.

Tests: identical paths, relative aliases, symlink/hard-link aliases, disk/write
failure, validation failure, and existing destination preservation.

### SOL-002: Existing outputs are silently and non-atomically overwritten

Severity: High

References: `crates/baegun-core/src/epub.rs:19-79`,
`crates/baegun-cli/src/main.rs:237-256`,
`src/routes/+page.svelte:422-440`

Every conversion truncates the destination before packaging succeeds. Desktop
collision tracking covers only the current queue, not files already on disk.
Batch inputs such as `a.pdf` and `a.PDF` can map to the same output on a
case-sensitive source filesystem.

Recommended change: backend preflight with platform-aware collision detection;
default to auto-rename or skip in the desktop app and no-clobber in automation;
make replacement explicit.

### SOL-003: OCR/raw HTML is not safely converted to XHTML

Severity: High

References: `crates/baegun-core/src/normalize.rs:642-750`,
`crates/baegun-core/src/normalize.rs:780-795`,
`crates/baegun-core/src/epub.rs:121-155`

`pulldown-cmark` passes raw OCR HTML through. The repair pass closes selected void
elements and escapes some ampersands, but does not create a well-formed XML DOM.
For example, `&nbsp;` is accepted as an entity even though it is not predefined in
XML. Malformed tables, invalid XML characters, scripts, forms, event handlers,
remote resources, and script-bearing SVG are not safely handled. Required OPF
properties such as `scripted`, `svg`, `mathml`, and `remote-resources` are also not
derived.

Recommended change: parse as HTML5, apply an EPUB-oriented allowlist, strip active
and remote content by default, normalize URLs and XML characters, then serialize
well-formed XHTML. Derive manifest properties from the sanitized DOM.

### SOL-004: Image IDs are incorrectly document-global

Severity: High

References: `crates/baegun-core/src/normalize.rs:173-227`,
`crates/baegun-core/src/normalize.rs:387-391`

The global `seen_placeholders` and `image_map` assume OCR image IDs are unique
across the whole document. If multiple pages use `img-0.jpeg`, later images are
discarded and references can point at the first page's asset.

Recommended change: key references by `(page.index, image.id)` and replace
placeholders while processing that page. Generate independent internal names.

### SOL-005: Image failures and media types are handled unsafely

Severity: High

References: `crates/baegun-core/src/normalize.rs:194-225`,
`crates/baegun-core/src/normalize.rs:798-860`

- One malformed base64 image aborts the complete conversion.
- Missing image payloads can leave broken references.
- Media type is inferred from an untrusted filename, not data URI metadata or
  magic bytes.
- Unsupported/mislabeled resources can enter the EPUB.
- The fallback collision name is not itself checked for collision.

Recommended change: sniff bytes, collect dimensions, admit only supported and
safe formats, transcode under an explicit compatibility policy, remove every
unresolved placeholder, and return structured warnings for skippable assets.

### SOL-006: PDF comic mode is not a faithful page conversion

Severity: High

References: `crates/baegun-core/src/normalize.rs:108-151`,
`crates/baegun-core/src/epub.rs:121-155`, `README.md:95`

Mistral `images[]` are extracted image regions, not guaranteed page rasterizations.
The code picks the first image on a page, silently skips pages without a mapped
image, and succeeds if any page was emitted. It also emits reflowable EPUB markup,
not fixed-layout metadata. The first image is likely displayed twice: once in a
separate cover spine item and again as page one.

Recommended change: rasterize each PDF page locally for comic mode or remove the
claim. Require page-count equality. Add fixed-layout metadata, viewport dimensions,
reading direction, spread behavior, and reader smoke tests.

### SOL-007: Remote cleanup failures are silently discarded

Severity: High

References: `crates/baegun-core/src/mistral.rs:35-56`,
`crates/baegun-core/src/mistral.rs:241-255`

The product promises default deletion, but all delete failures are ignored and the
response body is not checked for a confirmed deletion. Uploads currently do not
request narrow visibility or expiry. Process termination can also strand a file.

Recommended change: request user-scoped visibility and short expiry when the API
supports it; retry bounded cleanup; parse confirmation; return cleanup status and
a prominent warning; securely persist pending IDs for retry after restart.

### SOL-008: Cache and debug artifacts need privacy, atomicity, and limits

Severity: High

References: `crates/baegun-core/src/cache.rs:27-76`,
`crates/baegun-core/src/cache.rs:83-123`,
`crates/baegun-core/src/lib.rs:107-158`

OCR text and base64 images are written with ambient permissions, direct writes,
no locking, no symlink defense, and no size/age policy. Debug filenames collide
between batch inputs. A cache write failure aborts conversion after paid OCR.

Recommended change: private directories/files, temporary write and atomic rename,
per-key locking, semantic validation/quarantine, cache writes as warnings by
default, per-input debug directories, and user-visible stats/clear/retention tools.

### SOL-009: Cache keys are ambiguously framed and aliases never expire

Severity: Medium

Reference: `crates/baegun-core/src/cache.rs:7-20`

Variable-length byte fields are concatenated without tags or lengths, so distinct
input/model boundaries can hash the same stream. A moving model alias such as
`mistral-ocr-latest` can retain stale results indefinitely.

Recommended change: hash a versioned canonical structure with explicit field names
and lengths; pin model versions or apply TTL/revalidation to moving aliases.

### SOL-010: Cached payload validation differs from fresh payload validation

Severity: Medium

References: `crates/baegun-core/src/cache.rs:27-47`,
`crates/baegun-core/src/mistral.rs:199-210`

Fresh OCR rejects an empty `pages` array; cache loading only checks JSON shape.
Semantically invalid entries become persistent failures, while corrupt JSON is
left in place. Use one validator for both paths and quarantine bad cache entries.

### SOL-011: Header/footer behavior contradicts its wording

Severity: Medium

References: `crates/baegun-core/src/mistral.rs:172-183`,
`crates/baegun-core/src/normalize.rs:261-285`, `README.md:73-74`

Enabling extraction asks Mistral to place running matter in dedicated fields; the
normalizer then strips matching values and never inserts the dedicated fields.
This behaves more like "detect and remove" than "include." Separate OCR detection
from output retention and document both controls.

### SOL-012: Metadata precedence can override explicit English

Severity: Medium

References: `crates/baegun-core/src/metadata.rs:46-120`,
`crates/baegun-core/src/metadata.rs:151-199`

Configured `en` is treated as an unset sentinel and may be overridden. A weak
first-cover-line title heuristic outranks PDF metadata. Name inference is biased
toward ASCII capitalization, and PDF metadata extraction is a binary regex rather
than a PDF-aware parser.

Recommended change: model explicit language as an option, track metadata provenance
and confidence, prefer reliable PDF/XMP data over weak cover guesses, and use a
maintained PDF parser with bounded resource use.

### SOL-013: Chapter heuristics discard useful boundaries

Severity: Medium

References: `crates/baegun-core/src/normalize.rs:433-499`,
`crates/baegun-core/src/normalize.rs:569-595`

One H1 anywhere causes all H2 candidates to be ignored, so an H1 book title plus
H2 chapters can become one chapter. A final chapter under 400 characters is
silently merged, including genuine epilogues or acknowledgements.

Recommended change: score heading levels by repetition/position/density and never
merge an explicit heading solely because its body is short.

### SOL-014: Input/configuration validation occurs after too much trust

Severity: Medium

References: `crates/baegun-core/src/lib.rs:41-60`,
`crates/baegun-core/src/models.rs:43-67`

The engine checks only existence and regular-file status before upload. Validate
PDF signature/size, source/output distinction, nonempty model, BCP-47 language,
output policy, and XML-valid metadata before paid work.

### SOL-015: Retry and cancellation policies are incomplete

Severity: Medium

References: `crates/baegun-core/src/mistral.rs:94-163`,
`crates/baegun-core/src/mistral.rs:185-238`,
`crates/baegun-core/src/validate.rs:6-28`

Only OCR retries. `Retry-After` and jitter are ignored. A 300-second timeout is per
attempt. No cancellation token reaches upload, retry sleep, OCR, normalization,
packaging, or `epubcheck`; desktop cancellation only stops before the next file.

Recommended change: use one deadline-aware, jittered, `Retry-After`-aware policy
and a job cancellation token propagated through all stages and child processes.

### SOL-016: Peak memory multiplies document size

Severity: Medium

References: `crates/baegun-core/src/lib.rs:55-64`,
`crates/baegun-core/src/mistral.rs:126-135`,
`crates/baegun-core/src/normalize.rs:20-53`,
`crates/baegun-core/src/models.rs:132-160`

The complete PDF is loaded, cloned into multipart, retained beside OCR base64,
cloned pages, decoded images, markdown, XHTML, and final ZIP writes. Large scans
can exhaust memory. Stream hashing/upload where possible, avoid cloning pages,
spool or stream assets, release base64 promptly, and enforce byte/page/pixel limits.

### SOL-017: Validation can hang and reports unreliable counts

Severity: High

Reference: `crates/baegun-core/src/validate.rs:6-55`

`Command::output` has no timeout or cancellation and buffers unbounded output.
Counts are substring occurrences of uppercase `WARNING` and `ERROR`. A validation
failure happens after final output publication but is returned as total conversion
failure.

Recommended change: validate the temporary EPUB; consume structured epubcheck
output; cap retained diagnostics; enforce timeout/cancellation; represent packaging
success and validation status separately.

### SOL-018: Recursive CLI batches follow directory symlinks

Severity: High

Reference: `crates/baegun-cli/src/main.rs:399-439`

`path.is_dir()` follows symlinks, with no visited identity set or root boundary.
Cycles can recurse until failure; external trees can be uploaded and, with
`--delete-source`, deleted. Do not follow directory symlinks by default. If made
optional, enforce root boundaries and visited filesystem identities.

### SOL-019: Several documented CLI behaviors are incomplete

Severity: Medium

References: `crates/baegun-cli/src/main.rs:109-116`,
`crates/baegun-cli/src/main.rs:265-317`

- `--verbose` is parsed and copied but never used.
- `--quiet --verbose` is not rejected.
- Batch source deletion failures can still exit successfully.
- Cache-only conversion is blocked by frontends even though the core needs a key
  only on cache miss.
- Release artifacts do not currently include first-class CLI binaries.

## Confirmed Tauri and Desktop Findings

### SOL-020: Renderer IPC exposes excessive authority

Severity: Critical defense-in-depth issue

References: `src-tauri/src/commands.rs:12-36`,
`src-tauri/src/commands.rs:60-120`,
`crates/baegun-core/src/validate.rs:6-19`

The renderer can supply arbitrary input/output/cache/debug paths and an arbitrary
`epubcheck_bin`, which is directly executed. A compromised WebView could read and
upload user-readable files, overwrite files, place sensitive artifacts, and run a
chosen local executable.

Recommended change: remove desktop-internal fields from renderer requests, resolve
trusted tools entirely in Rust, canonicalize paths, and authorize dialog-selected
files/folders through backend state or opaque job IDs.

### SOL-021: API key is stored in plaintext WebView storage

Severity: High

References: `src/routes/+page.svelte:41-47`,
`src/routes/+page.svelte:110-173`, `src-tauri/tauri.conf.json:26-28`

The complete key is serialized to `localStorage` on every edit. It is recoverable
from the WebView profile and available to any renderer compromise. The production
CSP also permits broad WebSocket destinations.

Recommended change: store the key in the OS credential store through a narrow Rust
API, never return it to JavaScript after saving, persist only a configured flag,
and separate strict production CSP from development allowances.

### SOL-022: Batch completion summaries include previous runs

Severity: Medium confirmed bug

References: `src/routes/+page.svelte:406-430`,
`src/routes/+page.svelte:475-495`

The run starts with a pending snapshot but computes final success/failure totals
from every queue row. Old results inflate or contaminate later notifications.
Track the current run's IDs and summarize only those rows.

### SOL-023: Global errors are retried for every queued file

Severity: High

References: `src/routes/+page.svelte:430-473`,
`src-tauri/src/commands.rs:141-143`

The IPC bridge discards `ErrorKind` and returns only text. The loop continues after
invalid credentials, quota exhaustion, an unwritable destination, missing validator,
or network outage, potentially repeating cost and failure for every file.

Recommended change: return stable structured error codes and stop/pause on global
or configuration errors while continuing only file-specific failures.

### SOL-024: Validation partial success is represented as total failure

Severity: High

References: `crates/baegun-core/src/lib.rs:170-186`,
`src/routes/+page.svelte:454-467`

An EPUB can exist after failed validation, but the row is marked `error` without an
output path, preventing reveal/open actions and encouraging accidental reruns.
Use states such as `done_with_warnings` or a separate validation result.

### SOL-025: Output directory and conflicts are validated after paid work starts

Severity: High

References: `src/routes/+page.svelte:92-99`,
`src/routes/+page.svelte:396-440`,
`crates/baegun-core/src/lib.rs:66-170`

The editable persisted destination is checked only for nonempty text. It can be
stale, unwritable, or a file. Preflight/create it in Rust, perform a temporary write
test, and resolve all conflicts before the first upload.

### SOL-026: Closing the window can interrupt conversion and cleanup

Severity: High

References: `src/routes/+page.svelte:585-607`,
`src/lib/windowManager.ts:9-11`

Custom and native close paths are not guarded while converting. Offer Keep
Converting, Stop After Current File, and explicit Quit Now behavior. Atomic outputs
and restartable remote cleanup are still required.

### SOL-027: Window shading conflicts with native minimum height

Severity: High visual/functional bug

References: `src-tauri/tauri.conf.json:17-23`,
`src/lib/windowManager.ts:17-35`

The app minimum height is 620 pixels, but shading requests 36 pixels and then hides
the content regardless of whether resize succeeded. This can leave a large blank
window. Temporarily adjust the native minimum size or remove shading.

### SOL-028: Settings are immediate, non-transactional, and surprising

Severity: Medium

References: `src/routes/+page.svelte:110-118`,
`src/routes/+page.svelte:710-743`

Backdrop/Escape dismissal still preserves live changes; there is no Cancel. Enabling
comic mode permanently flips the independent Include images preference. Edit a draft,
offer Save/Cancel, and derive the effective request as `includeImages || comicMode`.

### SOL-029: Desktop output actions are detached from actual results

Severity: Medium

References: `src/routes/+page.svelte:89-95`,
`src/routes/+page.svelte:277-286`

Open Target Folder uses the current text field rather than a completed job's actual
output. Changing the field can open an unrelated folder. Add per-row Reveal/Open,
remember the last successful path separately, and retain output paths on warnings.

### SOL-030: Large queue insertion is quadratic

Severity: Medium performance issue

Reference: `src/routes/+page.svelte:330-352`

Each accepted path copies and reassigns the complete jobs array, retriggering sorted
copies. Build a batch array and assign once. Consider virtualized rows only after
measuring genuinely large queues.

### SOL-031: Error details are simultaneously hidden and potentially huge

Severity: Medium

References: `src/routes/+page.svelte:454-466`,
`crates/baegun-core/src/validate.rs:41-52`

Full validator output can be retained in an ellipsized single-line table cell.
Keep a bounded summary in the row and show full bounded diagnostics in an accessible
details dialog with copy/export.

### SOL-032: File dialogs, storage, and window operations have unhandled failures

Severity: Medium

References: `src/routes/+page.svelte:138-173`,
`src/routes/+page.svelte:235-252`,
`src/lib/updateChecker.ts:27-49`,
`src/lib/windowManager.ts:9-35`

Dialog and several window promises are not caught; storage access itself can throw.
Degrade to session-only settings when storage fails and surface actionable,
non-destructive notifications.

### SOL-033: Same-named inputs and output paths are unclear

Severity: Medium UX issue

References: `src/routes/+page.svelte:638-659`,
`src/routes/+page.svelte:514-527`

Rows show only basename and even their tooltip omits the full path. JavaScript also
guesses path separators and case behavior. Show parent path as secondary text, preview
the planned output before conversion, and move path derivation/collision checks to Rust.

## Accessibility, Layout, and Visual Findings

### SOL-034: Dialog semantics and focus behavior are incomplete

Severity: High accessibility issue

References: `src/routes/+page.svelte:710-762` and the locked system7-ui
`ModalDialog` implementation

Dialogs are not programmatically named. A non-dismissible progress dialog exposes a
focusable backdrop labeled "Close modal" that does nothing. Fixed dialog widths plus
large chrome can overflow. Add `aria-labelledby`, focus trapping/restoration,
accurate dismissibility, viewport max dimensions, and scrollable dialog content.

### SOL-035: Progress is weak for sighted and assistive users

Severity: Medium

References: `src/routes/+page.svelte:746-760`,
`src/routes/+page.svelte:122-136`

Stage updates are not in a live region, and the bar only advances after complete
files, appearing frozen during long OCR. Show two levels: overall files and current
stage, with an indeterminate OCR state, elapsed time, cache status, and explicit
"Stop After Current File" wording until real cancellation exists.

### SOL-036: Disabled-action explanations are mouse-only

Severity: Medium accessibility issue

References: `src/routes/+page.svelte:689-703`

Disabled buttons cannot receive focus and their wrapper spans are not focusable, so
keyboard and screen-reader users cannot discover why actions are unavailable. Use
persistent helper text connected with `aria-describedby` rather than required
information in hover-only balloons.

### SOL-037: Title bar and queue controls need accessible keyboard affordances

Severity: Medium

References: `src/routes/+page.svelte:599-607`,
`src/routes/+page.svelte:638-663`

The relevant system7-ui title controls are image-like roles without useful labels
or visible focus treatment. Queue users must tab through every remove action and
have no row selection, Delete shortcut, details action, or Retry Failed command.

### SOL-038: Semantic table headers are disconnected from body rows

Severity: Medium accessibility issue

Reference: current system7-ui `DataTable` usage at
`src/routes/+page.svelte:627-663`

The component uses separate header and body tables without cell/header association.
Prefer one semantic table with sticky headers or explicit IDs/`headers` links.

### SOL-039: The app is not actually responsive below desktop width

Severity: Medium visual/product issue

References: `src-tauri/tauri.conf.json:17-20`,
`src/routes/+page.svelte:1048-1071`

Minimum width is 860 pixels and the breakpoint is 900, so responsive rules cover
only a 40-pixel range. Inline `<col>` widths also defeat cell-width overrides.
Either declare desktop-only support or lower the minimum and use a queue card layout
below roughly 700 pixels with fewer visible columns.

### SOL-040: Several small visual defects reduce polish

Severity: Low to Medium

- `width: 100vw; height: 100vh` plus borders lacks `box-sizing: border-box`, clipping
  the frame edge (`src/routes/+page.svelte:767-784`).
- Update and ordinary notifications occupy the same corner/z-index and can overlap
  (`src/lib/components/UpdateNotice.svelte:43-55`).
- The update notice sits outside the themed root and can miss typography/accent
  variables (`src/routes/+layout.svelte:18-19`).
- The update card and actions can overflow narrow viewports.
- Notification stacking assumes fixed height in system7-ui.
- Important remove/dismiss/title controls have small pointer targets.

## Update, CI, and Release Findings

### SOL-041: Update URL opening is too broad

Severity: Medium security issue

Reference: `src-tauri/src/updates.rs:80-90`

Any renderer-provided HTTP(S) URL can be opened. Parse the URL, require HTTPS, and
allow only the expected GitHub release origin/path. Narrow opener permissions too.

### SOL-042: Update version comparison is not SemVer

Severity: Medium correctness issue

Reference: `src-tauri/src/updates.rs:92-116`

Invalid components become zero and prerelease semantics are discarded. Use strict
SemVer parsing and reject malformed release tags.

### SOL-043: Release tags are not checked against embedded versions

Severity: High release-integrity issue

References: `.github/workflows/release.yml:3-40`, `Cargo.toml:9-12`,
`package.json:1-4`, `src-tauri/tauri.conf.json:3-5`

Any matching tag can publish binaries even when Cargo, npm, and Tauri versions differ.
Add an initial gate that strictly parses the tag and compares every declaration;
make release creation/build depend on it.

### SOL-044: CI/release coverage and supply-chain artifacts are incomplete

Severity: Medium

- CI does not compile Windows-specific code before a Windows release.
- Release distributes desktop bundles but not the first-class CLI.
- Actions use mutable tags rather than pinned SHAs.
- Release permissions are broader than necessary.
- No dependency audit, checksums, SBOM, or provenance are published.
- Updater signing variables/documentation exist despite notification-only updates.

Recommended order: add Windows checks, CLI archives and checksums, version gate,
job-scoped permissions, action SHA pinning, dependency audits, then provenance/SBOM.

## CBZ-to-EPUB Design

CBZ support should be local, deterministic, fixed-layout, and independent of
Mistral. It should not fabricate an OCR response or pass through PDF normalization.

### Architecture

```text
Input source
  PDF -> Mistral OCR -> reflowable normalization ----+
                                                     +-> Publication -> EPUB -> validation
  CBZ -> safe ZIP/image/ComicInfo inspection --------+
```

Generalize the source boundary rather than building a plugin framework:

- `InputKind`: PDF or CBZ, detected by signature with extension used for diagnostics.
- Source-specific options: OCR/table/header options for PDF; order/direction/spread/
  image compatibility options for CBZ.
- Generic publication model: metadata, source identifier, reflowable or fixed layout,
  reading order, assets, cover page, page dimensions, spread properties, warnings.
- Generic progress: inspect, optional OCR, normalize/render, package, optional validate,
  complete.
- File-backed or archive-entry assets so large books do not require all bytes in RAM.

The current `RenderedBook`/EPUB writer is the natural reuse seam, but it needs layout,
viewport, spine, manifest-property, cover-page, and streamable-asset extensions.

### Safe Archive Ingestion

Open the central directory without extracting source paths. Enforce limits before
and during reads:

- Reject absolute paths, drive/UNC prefixes, NULs, `..`, excessive path lengths,
  symlinks, non-regular entries, and encrypted entries unless explicitly supported.
- Cap total entries, image pages, individual expanded bytes, total expanded bytes,
  dimensions, total pixels, and suspicious compression ratios.
- Continue enforcing actual streamed byte limits when ZIP metadata lies; require CRC
  completion.
- Ignore `__MACOSX`, `.DS_Store`, `Thumbs.db`, resource forks, directories, and known
  non-page metadata.
- Generate every EPUB path internally. Never copy source entry paths into the EPUB.
- Review/upgrade the current `zip 0.6` dependency before treating hostile archives as
  input.

Reasonable initial limits: 10,000 entries, 5,000 pages, 128 MiB expanded per entry,
2 GiB total expanded bytes, and 100 megapixels per image, with explicit advanced
overrides only if a real need emerges.

### Page Selection and Natural Sorting

Default to deterministic natural ordering over complete relative path components:

```text
Chapter 2/page 1.jpg
Chapter 2/page 2.jpg
Chapter 2/page 10.jpg
Chapter 10/page 1.jpg
```

- Decode names according to ZIP flags with a documented legacy fallback.
- Normalize Unicode for comparison only.
- Compare case-insensitively, then compare numeric runs without integer conversion.
- Tie-break by leading zeros, original bytes, and central-directory index.
- Warn on normalized/case-insensitive duplicates.
- Advanced options: natural (default), archive order, lexical.

### Image Handling

- Sniff type from bytes and reconcile misleading extensions.
- Read dimensions from headers without fully decoding pixels.
- Account for EXIF orientation in the viewport.
- Preserve JPEG/PNG as the conservative path; decide GIF policy deliberately.
- Make WebP an explicit modern-reader profile or transcode for compatibility.
- Reject/transcode BMP, TIFF, AVIF, corrupt images, and unsafe SVG.
- Store already-compressed images in the EPUB without a second deflate pass.
- Stream one page at a time and check cancellation between entries.

### ComicInfo.xml and Metadata

Precedence: explicit user values, then `ComicInfo.xml`, filename heuristics, safe
defaults. Parse XML with no external entities/network and a small metadata size cap.

Map title, series, number, volume, contributors and roles, publisher/imprint, summary,
genre/tags, language, date, GTIN/web, manga/RTL direction, and page-level cover type.
Use EPUB collection/group-position metadata for series and MARC role refinements for
contributors. Keep all CBZ metadata processing local by default.

### Fixed-Layout EPUB

- Emit one XHTML document per image with the actual viewport dimensions, preferably
  an SVG wrapper with matching `viewBox` for predictable scaling.
- Add `rendition:layout=pre-paginated`, orientation, and spread metadata.
- Set spine `page-progression-direction` to `ltr` or `rtl`; do not reverse page order.
- Resolve direction from explicit choice before ComicInfo manga metadata; do not infer
  it only from language.
- Default spreads to `none`; add optional spread modes only after reader testing.
- Emit a `page-list` navigation section.
- Reuse the selected page document as the first reading page/cover when appropriate,
  avoiding a duplicate cover spine page.

### CLI Contract

Keep the existing command shape but accept a generic input:

```text
baegun convert INPUT [OPTIONS]
baegun convert-batch INPUT_DIR [OPTIONS]
baegun inspect INPUT
```

Suggested options:

- `--input-format auto|pdf|cbz`
- `--reading-direction auto|ltr|rtl`
- `--spread none|auto|both`
- `--sort natural|archive|lexical`
- `--cover-page N` / `--no-cover`
- `--image-profile compatible|modern|preserve`
- `--overwrite` / `--skip-existing`

Only require a Mistral key after detecting a PDF cache miss. Reject explicitly supplied
PDF-only flags for CBZ rather than silently ignoring them. Batch discovery should
include both PDF and CBZ.

### Desktop Contract

- Rename Add PDFs to Add Books and show PDF/CBZ type badges.
- CBZ conversion must work with no API key and display "Local only, nothing uploaded."
- Add a fast inspect command returning source type, page count, metadata, thumbnail,
  dimensions, direction, estimated output size, and warnings.
- Add a preflight contact sheet for page order, rotation, cover, spread, and RTL.
- Make destination conflict policy explicit before conversion.
- Use real cancellation tokens and structured errors/partial success.

### CBZ Verification Matrix

- Natural order: `1.jpg`, `02.jpg`, `10.jpg`, nested chapters, Unicode names.
- Duplicate/case-colliding names and duplicate basenames.
- Traversal paths, symlinks, encrypted entries, ZIP bombs, extreme count/size/ratio.
- Corrupt, empty, mislabeled, unsupported, animated, and huge-dimension images.
- `ComicInfo.xml` cover, contributors, series, language, Manga/RTL, invalid page index.
- Fixed-layout epubcheck plus Thorium, Apple Books, Kobo, and at least one phone reader.
- Large-CBZ peak RSS, cancellation latency, output preservation, deterministic output.

## Testing Strategy Gaps

1. Add failure-injection tests for atomic output and existing destination preservation.
2. Add XML parsing and epubcheck tests for adversarial HTML/entities/metadata/resources.
3. Make the Mistral base URL/client injectable for upload/OCR/delete/retry lifecycle tests.
4. Add cache concurrency, privacy mode, invalid payload, expiry, and eviction tests.
5. Add property/fuzz tests for normalization, asset naming, PDF metadata strings, ZIP
   names, natural sorting, and cache-key framing.
6. Add resource/memory benchmarks for large scanned PDFs and image-heavy CBZ files.
7. Add frontend unit/component/accessibility tests and Tauri end-to-end smoke tests for
   run-local summaries, conflicts, partial validation, settings Save/Cancel, keyboard
   dialogs, close/cancel, and responsive widths.
8. Add reader smoke fixtures to releases so EPUB regressions are visually inspectable.

## High-Value Missing Features

- First-class CBZ conversion and inspection.
- Metadata editor with provenance, cover override, language, series, and direction.
- Per-row Reveal EPUB, Open in Reader, Copy Path, validation details, and Retry.
- Folder import with recursive preview and exclusion rules.
- Cache stats, size cap, expiry, clear, and per-conversion no-cache privacy control.
- Real active-job cancellation and resumable/persisted queues.
- Internal structural EPUB validation even when external epubcheck is unavailable.
- Compatibility profiles for conservative readers, Apple Books, Kobo, and modern EPUB.
- Bounded concurrency (small for Mistral, potentially larger for local CBZ) only after
  rate-limit and memory safeguards exist.
- OS completion notification when a long batch finishes in the background.
- CLI `inspect`, `cache stats`, `cache clear`, machine-readable JSON output, and release
  binaries with checksums.

## Delightful and Quirky Ideas

- **Contact-sheet editor:** reorder/rotate pages, choose the cover, and mark spreads by
  dragging miniature paper sheets around a System 7 desktop.
- **Spread assistant:** detect likely two-page scans and politely ask whether to keep,
  split, or center them instead of silently guessing.
- **Series shelf:** arrange queued CBZ volumes by ComicInfo series/number and flag missing
  or duplicate issues.
- **Privacy receipt:** after each job say exactly what stayed local, what was uploaded,
  whether remote deletion was confirmed, and what was cached.
- **Conversion audit slip:** a printable-looking receipt listing preserved, transcoded,
  skipped, rotated, suspicious, and validation-warning assets.
- **Searchable comics:** optional OCR transcript/page descriptions layered accessibly
  beside original comic pages, clearly opt-in because it uploads or computes OCR.
- **Page-number navigation:** add EPUB `page-list` to scanned PDFs and comics.
- **Size forecast:** estimate output size and compatibility changes before conversion.
- **Reader launch:** Open in Apple Books/default EPUB reader after a successful job.
- **Tiny pressroom sounds:** an optional, restrained mechanical click at job start and
  page-flip/chime at completion, off by default and honoring reduced-motion/sound prefs.

## Naming Review

"MacDring" is difficult to hear, spell, and pronounce; "Mac" incorrectly implies a
macOS-only product. "Baegun" is distinctive but ambiguous to pronounce for many users,
has little conversion/book meaning, and competes with established proper-name uses.

Preliminary collision notes below are directional only, not trademark, domain, company,
package registry, or app-store clearance. A final name needs formal searches.

| Name | Strength | Concern | Preliminary risk |
|---|---|---|---|
| **Rasterleaf** | Clear bridge from image/raster input to book leaves; memorable; works for PDF and CBZ | Slightly technical | Low in preliminary exact GitHub/npm checks |
| **BoundPixel** | Friendly, vivid "pixels becoming a bound book" idea | More comic/image-centric than text-centric | Low preliminary |
| **QuireMill** | Strong bookmaking/conversion metaphor; excellent with retro visual style | Some users will not know "quire" is pronounced "kwire" | Low preliminary |
| **FolioMill** | Polished and broad publishing meaning | Existing photography identity | Medium |
| **FolioFuse** | Energetic and understandable | Existing exact GitHub use; "folio" can imply portfolios | Medium |
| **PageKiln** | Memorable transformation metaphor and visual identity | Existing package/project use | Medium-high |
| **Panelbound** | Excellent comics signal | Too narrow for prose PDFs; existing comic-market use | High |
| **Baegun** | Distinctive once learned | Pronunciation/product meaning/search collision | Medium |
| **MacDring** | Unusual | High spelling/pronunciation/platform-brand friction | High brand friction |

Recommendation: **Rasterleaf** is the strongest umbrella brand. **BoundPixel** is the
best alternative if illustrated books become the center of gravity. **QuireMill** is
the strongest quirky/editorial choice and fits the System 7 pressroom personality.

Use a descriptive subtitle regardless of brand:

> Rasterleaf: PDF and CBZ to EPUB

Before renaming, check trademarks, GitHub/npm/crates.io, app stores, domains, social
handles, package identifiers, and existing reader/publishing products. Plan the rename
as a compatibility-aware release: binary/package IDs, cache/settings migration, Tauri
identifier implications, update channel, docs, icons, release automation, and old-name
search redirects.

## Suggested Implementation Slices

These slices are intentionally isolated to reduce review and merge conflicts:

1. **Release safety:** tag/version gate in `.github/workflows/release.yml` only.
2. **Update URL safety:** strict GitHub HTTPS allowlist in `src-tauri/src/updates.rs`.
3. **Run-local desktop summaries:** a focused state fix in `+page.svelte`.
4. **Atomic output and source identity:** core filesystem behavior and tests.
5. **Page-scoped image IDs:** normalization and focused fixtures.
6. **XHTML sanitization:** dedicated parser/sanitizer change with adversarial fixtures.
7. **CBZ foundation:** source-neutral model plus safe archive inspection and fixed-layout
   packaging, followed by CLI and desktop adapters in reviewable commits.
8. **Secret storage/Tauri authority:** keychain and IPC redesign as a security-focused PR.

CBZ, atomic packaging, and streamable assets touch the same core seams. If developed in
parallel, one should be a short-lived stacked branch or the sequencing should be agreed
before coding rather than accepting a large merge conflict later.
