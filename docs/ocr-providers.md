# Design: Pluggable OCR providers

Status: **Proposal — for review.** No code written yet.

## 1. Goal

Let Baegun convert PDFs using OCR backends other than Mistral. The user picks a
provider; the rest of the pipeline (normalize → chapterize → EPUB → validate) is
unchanged.

Candidates fall into two very different operational classes:

- **Hosted APIs** (API key + REST, no infra) — e.g. Mistral (today), LlamaParse,
  Azure AI Document Intelligence, Google Document AI, Reducto, Firecrawl,
  Upstage. These are near drop-in.
- **Self-hosted models** (you run a GPU server) — e.g.
  [Baidu Unlimited-OCR](https://github.com/baidu/Unlimited-OCR). Powerful and
  free of per-page cost, but a much heavier integration.

Non-goals (this round):
- Bundling/shipping any model or GPU runtime with Baegun.
- Auto-provisioning a server. The user supplies a key or runs the model; Baegun
  is a client.
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
plus per-page base64 image crops plus a `tables[]` array.** A provider is "clean"
to the degree it supplies those three.

## 3. Candidate providers

### 3.1 Hosted, PDF in → per-page markdown out (best fit)

These need no GPU and **accept PDFs directly**, so they avoid the Rust-side
rasterizer the self-hosted path forces on us. Pricing is indicative only —
verify current rates before committing.

| Provider | Fit for Baegun | Rough pricing | Notes |
|---|---|---|---|
| **Mistral OCR** (today) | Native — per-page markdown + image crops + tables | ~$4 / 1k pages (≈$2 batch) | The exact schema Baegun is built around |
| **LlamaParse** (LlamaIndex) | Very close — PDF→markdown per page, hosted REST | Free tier + usage-based | Purpose-built; can return page images/screenshots for the cover |
| **Reducto** | Close — markdown + structured blocks | Usage-based, enterprise-leaning | Strong on complex tables |
| **Firecrawl (Fire-PDF)** | Close — single-call PDF→clean markdown | Usage-based | Rust engine (2026); routes only scanned pages to GPU OCR |
| **Upstage Document Parse** | Close — markdown/HTML output | Usage-based | Good layout handling |

### 3.2 Big-cloud OCR (mature, more mapping work)

Hosted and battle-tested, but native output is block-JSON; you assemble markdown
yourself (Azure's Layout model and Google's Layout Parser get closest):

- **Azure AI Document Intelligence** — Layout model emits markdown directly;
  ~$0.01/page; very mature. Strongest *enterprise* alternative.
- **Google Document AI** — cheap (~$0.001–0.0015/page OCR; free tier 1k/month),
  but markdown is not native.
- **AWS Textract** — JSON blocks only, no markdown → most adaptation work;
  weakest fit.

### 3.3 General multimodal LLMs (hosted, not OCR-specific)

Gemini, Claude, and GPT-4o can take a PDF/page image and be prompted to emit
markdown. Hosted (no GPU), but like the self-hosted path they give **no per-page
image crops or tables array**, and output is prompt-dependent. Useful as a
fallback, not a clean OCR provider.

### 3.4 Self-hosted: Baidu Unlimited-OCR

| | Mistral OCR (today) | Unlimited-OCR |
|---|---|---|
| Type | Hosted REST API | Self-hosted ML model (built on DeepSeek-OCR), MIT |
| Infra | API key only | NVIDIA GPU + CUDA 12.9, Python 3.12, PyTorch, weights |
| Reachable via | `POST /v1/ocr` (Mistral schema) | SGLang **OpenAI-compatible** `/v1/chat/completions`, or Python `infer()` / `infer_multi()` / `infer.py` |
| Input | PDF uploaded directly | **Images** (the Python side rasterizes PDFs with PyMuPDF) |
| Output | `pages[]`: markdown + image crops + tables | Generated **markdown text** for prompt `<image>document parsing.`; **no image crops, tables inline**, possible grounding tokens |

So this one means *Baegun becomes a client of a GPU server the user operates,*
and Baegun must make up for what the model doesn't return (image crops, a tables
array, PDF input). See §6.3 for transport options and §4 for the gaps.

### 3.5 Recommendation

For a second *hosted* provider behind the trait:

- **LlamaParse** is the most natural fit — same PDF-in/markdown-out-per-page
  model as Mistral, so the trait mapping is small and **no rasterizer is needed**.
- **Azure Document Intelligence (Layout)** is the best if you want an
  enterprise-grade, markdown-native option with predictable pricing.

