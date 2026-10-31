# Interaction performance probe

Status: the retained damage fingerprint enumerates **all supported
computed CSS longhands**, and pseudo-element owners are invalidated on
style changes. Generated pseudo effects (shadows, outlines, filters,
text shadows and transforms) conservatively force a full repaint when
their ink cannot be bounded by the owner's box. Parallel strips remain
disabled by default.

The maintainer measured `c53500bf03f1530756d5c24325e47de536c85ba4`
on the phone: warm total p95 was **15.2/14.8 ms** for
`list_200_text`, **13.5/11.9 ms** for `gallery_selected`, and
**19.0/19.1 ms** for `form_invalid` (the form misses the 16.7 ms gate).
**This review-fix head has not yet been measured on the phone.**
Host before/after benchmarks also remain unmeasured here.

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
For elements with stable border boxes, including text-containing
leaf siblings, it compares resolved paint and text-layout properties
rather than treating a new computed-style allocation as a visual change.
On subsequent updates, matching computed-style identities reuse an
Arc-backed fingerprint from the prior snapshot instead of serializing
the same CSS values again for every unchanged element. Newly allocated
styles are compared by the full supported computed CSS longhand set;
owners with generated ::before/::after content additionally use
conservative style-identity invalidation to capture pseudo-only restyles.
Aggregate scrollable overflow changes on an unchanged container or
root are attributed to the descendant nodes whose ink actually changed.
A paint-only border, colour, background, text-colour or opacity toggle
in a fixed border box therefore invalidates the changed node's ink
rather than unchanged grid/flex siblings or ancestors. Changing
layout (including auto-height borders that shift siblings) still includes
all shifted boxes. This is a bounded snapshot heuristic, not a complete
fragment-level layout damage graph. Generated ::before/::after styles
require explicit owner invalidation when style identities change; the
computed fingerprint enumerates all supported CSS longhands rather
than a hand-maintained paint-property allowlist. Pseudo-element shadows,
outlines, filters, backdrop filters, text shadows and transforms require
a full repaint because their ink may extend outside the owner's bounds. Bounds include border
boxes, node-local scrollable overflow
(including content past an explicitly sized box), glyph overhang margins,
outlines and box shadows when their extent can be bounded safely. The
union is clipped to the viewport and rounded outward to whole device
pixels.

The partial path preserves the original viewport coordinate system:
a pixel-aligned rectangular clip restricts the result, and only dirty
rows/columns are copied into the retained bitmap. The **default is
serial raster for every damage area**, retaining the same clipped
full-viewport buffer as the pre-change implementation. Reusing the
cached signatures avoids the repeated CSSOM serialization that added
frame-invariant work even when only 21,844 pixels were damaged.

The former four-strip path is kept solely as an experimental opt-in
(`ISR_EXPERIMENTAL_RASTER_STRIPS=1`, minimum 300,000 device pixels)
with a persistent thread pool capped at four workers. It still records
the whole scene and generates viewport-sized temporary buffers per
strip. On the supplied phone it increased `form_invalid` paint p50
from 12.5 ms to 23.1 ms, so **it is not enabled by default**. A
production-worthy parallel path requires actual item-to-strip culling
and native-origin subrectangle rasterization into disjoint target
slices without whole-viewport copies; the current backend adapter does
not expose that optimized operation. The old 300k threshold was an
unverified estimate from 14 ns/device pixel, not a measured crossover.

`paint_us` includes rasterization and copying; `damage_px` describes
invalidated area rather than allocation size or all executed work.
Pixel parity with the fresh viewport remains mandatory. Offscreen
updates may repaint zero pixels, while unbounded updates repaint the
entire viewport.

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

The engine-owned motion integration changes narrowly scoped vendored source
and its regenerated patch. The existing damage fingerprint, pseudo-effect
invalidation, raster implementation and regression files are retained.
No WPT, parity baseline, threshold, expectation, exclusion or existing
damage-raster fixture has been modified. The existing six-point
hit-test fixture remains under `fixtures/interaction/`; two overlapping
hit checks use an in-memory page without changing any fixture.

