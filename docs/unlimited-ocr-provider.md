# Design: Pluggable OCR providers (adding Baidu Unlimited-OCR)

Status: **Proposal — for review.** No code written yet.

## 1. Goal

Let Baegun convert PDFs using OCR backends other than Mistral, with
[Baidu Unlimited-OCR](https://github.com/baidu/Unlimited-OCR) as the first
alternative. The user picks a provider; the rest of the pipeline
(normalize → chapterize → EPUB → validate) is unchanged.

Non-goals (this round):
- Bundling/shipping the model or its GPU runtime with Baegun.
- Auto-provisioning a server. The user runs the model; Baegun is a client.
- Replacing the Mistral metadata-generation LLM (see §8).

## 2. How Baegun does OCR today

The pipeline is built entirely around the Mistral response schema:

- `lib.rs:102` calls `mistral::run_mistral_ocr(cfg, &pdf_bytes, source_filename)`
  and gets back a `MistralOcrResponse`.
- That type (`models.rs:69`) is `pages: Vec<OcrPage>`, where each
  `OcrPage` (`models.rs:90`) carries `markdown`, `images: Vec<OcrImage>`
  (base64 crops), `tables: Vec<OcrTable>`, `header`, `footer`.
- `normalize.rs` consumes that schema; `epub.rs` packages it. The **first
  image of the first page becomes the EPUB cover**, and `--include-images`
  embeds the per-page crops.
- The Mistral path uploads the **whole PDF** to the hosted files API, then
  calls `/v1/ocr` (`mistral.rs:173`) which rasterizes/parses server-side.
  Baegun never rasterizes a PDF itself.
- Caching keys on PDF bytes + OCR options + crate version (`cache.rs:7`) and
  stores the raw `MistralOcrResponse` JSON.

The important consequence: **everything downstream assumes per-page markdown
plus per-page base64 image crops plus a `tables[]` array.**

## 3. What Unlimited-OCR is (and isn't)

| | Mistral OCR (today) | Unlimited-OCR |
|---|---|---|
| Type | Hosted REST API | Self-hosted ML model (built on DeepSeek-OCR), MIT |
| Infra | API key only | NVIDIA GPU + CUDA 12.9, Python 3.12, PyTorch, weights |
| Reachable via | `POST /v1/ocr` (Mistral schema) | SGLang **OpenAI-compatible** `/v1/chat/completions`, or Python `infer()` / `infer_multi()` / `infer.py` |
| Input | PDF uploaded directly | **Images** (the Python side rasterizes PDFs with PyMuPDF) |
| Output | `pages[]`: markdown + image crops + tables | Generated **markdown text** for the prompt `<image>document parsing.`; **no image crops, tables inline**, possible grounding tokens |

So "add a provider" really means: *Baegun becomes a client of a GPU server the
user operates,* and Baegun must make up for the three things the model does not
give us (per-page image crops, a tables array, and PDF input handling).

## 4. The three real gaps

1. **No per-page image crops.** Cover selection and `--include-images` depend
   on `OcrImage` base64 payloads. Unlimited-OCR returns text only. To preserve
   cover/comic/image features, **Baegun must rasterize PDF pages itself** and
   synthesize the images — something it does not do today.
2. **Input is images, not PDFs.** The OpenAI-compatible endpoint takes images,
   so rasterization is mandatory for the HTTP transport regardless.
3. **Tables & grounding tokens.** DeepSeek-OCR-style output embeds tables inline
   in markdown and may emit grounding/`<|ref|>`-style tokens. Baegun's
   `table_format` / `tables[]` handling and a token-stripping pass need
   adapting.

PDF rasterization in Rust means a new native dependency (e.g. `pdfium-render`
or a MuPDF binding) — a non-trivial addition to a project that is currently
pure-Rust + reqwest on the OCR path.

## 5. Proposed architecture

### 5.1 Provider trait

Introduce a trait in `baegun-core` and move the existing Mistral code behind
it. The pipeline keeps speaking `MistralOcrResponse` (renamed conceptually to
the canonical internal OCR payload; the serde type can stay for cache
compatibility).

```rust
pub trait OcrProvider {
    /// Produce the canonical OCR payload for a PDF.
    fn run(&self, cfg: &ConvertConfig, pdf_bytes: &[u8], source_filename: &str)
        -> Result<MistralOcrResponse>;
}
```

- `MistralProvider` — wraps today's `mistral.rs` verbatim.
- `UnlimitedOcrProvider` — see §6.

`lib.rs:102` becomes `provider.run(...)` where `provider` is selected from
`cfg.provider`.

### 5.2 Config / surface changes

- `ConvertConfig` (`models.rs:43`) gains:
  - `provider: OcrBackend` (enum `Mistral | UnlimitedOcr`, default `Mistral`).
  - `ocr_base_url: Option<String>` (the SGLang endpoint, e.g.
    `http://localhost:30000/v1`).
  - `rasterize_dpi: u32` (default ~200) for the rasterization step.
- CLI (`baegun-cli/src/main.rs:56` area): `--provider`, `--ocr-base-url`,
  `--rasterize-dpi`. `--api-key`/`--model` semantics stay; for Unlimited-OCR
  the "model" defaults to `baidu/Unlimited-OCR` and the API key is optional.
- Desktop (`commands.rs:95`): add provider + base-URL fields to the settings
  request and Settings dialog.

### 5.3 Caching

`cache.rs:7` must fold provider, base URL, and rasterize DPI into the key so a
Mistral payload and an Unlimited-OCR payload for the same PDF don't collide.
The stored format stays the canonical OCR JSON, so `normalize`/`epub` are
untouched.

## 6. Unlimited-OCR backend — transport options

The user asked for a design doc before choosing the transport. Here are the two
viable shapes with a recommendation.

### Option A — HTTP to a self-hosted SGLang endpoint (recommended)

Flow inside `UnlimitedOcrProvider::run`:
1. Rasterize each PDF page to PNG (new Rust dep).
2. For each page, `POST {ocr_base_url}/chat/completions` with the page image
   (base64 data URL) and prompt `<image>document parsing.`, honoring the
   `no_repeat_ngram_size: 35` sampling note from the project README.
3. Collect the streamed `delta.content` into that page's markdown; strip
   grounding tokens; map to `OcrPage { index, markdown, images: [rasterized
   page], tables: [] }`.
4. Assemble the `MistralOcrResponse` and hand it to the existing pipeline.

Pros: matches Baegun's current Rust-only, HTTP-client architecture; the GPU
server is fully decoupled and can live anywhere; no Python runtime coupling.
Cons: requires the Rust-side rasterizer; tables come through inline in markdown
only (no separate `tables[]`).

### Option B — shell out to the project's Python `infer.py` / `infer_multi`

Baegun invokes the project's own batch inference on the PDF; it rasterizes via
PyMuPDF and writes per-page results that Baegun reads back.

Pros: native PDF support and per-page output handled by the model's own code;
no Rust rasterizer.
Cons: couples Baegun to a Python 3.12 + CUDA environment and the project's CLI
contract; brittle across versions; harder to ship in the desktop app; still no
image crops for the cover.

**Recommendation: Option A.** It keeps the dependency surface in Rust, leaves
the GPU concern entirely on the user's side, and is the same client-of-an-HTTP-
endpoint shape Baegun already has for Mistral. Option B is only attractive if we
explicitly want to lean on the project's own PDF handling and accept a Python
runtime dependency.

## 7. Output mapping details (Option A)

- **markdown** → `OcrPage.markdown` after stripping any grounding tokens.
- **cover image** → use the rasterized first page so the existing cover logic
  in `normalize`/`epub` keeps working.
- **`--include-images`** → embeds rasterized full pages, not sub-image crops.
  In practice this overlaps heavily with **comic mode**; worth deciding whether
  non-comic image embedding is even meaningful for this provider, or should be
  documented as "comic-style only".
- **tables** → remain inline in markdown; `tables[]` stays empty. `--table-format`
  effectively becomes a no-op for this provider (document it).
- **header/footer** → not separately available; `extract_header/footer` ignored.

## 8. Metadata generation

`metadata::resolve_book_metadata` falls back to a Mistral chat call
(`mistral.rs:59`, `mistral-small-latest`). That is independent of the OCR
provider. Options: (a) keep requiring a Mistral key for LLM metadata even when
OCR runs on Unlimited-OCR; (b) skip LLM metadata when no Mistral key is present
(already the behavior — see README). Proposal: **(b)** for now, and treat
"metadata via a local LLM" as a separate future item.

## 9. Testing

- Unit: response-mapping (sample SGLang stream → `OcrPage`), token stripping,
  cache-key separation per provider.
- Integration: extend `convert_integration.rs` with a mocked OpenAI-compatible
  endpoint (wiremock-style) so no GPU is needed in CI.
- Rasterizer: a tiny fixture PDF → expected page count / non-empty PNG.
- CI does **not** run the real model; the GPU path is manual/local only.

## 10. Phasing

1. **PR1 — abstraction only (no behavior change):** add the `OcrProvider`
   trait, `OcrBackend` enum (default `Mistral`), config/CLI/desktop plumbing,
   cache-key update, move Mistral behind the trait. Ship-able on its own.
2. **PR2 — rasterizer:** add the PDF→PNG dependency and a `rasterize` module
   with tests.
3. **PR3 — Unlimited-OCR backend:** the SGLang client + output mapping, docs,
   integration test against a mock endpoint.

## 11. Open questions for review

1. **Transport: Option A (HTTP/SGLang) or Option B (Python shell-out)?**
   Recommendation is A.
2. **Rasterizer dependency:** `pdfium-render` (needs the pdfium lib) vs a MuPDF
   binding (AGPL/commercial licensing concerns) vs a pure-Rust renderer
   (quality/coverage concerns). This choice has licensing and bundling impact,
   especially for the Tauri desktop build.
3. **Desktop scope:** expose this provider in the GUI now, or CLI-only first
   (since it needs a separately-run GPU server)?
4. **Feature degradation:** is it acceptable that `--table-format`,
   `extract_header/footer`, and sub-image embedding become no-ops for this
   provider, documented as such?
5. **Default model string** for the provider (`baidu/Unlimited-OCR`) and whether
   `--api-key` should be optional/ignored for it.