Baidu Unlimited-OCR remains attractive only when avoiding per-page API cost or
keeping data fully on-prem outweighs running a GPU server. It is the heaviest
integration of the bunch.

The one caveat that carries across all non-Mistral providers: **the EPUB cover
depends on per-page base64 image crops** (§4). Mistral returns them; LlamaParse
can return page images via an option; Azure/Google return figure regions you'd
fetch separately; LLMs and Baidu return none. Image fidelity is the thing to
verify per vendor before committing.

## 4. The integration gaps

How much work a provider is reduces to how many of these it forces on us:

1. **No per-page image crops.** Cover selection and `--include-images` depend
   on `OcrImage` base64 payloads. Providers that return text-only (LLMs, Baidu)
   require **Baegun to rasterize PDF pages itself** to synthesize cover/page
   images — something it does not do today. Hosted parsers that expose page
   images (LlamaParse) or figure regions (Azure/Google) avoid most of this.
2. **Image-only input.** The self-hosted SGLang endpoint takes images, not PDFs,
   so rasterization is mandatory there regardless. Hosted parsers accept PDFs.
3. **Tables & grounding tokens.** DeepSeek-OCR-style output (Baidu) embeds tables
   inline in markdown and may emit grounding/`<|ref|>`-style tokens; Baegun's
   `table_format` / `tables[]` handling and a token-stripping pass need adapting.
   Hosted parsers generally return clean markdown tables.

PDF rasterization in Rust means a new native dependency (e.g. `pdfium-render`
or a MuPDF binding) — only needed for the text-only providers (LLMs, Baidu).

## 5. Proposed architecture

### 5.1 Provider trait

Introduce a trait in `baegun-core` and move the existing Mistral code behind
it. The pipeline keeps speaking `MistralOcrResponse` (conceptually the canonical
internal OCR payload; the serde type can stay for cache compatibility).

```rust
pub trait OcrProvider {
    /// Produce the canonical OCR payload for a PDF.
    fn run(&self, cfg: &ConvertConfig, pdf_bytes: &[u8], source_filename: &str)
        -> Result<MistralOcrResponse>;
}
```

- `MistralProvider` — wraps today's `mistral.rs` verbatim.
- `LlamaParseProvider` / `AzureDocIntelProvider` — hosted alternatives (§6.2).
- `UnlimitedOcrProvider` — self-hosted (§6.3).

`lib.rs:102` becomes `provider.run(...)` where `provider` is selected from
`cfg.provider`.

### 5.2 Config / surface changes

- `ConvertConfig` (`models.rs:43`) gains:
  - `provider: OcrBackend` (enum, default `Mistral`).
  - `ocr_base_url: Option<String>` (endpoint for self-hosted / region-specific
    hosted providers, e.g. `http://localhost:30000/v1`).
  - `rasterize_dpi: u32` (default ~200) — only used by text-only providers.
- CLI (`baegun-cli/src/main.rs:56` area): `--provider`, `--ocr-base-url`,
  `--rasterize-dpi`. `--api-key`/`--model` semantics stay; the default model
  string and whether the key is required vary per provider.
- Desktop (`commands.rs:95`): add provider + base-URL fields to the settings
  request and Settings dialog.

### 5.3 Caching

`cache.rs:7` must fold provider, base URL, model, and rasterize DPI into the key
so payloads from different providers for the same PDF don't collide. The stored
format stays the canonical OCR JSON, so `normalize`/`epub` are untouched.

## 6. Backend notes

### 6.1 Mistral (today)

Unchanged; becomes `MistralProvider` behind the trait.

### 6.2 Hosted alternatives (LlamaParse / Azure DI)

Both accept a PDF and return per-page markdown via REST + API key, so the
backend is: submit PDF → poll/await result → map per-page markdown into
`OcrPage`. Open items per vendor:
- **Cover/images:** request page images (LlamaParse) or fetch figure regions
  (Azure) to populate `OcrImage`, or accept no cover for these providers.
- **Tables:** map their markdown/HTML tables onto `OcrTable` (likely clean).
- **Async:** these are job-based APIs; the backend needs a poll loop with the
  existing retry/backoff style from `mistral.rs:166`.

### 6.3 Baidu Unlimited-OCR — transport options

**Option A — HTTP to a self-hosted SGLang endpoint (recommended for this provider).**
1. Rasterize each PDF page to PNG (new Rust dep).
2. For each page, `POST {ocr_base_url}/chat/completions` with the page image
   (base64 data URL) and prompt `<image>document parsing.`, honoring the
   `no_repeat_ngram_size: 35` sampling note from the project README.
