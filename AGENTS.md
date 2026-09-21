# AGENTS.md

Baegun is now a Rust/Tauri codebase.

This document is the implementation handoff and must be kept in sync with the code.

When behavior changes, also update `README.md`.

## Product Direction

Build one shared conversion engine and expose it through two frontends:

- CLI: `baegun`
- Desktop app: Tauri (`src-tauri`) + SvelteKit (`src`) + `system7-ui`

No Python runtime or Tk GUI remains in the main architecture.

## Notes

- PDF content is uploaded to Mistral OCR when cache is missed.
- CBZ content is converted locally without Mistral, OCR, extraction, or cache use.
- OCR payloads are cached under `.baegun-cache` by default.
- Use `--no-cache` for sensitive documents.
- Uploaded OCR files are deleted by default unless `--keep-remote-file` is set.
- Input and output paths that identify the same filesystem file are rejected before cache or network work, before temporary-file creation, and immediately before publication, including noncanonical, symlink, and hard-link aliases where supported.
- EPUB output is staged in a securely created temporary file in the destination directory, optionally validated there, and atomically replaces the destination only after success on supported Unix and Windows platforms.
- Packaging, validation, and atomic-publication errors preserve an existing destination and clean up the temporary output.
- In the desktop app, API key entry and conversion toggles (`Include images`, `PDF comic mode`, `Run epubcheck`) live in `Settings...`; a key is required only when pending PDFs exist.
- The desktop app Settings dialog includes a shortcut link to the Mistral API key page.
- After at least one successful conversion, the desktop app can open the selected target output folder.
- During desktop conversions, backend stage progress events are emitted and shown in the progress modal. PDF uses input, OCR, normalize, package, optional validate, and complete; CBZ omits OCR/cache work.
- The desktop queue supports per-file removal, and the progress modal includes a cancel button that stops after the current in-flight file.
- EPUB output marks the first extracted image from the first PDF page as the cover image.
- EPUB metadata is resolved from explicit config, cover/title-page OCR text, PDF metadata, and best-effort Mistral LLM generation from OCR content when needed.
- CBZ metadata is parsed locally from one bounded root `ComicInfo.xml`; explicit config wins. Deleted page records are filtered, and only `Manga=YesAndRightToLeft` enables RTL.
- Desktop validation resolves `epubcheck` from `PATH`, bundled resources, common Homebrew/MacPorts locations, or `EPUBCHECK_BIN`.

## Quality Gates

Automatic checks are wired into both commits and builds:

- `npm run build` runs `npm run verify` first, which runs:
    - `npm run check`
    - `npm run test` (`cargo test --workspace`)
- Git pre-commit hook runs `npm run verify` automatically.

If you commit from IntelliJ, keep **Run Git hooks** enabled in the commit dialog.

## Architecture

```text
PDF -> Mistral OCR -> normalization -> chapter split -> reflowable EPUB
CBZ -> safe local ZIP adapter -> ordered image pages -> fixed-layout EPUB
```

Workspace layout:

```text
crates/
  baegun-core/   shared conversion logic used by CLI and Tauri
  baegun-cli/    `baegun` binary
src/             SvelteKit frontend
src-tauri/       Tauri host + Rust command bridge
```

## Current Architecture

```text
PDF
 -> Mistral OCR (files upload + OCR endpoint)
 -> normalization (headers/footers, placeholders, images, tables)
 -> chapter segmentation (H1 boundaries)
 -> markdown -> HTML -> XHTML
 -> EPUB 3 zip packaging in destination-directory temporary file
 -> optional epubcheck validation of temporary EPUB
 -> atomic destination publication

CBZ
 -> extension/signature validation
 -> bounded in-place ZIP inspection and CRC-checked reads
 -> JPEG/PNG sniffing, validation, dimensions, and natural path ordering
 -> bounded local ComicInfo.xml metadata
 -> viewport XHTML page per image
 -> fixed-layout EPUB 3 zip packaging
 -> optional epubcheck validation
```

Shared modules live in `crates/baegun-core` and are used by both CLI and Tauri command handlers.

## Workspace Layout

```text
Cargo.toml              # workspace
crates/
  baegun-core/          # shared conversion pipeline
  baegun-cli/           # `baegun` binary
src/                    # SvelteKit app using system7-ui
src-tauri/              # Tauri host and command bridge
```

## Core API (`baegun-core`)

Main entry point:

- `convert_to_epub(cfg: &ConvertConfig) -> Result<ConvertSummary>` dispatches PDF/CBZ
- `convert_to_epub_with_progress(cfg, on_progress) -> Result<ConvertSummary>` for generic stage callbacks
- `convert_pdf_to_epub(cfg: &ConvertConfig) -> Result<ConvertSummary>`
- `convert_pdf_to_epub_with_progress(cfg, on_progress) -> Result<ConvertSummary>` compatibility wrapper
- `detect_source_format(path) -> Result<SourceFormat>` validates extension and signature

