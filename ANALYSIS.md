# Engineering Analysis and Roadmap

Last reconciled: 2026-07-11

This is the maintained forward-looking roadmap for Baegun. It folds in the full
project review originally captured in `sol.md` (PR #9, closed as superseded by
this document) while removing completed work from the active backlog. Keep this
document aligned with behavior and delete items when they are implemented and
verified; do not leave completed tasks mixed into future work.

## Recently Merged

These changes have landed on `main` and are intentionally not duplicated in the
active backlog. If any is later reverted without replacement, restore its
relevant items below.

- PR #10: local CBZ-to-EPUB conversion, guarded ZIP reads, natural page ordering,
  JPEG/PNG decoder validation, bounded `ComicInfo.xml`, deleted-page filtering,
  cover/RTL handling, fixed-layout EPUB, CLI/desktop support, and local-only CBZ UX.
- PR #11: strict release tag/version gate across Cargo, npm, and Tauri versions.
- PR #12: updater browser links restricted to the project's exact HTTPS GitHub
  release path.
- PR #13: desktop completion/cancellation summaries scoped to the current run.
- PR #15: repository-wide Rust formatting and current-stable Clippy baseline restored.
- PR #16: source/output filesystem identity protection and staged, validated, atomic
  EPUB publication with failure preservation.
- PR #8: design doc for pluggable OCR providers (`docs/ocr-providers.md`).

Known limitations of the first CBZ slice remain active below: memory streaming,
additional image compatibility profiles, richer ComicInfo metadata, user controls,
inspection/contact sheets, cancellation, and reader/epubcheck smoke
coverage.

## Recommended Sequence

### P0: Data Safety and Trust Boundaries

1. Sanitize OCR-derived content into valid, inert XHTML.
2. Move the Mistral API key out of WebView storage and narrow Tauri IPC authority.
3. Surface and retry remote upload cleanup failures.
4. Make cache/debug storage private, atomic, bounded, and manageable.

### P1: Correctness and Responsiveness

1. Make image references page-scoped and validate all OCR image assets.
2. Replace misleading PDF comic mode with real page rasterization and fixed layout.
3. Add active-job cancellation and bounded validation subprocesses.
4. Stream large PDF/CBZ inputs and EPUB assets instead of retaining every byte.
5. Preflight destination validity/conflicts before paid OCR.
6. Return stable structured errors and partial-success results through Tauri.

### P2: Product and Quality

1. Add CBZ inspection, metadata/cover/direction controls, and compatibility profiles.
2. Improve metadata/chapter inference and validation visibility.
3. Complete keyboard, accessibility, responsive layout, and error UX work.
4. Strengthen release distribution, CI coverage, and supply-chain artifacts.
5. Add cache tools, reader actions, privacy receipts, and test/benchmark coverage.

## Core Conversion Safety

### EPUB XHTML Parsing and Sanitization

Severity: High

References: `crates/baegun-core/src/normalize.rs:642-795`,
`crates/baegun-core/src/epub.rs`

Raw OCR/table HTML passes through Markdown rendering. Regex repair does not guarantee
well-formed XML and does not remove scripts, forms, event handlers, unsafe URLs,
remote tracking resources, or unsafe SVG. HTML named entities such as `&nbsp;` are
not automatically valid XML entities. OPF properties for scripted, SVG, MathML, and
remote content are not derived.

Required behavior:

- Parse rendered content with an HTML5 parser.
- Apply an explicit EPUB-safe element/attribute/URL allowlist.
- Strip active and remote content by default.
- Remove XML 1.0-invalid characters from generated XML and metadata.
- Serialize well-formed XHTML rather than repairing markup with regexes.
- Derive required manifest properties from the final DOM.

Tests should include malformed tables, `&nbsp;`, invalid numeric entities, NUL/control
characters, scripts, event attributes, forms, remote images, SVG, and MathML. Parse
every generated XML document and run epubcheck over adversarial fixtures.

### OCR Image Identity and Validation

Severity: High

References: `crates/baegun-core/src/normalize.rs:173-227`,
`crates/baegun-core/src/normalize.rs:387-419`

OCR image IDs are treated as globally unique even though page-local IDs can repeat.
Later assets may be discarded or references may point to an earlier page. Missing
payloads can leave broken references; one invalid base64 image aborts the book; media
type is inferred from the filename rather than bytes; unsupported resources can enter
the package; fallback names can collide.

