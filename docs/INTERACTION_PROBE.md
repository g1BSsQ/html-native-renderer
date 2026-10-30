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
element paint bounds, border boxes, and style identities. After
incremental resolve it unions **old and new** ink bounds for explicitly
touched nodes, their descendants, and nodes with changed geometry or paint.
For containers with stable border boxes and ink, it compares computed
paint properties rather than treating a new computed-style allocation as
a visual change. A paint-only border, colour, background, text-colour or
opacity toggle in a fixed border box therefore invalidates the changed
node's ink rather than the entire stable grid/flex ancestor. Changing
layout (including auto-height borders that shift siblings) still includes
all shifted boxes. This is a conservative snapshot comparison, not a
complete fragment-level layout damage graph. Bounds include border
boxes, node-local scrollable overflow
(including content past an explicitly sized box), glyph overhang margins,
outlines and box shadows when their extent can be bounded safely. The
union is clipped to the viewport and rounded outward to whole device
pixels.

The partial path preserves the original viewport coordinate system:
a pixel-aligned rectangular clip restricts the result, and only dirty
rows/columns are copied into the retained bitmap. Small damage uses one
full-viewport-sized clipped raster buffer, unchanged from the serial
path. For damage **at or above 300,000 device pixels**, a recorded scene
is replayed under independent horizontal strip clips on a reusable
worker pool capped at **four threads** and the number of available CPU
cores. The strips have disjoint integer row ranges; every strip retains
the full viewport origin, paint order, layers, opacity and clipping
rather than translating fractional geometry into local tile coordinates.
This intentionally uses full-viewport-sized temporary buffers per
strip until the raster adapter offers an exact viewport-origin subtarget.
The recorded scene is created on the document thread before workers
start. There are no concurrent DOM borrows or writes to the cached
bitmap; the completed strip rows are copied sequentially.

The 300,000-pixel threshold is based on the target-phone measurements:
at approximately 14 ns per damaged pixel, it represents about 4.2 ms
of serial pixel work. The passing 21,844-pixel text update is well below
it; both failing updates (approximately 1.09–1.12 million pixels) are
above it. This is a starting threshold, not a measured crossover from
a threshold sweep. It requires confirmation on the target phone.
Unchanged cached pixels remain intact; an offscreen mutation may
repaint zero pixels. `paint_us` includes scene recording, clipped
rasterization, scheduling and copying; `damage_px` describes the
invalidated region, **not** necessarily allocation size or total work.
Smaller raster targets preserving the same coverage would require
upstream adapter support for viewport-origin subrects and selectable
thread/strip settings.

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
`damage_px` (nearest-rank device-pixel area p50/p95/max),
`raster_threads` (maximum active worker count per sample), `strips`
(maximum strip count per sample), and `paint_path` (counts of
`partial` and `full_viewport` updates).
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
and prints exactly three JSON lines. Only the phone results can satisfy
the gate.

Correctness tests are in `crates/isr-core/tests/interaction.rs` and
`crates/isr-core/tests/viewport_repaint.rs`: retained-vs-fresh pixel
parity, six viewport changes (three benchmark pages plus shadow,
outline, transform), outside-viewport no-repaint, hit ordering, scaled
rasterization, and text/attribute mutations. CI executes ordinary
workspace tests; new retained-raster tests additionally enforce local
paint-only damage, shifted-layout correctness, and byte-for-byte equality
between serial paint and 1/2/3/8 strips across gradients, rounded borders,
glyphs, opacity and overflow clipping. Host and device results still
need to be measured in the relevant environments.

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

Capture the three `ISR_BENCH {json}` messages, device model, CPU mode,
build type, resolution and density, and an unedited logcat attachment.
Run under stable thermal and power conditions; repeat to assess device
variability rather than promoting one favorable run.

The separate `InteractionBenchSmokeTest` instrumentation invokes all
three JNI pages on an executor but deliberately uses **1 warm-up and 2
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
