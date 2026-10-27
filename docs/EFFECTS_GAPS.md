# Effects and stacking gap ledger (K6 phase 1)

**Validation: NOT_RUN.** This document is a plan for the first CI measurement, not a claimed measurement. Every native observation is deliberately **MEASURE_IN_CI**. Expected Chromium behavior is specified for each self-contained property probe; actual behavior and likely root causes remain unverified.

The 25 new HTML probes under `fixtures/probes/` embed the same public data-URI font as existing probes; they contain no scripts or external resource requests. The flat probe manifest adds `group: "effects"` to each new entry while retaining the schema consumed by the existing comparison runner. Effects probes are classified as `suite: "probe"`, excluded from the 21-screen regression gate. Pages measure initial static painting only (fixed inside an unscrolled viewport; sticky without scrolling). The perspective page is an identity-appearance negative control, without 3D movement.

## Expected and pending behavior

The owning component and proposed fix are **hypotheses**, to be validated from CI images and downstream code before a phase-2 implementation.

| Property | Probe | Expected Chromium behavior | Native result | Likely owning upstream component | Proposed phase-2 fix |
|---|---|---|---|---|---|
| opacity | `probes/opacity-block` | Block background, border, and text share 45% opacity. | MEASURE_IN_CI | blitz-paint / vello | Composite block as one opacity group. |
| opacity on nested blocks | `probes/opacity-nested` | Parent and overlapping descendants share one 50% opacity group. | MEASURE_IN_CI | blitz-paint / vello | Flatten subtree once before group-alpha composition. |
| filter: drop-shadow() | `probes/filter-drop-shadow` | Offset blurred shadow follows painted alpha, not only border box. | MEASURE_IN_CI | blitz-paint / vello | Render alpha-aware drop shadow with expanded paint bounds. |
| filter: blur() | `probes/filter-blur` | Entire painted subtree blurs, including borders and text. | MEASURE_IN_CI | blitz-paint / vello | Add subtree blur surface and filter outset. |
| filter: grayscale() | `probes/filter-grayscale` | Grayscale affects target only, not stage background. | MEASURE_IN_CI | blitz-paint / vello | Apply grayscale filter color matrix. |
| filter: brightness() | `probes/filter-brightness` | Brightness scales target colors with clamping. | MEASURE_IN_CI | blitz-paint / vello | Apply brightness transformation to subtree. |
| box-shadow: inset | `probes/box-shadow-inset` | Interior shadow clips to inner box behind target content. | MEASURE_IN_CI | blitz-paint | Paint inner shadow using inset clipping. |
| box-shadow: spread | `probes/box-shadow-spread` | Shadow expands beyond border box by spread distance. | MEASURE_IN_CI | blitz-paint / vello | Respect shadow spread geometry and extent. |
| box-shadow: multiple | `probes/box-shadow-multiple` | Both listed shadows paint in CSS list order. | MEASURE_IN_CI | blitz-paint | Render all shadow layers in order. |
| clip-path: inset() | `probes/clip-path-inset` | Target paint clips to inset rectangle. | MEASURE_IN_CI | stylo / blitz-paint | Resolve inset lengths and clip subtree. |
| clip-path: circle() | `probes/clip-path-circle` | Target paint clips to circular silhouette. | MEASURE_IN_CI | stylo / blitz-paint / vello | Build and apply circle clip path. |
| clip-path: polygon() | `probes/clip-path-polygon` | Target paint clips to polygon boundary. | MEASURE_IN_CI | stylo / blitz-paint / vello | Resolve vertices and paint polygon clipping. |
| overflow: hidden + border-radius | `probes/overflow-hidden-radius` | Overflowing child clips to rounded padding edge. | MEASURE_IN_CI | blitz-paint | Use rounded descendant clip for hidden overflow. |
| overflow: clip + border-radius | `probes/overflow-clip-radius` | Overflowing child clips to rounded edge without scroll container. | MEASURE_IN_CI | stylo / blitz-paint | Honor clip overflow and rounded geometry. |
| z-index: positive | `probes/z-index-positive` | Positive target overlays later positioned sibling. | MEASURE_IN_CI | stylo / blitz-paint | Order positive stacking groups correctly. |
| z-index: negative | `probes/z-index-negative` | Negative positioned target falls behind level-zero content. | MEASURE_IN_CI | stylo / blitz-paint | Honor negative levels and background layers. |
| z-index: auto | `probes/z-index-auto` | Auto stacking observes tree order of positioned siblings. | MEASURE_IN_CI | blitz-paint | Maintain auto paint order. |
| z-index: nested contexts | `probes/z-index-nested` | Nested z=999 stays below sibling z=2 when parent has z=1. | MEASURE_IN_CI | stylo / blitz-paint | Make nested contexts atomic in sort. |
| z-index with opacity < 1 | `probes/z-index-opacity` | Opacity group at z=2 composites above lower sibling. | MEASURE_IN_CI | stylo / blitz-paint / vello | Compose atomic opacity group in proper z layer. |
| position: absolute | `probes/position-absolute` | Absolute target uses positioned stage as containing block. | MEASURE_IN_CI | stylo / blitz-paint | Correct containing-block geometry if CI demonstrates gap. |
| position: fixed | `probes/position-fixed` | Fixed target uses unscrolled viewport coordinates. | MEASURE_IN_CI | stylo / blitz-paint | Correct fixed-position export/capture if CI demonstrates gap. |
| position: sticky (initial) | `probes/position-sticky` | Sticky box stays in initial flow position without scrolling. | MEASURE_IN_CI | stylo / blitz-paint | Correct initial sticky geometry; defer scroll behavior. |
| mix-blend-mode: multiply | `probes/mix-blend-multiply` | Overlap multiplies target and backdrop RGB. | MEASURE_IN_CI | blitz-paint / vello | Composite with multiply against correct backdrop. |
| mix-blend-mode: screen | `probes/mix-blend-screen` | Overlap brightens using screen blend equation. | MEASURE_IN_CI | blitz-paint / vello | Composite with screen against correct backdrop. |
| transform: perspective() negative control | `probes/transform-perspective-no-3d` | At z=0, perspective alone does not visibly change the flat tile. | MEASURE_IN_CI | stylo / blitz-paint | Avoid spurious projection; investigate only if measured. |