Required behavior:

- Key references by `(page.index, image.id)` and replace within that page.
- Generate collision-proof internal names independent of OCR IDs.
- Sniff decoded bytes/data URI media type and validate dimensions/content.
- Remove or replace every unresolved placeholder.
- Transcode only under an explicit compatibility policy.
- Return structured warnings for skippable assets and support strict mode.

### PDF Comic Mode

Severity: High

References: `crates/baegun-core/src/normalize.rs:108-151`,
`crates/baegun-core/src/epub.rs`, `README.md`

Mistral image regions are not guaranteed full PDF page renders. Choosing the first
region can select a panel/logo, and pages without a region are silently skipped.
The current claim of one page image per source page is therefore unreliable.

Required behavior:

- Rasterize complete PDF pages locally or use a source guaranteed to return page
  images.
- Require source/output page-count equality and report any missing page.
- Use the fixed-layout publication support introduced for CBZ.
- Avoid a duplicate first-page cover spine item.
- Capture actual viewport dimensions, direction, orientation, and spread policy.

### Input and Configuration Validation

Severity: Medium

Validate before network or destructive work:

- PDF/CBZ extension and signature, source size, regular file, and stable open handle.
- Source/output distinction and destination policy.
- Nonempty OCR model and valid option combinations.
- BCP-47 language tags and XML-safe metadata.
- Mistral size limits before upload.
- Output directory existence/type/writability before OCR.

The generic source detection added by PR #10 covers extension/signature only; the
remaining validation still belongs in a central `ConvertConfig::validate`-style seam.

## Privacy, Cache, and Remote Lifecycle

### Remote File Cleanup

Severity: High

References: `crates/baegun-core/src/mistral.rs:35-56`,
`crates/baegun-core/src/mistral.rs:241-255`

Remote delete failures are discarded even though the product promises default
deletion. The response is not checked for confirmed deletion, and process termination
can strand an upload.

Required behavior:

- Request narrow user visibility and a short expiry where supported.
- Retry cleanup with a bounded, cancellation-independent finalization path.
- Parse deletion confirmation.
- Return cleanup status/warnings in `ConvertSummary` and desktop/CLI output.
- Offer strict privacy mode where unconfirmed cleanup is a failure.
- Securely persist pending remote IDs for cleanup retry after restart.

### Cache and Debug Storage

Severity: High

References: `crates/baegun-core/src/cache.rs`, `crates/baegun-core/src/lib.rs`

OCR text and image payloads use ambient permissions and direct writes. Concurrent or
interrupted writers can corrupt data; symlink handling is unsafe; cache growth is
unbounded; batch debug filenames overwrite one another; a cache write failure can
abort after paid OCR.

Required behavior:

- Private directories/files (`0700`/`0600` where available).
- Temporary writes plus atomic rename and safe symlink policy.
- Per-key locking for concurrent conversions.
- Shared semantic validation for cached and fresh OCR payloads.
- Quarantine invalid entries and treat optional cache writes as warnings.
- Size/age retention, stats, clear, and per-conversion no-cache controls.
- Per-input/hash debug subdirectories with the same privacy policy.

### Cache Key and Moving Models

Severity: Medium

Reference: `crates/baegun-core/src/cache.rs:7-20`

Variable fields are concatenated without tags/lengths, allowing ambiguous boundaries.
Moving aliases such as `mistral-ocr-latest` can keep stale payloads indefinitely.

Hash a versioned canonical structure with explicit field names and lengths. Separate
pipeline schema version from package version. Pin concrete models or apply TTL and
resolved-model revalidation for moving aliases.

### API Key Storage

Severity: High

References: `src/routes/+page.svelte:41-173`, `src-tauri/tauri.conf.json`

The Mistral key is serialized in plaintext WebView `localStorage`, including partial
values while typing. Move it to the OS credential store through a narrow Rust API.
JavaScript should receive only configured/not-configured state, not the saved secret.
Offer an explicit remember-key policy and tighten production CSP WebSocket/connect
allowances separately from development.

## Performance and Cancellation

### Active-Job Cancellation

Severity: High

Desktop cancellation currently stops only before the next queue item. Upload, retry
sleep, OCR, normalization, CBZ decode, packaging, and epubcheck cannot be interrupted.

