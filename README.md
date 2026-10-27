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

Pre-alpha. Nothing here is production-ready.
