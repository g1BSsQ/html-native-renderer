# Font family, weight, style and opt-in filesystem policy

## Source of the Linux mappings (CI reference, not a universal default)

The **Chromium reference captures from the Ubuntu 24.04 CI run** are the
ground truth; the independent review's suggested generic-name defaults are
not verified Chromium configuration. The measured PR artifact for head
82c3e468 (week-2-parity run 40504887625, Chromium ref-layout JSON) reports:

| Requested face | Native before revision | Chromium reference | Preferred installed face for next CI |
|---|---|---|---|
| Arial | 385 × 32 CSS px | 396.2 × 28 | Liberation Sans (397 × 28) |
| Georgia | same fallback as unresolved Arial | 367.27 × 27 | Liberation Serif |
| serif | sans face, 32 px line box | 27–28 px | Liberation Serif |
| sans-serif | sans fallback, 32 px | 27–28 px | Liberation Sans |
| monospace | native differed (reference boxes in artifact) | CI reference | Liberation Mono (candidate, NOT_RUN) |
| system-ui | same sans face, 32 px | CI reference | Liberation Sans (candidate, NOT_RUN) |
| cursive and fantasy | same sans face, 32 px | serif-like, 27–28 px | Liberation Serif (candidate, NOT_RUN) |

These are **CI-specific selection preferences**, not a hard-coded claim
about Chromium on every Linux distribution. Exact face equivalence is
pending CI after this change. On other hosts, preferences are inserted
only when the installed family exists; the normal Fontique fallback chain
is retained after the preferred face.

The Chromium reference for 07-table has first-cell width 150.02 CSS px
vs native 140.33 on main; the native font fallback, not table-separator
painting, explains why a wider face is needed. In the prior attempted
single-face alias registration, width changed to 139 px, with worse IoU.
Using the **entire installed family** (real regular/bold/italic faces) and
preserving the fallback list avoids that partial-family problem. 06-stat-tile
retained a 26px maximum edge displacement on main and the PR head;
it is not evidence of a new regression.

## Selection, matching and I/O

By default, `RenderOptions::system_fonts=false`. No fontconfig initialization,
installed-font scans, CLI subprocesses or installed font-file reads are
performed by the font module; the SVG parser's vendored fontdb likewise
does not load system fonts without opt-in. The in-memory Bullet face and
host-provided fonts remain available. Host-injected font files keep their
native family metadata and retain the existing generic-family fallback
registration.

On Linux with `system_fonts=true`, `yeslogic-fontconfig-sys` invokes the
already-linked libfontconfig **in-process**. An opt-in-only `OnceLock`
holds a process-wide mutex-protected fontconfig configuration and source
cache. `FcConfigSubstitute` and `FcFontMatch` select the configured
installed family; `FcFontList` asks only for files belonging to a
selected target family. All weights and slants of that family are
registered with the requested CSS alias. File bytes are cached in-process,
not decoded repeatedly for each render. No `fc-match`, `fc-list`,
external subprocess, enumeration of every font file, or XML traversal
is performed by the renderer's alias adapter.

Only a bounded set of compatibility names receives a substitute: Arial,
Helvetica, Georgia, Times New Roman, Times, Courier New and Courier.
An unknown name does not register an unrelated weak fallback (and thus
does not block the next CSS family in an ordered list). Requested-name
detection checks token boundaries, so "sometimes" does not request
"Times". Where the installed target is absent, the adapter does nothing.
Chromium's measured Georgia→Liberation Serif preference is explicitly
allowed when fontconfig would otherwise choose an unrelated weak default.

Fontique 0.12.0 continues to perform CSS width/style/weight matching
within the chosen family before subsequent fallback families, including
the 600→700 preference and returned synthetic flags. The vendored
`blitz-paint` now honors `synthesis.embolden()` when no real bold face
exists; real bold faces remain preferred and do not require synthesis.
Stylo computes UA bold/italic styles for headings, table headers and
emphasis. Parley's oblique matching threshold is still an upstream
limitation; perfect Chromium style fidelity is not claimed.

## SVG integration and build independence

The complete pinned `blitz-dom` and `blitz-paint` crates are vendored
under `third-party/`, with the pinned upstream revision and dual-licence
provenance in NOTICE. Cargo's `[patch."https://github.com/DioxusLabs/blitz"]`
routes both crates through local paths in **every** build, including raw
Cargo, Android, iOS, downstream users and WPT. No source-checkout
modification script, global RUSTFLAGS or `cfg(svg_font_patch)` exists.
The scoped SVG fontdb is instantiated lazily only for SVG parsing. If
system-font access is enabled, SVG rendering uses a process-wide cached
system fontdb when no injected fonts are present. With additional injected
fonts it creates a separate SVG database for that render.

## Regression evidence and tests

Linux alias tests require Helvetica, Times and Courier to resolve on the
CI's Liberation-font image. Existing tests check the 600→700 weight
selection, synthetic embolden and skew flags, computed UA font styles,
marker painting and SVG filesystem isolation. New **pixel comparisons**
require 700 to paint differently from 400 both for a regular-only
embedded face (synthetic weight) and for a system family with a real bold
face. The five system-font probes are reported separately from the
original data-URI probes and the 21 gated screens. Table separators
remain explicitly deferred. No baselines or parity thresholds changed.

New code, tests, fixture renders, WPT and parity at this revision:
**NOT_RUN by the author**. The measurements above are from earlier CI
artifacts, not a claim that this revision has passed validation.
