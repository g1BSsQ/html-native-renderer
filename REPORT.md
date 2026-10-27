# Week 1: desktop offscreen baseline

The CLI uses Blitz's HTML parser, Stylo cascade, Taffy geometry and Parley text
layout. `blitz-paint` sends the resulting boxes to AnyRender's Vello CPU backend
(`anyrender_vello_cpu` 0.18.0). No window, GPU adapter, browser or widget catalogue
is used in native rendering. The script integration crate is not linked.
The resource provider decodes only data URIs and records refused requests.

The 21 authored fixtures cover cards, list rows, headers, badges, progress, stats,
tables, a form-like grid, a gallery, nested blocks, box decoration, typography,
inline SVG and a page longer than the viewport. Results below describe this
bounded suite; they are not a general CSS conformance claim.

Measurements use a 412×800 CSS px layout viewport, scale 1, light scheme and
automatic capture height. Whole-page pixels are compared without alignment.
See [metric definitions](tools/compare/README.md). Zero boxes in the DOM count in
geometry scores; all element results are available in the JSON artifact.

## Observed limitations

Local evidence: `07-table` exports zero rectangles for `thead`, `tbody` and
`tr`, despite drawing their cells; Chromium reports nonzero boxes (up to 392px
edge error). Visual review also shows horizontal table separators in Chromium
that are absent in the native image. `16-inline-svg` draws its icons but exports
zero rectangles for `circle` and `path` (up to 184.19px edge error). These are
concrete geometry/export gaps, distinct from paint support for whole SVG icons.
The element records and corresponding `out/<fixture>/{out,ref,diff}.png` files
provide evidence. Inline, heading and wrapped-text differences may include host
font selection; this is an inference, not a confirmed cause for every mismatch.

## Backend and mobile feasibility

Vello CPU was selected because Blitz already demonstrates its direct image-buffer
path in [the pinned screenshot example](https://github.com/DioxusLabs/blitz/blob/92b11f1bd8ab7975411328b0406a95a09445b4e7/examples/screenshot.rs).
The [AnyRender image backend](https://docs.rs/anyrender_vello_cpu/0.18.0/anyrender_vello_cpu/)
separates image-buffer rendering from window rendering; this avoids display-server
and GPU requirements for the baseline.

Vello CPU renders on the CPU and uses SIMD; its output can be an RGBA buffer.
See [upstream Vello CPU 0.3.0](https://docs.rs/vello_cpu/0.3.0/vello_cpu/).
That avoids GPU API requirements on Android and iOS. Rust lists
`aarch64-linux-android` and `aarch64-apple-ios` as supported targets in its
[platform support documentation](https://doc.rust-lang.org/rustc/platform-support.html).
Target availability alone does not establish that this entire dependency graph
cross-compiles or meets mobile performance targets. RGBA presentation, platform
fonts, packaging, input and lifecycle integration still require platform work.
Android build/device tests: NOT_RUN. iOS build/device tests: NOT_RUN.
No mobile code is included.

## Build measurements

The first successful Linux x86_64 build ran under Ubuntu 24.04 in WSL, Rust
1.95.0, four Cargo jobs, with downloaded dependency sources and a fresh Linux
build target. Cargo reported **2m33s**. This excludes toolchain installation and
dependency downloads. The initial build's unoptimized binary was **70,985,208
bytes** (67.70 MiB), with debug info disabled by the workspace profile. After
the exporter fix, the validated binary was **70,985,768 bytes** (67.70 MiB);
Cargo reported **4m14s** for that rebuild, which recompiled some dependencies
after the test-profile build. These are observed runs, not repeatable benchmark
medians. The first native test build took **5m06s**, then all three tests passed.
No optimized binary measurement is implied by this size. CI records its own
build time and size, which may benefit from its dependency cache.

The host's Windows build could not link because MSVC is absent. Native Windows
build validation: NOT_RUN to completion. Local native images use Linux system
fonts; local Chromium 145.0.7632.6 uses Windows system fonts. The embedded font
bytes are identical. CI runs both engines on Ubuntu; its artifact is the
same-platform comparison. Do not treat the local baseline as platform-neutral.

The first CI attempt stopped before building because Playwright 1.58.2 does not
support the Ubuntu 26.04 userspace now supplied by `ubuntu-latest`. The workflow
keeps that runner label and uses an Ubuntu 24.04 job container for the pinned
browser and build toolchain. CI build, test and parity results come from that
container; they are recorded in the run artifact.

## Open questions

- Which remaining differences come from font fallback versus engine layout?
- Which isolated property probes should replace compound screen probes next?
- How closely do SVG descendant and inline fragment border boxes match CSSOM?
- What CPU time and buffer cost will typical mobile pages incur? NOT_RUN.

<!-- measurements -->

# Week 1 measurements

Native: linux (WSL); Chromium: win32.

| Fixture | Mean IoU | Edges ≤1px | Max edge px | Pixel diff % |
|---|---:|---:|---:|---:|
| 01-card | 0.9995 | 100.0% | 0.31 | 2.10 |
| 02-list-rows | 0.9734 | 70.0% | 6.95 | 1.45 |
| 03-header | 0.9505 | 71.4% | 17.31 | 1.89 |
| 04-badges | 0.9276 | 61.5% | 21.06 | 1.61 |
| 05-progress | 1.0000 | 100.0% | 0.00 | 1.00 |
| 06-stat-tile | 0.9151 | 90.9% | 26.00 | 1.42 |
| 07-table | 0.6780 | 19.2% | 392.00 | 2.78 |
| 08-form-layout | 0.9994 | 100.0% | 0.02 | 2.04 |
| 09-grid-gallery | 0.9998 | 100.0% | 0.02 | 1.44 |
| 10-nested-blocks | 0.9336 | 50.0% | 24.02 | 8.88 |
| 11-margin-collapse | 1.0000 | 100.0% | 0.00 | 1.27 |
| 12-borders-radius | 0.9999 | 100.0% | 0.02 | 1.52 |
| 13-shadows | 0.9998 | 100.0% | 0.02 | 1.56 |
| 14-gradients | 0.9999 | 100.0% | 0.02 | 1.16 |
| 15-embedded-font | 0.9998 | 100.0% | 0.02 | 1.97 |
| 16-inline-svg | 0.8095 | 70.6% | 184.19 | 1.16 |
| 17-text-spacing | 0.9322 | 50.0% | 17.63 | 10.56 |
| 18-ellipsis | 0.9998 | 100.0% | 0.02 | 1.85 |
| 19-flex-alignment | 0.9854 | 66.7% | 4.20 | 2.09 |
| 20-dashboard | 0.9897 | 91.7% | 27.08 | 3.53 |
| 21-long-page | 0.9999 | 100.0% | 0.02 | 1.54 |

Top measured geometry gaps (distinct fixture frequency; ties by maximum edge error):

- `inline element bounds`: 4/21; max edge 26.00px
- `<div> bounds`: 4/21; max edge 24.02px
- `document/body bounds`: 3/21; max edge 24.02px
- `table row/group bounds`: 1/21; max edge 392.00px
- `SVG descendant bounds`: 1/21; max edge 184.19px
- `heading bounds`: 1/21; max edge 27.08px
- `<p> bounds`: 1/21; max edge 17.63px
- `<section> bounds`: 1/21; max edge 17.63px
- `table cell bounds`: 1/21; max edge 9.34px
- `<nav> bounds`: 1/21; max edge 8.08px

Feature-probe associations and computed-style string differences are listed separately in the JSON; neither establishes an unsupported CSS property.


Details and image evidence: `out/report.json` and `out/<fixture>/{out,ref,diff}.png`.