Important types:

- `ConvertConfig`
- `ConvertProgress`
- `ConvertStage`
- `TableFormat`
- `SourceFormat`
- `ConvertSummary`
- `ValidationResult`
- `BaegunError` (`ErrorKind` includes CLI-friendly exit mapping)

Key modules:

- `mistral.rs`: file upload + OCR request + retry + cleanup
- `metadata.rs`: PDF metadata extraction + metadata merge + optional LLM enrichment
- `cache.rs`: `.baegun-cache` SHA256-keyed OCR payload cache
- `cbz.rs`: safe local CBZ inspection, ComicInfo parsing, image validation/order, and fixed-layout rendering
- `normalize.rs`: placeholder replacement, chapterization, XHTML rendering
- `epub.rs`: EPUB packaging (zip + `content.opf` + `nav.xhtml`)
- `validate.rs`: optional `epubcheck` execution

## CLI Contract

Command:

```bash
baegun convert INPUT [OPTIONS]
baegun convert-batch INPUT_DIR [OPTIONS]
```

Notable options:

- `-o, --output`
- `-o, --output-dir` (batch)
- `--recursive` (batch)
- `--api-key` (fallback `MISTRAL_API_KEY`)
- `--model` (default `mistral-ocr-latest`)
- `--table-format html|markdown`
- `--extract-header true|false`
- `--extract-footer true|false`
- `--include-images true|false`
- `--comic`
- `--cache-dir`
- `--no-cache`
- `--validate`
- `--epubcheck-bin`
- `--debug-dir`
- `--keep-remote-file`
- `--delete-source`
- `--fail-on-warn`
- `--quiet`
- `--verbose`

`INPUT` accepts PDF or CBZ. Batch mode discovers both. CBZ requires no API key and does not use PDF OCR/cache options. `--comic` is PDF-only and is rejected for CBZ.

Exit code mapping:

- `2` bad args/config
- `3` API/auth/quota/network errors
- `4` OCR schema/parsing issues
- `5` EPUB build/write issues
- `6` validation failure
- `1` all other internal failures

## Desktop App Contract

Frontend: `src/routes/+page.svelte`.

Backend command:

- `convert_book(request: ConvertRequest) -> ConvertResponse`
- `convert_pdf(request: ConvertRequest) -> ConvertResponse` compatibility alias

Progress event:

- `baegun://convert-progress` with stage payload (`reading_input`, `ocr`, `normalize`, `package_epub`, optional `validate`, `complete`)

The desktop app accepts PDF and CBZ books and should remain a thin orchestrator over the shared `baegun-core` conversion logic.

Drag-and-drop is handled through Tauri window drag-drop events, while file/folder picking uses `@tauri-apps/plugin-dialog`.

## system7-ui Integration

Frontend imports:

- `@lkmc/system7-ui/styles.css` in `src/routes/+layout.svelte`
- Components from `@lkmc/system7-ui` in page/UI components

Dependency source is the npm registry:

- `@lkmc/system7-ui`: `^0.2.1` (published from the `system7-ui` repo; for local
  library development use `npm link ../system7-ui` or a temporary `file:` override)

Reference apps for style/patterns:

- `../Lantenna`
- `../Obtainintosh`

## Mistral OCR Notes

Preferred flow:

1. `POST /v1/files` (`purpose=ocr`) with PDF file
2. `POST /v1/ocr` using uploaded `file_id`
3. Optional `DELETE /v1/files/{id}` cleanup

Request fields used:

- `model`
- `table_format`
- `extract_header`
- `extract_footer`
- `include_image_base64`

OCR payloads are expected to include `pages[]` with markdown + optional images/tables.
Image payloads can arrive as raw base64 or `data:*;base64,...` data URIs and should be decoded in either shape.
OCR image payloads are requested for all conversions so the first page image can be used as the EPUB cover, even when body image embedding is disabled.
When title/author/description/subjects are missing, cached OCR text can be sent to Mistral chat completions for best-effort EPUB metadata generation. Explicit config values take precedence, and cover/title-page OCR is preferred for title and author.

## EPUB Packaging Rules

Generated archive includes:

- `mimetype` (stored/uncompressed)
- `META-INF/container.xml`
- `OEBPS/content.opf`
- `OEBPS/nav.xhtml`
- `OEBPS/styles/book.css`
- `OEBPS/text/cover.xhtml` when a reflowable PDF cover image is available
- `OEBPS/text/chapter-*.xhtml`
- `OEBPS/text/page-*.xhtml` for fixed-layout CBZ pages
- `OEBPS/images/*`