Add backend job IDs and cancellation tokens. Check cancellation between and within
stages, make retry delays interruptible, use cancellation-aware HTTP, stop child
process trees, remove temporary output, and still attempt remote cleanup.

Tests: cancel during upload, rate-limit delay, normalization, CBZ page reads, packaging,
and validation; assert bounded latency, no partial destination, and cleanup attempt.

### Memory and Streaming

Severity: High for large scanned books

The PDF path holds the full source, multipart clone, OCR base64, cloned pages, decoded
images, Markdown, XHTML, and EPUB assets. The first CBZ implementation enforces limits
and bounded reads but retains accepted page images in memory until packaging.

Future architecture:

- Stream PDF hashing and file-backed multipart upload.
- Avoid cloning OCR pages and release base64 as soon as decoded.
- Spool or stream chapter/assets into the package writer.
- Represent assets as memory/file/archive-entry sources rather than mandatory `Vec<u8>`.
- For CBZ, retain central-directory/page metadata and stream one page into EPUB.
- Bound transcoding workers and decoded pixel memory.
- Measure peak RSS and cancellation latency with large fixtures.

### Retry Policy

Severity: Medium

Only OCR retries; `Retry-After` and jitter are ignored; upload/metadata/delete do not
share policy; a 300-second timeout applies per attempt. Centralize retry classification,
bounded total deadline, jitter, `Retry-After`, and cancellation. Treat ambiguous upload
timeouts carefully because blind retries can create orphan files.

### Packaging CPU and Reproducibility

- PR #10 stores already-compressed JPEG/PNG/GIF/WebP EPUB entries, resolving the
  needless image-deflate issue when merged.
- EPUB output still embeds the current UTC `dcterms:modified`, so identical inputs are
  not byte reproducible. Support an explicit timestamp or `SOURCE_DATE_EPOCH` mode.
- Table placeholder replacement repeatedly compiles regexes/rescans text. Parse once
  or build a page-local replacement map after profiling table-heavy fixtures.

## Metadata and Structure Quality

### Header/Footer Semantics

The options are documented as including headers/footers, but extracted dedicated
values are stripped and never reinserted. Separate API detection from output policy,
for example request detection versus preserve running matter, and document both.

### Metadata Precedence and Provenance

Configured `en` is treated as an unset sentinel and can be overridden. Weak first-line
cover inference outranks PDF metadata. Name heuristics are ASCII-centric. LLM failures,
truncation, provenance, cache failures, and repeated costs are hidden.

Required behavior:

- Represent explicit language as an option and honor it unconditionally.
- Track source/provenance/confidence for each field.
- Prefer resolved PDF Info/XMP over weak cover guesses unless title-page evidence is
  strong.
- Use a maintained PDF parser for active Info/XMP, encoding, object streams, and
  incremental updates under resource limits.
- Return metadata generation warnings and provenance.
- Use schema-constrained output, adequate token limits, and bounded negative caching.
- Improve Unicode/non-Western contributor inference.

### Chapter Splitting

One H1 anywhere suppresses all H2 candidates, so an H1 book title plus H2 chapters can
collapse into one chapter. A final explicit chapter under 400 characters is merged,
which can erase epilogues or acknowledgements.

Score headings by repetition, position, and density. Preserve explicit-heading
boundaries regardless of body length; make heuristic merging configurable and test
mixed H1/H2, decorative headings, short final chapters, Setext, and chapter lines.

### CBZ Metadata and Reader Controls

PR #10 provides title, writer, publisher, summary, language, deleted pages, cover,
legacy XML encodings, and strict RTL handling. Future work:

- Map series, number, volume, date, imprint, GTIN/web, genres/tags, characters/teams/
  locations, and contributor roles.
- Use EPUB collection/group-position and MARC role refinements.
- Add explicit reading direction, spread, cover page, sort mode, and image profile.
- Handle EXIF orientation when deriving viewport dimensions.
- Add conservative/modern profiles for GIF, WebP, BMP/TIFF/AVIF transcoding policy;
  keep unsafe SVG rejected unless sanitized.
- Decide nested/non-root ComicInfo policy and surface metadata warnings.

## Validation and Result Modeling

### epubcheck Process Control

Severity: High

`Command::output` can hang indefinitely and buffer unlimited output. Warning/error
counts are uppercase substring occurrences rather than structured diagnostics.

