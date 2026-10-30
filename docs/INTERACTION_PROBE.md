# Interaction performance probe

Status of author-run host benchmarks, device benchmarks, parity tests and
instrumentation: **NOT_RUN**. CI and the maintainer collect the results.
No performance conclusion can be drawn from this change alone.

## Question and binding gate

Can a retained native HTML document process a discrete change and finish
restyle, relayout and repaint within one frame on the target mid-range
**aarch64 phone**?

**GO** only if the **p95 of the per-iteration sum**
`style_us + layout_us + paint_us` is **<= 16,700 microseconds (16.7 ms)**
on the target phone **for each of all three pages**. Otherwise **NO-GO**.
This is the **maintainer's confirmed decision rule**, not an inferred threshold.
Use the `total_us.p95` field, **not** the sum of the three individual p95s.
The emulator and host are diagnostic, never gate measurements.

Motion is restricted to transform and opacity. Discrete user changes may
trigger one style/layout update; animation frames must not repeat layout.
This probe measures a single update after each mutation, not animation
frames, input dispatch, compositor swap or screen presentation.

## What is retained, incremental, and full

`Document::new(html, options)` parses and keeps one DOM and its computed
styles, layout caches and paint-order tree. It performs the initial
resource-settling passes, as the one-shot renderer does. The first
`paint()` or `set_viewport()` call allocates and paints the bitmap;
constructing a long document need not first rasterize its entire height.
`element_by_id` and `hit_test` provide stable DOM targets;
`hit_test` uses the engine's paint-order traversal, which excludes
`visibility:hidden` and skips `pointer-events:none` targets while
allowing eligible descendants.

`set_attribute`, `toggle_class` and `set_text` use the vendored
mutator's damage tracking. `update()` calls the engine's **incremental**
resolve: it restyles invalidated nodes and uses its damaged-subtree layout
cache invalidation. For text or class changes, this may still propagate
through parent layout, including much of a page. The probe does **not**
claim a guaranteed constant-size update.

The engine's public resolve operation combines restyle, damage propagation
and layout. It does **not** offer externally separable phase timings.
Therefore `style_us` is recorded as **0 (unavailable, not zero work)**,
and `layout_us` contains the entire style + layout resolve and the
document-height scan. The `total_us` measurement remains valid for the
binding gate; do not interpret `layout_us` as pure layout time.

Painting is viewport-sized when the caller sets
`Document::set_viewport(scroll_y_css, height_css)`. That call stores a
device-scale RGBA bitmap of only the visible CSS rectangle; layout and
hit-test coordinates remain in document CSS pixels. `paint()` returns a
clone of this cached bitmap, with no timed rasterization after `update()`.
Without `set_viewport`, the retained API still supports full-document
painting for compatibility. The independent one-shot `render()` still
paints the full document with its original byte-identical behavior.

Before the first mutation in a batch, the retained document captures
element paint bounds and style identities. After incremental resolve it
unions **old and new** paint bounds for explicitly touched nodes, their
descendants, and nodes whose computed-style identity or captured ink
rectangle changed. This is a conservative snapshot comparison, not a
complete fragment-level layout damage graph. Bounds include border
boxes, node-local scrollable overflow
(including content past an explicitly sized box), glyph overhang margins,
outlines and box shadows when their extent can be bounded safely. The
union is clipped to the viewport and rounded outward to whole device
pixels.

The partial path does **not** rasterize a small tile-sized image: it
rasterizes a **full-viewport-sized temporary RGBA buffer** under a
pixel-aligned rectangular clip in the original viewport coordinates,
then copies only the dirty rows/columns into the retained bitmap. It
replays the original paint-order scene, including ancestor backgrounds
and layers. Unchanged cached pixels remain intact; an offscreen mutation
may repaint zero pixels. `paint_us` includes full-sized clipped
rasterization and copying; `damage_px` describes the invalidated region,
**not** necessarily the allocation size or the amount of raster work.

If damage is not safely bounded, `update()` repaints the **full
viewport**. This includes root/body background mutations, unknown
bounds, and the presence of a filter or backdrop-filter **on any element
in the document**, or a transform combined with box shadow or outline
ink (including when the transform is on an ancestor), or an
ancestor transform combined with content overflowing an element's box.
Text shadows and outside list markers also use the conservative
full-viewport path. A mutation or detected geometry/style change to
non-atomic inline content, or to an atomic inline box sharing a line
with neighboring inline text/items, also uses the full-viewport fallback
because sibling glyphs can reflow outside the box's own bounds.
If layout shrink re-clamps the scroll offset, `update()` likewise
repaints the full viewport to avoid mixing scroll coordinate spaces.
The filter, shadow and marker checks are whole-document conservative
checks, not just checks of mutated nodes. `Timings::paint_path` reports `partial` or
`full_viewport` for viewport updates; the legacy non-viewport mode
reports `full_document`. `Timings::damage_px` is the invalidated area
in device pixels; zero means the cached bitmap was not touched.
Pixel parity with a fresh viewport paint is required separately from
the timing gate.

