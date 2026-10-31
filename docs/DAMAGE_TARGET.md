# Damage-sized CPU raster targets

**Verification:** Maintainer local Linux and CI render run **41184816386**
at head `c3855623`: `isr-core --lib` **151 tests PASS**;
`damage_target` **6/6 PASS**, `damage_raster` **6/6 PASS**,
and `viewport_repaint` **20/20 PASS**.
No phone performance PASS is claimed.

## Problem and implementation

The original bounded partial repaint reused the damage rectangle for the
retained-bitmap copy but still allocated and rasterized a viewport-sized CPU
output. For a 392 CSS-pixel by 788 CSS-pixel viewport at scale 2.75, the
device target is 1078 × 2167 = 2,336,026 pixels (9,344,104 RGBA bytes),
even when the dirty region is substantially smaller. The CPU backend
writes RGBA directly; there is no GPU readback.

A reusable `VelloCpuImageRenderer` and RGBA buffer now serve partial paints.
For compact-eligible paint, `blitz_paint::paint_scene_damage` prunes safe
off-damage elements while preserving normal viewport-origin transforms.
The target origin and extent round outward to 16-device-pixel multiples,
which preserve the CPU rasterizer's **4×4 fine-tile phase**; 16 is an
alignment choice, not the fine-tile size. The aligned output can extend
past the viewport's right or bottom edge. It is **not** clamped before
rasterization.

The pinned CPU rasterizer accepts a **signed destination crop offset**:
the scene is encoded and antialiased at its original full-viewport
coordinates, but only the aligned compact destination pixmap is written.
This preserves half-device-pixel coverage and the original strip indices,
including an incomplete final strip (e.g. viewport height 770 gives a
two-row strip at y=768..769). Only the actual unsnapped damage rectangle
is copied back into the retained full-viewport bitmap. The public
`damage_px`, `damage_rect_px` and `paint_path` meanings are unchanged.

**Conservative viewport-target path:** Nonzero viewport scroll and
intersecting opacity/compositing, gradient/image, masks, filters, shadows,
transforms or unbounded pseudo paint use a viewport-sized target, with
the same paint-command and layer sequence as a fresh full repaint.
Only reported damage pixels are copied into the retained bitmap.
`paint_path="partial"` does not by itself imply a compact target;
`Timings.viewport_target` distinguishes the actual path.
Full-document and unbounded repaint behavior is unchanged.

The optional parallel-strip path also retains full-viewport strip
coordinates and crops each strip's aligned output, rather than
translating scene geometry or adding an extra clip layer.

## Byte-exact parity and verification

The neutral text/rounded-border compact cases assert
`viewport_target == false` and compare **byte-for-byte** against fresh
full-viewport painting. They include seven offsets, including the
half-device-pixel interior position (102,514), a near-origin half-pixel
case (2,2), and right/bottom viewport-edge cases.
Other damage-target fixtures assert their expected compact or
viewport-target path. No test tolerance was introduced or weakened.

```sh
cargo test --locked -p isr-core --lib
cargo test --locked -p isr-core --test damage_target
cargo test --locked -p isr-core --test damage_raster
cargo test --locked -p isr-core --test viewport_repaint
```

The listed suites passed in the maintainer's Linux run and the CI
render run cited above.

## Instrumentation and benchmark boundaries

The existing JSON field meanings are preserved. Additional
partial-paint stage fields, in microseconds, are:

| Field | Meaning |
|---|---|
| `encode_us` | Paint traversal and command encoding |
| `raster_us` | CPU rasterization, flush and maintenance |
| `readback_us` | Zero: the backend is CPU-only |
| `blit_us` | Copy of the dirty rows/columns into retained RGBA |
| `target_setup_us` | Renderer creation/resize and reusable-buffer resizing |

`paint_us` remains whole-path wall time; the stage values need not
sum exactly because of timing boundaries and rounding.
Existing host per-stage before/after measurements have **not**
been supplied; do not report unmeasured values or treat absent
baseline instrumentation as zero runtime. On Android,
`Document::update()` timing excludes bitmap publication.

```sh
cargo run --release --locked -p isr-bench -- 392 --height 788 --scale 2.75 > damage-target-host.jsonl
```

`list_200_text` runs at nonzero scroll, and `gallery_selected`
has opacity/gradient effects: **both use the viewport target**,
so any improvement there is from renderer/buffer reuse, **not**
compact rasterization. The form case is a compact candidate.
Current phone measurements for this PR head remain pending.

## Vendored crates and maintenance cost

- **`anyrender_vello_cpu` 0.18.0**: exact published source from
  `DioxusLabs/anyrender` at `cf0f14776102ec775af61bcdcaac2ab345f4fafb`,
  except `src/image_renderer.rs`, which adds `render_window`.
  Its patch is `third-party/anyrender_vello_cpu.patch`.
  The original `filters.rs` and published scene behavior are retained.
- **`vello_cpu` 0.3.0**: pinned source at
  `f3000c8d9e7a9c7e6abb09b9587238bf8b4860ff`.
  Five modified Rust modules backport signed destination offsets
  and preserve original strip/tile indexing. The patch is
  `third-party/vello_cpu.patch`.

Both crates are used through `[patch.crates-io]`. Copied source paths,
origin, licences and modification lists are in `NOTICE` and each
crate's `SOURCE.md`; `third-party/rust-dependencies.json`
preserves their original released commits even with local path overrides.
This creates an ongoing maintenance cost: **every upstream version bump
requires re-vendoring the exact published source, regenerating both
patches, and rerunning byte-parity tests**. We plan to propose the
signed destination-offset support upstream to reduce this fork.
