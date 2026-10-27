# Compare

Run `npm run fixtures` after building, then `npm run compare`. The suite is
sequential to bound host load. Any render/tool failure fails the command.
Numeric parity thresholds never fail week 1 CI.

Outputs: `out/report.json`, `out/REPORT.md`, per-fixture `diff.png`, and the
measurement section of root `REPORT.md`. Set `REPORT_PATH` to another path to
retain the committed local baseline (CI uses `out/CI-REPORT.md`).

Elements pair by document-order index and must also have identical tag/id/class.
Sequence mismatches are explicit issues. IoU = intersection / union of the two
border boxes. Equal zero-area boxes score 1; different empty boxes score 0.
Maximum edge error is the largest absolute difference of left/top/right/bottom.
The table's geometry match requires all four edges within 1 CSS px, including
non-rendered elements. Inspect individual elements rather than treating the mean
as coverage of CSS.

Pixel comparison covers the larger image extent, padding missing pixels white.
The changed-pixel ratio counts a pixel if any RGB channel differs by more than
16/255. Mean absolute RGB error is also reported in 0..255 units. No image
registration, cropping, blur or antialias masking is applied. PNGs are opaque.
Font rasterization and platform fallback therefore affect these measurements.
Diff PNGs show maximum RGB error as magenta on black.

`fixtures/manifest.json` names feature probes. Each probe reports geometry error
and pixel error in its Chromium box with links to both images and the diff.
The ranked geometry gaps count distinct affected fixtures, grouped by related
elements, with ties ranked by maximum edge error. They identify wrong boxes.
Feature-probe associations are ranked separately in the JSON. Probe mismatches
are associations: inherited text metrics and ancestor positions can affect all
descendants. Computed CSS string differences are listed separately because
equivalent values can serialize differently. No tool claims an unsupported
property solely from a screenshot difference.

Native nodes without a stored computed style (typically non-rendered head
descendants) export an empty diagnostic map. Their DOM entries and geometry
remain in the report. This avoids a separate on-demand Stylo traversal while
exporting diagnostics.
