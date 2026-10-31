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

Only the narrowly scoped vendored style clock, media preference and layout
counter code and its regenerated patch are changed. No WPT, parity baseline,
threshold, expectation, exclusion or existing fixture has been modified. The existing six-point
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

## Engine-driven retained CSS motion and explicit clock

Retained `Document` uses the pinned style engine's own Servo animation sets,
keyframe resolver, transition reversing-shortening, cascade, and computed
interpolation. There is no custom animation sampler, author CSS declaration
scraper, saved inline style, or paint-time `!important` override.

`set_clock(now_ms)` establishes the mutation timestamp; call
`update_at(now_ms)` after a mutation to begin transitions at that exact
host time. Legacy `update()` uses the most recently set clock (initially 0).
`tick(now_ms) -> (Timings, bool)` runs the style traversal at the supplied
monotonic milliseconds without invoking the combined layout resolver.
It reads the style engine's animated node IDs before and after traversal,
refreshes their paint transforms, unions old/new ink bounds, and repaints
the clipped damage rectangle. The boolean indicates whether more frames
are needed. Geometry-changing CSS is not a supported animated target.

`layout_pass_count()` is an actual Taffy layout-invocation counter in the
vendored engine. Every `tick` compares the counter before and after the
style/paint operation and asserts it is unchanged. `layout_us=0` on
successful ticks therefore represents an enforced invariant, not an
unobserved assumption. `style_us` includes both ink snapshots, the
engine traversal, and damage computation; `paint_us` includes clipped
rasterization and bitmap copying. Together they account for the entire
tick apart from small timer and return overhead.

The pinned Servo style crate lacks the `prefers-reduced-motion` media
feature (its media-query parser otherwise converts unknown features to
`not all`). A **targeted vendored stylesheet media-prelude
normalization** maps only that missing feature to an always-matching or
never-matching width condition at stylesheet parse time. All other
conditions, selector specificity, rules, timing declarations and
`!important` remain in the style engine. On
`set_reduced_motion(bool)`, the retained document reprocesses its
DOM-owned style elements and recascades on the next tick, without
layout. Existing external stylesheets are not automatically re-fetched
on preference toggles. Reduced mode also chooses the engine's terminal
animation time for finite iterations (first cycle for infinite ones)
without separately interpolating any property.

## Engine diagnosis and verification boundary

`crates/isr-core/tests/engine_motion.rs` asserts actual resolved styles
for delay/fill/direction/iterations, steps and cubic-bezier; class-driven
transitions; interruption and shortening reversal; implicit keyframes;
two-property composition; unitless translation vs px; rotation;
dynamic animation names; and the reduced-motion media idiom. Pixel
regressions additionally cover idle then mutate, reversal, unchanged
inline attributes, new/removed class animations, partial/multiple
keyframes, unit conversion, rotation, and a non-reduced user with
`.01ms !important` only inside the reduce media query.

**Source-code finding:** Servo's `process_animations_for_style` and
`ElementAnimationSet::update_transitions_for_new_style` implement
animation starts and the transition reversing-shortening factor. The
commented animation hooks in Blitz are for the *Gecko* build and do not
establish a Servo gap. Two source-level gaps are accounted for: the absent
`prefers-reduced-motion` media feature, and forward-filled animations
being dropped when marked Finished before their terminal declarations
are built. The vendored clock integration retains the latter as an
unticked terminal fill while the original engine computes its values.
**Runtime status: NOT_RUN** in this
environment (no Rust toolchain). Accordingly none of the other
computed-style cases is claimed verified, passing or failing until
the diagnosis tests run; any failing case must be fixed at the engine
boundary before merging.

## Motion workload

The shared host and Android interaction benchmark emits
`motion_12_cards` as the fourth JSON record, using
`fixtures/motion/staggered-cards.html`. Twelve cards slide and fade for
300 ms with staggered starts and an infinite opacity pulse.
It samples **120 ticks at nominal 60 Hz**, with viewport width 392 CSS
pixels and scale 2.75 (when run with those parameters).
The record retains `total_us.p50`, `total_us.p95`, `paint_path`
and `damage_px`, with `phase_accounting:
"style_only_per_tick_no_layout"`.

```sh
cargo run --release --locked -p isr-bench -- 392 --height 800 --scale 2.75
```

The target-device p95 requirement is 16,700 microseconds. No host or
device timing is asserted from unrun code; CI/device results are owned
by the maintainer.

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
