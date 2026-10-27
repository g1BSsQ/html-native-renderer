# Chromium reference

Playwright's pinned Chromium is the only browser dependency. The tool needs no
HTTP server or browser extension. Install with `npm ci` and
`npx playwright install chromium` (`--with-deps` on Linux).

```
node tools/chromium-ref/index.mjs fixtures/01-card.html --width 412 --out ref.png --layout ref-layout.json
```

The context has `javaScriptEnabled: false` before the document is loaded.
Document scripts, event handlers and script URLs cannot run. Privileged
Playwright evaluation measures DOM rectangles and waits for `document.fonts.ready`;
this does not enable document scripts. A test checks that a script cannot change
an element's ID. Requests other than `data:` are aborted and recorded.
Service workers are blocked. Pages must be self-contained.

Layout viewport: width from CLI, height 800 CSS px, device scale 1, light scheme.
Auto height is at least 800 and includes positive element bounds and document
scroll height. Capture extends beyond that viewport without resizing it.
The native renderer also retains the 800px layout viewport.
Capture uses CDP `Page.captureScreenshot` with `captureBeyondViewport: true`;
the test checks both image height and a colored pixel below 800px. The comparator
rejects screenshots whose dimensions disagree with their layout JSON.
The 32768px height guard is shared. Oversized widths fail in the native CLI.

Schema version 1 is shared with `isr`: `width`, `height`, `viewport_height`,
`device_scale_factor`, `elements`, `blocked_resources`. Each element has
`index`, `tag`, `id`, `class`, `x`, `y`, `width`, `height`, `computed`.
Elements include html/head/style/title/meta and SVG children, in DOM preorder.
Non-rendered elements have zero border boxes; CSS pixels use scale 1.
The Chromium reference uses the union border rectangle from
`getBoundingClientRect`. Native inline and SVG descendant geometry may differ;
these are measured gaps, not omitted nodes. CSS transforms are not in this suite.
`computed` is diagnostic and does not change the geometry schema.