Use epubcheck machine-readable output where available, enforce timeout/cancellation,
bound retained diagnostics, kill the process tree, and keep a log only when requested.
Prefer a bundled verified validator over mutable `PATH`, or clearly present validation
as an external-tool feature.

### Partial Success and Structured Errors

An EPUB is written before validation. A validation failure is surfaced as total
conversion failure, and the desktop drops the output path. Tauri also discards
`ErrorKind` and returns only message text, preventing global/file-specific handling.

Return versioned structured payloads:

- Stable error code/kind/message and optional details.
- Packaging result/output path independent of validation status.
- `done`, `done_with_warnings`, validation failure, and conversion failure states.
- Validation counts/report, cache state, remote cleanup, metadata provenance, and
  recoverable warnings.

Add an internal structural validator for required ZIP order/mimetype, XML well-formedness,
manifest/spine/nav integrity, unique IDs/paths, media type, viewport, cover, and layout
consistency even when external epubcheck is unavailable.

## CLI and Batch Work

### Recursive Symlink Safety

Severity: High

`path.is_dir()` follows directory symlinks without cycle detection or a canonical root
boundary. Recursive batches can leave the requested tree, loop, upload external files,
and delete them with `--delete-source`.

Use `symlink_metadata`; do not follow directory symlinks by default. If opt-in following
is added, track filesystem identity, enforce root boundaries, and deduplicate file
identity before conversion/deletion.

### Output and Deletion Reporting

- Preflight every batch output and existing file before conversion.
- Use platform/filesystem-aware collision behavior; PR #10 handles case-insensitive
  PDF/CBZ collisions within one batch but not existing files.
- Source deletion failures should affect automation exit status or use a separately
  documented best-effort mode.
- Preserve relative folder structure while preventing path alias collisions.

### CLI Contract Gaps

- `--verbose` is parsed but unused; implement defined diagnostics or remove it.
- Reject contradictory `--quiet --verbose`.
- Add `--overwrite`, `--skip-existing`, and machine-readable JSON results.
- Add `inspect INPUT`, `cache stats`, and `cache clear`.
- Accept API key absence until an actual PDF cache miss; PR #10 fixes CLI core routing,
  but desktop still preemptively requires a key for every pending PDF.
- Distribute tested platform CLI binaries with checksums in releases.

## Tauri Security Boundary

Severity: Critical defense in depth

References: `src-tauri/src/commands.rs`, `crates/baegun-core/src/validate.rs`

Renderer-controlled requests expose arbitrary source/destination/cache/debug paths and
an arbitrary `epubcheck_bin`, which can directly execute a chosen local program. A
compromised WebView could read/upload files, overwrite files, place sensitive output,
or execute programs.

Required redesign:

- Remove desktop-internal executable/cache/debug/remote-retention controls from IPC.
- Resolve trusted validator paths entirely in Rust.
- Treat every request as untrusted and canonicalize/validate paths.
- Authorize dialog-selected files/folders in backend state with opaque job IDs.
- Narrow Tauri capabilities and opener permissions to required commands/actions.
- Validate source signatures and destination authority in Rust.

## Desktop Functional UX

### Destination Preflight and Actions

The output field is only checked for nonempty text and can be stale, unwritable, or a
file. Conflicts are detected only within in-memory jobs. Preflight/create/write-test the
directory in Rust and resolve all conflicts before the first upload.

Associate each success with its actual output. Add per-row Reveal EPUB, Open in Reader,
Copy Path, and validation report. Remember the last successful directory independently
of the current editable field. Preserve output actions on validation warnings/failure.

### Queue Error Policy

Without structured error kinds, invalid credentials, quota exhaustion, service outage,
missing validator, or invalid destination are retried for every queued file. Stop or
pause on global/configuration errors and continue only file-specific failures.

Add Retry Failed, per-row retry, Retry Without Validation, and explicit next-run behavior
for files dropped during an active conversion.

### Window Lifecycle

- Closing the window can terminate conversion/cleanup. Intercept close requests and
  offer Keep Converting, Stop After Current File, and explicit Quit Now.
- Window shade requests 36 px despite a 620 px native minimum, then hides content even
  if resize fails. Temporarily change minimum size and confirm the result, or remove
  shading.
- Catch and serialize close/drag/shade failures and prevent overlapping shade actions.

### Settings

