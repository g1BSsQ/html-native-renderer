# Damage-sized raster target

**Status: maintainer-reported Linux parity at previous head
`2fb93e49`: `damage_target` 5/5 PASS and `damage_raster`
6/6 PASS. New compact-path assertions and current-main integration are
not yet re-tested; host/device performance is not yet measured.** The local
runtime has neither `cargo` nor `rustc`. This work starts from merged
main `6f6be0c6440b4e39f2d2bbaa77064e7a654add7c`. Existing
`isr-bench` workload definitions, fixtures and previous tests are unchanged.

## Diagnosis

A partial frame in the merged implementation executes
`Document::update -> repaint_rect -> render_to_buffer`.
The former `render_to_buffer` always constructed a new CPU renderer and
allocated a full-viewport output, even when damage was small. The
`blitz_paint::paint_scene` traversal culled to the *viewport* rather than
the *damage* rectangle, so scene encoding and glyph/path work included
off-damage elements. The CPU renderer writes RGBA directly: there is no
GPU readback. `repaint_rect` already copied only the damaged pixels
from the temporary full-size target to retained RGBA.

For the supplied 1078 x 2167 device viewport, the old temporary output
is 2,336,026 device pixels (9,344,104 bytes) on **every** partial
frame. The measured damage is 21,844 pixels for list, 192,556 for
gallery, and 1,088,780 for form. This is a concrete allocation and
render-target mismatch, not proof that the entire ~8 ms hypothesis comes
from one step. The phone reported warm total p95 at the final retained-damage
head: list 15.3/15.4 ms, gallery 15.2/15.2 ms, form 20.1/20.0 ms
against a 16.7 ms threshold.

The Android interaction benchmark measures `Document::update()`, not
`Document::paint()` or `Bitmap.copyPixelsFromBuffer()`; Android bitmap
publication and the one-shot JNI conversion are **not** part of these
paint numbers.

## Changes and stage definitions