When a cover image is available, mark its manifest item with `properties="cover-image"`.
For CBZ, use generated image/page names, store image ZIP entries without recompressing, emit `rendition:layout=pre-paginated`, `rendition:orientation=auto`, `rendition:spread=none`, `ltr`/`rtl` spine direction, viewport dimensions, and page-list navigation. The selected cover image remains its normal CBZ page and must not get a duplicate cover spine document.

## CBZ Safety Contract

- Open with `ZipArchive` and never extract.
- Limit archives to 10,000 entries and 2,000 pages. Enforce 100 MiB per actually expanded entry, 2 GiB cumulative actual expanded bytes, and 1000:1 observed expansion ratio during bounded reads, in addition to metadata preflight checks.
- Reject encrypted entries, symlinks/non-regular entries, absolute paths, traversal paths, malformed JPEG/PNG pages, and unsupported content presented with a supported image extension.
- Ignore directories, `__MACOSX`, `.DS_Store`, `Thumbs.db`, and `._` resource forks.
- Fully read accepted regular entries through EOF so ZIP CRC validation completes; rejected over-limit entries may stop at the detection byte.
- Sniff JPEG/PNG bytes rather than trusting archive extensions, then fully decode with 100,000-pixel per-axis, 100-million-pixel total, and 512-MiB decoded-allocation limits; use generated EPUB asset names only.
- Natural-sort relative archive paths deterministically, component by component, with numeric runs ordered by numeric value.
- Bound root `ComicInfo.xml` to 1 MiB, reject duplicates and DTDs/external entities, and honor supported declared legacy encodings.
- Hash the complete CBZ source stream for the EPUB identifier without loading another full source copy.

Package through the shared atomic-output seam rather than opening the final destination directly. Validation must receive the temporary EPUB path, and successful publication preserves overwrite behavior by using `tempfile::NamedTempFile::persist`, which atomically replaces existing files on supported Unix and Windows targets.

## Operational Notes

- Keep core conversion behavior deterministic.
- Keep cache key tied to PDF bytes + OCR-relevant options + pipeline version.
- Keep frontend and CLI behavior aligned for the same config.
- Keep Tauri command payloads serializable and stable.
- Keep source/destination identity checks and atomic publication format-neutral so additional conversion inputs can reuse them.

## Testing and Validation

Preferred checks (when toolchain is available):

```bash
cargo fmt --all
cargo check --workspace
npm run check
```

If `epubcheck` is installed, test one end-to-end conversion with `--validate`.

## Backlog Ideas

- Tune chapter merge/split thresholds with a broader OCR fixture corpus
- Add additional table-placeholder fixture variants from real OCR edge cases

## Dependency release-age policy

- npm installs in this repo are protected by `min-release-age=10` in `.npmrc`.
- If `npm install` fails because a package is too new, do not repeatedly retry.
- Use this order:
  1. wait until the package ages past 10 days,
  2. pin to an older known-good version,
  3. temporarily bypass with `npm install --min-release-age=0` for urgent fixes, then restore the policy.

<!-- shared-rules:start -->

## Working practices

- Follow explicit task instructions over the default workflow below.
- Writing the code is not finishing the task. A task is finished when
  its changes are merged to main through a PR that passed CI and review,
  or when the user explicitly accepts a different end state.
- Start every task on current code. Fetch first, then cut the task
  branch from origin/main — never from a stale local branch or an old
  checkout. To continue existing work, rebase or merge the latest
  origin/main into it before editing. Never overwrite existing work to
  update.
- Resolve ambiguity before making consequential changes. State low-risk
  assumptions; ask when scope, safety, or expected behavior is unclear.
- Keep changes focused. Do not modify unrelated code, formatting, or comments.
- Prefer surgical edits over whole-file rewrites when the result is equivalent.
- Stage only intended files. Inspect the diff before committing.

## Communication

- Be concise, factual, and direct. Preserve necessary context and uncertainty.
- Avoid praise, motivational filler, emojis, and em dashes in new prose.
- Address the reader directly in user-facing copy.
- Report what was verified and what remains unverified. Never imply that an
  unavailable check passed.

## Code design

- Prefer early returns and shallow nesting. Separate logical blocks with
  blank lines.
- Use descriptive constants or enums for meaningful or repeated values.
  Use existing standard definitions for protocol/specification constants.
  Keep obvious, one-off values inline.
- Use enums for behavioral modes that would otherwise require ambiguous
  boolean arguments.
- Default members to private. Widen visibility only for required consumers,
  and review the change as an API design decision.
- Follow the repository's declared dependency boundaries. UI and controllers
  must use application services rather than directly accessing databases,
  subprocesses, sockets, or other low-level mechanisms.
- Encapsulate low-level mechanics behind domain-oriented interfaces.
- Reuse genuinely shared logic. Avoid speculative abstractions and layers
  that only forward calls.