## Workloads and JSON records

The three mutation pages each receive **20 warm-up** mutations/updates
followed by **200 measured** updates. The motion page runs **120 ticks**
with **no warm-up**.
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

## Engine-driven retained CSS motion and explicit clock

Retained `Document` uses the pinned style engine's own the style engine animation sets,
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
are needed. Transitions and animations on other properties use the combined style/layout/paint
resolver on ticks, so width, geometry, and non-compositor paint values remain
consistent with the current layout.

`layout_pass_count()` is an actual layout-invocation counter in the
vendored engine. Fast transform/opacity `tick` calls compare the counter before and after
style/paint work with a debug assertion. `layout_us=0` on
successful ticks therefore represents an enforced invariant, not an
unobserved assumption. `style_us` includes both ink snapshots, the
engine traversal, and damage computation; `paint_us` includes clipped
rasterization and bitmap copying. Together they account for the entire
tick apart from small timer and return overhead.

The pinned the style engine style crate lacks the `prefers-reduced-motion` media
feature (its media-query parser otherwise converts unknown features to
`not all`). A **targeted vendored stylesheet media-prelude
normalization** maps only that missing feature to an always-matching or
never-matching width condition at stylesheet parse time. All other
conditions, selector specificity, rules, timing declarations and
`!important` remain in the style engine. On
`set_reduced_motion(bool)`, the retained document reprocesses its
DOM-owned style elements and recascades on the next tick, without
layout. Existing external stylesheets are not automatically re-fetched
on preference toggles. Reduced motion settles each element's own tracks independently: transitions
return their final underlying computed value, finite keyframes sample the last
iteration according to direction, iteration count, and fill mode, and infinite
keyframes freeze just before the end of their first iteration (99.9999%).
Further reduced-motion frames are not requested; normal sampling uses the
host's explicit clock. Animated properties still use the style engine's
interpolation rather than a replacement sampler.

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

**Source-code finding:** the style engine's `process_animations_for_style` and
`ElementAnimationSet::update_transitions_for_new_style` implement
animation starts and the transition reversing-shortening factor. The
commented animation hooks in the document engine are for the *the alternate configuration* build and do not
establish a the style engine gap. Two source-level gaps are accounted for: the absent
`prefers-reduced-motion` media feature, and forward-filled animations
being dropped when marked Finished before their terminal declarations
are built. The vendored clock integration retains the latter as an
unticked terminal fill while the original engine computes its values.
The maintained regression suites are `engine_motion`, `motion`,
`damage_raster`, and `viewport_repaint`, together with the library tests.
The style-only invariant applies to compositor-only tracks; non-compositor
tracks intentionally run layout and repaint before publishing a frame.

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