## Maintainer: first CI review

For **every effects probe** inspect `out/report.json`, `out/REPORT.md`, the native/Chromium layout JSON pair, and `out/probes/<name>/{out,ref,diff}.png`. Record the **mean per-probe IoU**, **changed pixels** (`pixel.changed_pixel_ratio` as a percentage), and **maximum edge error** (CSS px) against Chromium. Changed pixels are counted when any RGB-channel error exceeds 16/255. Review the entire image difference (especially shadow and filter extents outside `#probe`), not just the reference target's box. A property-probe association or serialization difference is diagnostic, not proof of root cause.

Report the **screen gate** outcome separately; do not change its threshold or baselines. For WPT, inspect `out/wpt/DELTA.md` and `out/wpt/delta.json` where a comparable baseline exists, and record deltas for **css-masking**, **css-transforms**, **css-position**, **compositing**, and **filter-effects**. Mark categories `NOT_SELECTED`, `NO_BASELINE`, or `NOT_AVAILABLE` when not evaluated; do **not** equate an absent category with zero failures. The pinned WPT selection covers the first three named areas, but has no dedicated selected area for the last two. WPT compares native output to native reftests; it is not direct Chromium parity.

## Phase boundary and local diagnostic change

The sole `isr-core` change adds effect/stacking computed-property keys to its layout JSON export and the Rust regression test `exports_effect_and_stacking_computed_style_keys`. That test checks the keys exist (they were absent from the previous code), not paint correctness. The browser reference tool currently exports a smaller computed-style allowlist; these new native-only keys are for direct inspection, not automatically compared computed-style pairs.

No Blitz sources, patches, workspace patch configuration, workflow, gate, baseline, or WPT harness is changed. Phase-2 paint/compositing changes requiring upstream Blitz modifications are deferred until the fonts vendoring PR lands.

**Author execution status: NOT_RUN** — Rust tests, native render, Chromium comparison, screen gate and WPT remain for CI. No numeric outcomes are asserted.
