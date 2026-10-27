# Linux font resolution and matching

The renderer's **library default remains `system_fonts=false`**. In this
mode `crates/isr-core/src/fonts.rs` does not invoke fontconfig, read a
configuration file, or load installed faces. Supplied font bytes retain their
own family names and are not inserted into generic fallbacks. The compiled
in-memory list-marker font remains registered as family `Bullet`.

## Linux alias and generic lookup

When system fonts are enabled, Fontique 0.12.0 uses fontconfig's native
cached family and generic mappings, including the **full ordered** generic
fallback chain. We do not replace that chain with just the first fc-match
result. We also no longer enumerate installed faces with fc-list, traverse
/etc/fonts XML, or eagerly read an installed font family for every configured
alias in every renderer process.

A small list of compatibility-name requests (Georgia, Helvetica, Times New
Roman, Times, Courier New, Courier, Verdana, Trebuchet MS, Tahoma, Calibri,
Cambria, Consolas, Andale Mono, Comic Sans MS, Impact, Palatino, Bookman)
is checked only if the corresponding string occurs in the HTML. For a missing
exact-name family, one `fc-match -f` query reads fontconfig's installed
configuration and cache, returns a **real installed face path**, and registers
just that selected font with a `FontInfoOverride` alias. Fontique handles the
remaining generic and Unicode fallbacks without registration. Fontconfig
substitutions remain host-dependent; full Chromium alias parity is not
established by this heuristic, and selected bold/italic variant matching
may need a subsequent face-specific extension.

Arial is intentionally left on Fontique's existing generic fallback path:
the previous explicit copy of its selected installed face changed 07-table's
intrinsic column widths, regressing the established 21-screen IoU gate.
In the measured PR artifact the first table column narrowed by 1.33px vs
main and the third column start moved 2.67px left; no separator was added.
Using native fontconfig ordering also avoids the single-face generic mapping
that gave incorrect line heights in the generic-family probe.

## CI artifact investigation (existing results, not local reruns)

07-table: the first table cell width was 140.33px on main, 139px on the
failed PR, and 150.02px in Chromium; column boundaries moved, not separator
pixels. 06-stat-tile: the 26px maximum edge displacement is unchanged
from main (the same inline span retains a 48px native height vs 17px in
Chromium); the number text width changed, confirming face metrics rather
than separator painting. The failed alias and generic probes also showed
line wrapping and line-box-height divergence. These observations come from
the completed CI artifacts. Revised-render results remain NOT_RUN.

## Matching path

Stylo's computed style is converted by pinned Blitz
`stylo_to_parley::style` to Parley family lists, numerical font weights,
styles and stretch/width values. Fontique 0.12.0's CSS matching chooses
**width, then style, then weight**, using the CSS 400–500 special ordering
and 600→700 preference where only 400/700 exist; matching runs inside a
family before considering later families. Parley's charmap lookup takes
candidates in this order, selecting a fallback only if the glyph is absent.
The returned font metadata carries synthetic embolden/skew when a requested
face cannot be matched. It already handles Stylo's computed bolder/lighter.

This is reuse of the pinned upstream implementation, not a new matching
algorithm. Its documented oblique style decision threshold is 14° rather
than the 20° noted by its authors for newer CSS Fonts 4 text; exact
Chromium equivalence **has not been verified**. Tests inspect the selected
600/700 face, synthesis flags and heading/table/emphasis computed styles.

## SVG and build preparation

The pinned Blitz revision had a separate process-global SVG `FONT_DB`
which unconditionally discovered system fonts. The replacement is
render-thread-scoped **and lazy**: creating an SVG font scope records policy
and supplied bytes without reading font files. Only parsing an actual SVG
creates a fontdb, with host discovery behind the explicit opt-in. Ordinary
non-SVG WPT renders pay no SVG fontdb initialization cost.

The repository's main CI and `tools/native-check.sh` apply this patch to
the exact Cargo Git checkout before building and set
`RUSTFLAGS=--cfg svg_font_patch`. The WPT workflow now likewise applies the same pinned Blitz patch and sets
`RUSTFLAGS=--cfg svg_font_patch` before its locked release build. A raw
Cargo invocation still does not patch dependencies automatically and does
not establish the SVG isolation guarantee.

## New parity probes

The five `fixtures/probes/font-*.html` pages use installed fonts:
aliases (four 24px inline-block lines), 400/500/600/700 at 21px and
220px width, normal/italic/oblique/bold, all six CSS generics, and a
missing-first ordered family list. The existing twelve probes use a data-URI
font and remain in a separate report table; neither set changes the screen
parity thresholds.

All code, tests, fontconfig resolutions, image comparisons and gate results:
**NOT_RUN by the author**. The first Ubuntu CI run must supply evidence.