For a **host comparison** against the fixed baseline, use two
detached worktrees and preserve the JSONL outputs; the list page is the
small-damage control (21,844 device pixels in the maintainer's phone run):

```sh
git worktree add --detach /tmp/isr-bench-main b821a3ebb51b1fa64dc9119e4872fd5d0fb48a3f
git worktree add --detach /tmp/isr-bench-head damage-raster
(cd /tmp/isr-bench-main && cargo run --release --locked -p isr-bench -- 392 --height 788 --scale 2.75) > /tmp/isr-main.jsonl
(cd /tmp/isr-bench-head && cargo run --release --locked -p isr-bench -- 392 --height 788 --scale 2.75) > /tmp/isr-head.jsonl
python3 - <<'PY'
import json
def load(path):
    with open(path) as handle:
        return {row["page"]: row for row in map(json.loads, handle)}
base, head = load("/tmp/isr-main.jsonl"), load("/tmp/isr-head.jsonl")
assert base.keys() == head.keys()
for name in ("gallery_selected", "list_200_text", "form_invalid"):
    def fields(row):
        return {
            "total_us": {k: row["total_us"][k] for k in ("p50", "p95")},
            "paint_us": {k: row["paint_us"][k] for k in ("p50", "p95")},
            "damage_px": row["damage_px"],
            "raster_threads": row.get("raster_threads", 1),
            "strips": row.get("strips", 1),
        }
    print(json.dumps({"page": name, "main": fields(base[name]),
                      "head": fields(head[name])}))
PY
```

The baseline predates the `raster_threads` and `strips` JSON
fields; both are reported as 1 because its paint path is serial.
These outputs are diagnostic and have **not** been run by the author.

The optional positional argument is the CSS viewport width (default 412).
`--height` sets the CSS viewport height (default 800); set it to the actual
screen height in CSS pixels. `--scale` sets the **actual vector paint
scale** (default 1.0, allowed 0.25–4.0). Match both CSS viewport dimensions
and the target phone's actual density
when comparing host diagnostics; the host supplies no font files by default
and prints exactly four JSON lines (three interaction pages and one motion workload). Only the phone results can satisfy
the gate.

Correctness tests are in `crates/isr-core/tests/interaction.rs` and
`crates/isr-core/tests/viewport_repaint.rs`: retained-vs-fresh pixel
parity, six viewport changes (three benchmark pages plus shadow,
outline, transform), outside-viewport no-repaint, hit ordering, scaled
rasterization, and text/attribute mutations. CI executes ordinary
workspace tests; new retained-raster tests additionally enforce local
paint-only damage, shifted-layout correctness, and byte-for-byte equality
between serial paint and 1/2/3/8 strips across gradients, rounded borders,
glyphs, opacity and overflow clipping. The new head still needs host and target-device measurements; the
prior head's parity and CI results do not establish its performance.

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
**same** three mutation workloads via JNI, with 20 warm-ups and 200 measured
iterations; motion uses its separate fixed 120-tick sequence.

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
four JNI pages on an executor. Its three mutation pages use **1 warm-up
and 2 measured updates**; the motion page uses **120 ticks and no warm-up**
to prove the entry point runs on the CI emulator. Its results
are **NON-GATE**. For meaningful distributions use the Activity extra
on the target phone.

## Maintainer phone results

Warm runs 2 and 3 from three runs of the same verified APK at
`c53500bf03f1530756d5c24325e47de536c85ba4`, thermal 0.
All updates were partial with one raster thread and one strip.
The binding gate requires p95 <=16.7 ms on **both** warm runs.

| Source | Page | Warm total p95 run 2 / 3 (ms) | Damage (device px) | Verdict |
|---|---|---:|---:|---|
| Target phone, c53500bf | `form_invalid` | 19.0 / 19.1 | 1,088,780 | FAIL |
| Target phone, c53500bf | `list_200_text` | 15.2 / 14.8 | 21,844 | PASS |
| Target phone, c53500bf | `gallery_selected` | 13.5 / 11.9 | 192,556 | PASS |

The review-fix head has not yet been remeasured. Host numbers remain
unverified. The form case is still above the frame-time gate.

Prior target-phone runs, supplied by the maintainer (three runs with
the same APK; two warm-run p95 values shown for each build, baseline
`b821a3ebb51b1fa64dc9119e4872fd5d0fb48a3f`, earlier head
`1d1c9c5357691c15c3827013a3dc45f6ff1bb581`):

| Page | Damage px before → after | Total p95 before → after (ms) | Paint p50 before → after (ms) |
|---|---:|---|---:|
| `gallery_selected` | 1,122,700 → 192,556 | 17.4/17.5 → 16.0/14.5 | 15.7 → 10.0 |
| `list_200_text` | 21,844 → 21,844 | 10.7/14.0 → 25.9/25.0 | 4.9 → 9.3 |
| `form_invalid` | 1,088,780 → 1,088,780 | 17.3/19.6 → 33.5/33.3 | 12.5 → 23.1 |

The measured c53500bf head fails the form budget (p95 19.0/19.1 ms).
Gate verdict for the subsequent review-fix head: **undetermined** until
it is measured on the target phone.
