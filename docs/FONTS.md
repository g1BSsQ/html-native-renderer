# Font selection, synthesis and vendor layout integration

## Verification and scope

This branch applies opt-in installed-font aliases, font-family/weight/style
matching, glyph synthesis, explicit CSS face descriptors and an auto-table
distribution correction. The most recent completed review head
`0e4d0fa5` passed all CI checks, including the parity gate (07-table
IoU **0.9191**). The [week-2-parity run 40672195352](https://github.com/g1BSsQ/html-native-renderer/actions/runs/40672195352)
contains the measured font-generics probe: **IoU 0.7537**, native monospace
paragraph **388 × 54** versus reference **388 × 32**, and native system-ui
paragraph **28 px** high versus reference **32 px**. Both matched at head
`82c3e468`, before the unwanted generic preferences.

The latest reported WPT coverage delta against main `16f0603e` is
**+33 / −3**. The three accepted DOWN tests are
`css/CSS2/text/bidi-flag-emoji-02.html`,
`css/CSS2/positioning/position-relative-035.xht`, and
`css/css-flexbox/flexbox_item-clear.html`. These are known
font-metric exposures, not grounds to change the verified serif choice or
non-table layout. The Times-metric Liberation Serif choice changes line
breaking: in particular, `position-relative-035` now fits on one line.
The current edit has **NOT_RUN** local builds, tests or probes; measurements
above belong to prior CI and are not new-head claims.

## Installed font selection

`RenderOptions::system_fonts=false` performs no installed-font discovery or
fontconfig reads. The compiled-in Bullet face, data-URI font faces and
in-memory supplied faces still work. Host-supplied fonts keep their
pre-existing generic fallback registration. SVG font database construction
is lazy and obeys the same host-font opt-in.

On Linux with `system_fonts=true`, an in-process, mutex-protected
fontconfig cache loads only requested alias families and caches the bytes.
Name matching uses token boundaries. Unknown names never create a weak
alias that would mask a later CSS family. The aliases are:

| CSS alias | Installed compatibility family |
|---|---|
| Arial, Helvetica | Liberation Sans |
| Georgia, Times, Times New Roman | Liberation Serif |
| Courier, Courier New | Liberation Mono |

The **Georgia → Liberation Serif** rule is intentionally unconditional
*once Liberation Serif is installed*, even if the host's fontconfig match
for Georgia differs. This reproduces the recorded reference rather than an
arbitrary weak match. It does not register an absent family.

Only four generic preferences are prepended when installed: serif →
Liberation Serif, sans-serif → Liberation Sans, cursive → Liberation Serif,
and fantasy → Liberation Serif. **Monospace and system-ui are not overridden**;
their normal platform/fontconfig generic fallback ordering is retained,
as is the existing per-character fallback chain. This corrects the
font-generics paragraph heights described above without changing the
accepted serif metric behavior.

## Face matching and pixel behavior

Pinned Fontique/Parley 0.12.0 selects real weight/style/width faces first,
including weight 600 → real 700 when 400 and 700 exist. A regular-only
family may request synthesized bold and oblique. CSS `@font-face`
registrations use the declared family, style and weight. A static face
whose weight descriptor is omitted, `auto`, or not resolvable by the
stylesheet parser is registered at CSS's normal weight **400** instead of
silently using the file metadata.

The painter thickens synthesized runs by a nine-offset shifted-fill union
at approximately `fontSize/24`; base glyphs and zero-advance combining
marks receive identical run offsets, avoiding separate displacement. It
does not add synthetic advances. Computed `font-synthesis-weight:none`
prevents thickening. A face with an intrinsic `OS/2 usWeightClass ≥ 700`
is never thickened again even if its declared face weight is 400. Real
bold face selection and the strict painted-ink regression test remain
intact.

Visible C0/C1 control characters are shaped as U+FFFD instead of being
silently lost to fallback or glyph-zero handling. Whitespace controls
(tab, newline, carriage return and form feed) keep their layout handling.

## Automatic table sizing

The vendor table adapter previously forwarded auto columns to ordinary
grid auto tracks, which shared excess width equally. For a definite-width
unconstrained auto table with non-spanning cells, it now measures each
column's min/max-content border-box width and allocates width in proportion
to its **max-content** contribution when the table exceeds the sum of
maxima. Narrow tables, explicit widths, spans and ordinary grids retain
their existing paths. For the 372 px `07-table`, maxima
`86.78125 / 65.765625 / 62.625` (sum `215.171875`) predict
`150.03 / 113.70 / 108.27` px columns against the measured reference
`150.02 / 113.70 / 108.28`.

## Tests, provenance and regeneration

The root `isr-core` pixel tests cover regular-only synthetic bold,
real-bold faces, synthesis opt-out, an embedded OS/2-bold face declared
without a weight descriptor, attached combining marks and C0/C1 controls.
The render CI also runs `cargo test -p blitz-dom -p blitz-paint --lib`
so vendored behavior tests execute. Fontconfig-specific tests skip with a
clear message if the library or required installed families are absent.

Vendored `blitz-dom` and `blitz-paint` are pinned to upstream
`92b11f1bd8ab7975411328b0406a95a09445b4e7`, with six
modified source files listed in NOTICE, one-line modification headers
and reproducible patch files in `third-party/`. The dependency
provenance generator includes both path-based vendored crates (source
`null`) and their pinned upstream commit. The build uses Cargo's path
patches, not runtime checkout changes.

No parity baseline, gate threshold, WPT expectation, exclusion or harness
was edited for this revision. Build/test/parity/WPT/mobile invocations
by the author: **NOT_RUN**.