- Edit a draft settings copy with Save/Cancel; backdrop/Escape must not silently commit.
- Keep Include images independent and derive effective PDF request as
  `includeImages || comicMode`.
- Trim copied API keys before use.
- Allow backend environment credentials and cache-only PDF conversion without exposing
  the key.
- Expose no-cache/privacy controls, cache size/clear, and upload disclosure.

### Queue Clarity and Performance

- Build additions in one array assignment; current per-path copies are quadratic.
- Show parent folder/full path and planned output for same-basename inputs.
- Move output path joining, normalization, case behavior, and existence checks to Rust.
- Keep short row summaries; show bounded full diagnostics in a details dialog.
- Catch file/folder dialog and storage failures; degrade to session-only settings.
- Register essential progress listeners before enabling conversion.
- Decide whether drops during conversion become next-run jobs or are disabled.
- Use one Tauri drag event model/hit test for output-folder drops.

## Accessibility and Layout

### Dialogs and Focus

- Programmatically name dialogs with `aria-labelledby`/labels.
- A non-dismissible progress modal must not expose a fake "Close modal" backdrop.
- Trap and restore focus; define Escape behavior.
- Add viewport max width/height, border-box sizing, and internal scrolling.
- Make settings dismissal and progress cancellation semantically accurate.

Some changes belong in `system7-ui`; coordinate upstream rather than locally forking
component behavior.

### Progress

Add a polite live region and two-level progress: overall files plus current stage.
Use indeterminate OCR progress, elapsed time, cache state, and explicit "Stop After
Current File" wording until active cancellation exists.

### Keyboard and Controls

- Replace mouse-only disabled tooltips with persistent `aria-describedby` help.
- Add accessible names and visible focus to title-bar controls.
- Increase remove/dismiss/title pointer targets.
- Add selectable queue rows, roving focus, Delete/Backspace, Enter for details, and
  discoverable Ctrl/Cmd+O and settings shortcuts.
- Avoid a `<label>` containing both output input and unrelated Choose button.

### Data Table Semantics

The current system7-ui table uses separate header/body tables without reliable header
association. Use one semantic table with sticky header or explicit header IDs. Responsive
column changes must affect `<col>` definitions, not only cells.

### Responsive and Visual Polish

- Decide whether the app is desktop-only. Current minimum width is 860 px and the main
  breakpoint is 900 px, so responsive behavior covers only a narrow band.
- If small windows matter, lower the minimum and switch queue rows to cards/fewer columns
  below roughly 700 px.
- Add `box-sizing: border-box` to the viewport frame to avoid clipped borders.
- Coordinate update and ordinary notifications so they cannot overlap.
- Put update notices inside the themed root/provider and make actions wrap on narrow
  widths.
- Replace fixed-height notification offset assumptions with a flex stack upstream.
- Add document title/application metadata.

## CI, Release, and Distribution

PR #11 resolves tag/version mismatch gating when merged. Remaining work:

- Replace the in-app updater's permissive numeric parser with strict SemVer handling;
  malformed components currently become zero and prerelease-to-stable transitions can
  compare incorrectly. Cover malformed, prerelease, build metadata, and stable upgrade
  cases. PR #12 secures the release URL but does not change version comparison.
- Add Windows to CI before Windows release builds.
- Build, test, archive, checksum, and publish CLI binaries.
- Pin actions by full commit SHA.
- Scope release write permissions to only jobs that need them.
- Add Cargo/npm vulnerability audits.
- Publish checksums, SBOM, and build provenance.
- Reconcile updater-signing secrets/docs with the current notification-only updater;
  either remove dead claims or implement signed Tauri updating end to end.
- Add an action/workflow syntax linter and version-gate tests.

## Test and Benchmark Plan

### Core and EPUB

1. Failure-injection tests for atomic output and destination preservation.
2. XML parsing and epubcheck for adversarial OCR HTML/entities/metadata/resources.
3. Injectable Mistral base URL/client for upload/OCR/delete/retry tests.
4. Cache concurrency, semantic invalidity, permissions, symlinks, expiry, and eviction.
5. Property/fuzz tests for normalization, asset naming, cache-key framing, PDF strings,
   ZIP names, and natural ordering.
6. Reader smoke fixtures for covers, reflowable books, image-heavy books, and fixed
   layout in Thorium, Apple Books, Kobo, and a phone reader.
