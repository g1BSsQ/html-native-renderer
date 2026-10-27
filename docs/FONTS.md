# Linux font resolution and matching

The renderer's **library default remains `system_fonts=false`**. In this
mode `crates/isr-core/src/fonts.rs` does not invoke fontconfig, read a
configuration file, or load installed faces. Supplied font bytes retain their
own family names and are not inserted into generic fallbacks. The compiled
in-memory list-marker font remains registered as family `Bullet`.

## Source of the family and alias table

With `system_fonts=true` on Linux, the source is the **installed fontconfig
configuration on that host**, queried at render startup using `fc-match`
and `fc-list`, plus request-family names read from
`/etc/fonts/fonts.conf` and the XML alias rules in
`/etc/fonts/conf.d` and `/etc/fonts/conf.avail`. The actual selected
family is *never* a compiled-in guess. The CI image is Ubuntu 24.04 with
`fonts-liberation` and `fontconfig` installed; the following rows show
the queries to compare with CI, **not measurements performed by the author**.

| CSS requested family | Fontconfig query | Expected CI compatible target; verify in CI |
| --- | --- | --- |
| Arial, Helvetica | `fc-match Arial`, `fc-match Helvetica` | Liberation Sans (NOT_RUN) |
| Times New Roman, Times | `fc-match 'Times New Roman'`, `fc-match Times` | Liberation Serif (NOT_RUN) |
| Courier New, Courier | `fc-match 'Courier New'`, `fc-match Courier` | Liberation Mono (NOT_RUN) |
| Georgia and additional XML-defined aliases | `fc-match Georgia` and each discovered alias name | Host-selected face (NOT_RUN) |
| serif, sans-serif, monospace | `fc-match serif`, `fc-match sans-serif`, `fc-match monospace` | Host-selected faces (NOT_RUN) |
| system-ui, cursive, fantasy | `fc-match system-ui`, `fc-match cursive`, `fc-match fantasy` | Host-selected faces (NOT_RUN) |

`fc-list` supplies the paths for *all available faces* of each resolved
installed family. When the CSS alias has no installed family of its own,
those faces are registered with the requested family name via
`FontInfoOverride` without changing their intrinsic weights or styles.
An installed exact-name family wins over a substituted alias. The backend's
system families remain available for later fallback. On hosts without the
fontconfig programs or corresponding faces, the collection keeps its
normal system backend resolution; successful substitution is not assumed.

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

The pinned Blitz revision had a separate process-global SVG `FONT_DB` that
called `load_system_fonts` on first SVG use. The minimal modification in
`patches/blitz-svg-scoped-fonts.patch` makes that database render-thread
scoped. The outer render supplies marker and host font bytes and invokes
`load_system_fonts` only with explicit opt-in.

The repository's main CI and `tools/native-check.sh` apply this patch to
the exact Cargo Git checkout before building and set
`RUSTFLAGS=--cfg svg_font_patch`. The untouched WPT workflow and a raw
Cargo invocation do not apply the patch automatically; builds in those
paths do not establish the SVG isolation guarantee. This integration
boundary needs review before this draft can be merged.

## New parity probes

The five `fixtures/probes/font-*.html` pages use installed fonts:
aliases (four 24px inline-block lines), 400/500/600/700 at 21px and
220px width, normal/italic/oblique/bold, all six CSS generics, and a
missing-first ordered family list. The existing twelve probes use a data-URI
font and remain in a separate report table; neither set changes the screen
parity thresholds.

All code, tests, fontconfig resolutions, image comparisons and gate results:
**NOT_RUN by the author**. The first Ubuntu CI run must supply evidence.