Partial painting now reuses a `VelloCpuImageRenderer` and a Vec allocation.
For ordinary paint, the internal raster target is **snapped outward to
16-pixel alignment (a multiple of the CPU rasterizer's 4×4 fine tiles)**, including its origin and dimensions. Its
translation is an *integer multiple of 16 pixels* so that glyph/path
coverage keeps the full rasterizer's tile phase. The externally reported
`damage_px` and `damage_rect_px` retain their existing ink-damage meaning;
only the internal allocation is padded, and only unsnapped damage pixels
are copied back. The painter uses original-viewport coordinates for
damage-aware culling.

**Full-viewport raster fallback:** If `Document::scroll_y_css != 0.0`,
partial updates use the **original viewport-sized clipped raster target**,
with no damage-origin translation or extra damage-aware subtree culling.
This is triggered even for an integer device-pixel scroll: the maintainer
observed three persistent one-channel glyph/rounded-edge AA differences
at 400 CSS px × 2.75 density after 16-pixel alignment. The scroll
fallback preserves the original viewport-space f32 transform composition.
It does **not** enlarge the reported damage or change `paint_path`
(`partial` remains `partial`); only the internal render target grows.
For unscrolled viewports, `damage_paint::needs_viewport_raster`
checks both old and new paint snapshots against the damage rectangle.
If an intersecting element uses a background/border/mask image or
gradient, blur/shadow/filter, non-unit opacity, clip-path, blend/isolation
layer, or transform, the renderer uses the original full-viewport-size
clipped target and original viewport paint traversal (while still copying
only the damaged pixels back). Generated pseudo-elements are treated
as unsafe without a reliable independent effect bound. This is a
conservative parity safeguard, not a claim that all those effects
actually fail tile-aligned rasterization. Full-document/unbounded
repainting is unchanged.
Elements with potentially unbounded/overhanging effects are not pruned
by the stricter cull. The full-viewport fallback and opt-in strip
prototype are not modified. Partial output rows are copied into the
retained bitmap using the damage width, without full-buffer copies.

The existing JSON field names and meanings are unchanged. Added
nearest-rank p50/p95/max fields, all in microseconds:

- `encode_us`: paint-command construction inside the CPU renderer callback,
  including text glyph/path encoding and paint traversal
- `raster_us`: CPU backend `flush`, raster, and maintenance after the
  encode callback (for the opt-in legacy strip branch this is an aggregate)
- `readback_us`: zero for a CPU renderer, which writes output directly
- `blit_us`: dirty rows/columns copied back into retained RGBA
- `target_setup_us`: reusable renderer creation/resize and buffer resize

`paint_us` remains the wall time of the entire painting path and need
not equal the exact sum of integer-rounded stage microseconds. The
stage breakdown is diagnostic and partial-specific. For full fallback,
these new fields are zero; `paint_us` still includes full painting.

## Host per-stage timing comparison

Actual per-stage host timings **cannot be reported yet**: this execution
environment has no Rust toolchain, and no host JSONL measurements were
provided. Main `6f6be0c6` did not emit the new per-stage keys, so
equivalent before numbers must be obtained by applying the same
instrumentation to the baseline; reporting zeros as measured baseline
times would be misleading.

| Workload | Version | Encode p50/p95 (ms) | Raster p50/p95 (ms) | Readback p50/p95 (ms) | Blit p50/p95 (ms) | Target setup p50/p95 (ms) |
|---|---|---|---|---|---|---|
| `list_200_text` | main (before) | Not instrumented | Not instrumented | Not instrumented | Not instrumented | Not instrumented |
| `list_200_text` | damage-target (after) | Not measured | Not measured | Not measured | Not measured | Not measured |
| `gallery_selected` | main (before) | Not instrumented | Not instrumented | Not instrumented | Not instrumented | Not instrumented |
| `gallery_selected` | damage-target (after) | Not measured | Not measured | Not measured | Not measured | Not measured |
| `form_invalid` | main (before) | Not instrumented | Not instrumented | Not instrumented | Not instrumented | Not instrumented |
| `form_invalid` | damage-target (after) | Not measured | Not measured | Not measured | Not measured | Not measured |

To populate measured after-values on the host, run the existing interaction
probe from this branch in the **same** release environment as the baseline:

```sh
cargo run --release --locked -p isr-bench -- 392 --height 788 --scale 2.75 > damage-target-host.jsonl
```

The JSONL supplies `encode_us`, `raster_us`, `readback_us`,
`blit_us` and `target_setup_us`, each with p50/p95/max; divide by
1000 for milliseconds. The earlier baseline can be compared on
`paint_us` and `total_us`, but **not** on per-stage values without
instrumenting it equivalently. Before/after phone values are not host
measurements.

## Reported local parity

The maintainer ran the two test files on Linux at `2fb93e49`:
`damage_target` **5/5 PASS** and `damage_raster` **6/6 PASS**.
The earlier 3-pixel scrolled AA mismatch was resolved by the nonzero-scroll
viewport-target fallback. This patch adds an explicit `viewport_target`
flag to `Timings` and makes each parity scenario assert its route.
It adds neutral paint (text + rounded border) cases at multiple y offsets
in a 788-CSS-pixel (2167-device-pixel) viewport, including right/bottom
clipping. These new tests have **not yet been executed**. No tolerance
or byte-parity assertion was weakened.

List row 100 uses nonzero scroll, so `list_200_text` takes the
viewport-sized raster route. The gallery case has non-unit opacity
and a background gradient, so `gallery_selected` also takes the
viewport-sized raster route. For these two pages, potential improvements
come from renderer/buffer reuse **only**, not compact rasterization.
The form page remains a compact-raster candidate; actual route counts
and timing gains require a host/phone run.


## Required host diagnostic

At the repository root, run in a stable release-build environment:

```sh
cargo test --locked -p isr-core --test damage_raster
cargo test --locked -p isr-core --test damage_target
cargo test --locked -p isr-core raster_strip_tests
cargo run --release --locked -p isr-bench -- 392 --height 788 --scale 2.75 > damage-target-host.jsonl
```

Compare each `encode_us`, `raster_us`, `target_setup_us`,
`readback_us` and `blit_us` distribution to `paint_us` for each
of the three workloads. Compare with main at the same release flags,
viewport and fonts; note that the host default has no bundled font,
unlike the phone. If pixel parity or host performance fails, do
not claim the optimization has passed; retain the original full-size
serial target until the backend path is corrected.

## Correctness constraints

The new `damage_target.rs` checks gallery, grid, flex, opacity,
scroll-positioned and transformed partial frames against a freshly
painted viewport at 2.75 device scale. Existing `damage_raster.rs`
tests remain intact. This is necessary because shifted raster targets
can alter fractional-edge coverage. All output comparisons are
byte-for-byte, not tolerance based.