7. Reproducible-output tests under an explicit deterministic mode.

### CBZ

PR #10 covers natural order, malformed decoder input, traversal, symlink, duplicate
ComicInfo, metadata/RTL/cover/deleted pages, fixed-layout package strings, and local
conversion. Add:

- Encrypted entries, CRC corruption, absolute/UNC/backslash/NUL paths, non-regular modes.
- Entry/page/actual expanded byte/compression-ratio boundaries and forged ZIP metadata.
- Duplicate/case-normalized names, Unicode/legacy filename encodings, extreme counts.
- Mixed JPEG/PNG dimensions, EXIF orientation, unsupported formats, animation.
- epubcheck-backed fixed-layout fixtures and reader smoke matrix.
- Large archive peak RSS, byte/page progress, and mid-stream cancellation.

### Desktop

There are no frontend unit, component, accessibility, or end-to-end tests. Add coverage
for destination conflicts, validation partial success, global batch errors, settings
Save/Cancel, modal keyboard/focus, disabled explanations, same basenames, large queue
insertion, close/cancel/shade behavior, and widths at 860/700/480/320 px if supported.

## Product Features

High-value future features:

- Fast `inspect_input` for source type, pages, metadata, thumbnail, dimensions,
  direction, estimated output size, and warnings.
- Metadata editor with provenance, cover override, language, series, and direction.
- Contact-sheet page order/rotation/cover/spread editor for CBZ.
- Folder import with recursive preview and exclusion rules.
- Compatibility profiles for conservative, Apple Books, Kobo, and modern EPUB readers.
- Bounded batch concurrency after rate-limit and memory safeguards.
- Persisted/resumable queue and OS background completion notification.
- Internal EPUB validation and a readable conversion/validation report.
- Searchable comics through optional OCR transcript or accessible page descriptions,
  with explicit privacy/cost consent.
- EPUB `page-list` and original page labels for scanned PDFs as well as CBZ.

## Delightful Ideas

- **Contact-sheet desktop:** miniature paper sheets that can be reordered, rotated,
  marked as covers, or paired as spreads.
- **Spread assistant:** detect likely double-page scans and ask whether to keep, split,
  or center instead of guessing.
- **Series shelf:** sort queued issues by ComicInfo series/number and flag gaps/duplicates.
- **Privacy receipt:** state what stayed local, what was uploaded, whether deletion was
  confirmed, and what was cached.
- **Conversion audit slip:** list preserved, transcoded, skipped, rotated, suspicious,
  and validation-warning assets in the retro pressroom visual language.
- **Size forecast:** estimate EPUB size and compatibility transformations before work.
- **Open in reader:** launch Apple Books/default EPUB reader after success.
- **Optional pressroom sounds:** restrained mechanical start/page/completion sounds,
  off by default and honoring reduced-motion/sound preferences.

## Naming Direction

`MacDring` has high spelling, pronunciation, and platform-brand friction. `Baegun` is
distinctive once learned but has ambiguous pronunciation, weak product meaning, and
proper-name search competition.

Shortlist, subject to trademark/domain/app-store/package clearance:

| Name | Positioning | Preliminary concern |
|---|---|---|
| **Rasterleaf** | Best umbrella: raster/image material becomes book leaves; works for PDF and CBZ | Slightly technical; low preliminary exact project/package collision |
| **BoundPixel** | Friendly, memorable, strongest if comics/illustration lead | More image-centric than prose; low preliminary collision |
| **QuireMill** | Best quirky/editorial/retro pressroom identity | “Quire” pronunciation is not universal; low preliminary collision |
| **FolioMill** | Polished, broad publishing metaphor | Existing photography identity |
| **FolioFuse** | Energetic combination/conversion metaphor | Existing exact project use |
| **PageKiln** | Memorable transformation metaphor | Existing package/project use |
| **Panelbound** | Strong comic signal | Too narrow and existing comic-market uses |

Current recommendation: **Rasterleaf**, with the descriptive subtitle:

> Rasterleaf: PDF and CBZ to EPUB

Before renaming, search trademarks, companies, GitHub, crates.io, npm, app stores,
domains, and social handles. Treat the rename as a migration: binary/package names,
Tauri identifier, cache/settings, update channel, release automation, docs, icons, and
old-name redirects all need an explicit compatibility plan.