Only the retained paint path changed. The on-device benchmark uses the
real display width, height and `displayMetrics.density`, rendering vector
content directly at device resolution. The bitmap-copy operation in
`paint()` and Android image publication remain outside the timed gate;
the measurement does not include end-to-end visible tap latency.

`render()` is unchanged. The retained API currently diverges from
one-shot `render()` in two known cases: `Document` **clamps** measured
CSS height to 32768 instead of returning an error for taller documents,
and it **does not re-materialize counter-generated text** after mutations.
Neither divergence is hidden by this benchmark.

No vendor source or patch, WPT, parity baseline, threshold, expectation,
exclusion or existing fixture has been modified. The existing six-point
hit-test fixture remains under `fixtures/interaction/`; two overlapping
hit checks use an in-memory page without changing any fixture.

## Workloads and JSON records

Each page receives **20 warm-up** mutations/updates that are discarded,
then **200 measured** mutations/updates alternating the value or class.
Each JSON line retains `page`, `width_css_px`, `scale`, `bitmap_w`,
`bitmap_h`, `warmup`, `iterations`, `phase_accounting`, and
`style_us`, `layout_us`, `paint_us`, `total_us` nearest-rank
`p50`, `p95`, `max` objects in microseconds. New fields are
`viewport_height_css_px`, `viewport_scroll_y_css`,
`damage_px` (nearest-rank device-pixel area p50/p95/max), and
`paint_path` (counts of `partial` and `full_viewport` updates).
Each page is scrolled to its mutation target before sampling;
`list_200_text` centers row 100 in the viewport.
`baseline_render_us` is one **fresh one-shot render** of exactly the
same final HTML state after that page's updates, including a fresh
parse/style/layout/paint. The unchanged one-shot baseline remains at **1x**
(`baseline_render_scale: 1.0`) even when the retained repaint runs at
phone density; do not treat it as a density-matched performance comparison.
Baseline is one observation, not a distribution.

| JSON page | Source | Discrete change |
|---|---|---|
| `form_invalid` | `fixtures/08-form-layout.html` | Toggle `.invalid` on one generated form input |
| `list_200_text` | `fixtures/02-list-rows.html` styles | Change the value text in row 100 of 200 |
| `gallery_selected` | `fixtures/09-grid-gallery.html` | Toggle `.selected` on one gallery tile |

Pages are derived in memory at bench startup. The checked-in source
fixtures remain untouched.

## Retained motion and explicit clock

For retained pages only, CSS keyframes and transitions are sampled using
`Document::tick(now_ms: f64) -> (Timings, bool)`. The supplied time is a
monotonic, document-relative timestamp in milliseconds; the returned flag is
true while an active delay, animation iteration, or transition can require
more samples. Stop requesting frames when it is false. Read cached pixels
with `paint()` after `tick()`. After a DOM mutation, call `update()`
once before ticking; the mutation update may relayout, but ticks do not.

The supported motion subset is `transform` (translate, scale and rotate
functions) and `opacity`. CSS `@keyframes`, `animation-name`,
`animation-duration`, `animation-delay`,
`animation-timing-function`, `animation-iteration-count`,
`animation-direction`, `animation-fill-mode`, and their `animation`
shorthand work with linear, ease family, cubic-bezier, and steps timing
functions. `transition` and `transition-*` are sampled when a mutation
changes either supported property. Unsupported animated properties have
their regular static behavior. When transform function lists match,
individual numerical components interpolate; mismatched lists use a
discrete step at 50%, rather than matrix decomposition. Rotation is
expressed in matching angle units. The sampler reads inline style
elements; external keyframe sources are not currently collected.
Transition starts compare computed values before and after a mutation;
the retained sampler also reads matching author timing declarations
when the dependency does not expose the shorthand's effective timing.

`set_reduced_motion(true)` causes sampled keyframes and transitions to
jump to their final directed state on the next tick, and false restores
normal timeline sampling. Infinite animations freeze at their first
iteration's end. The retained path also makes the page's own
`@media (prefers-reduced-motion: reduce)` rules match (and
`no-preference` rules stop matching) by substituting the preference
conditions in the existing author stylesheets and recascading without
layout. Combined media queries retain their other conditions. One-shot
`render()` and its stylesheets are unchanged. A media rule that changes
box geometry still requires a discrete layout update by the host; only
transform and opacity are guaranteed to be layout-free while ticking.