- Prefer pure functions for business rules and immutable data where practical.
  Isolate side effects; document non-obvious state ownership or synchronization.
- Explain non-obvious intent, constraints, and tradeoffs in comments.
  Do not narrate obvious code. Add examples or diagrams when they clarify it.

## Validation and errors

- Validate untrusted input at entry points. Where practical, represent valid
  states in types and enforce persistent invariants in database schemas.
- Represent absence and failure explicitly.
- Use assertions for internal programming invariants, not external-input
  validation or required runtime error handling.
- Prefer explicit, actionable errors over silent failure or undocumented
  fallback. Document intentional recovery behavior.
- Never report a skipped or failed operation as successful.

## Bug fixes

1. Identify the root cause and define an observable success criterion.
2. Add a regression test and observe the relevant failure before fixing it.
3. Implement the fix and observe the test passing.
4. Check surrounding behavior for regressions and architectural consistency.

If an automated regression test is impractical, document the reproduction
and verification procedure. State any inability to reproduce the failure.

## Verification

- Run relevant tests and lint after changes.
- Choose coverage by affected behavior and risk, not patch size.
- Use integration or end-to-end tests for critical workflows and boundaries;
  test isolated business rules at the lowest effective level.
- Run broader suites for cross-cutting or high-risk changes, and the full
  required release checks before releasing.
- Validate the requested command, options, platform, and configuration.
  Unrelated green CI is not proof that the reported problem is fixed.
- Recheck after the final edit. Distinguish local checks from CI results.

## Commit messages

- Use a capitalized, imperative subject without a final period.
- Target 50 characters; never exceed 72.
- Separate the subject and body with one blank line.
- Wrap body text at 72 characters.
- Explain what changed and why. Leave implementation mechanics to the code.

## Implementation and review

Unless explicitly instructed otherwise:

1. Work on a focused branch cut from the latest origin/main and open a PR
   against main before reporting the task as done.
2. Inspect CI results and completed review feedback for the latest commit.
   A successful reviewer job does not mean the review found no problems.
3. Address important findings or explain why they do not apply. Handle minor
   findings according to the stopping rules below.
4. Evaluate each fix in the surrounding project, add regression coverage,
   and rerun affected checks before pushing.
5. Repeat until a stopping criterion is met.
6. Merge without asking again once the stopping criterion is met, required
   checks pass on the latest commit, and no unresolved blockers or required
   human review requests remain.

### Reviewer context limits

The automated PR reviewer does not see the user's original prompt or
conversation. It may suggest changes that go against or beyond what the
user asked for. Do not implement such suggestions. Note each conflict and
report it to the user at the end of the thread.

### Automated review stopping rules

Judge findings by verified impact, not the reviewer's severity label.
Important findings concern correctness, security, data loss, broken builds,
or materially degraded behavior/performance.

Track completed review rounds and consecutive rounds without important
findings. Reruns of the same revision and integration failures do not count.

- No applicable actionable feedback: finish immediately.
- First minor-only round: optionally fix worthwhile, low-risk findings.
  Do not manufacture another push merely to obtain another review.
- Two consecutive rounds without important findings: stop responding to
  automated nitpicks, even if actionable minor suggestions remain.
  Defer worthwhile leftovers rather than continuing the cycle.
- A confirmed important finding resets the minor-only streak. Address it
  and verify the fix before continuing.

After ten completed rounds, enter stabilization:

- Stop optional cleanup, refactoring, and nitpick fixes.
- One completed review without confirmed important findings is sufficient
  to finish, even if minor suggestions remain.
- Continue only for confirmed important defects. If resolving them stalls,
  report the blockers rather than continuing indefinitely.

These limits end optional automated-feedback work. They do not waive
confirmed blockers, unresolved human review requests, or required checks.

### Reviewer integration failures

After two consecutive reviewer-integration failures, stop and report the
review gap. Do not treat failures as approval. An explicit user instruction
may waive review; report that waiver rather than claiming review passed.

## Ending a task

- A task ends with its changes merged to main — not with code written,
  and not with a PR merely opened. An open PR is work in progress:
  monitor CI on the latest commit, address review findings per the
  stopping rules, and merge once the criteria are met.
- Never finish with uncommitted changes or unpushed commits in the
  worktree. Commit, push, and open or update the PR first.
- If a step is impossible (missing push access, CI failure, reviewer
  outage), report the exact blocker instead. Never present unreviewed or
  unmerged work as finished.
- Before finishing, confirm: the requested behavior is implemented
  without unrelated changes; relevant checks pass on the latest code;
  important review findings are addressed or rejected with reasons;
  deferred suggestions, remaining risks, and validation gaps are
  disclosed.
- The final response states where the work stands: branch, PR, CI
  status, review rounds completed, and whether it is merged.

<!-- shared-rules:end -->

