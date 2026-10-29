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
resource-settling passes, as the one-shot renderer does, then paints an
initial bitmap. `element_by_id` and `hit_test` provide stable DOM targets;
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

Painting is **full-bitmap CPU repaint**, not partial invalid-rectangle
repaint. The retained painter receives the actual device `scale`
(physical pixels per CSS pixel) and draws vector boxes, glyphs and other
scene contents directly into a rounded `width × scale` by
`height × scale` device-pixel buffer. This vector-scaled repaint,
including rasterization, is timed in `paint_us`; it is **not** a
nearest-neighbour upscale of a 1x raster. `paint()` returns a clone
of the current cached RGBA bitmap; that transfer/copy and Android image
publication remain outside the timed gate, so these numbers alone do
not establish end-to-end visible tap latency. Android supplies the
real display width and `displayMetrics.density` as the actual paint scale.
Layout and hit-test coordinates stay in CSS pixels.

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
Each JSON line contains `page`, `width_css_px`, `scale`,
`bitmap_w`, `bitmap_h`, `warmup`, `iterations`, `phase_accounting`, and
`style_us`, `layout_us`, `paint_us`, `total_us` objects with
nearest-rank `p50`, `p95`, `max` in microseconds.
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
cargo run --release --locked -p isr-bench -- 412 --scale 2.625
```

The optional positional argument is the CSS viewport width (default 412).
`--scale` sets the **actual vector paint scale** (default 1.0, allowed
0.25–4.0). Match both the target phone's CSS width and its actual density
when comparing host diagnostics; the host supplies no font files by default
and prints exactly three JSON lines. Only the phone results can satisfy
the gate.

Correctness tests are in `crates/isr-core/tests/interaction.rs`:
three pixel-for-pixel retained-vs-fresh class toggles at 1x, eight
hit points (including two overlapping-order cases), a 2x solid-box
raster-scaling test, and text/attribute mutation parity. CI executes ordinary
workspace tests; the author has not run them.

## Android run

Build and install `android/demo` using the native library packaging
procedure in `docs/ANDROID_SURFACE.md`. For the device, build the
`aarch64-linux-android` native library and package it in
`app/src/main/jniLibs/arm64-v8a/`. The Activity starts its measurement
on a worker executor and uses the real display metrics for
`width_css_px = floor(widthPixels / density)`. It also passes that
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
