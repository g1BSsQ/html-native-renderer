# K14b: damage-sized raster target

**Status: code submitted for independent verification; host/device performance
and byte parity have not been executed in this environment.** The local
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
from one step. The phone reported warm total p95 at the final K14a
head: list 15.3/15.4 ms, gallery 15.2/15.2 ms, form 20.1/20.0 ms
against a 16.7 ms threshold.

The Android interaction benchmark measures `Document::update()`, not
`Document::paint()` or `Bitmap.copyPixelsFromBuffer()`; Android bitmap
publication and the one-shot JNI conversion are **not** part of these
paint numbers.

## Changes and stage definitions

Partial painting now reuses a `VelloCpuImageRenderer` and a Vec allocation,
sized to the damage width/height. The painter shifts the device origin by
an **integer** pixel amount, supplies original-viewport coordinates for
damage-aware culling, and emits only paint commands that may contribute.
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

## Required host diagnostic

At the repository root, run in a stable release-build environment:

```sh
cargo test --locked -p isr-core --test damage_raster
cargo test --locked -p isr-core --test damage_target
cargo test --locked -p isr-core raster_strip_tests
cargo run --release --locked -p isr-bench -- 392 --height 788 --scale 2.75 > k14b-host.jsonl
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