3. Collect streamed `delta.content` into the page's markdown; strip grounding
   tokens; map to `OcrPage { index, markdown, images: [rasterized page],
   tables: [] }`.
4. Assemble the `MistralOcrResponse` and hand it to the existing pipeline.

Pros: matches Baegun's Rust-only HTTP-client architecture; GPU server fully
decoupled. Cons: requires the Rust rasterizer; tables only inline.

**Option B — shell out to the project's Python `infer.py` / `infer_multi`.**
Native PDF support via the model's own PyMuPDF, no Rust rasterizer — but couples
Baegun to a Python 3.12 + CUDA environment and the project's CLI contract;
brittle and hard to ship in the desktop app.

**Recommendation: Option A** for this provider.

## 7. Output mapping details (text-only providers)

- **markdown** → `OcrPage.markdown` after stripping any grounding tokens.
- **cover image** → use the rasterized first page so existing cover logic works.
- **`--include-images`** → embeds rasterized full pages, not sub-image crops;
  overlaps heavily with **comic mode**. Decide whether non-comic embedding is
  meaningful here or documented as "comic-style only".
- **tables** → remain inline in markdown; `tables[]` empty; `--table-format`
  effectively a no-op (document it).
- **header/footer** → not separately available; `extract_header/footer` ignored.

## 8. Metadata generation

`metadata::resolve_book_metadata` falls back to a Mistral chat call
(`mistral.rs:59`, `mistral-small-latest`), independent of the OCR provider.
Options: (a) keep requiring a Mistral key for LLM metadata even when OCR runs
elsewhere; (b) skip LLM metadata when no Mistral key is present (already the
behavior — see README). Proposal: **(b)** for now; "metadata via the chosen
provider's LLM" is a separate future item.

## 9. Testing

- Unit: response-mapping per provider, token stripping, cache-key separation.
- Integration: extend `convert_integration.rs` with mocked endpoints
  (wiremock-style) so no GPU/keys are needed in CI.
- Rasterizer: a tiny fixture PDF → expected page count / non-empty PNG.
- CI does **not** call real providers; live paths are manual/local only.

## 10. Phasing

1. **PR1 — abstraction only (no behavior change):** add the `OcrProvider`
   trait, `OcrBackend` enum (default `Mistral`), config/CLI/desktop plumbing,
   cache-key update, move Mistral behind the trait. Ship-able on its own.
2. **PR2 — first hosted alternative (LlamaParse or Azure):** REST backend +
   per-page mapping + mocked integration test. No rasterizer needed.
3. **PR3 — rasterizer + a text-only provider (Baidu / LLM):** add the PDF→PNG
   dependency and the SGLang/LLM client.

## 11. Open questions for review

1. **Which alternative provider first?** Recommendation: a hosted one
   (LlamaParse or Azure DI) before the self-hosted Baidu path, since it ships
   without a rasterizer and serves more users.
2. **Cover/image strategy for non-Mistral providers:** request page images
   where available, rasterize, or accept no cover?
3. **Rasterizer dependency** (only if we do a text-only provider):
   `pdfium-render` vs a MuPDF binding (AGPL/commercial concerns) vs a pure-Rust
   renderer. Licensing/bundling impact, especially for the Tauri desktop build.
4. **Baidu transport:** Option A (HTTP/SGLang) or Option B (Python shell-out)?
   Recommendation is A.
5. **Desktop scope:** expose new providers in the GUI now, or CLI-only first?
6. **Feature degradation:** acceptable that `--table-format`,
   `extract_header/footer`, and sub-image embedding become no-ops for text-only
   providers, documented as such?

## Sources

- [Best OCR Software of 2026 — LlamaIndex](https://www.llamaindex.ai/insights/best-ocr-software)
- [Best Document Parsing APIs to Try in 2026 — Firecrawl](https://www.firecrawl.dev/blog/best-document-parsing-apis)
- [Best PDF Parsers for AI and RAG Workflows in 2026 — Firecrawl](https://www.firecrawl.dev/blog/best-pdf-parsers)
- [Top 10 best OCR APIs of 2026 — Mindee](https://www.mindee.com/blog/leading-ocr-api-solutions)
- [Mistral OCR — Mistral AI](https://mistral.ai/news/mistral-ocr/)
- [Best PDF Extraction Tools in 2026 — Mixpeek](https://mixpeek.com/curated-lists/best-pdf-extraction-tools)
- [Baidu Unlimited-OCR — GitHub](https://github.com/baidu/Unlimited-OCR)