A tick runs the style traversal only and refreshes the retained
paint transforms; it never calls the combined resolve/layout operation.
The old and new painted-ink snapshots are unioned by the existing damage
path, clipped to the viewport, and repainted with the pixel-aligned
partial rectangle. `Timings::layout_us` is always zero for a tick,
`style_us` measures the style-only traversal and its bookkeeping, and
`paint_us` measures the repaint. Some complex effects still require
the conservative full-viewport fallback; the motion fixture deliberately
uses boxes whose damage is bounded.

The shared host and Android interaction bench now emits
`motion_12_cards` as a fourth JSON line. The new fixture is
`fixtures/motion/staggered-cards.html`: twelve cards slide and fade in
for 300 ms with 25 ms staggered delays, plus an infinite opacity pulse.
Each motion record samples exactly 120 ticks at nominal 60 Hz.
The recommended diagnostic run is:

```sh
cargo run --release --locked -p isr-bench -- 392 --height 800 --scale 2.75
```

It uses the same JSON keys as interaction pages, including `total_us`
with `p50`, `p95`, and `max`; `paint_path` counts partial/full
viewport frames, and `damage_px` records affected device pixels.
The motion record uses `warmup: 0`, `iterations: 120`, and
`phase_accounting: "style_only_per_tick_no_layout"`. Evaluate
`total_us.p95 <= 16700` on the target phone; host and emulator figures
are diagnostic. The results table below is reserved for the maintainer.

## Host run

At the repository root, with the repository's Rust toolchain and build
requirements installed:

```sh
cargo run --release --locked -p isr-bench -- 392 --height 800 --scale 2.75
```

The optional positional argument is the CSS viewport width (default 412).
`--height` sets the CSS viewport height (default 800); set it to the actual
screen height in CSS pixels. `--scale` sets the **actual vector paint
scale** (default 1.0, allowed 0.25–4.0). Match both CSS viewport dimensions
and the target phone's actual density
when comparing host diagnostics; the host supplies no font files by default
and prints four JSON lines (three interactions and one motion workload). Only the phone results can satisfy
the gate.

Correctness tests are in `crates/isr-core/tests/interaction.rs` and
`crates/isr-core/tests/viewport_repaint.rs`: retained-vs-fresh pixel
parity, six viewport changes (three benchmark pages plus shadow,
outline, transform), outside-viewport no-repaint, hit ordering, scaled
rasterization, and text/attribute mutations. CI executes ordinary
workspace tests; the author has not run them.

## Android run

Build and install `android/demo` using the native library packaging
procedure in `docs/ANDROID_SURFACE.md`. For the device, build the
`aarch64-linux-android` native library and package it in
`app/src/main/jniLibs/arm64-v8a/`. The Activity starts its measurement
on a worker executor and uses the real display metrics for
`width_css_px = floor(widthPixels / density)` and
`viewport_height_css_px = floor(heightPixels / density)`. It also passes that
**actual density into the retained vector painter**, which renders directly
at device resolution and records `scale`, `bitmap_w` and `bitmap_h`.
It supplies the same bundled font bytes as normal rendering and calls the
**same** benchmark workload via JNI, with 20 warm-ups and 200 measured iterations.

```sh
adb shell am force-stop org.htmlnativerenderer.demo
adb shell am start -n org.htmlnativerenderer.demo/.MainActivity --ez interaction_bench true
adb logcat -s ISR_BENCH:I '*:S'
```

Capture the four `ISR_BENCH {json}` messages, device model, CPU mode,
build type, resolution and density, and an unedited logcat attachment.
Run under stable thermal and power conditions; repeat to assess device
variability rather than promoting one favorable run.

The separate `InteractionBenchSmokeTest` instrumentation invokes all
four JNI pages on an executor but deliberately uses **1 warm-up and 2
samples** to prove the entry point runs on the CI emulator. Its results
are **NON-GATE**. For meaningful distributions use the Activity extra
on the target phone.

## maintainer results (unfilled)

| Source | Device/build | Page | p95 total (ms) | Max (ms) | Verdict | Evidence link |
|---|---|---|---:|---:|---|---|
| Target phone | | form_invalid | | | | |
| Target phone | | list_200_text | | | | |
| Target phone | | gallery_selected | | | | |
| Emulator (non-gate) | | form_invalid | | | | |
| Emulator (non-gate) | | list_200_text | | | | |
| Emulator (non-gate) | | gallery_selected | | | | |

Gate verdict: **undetermined until target-phone results exist**.
