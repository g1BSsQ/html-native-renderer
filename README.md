# html-native-renderer

An experimental renderer that draws **script-free HTML/CSS** natively, without a WebView.

It is a rendering engine, not a component mapper. It does not match markup to a catalogue of stock native widgets. Instead it:

1. computes every element's style with a real CSS cascade;
2. computes its box geometry with a real layout engine;
3. draws exactly those boxes, text and images.

Native gestures, scrolling and input attach to the drawn boxes.

## Pipeline

```
HTML + CSS (no scripts)
  → html5ever        parse
  → Stylo            selectors, cascade, computed style
  → Taffy            block / flex / grid layout
  → Parley           text shaping and line layout
  → display list     paint order, clipping, hit testing
  → native surface   desktop first, then Android, then iOS
```

The starting point is [Blitz](https://github.com/DioxusLabs/blitz), which already connects these libraries. We reuse working code instead of rewriting it.

## Scope

- **Bounded, not "all of CSS".** Coverage grows from a measured allowlist of elements and properties, one feature at a time. Each addition comes with tests.
- **No JavaScript, no network subresources.** The input is a self-contained document.
- **Acceptance is parity with a browser.** The same document is rendered by Chromium and by this engine. We compare box geometry and screenshots, and track Web Platform Tests where they apply.

## Stages

1. **Desktop CLI.** Input: an HTML file. Output: a PNG and a layout-tree JSON. Plus a comparison tool against headless Chromium.
2. **Android surface.** The same engine in one Android view, with touch, scroll, drag and text input.
3. **iOS surface.**

## Rules for contributors

- **Provenance.** Keep every upstream copyright and licence notice. Record where reused code came from in `NOTICE`.
- **Licences.**
  - Our code is Apache-2.0.
  - Dependencies keep their own licences; Stylo is MPL-2.0.
  - Do not vendor MPL code without keeping it under MPL in its own files.
- **Public test data only.** Fixtures are hand-written or come from public test suites with compatible licences. No private product data.
- **Every change builds.** Run `cargo build` and the fixture renders before pushing.

## Status

Pre-alpha. Week 1 includes an offscreen CLI and measured fixture suite. Nothing
here is production-ready. See [REPORT.md](REPORT.md) for measured limitations.

## Build and render

Requires Rust 1.95, a C/C++ linker, libclang for Stylo's build, and system fonts.
On Ubuntu:

```sh
sudo apt-get install build-essential pkg-config libclang-dev libfontconfig1-dev fonts-liberation
cargo build --locked -j 4
cargo test -j 4
target/debug/isr render fixtures/01-card.html --width 412 --height-auto --out out/card.png --layout out/card.json
```

The CLI opens no window. It links Blitz at exact commit
`92b11f1bd8ab7975411328b0406a95a09445b4e7` (`v0.3.0-beta.4`), and uses its
Vello CPU painting backend. HTML scripts have no execution engine. The only
subresources it accepts are `data:` URIs; external and filesystem requests are
refused and listed in the layout JSON. The **CLI explicitly enables**
`system_fonts: true`, retaining its existing system-font fallback; it reads
installed font files through the platform font system and may read platform
font configuration. Document URLs cannot request filesystem resources.
This experimental process is not a sandbox for hostile documents.

`--height-auto` retains an 800 CSS px layout viewport and captures at least 800px
and up to 32768px of page content at scale 1. Width is bounded to 1..8192px.
The JSON includes every DOM element in document order, tag/id/class and its
border-box x/y/width/height in CSS px. Inline and SVG geometry limitations appear
in the comparison results. See [the shared schema](tools/chromium-ref/README.md).

## In-memory library API

`crates/isr-core` exposes `render(html: &str, RenderOptions) -> Result<Rendered>`.
`Rendered` contains RGBA8 pixels, image dimensions, document-order element boxes
in CSS pixels, and refused resource URLs. `rendered.layout()` retains the CLI's
existing JSON schema. `RenderOptions` contains `width`,
`system_fonts: bool` (default `false`), and `fonts: Vec<Vec<u8>>` for
font bytes supplied directly in memory.

**With `system_fonts=false`, the library performs no filesystem access
and does no system font discovery.** The only available fonts are those
declared using `data:` URI `@font-face` rules in the input and the
in-memory font bytes supplied through `RenderOptions.fonts`.
**With `system_fonts=true`, the library reads installed font files via the
platform font system and may read its configuration.** This is an explicit
host permission; it is never enabled by document content.
**The library never performs network access, and documents can never trigger
filesystem access.** Document subresources may use only `data:` URLs;
HTTP(S), `file:`, and other resource URLs are refused and recorded.
The CLI opts into system fonts and remains the file/PNG/JSON I/O adapter
with its original arguments. The compiled `system-fonts` dependency feature
makes the host opt-in possible; it does not enable runtime discovery by default.

**Known geometry limitation:** a `<tr>` box is exported as the union of
its laid-out cell boxes. A `rowspan>1` cell therefore extends the exported
box of the row it starts in, rather than producing a separate
per-row fragment for the spanning cell.

## Measure against Chromium

```sh
npm ci
npx playwright install --with-deps chromium
npm test
npm run fixtures
npm run compare
npm run gate
```

The pinned Playwright reference disables document JavaScript and blocks external
resources. Outputs are `out/<fixture>/{out,ref,diff}.png`, paired layout JSON,
`out/report.json` and a concise table in `REPORT.md`. Parity numbers are diagnostic
in week 1. Build, test and tool failures fail CI. See
[reference details](tools/chromium-ref/README.md) and
[comparison methodology](tools/compare/README.md).

The 21 screen fixtures are the regression-gated set. The twelve single-property
pages in `fixtures/probes/` are independent font-controlled diagnostics using an
embedded data-URI font; they appear separately in `out/report.json` and the
generated report. `baselines/ci.json` records the printed numbers from the
successful Ubuntu CI job for `e72fce3d`; fixture-level sources override
that origin for `07-table` and `16-inline-svg` with measurements from
CI run `40467251947`, job `122072057250`. The gate rejects missing or extra
screen fixtures, mean-IoU decreases greater than 0.002, or pixel-difference
percentage increases greater than 0.25 points per fixture. An observed parity
improvement must update the baseline and be explained in its PR. Measurements
for this change are NOT_RUN by the author; CI is the measurement source.

There are 21 independent, hand-authored fixtures with no network resources.
`fixtures/manifest.json` identifies their feature probes. To edit the common
fixture scaffolding, use `tools/author-fixtures.py` with the exact public font
recorded in NOTICE. Python is not needed to render or compare the checked-in pages.

On a Windows host without MSVC, build in an existing Ubuntu WSL environment.
Use `CARGO_TARGET_DIR=target/linux` there, then set `ISR_WSL=Ubuntu-24.04` and
`ISR_BIN=target/linux/debug/isr` for the Windows Node fixture runner. This mixes
Linux native fonts with Windows browser fonts; report that platform difference.
For comparable Linux-only numbers, run both tools within Linux as CI does.
