# Effects paint — K6 phase 2 head ledger

Base: main at `91427f00`. Branch: `k6-effects-paint`.

**Author execution: NOT_RUN.** This revision's pixel, fixture, timing, and WPT results have not been measured by the author. CI and probe parity remain pending for this head; test assertions are not evidence of their outcomes.

## Painted by this head

| Priority | Effect | Head implementation | Analytic test |
| --- | --- | --- | --- |
| 1 | Group opacity | An effect layer encloses the element's background, borders, text, and descendants, extending its clip to scrollable descendant overflow. A single opacity value composites the group above its backdrop. | Overlapping, partially overflowing children |
| 2 | `grayscale()` | Luma matrix on straight sRGB extracted from premultiplied subtree pixels; premultiplication restored once. | Red luma and semitransparent blue composited once |
| 2 | `brightness()` | Component transfer on rasterized subtree pixels. | All source channels halved ±2 |
| 2 | `blur()` | Separable Gaussian convolution of premultiplied channels and alpha on cropped subtree pixels. | Softened boundary; red edge without dark fringe |
| 2 | `drop-shadow()` | Blur and offset painted alpha, then composite premultiplied shadow beneath source; not a shadow of the element's border box. | Alpha silhouette; 412×4000 page with shadow near bottom |
| 2 | Filter function lists | Apply all supported functions in CSS order. When any function is unsupported, skip **the entire list** and paint the original subtree unchanged. | Ordered composition; mixed URL and brightness skipped together |
| 3 | `mix-blend-mode: multiply` | Group blends against the previously painted backdrop at its stacking position. | White backdrop identity; yellow × cyan = green |
| 3 | `mix-blend-mode: screen` | Group blends against the previously painted backdrop at its stacking position. | Red and blue backdrop/channel combination |
| 4 | `clip-path: circle(<percentage>)` | Percentage radius uses the normalised diagonal of its reference box, rather than only its width. | Radius geometry check added in pixel tests |

Existing clip-path inset/polygon and rounded overflow clipping paths are retained without a new claim of pixel parity. Existing box-shadow painting remains in place; the phase-1 comparison showed residual inset, spread and multiple-layer pixel differences.

## Path and resource limits

- **No-effect pages use the unchanged direct painter.** Filter pages record groups and rasterize only the affected subtree; the existing backdrop is not an input to colour or blur filters.
- Each filter group is cropped to the device-space bounding box of its transformed layer clip, with bounded 3σ sampling padding and directional drop-shadow offsets. The clip already includes the painter's filter expansion. Blur σ is capped at 8 device pixels (24-pixel Gaussian support); convolution uses two separable passes with per-pass scratch allocation, not per-pixel allocation.
- The offscreen raster remains premultiplied RGBA8 throughout blur and drop-shadow compositing, and is uploaded with premultiplied alpha. Colour operators temporarily unpremultiply, operate on straight sRGB, then premultiply once. Drop-shadow blurs only an alpha plane; offsets are bounded by the cropped raster surface and rounded to whole device pixels.
- A **4,000,000-pixel cap applies per cropped filter group**, not per page. A page may be much larger and still filter a small group near its bottom. Only a group whose cropped surface exceeds the cap skips its entire filter list and paints unfiltered.
- A non-gating Rust test measures and prints one four-filter page render duration. **Time: NOT_RUN.**

## Known gaps

- Other blend modes (`overlay`, `darken`, `lighten`, `color-dodge`, `color-burn`, `hard-light`, `soft-light`, `difference`, `exclusion`, `hue`, `saturation`, `color`, `luminosity`, `plus-lighter`) still paint with normal blend.
- Any list containing a URL filter, non-linear component transfer or another unsupported primitive is **skipped as a whole**: the original subtree paints unfiltered, with none of that list's supported functions applied. Color matrices and linear component transfers are handled generically, but effects outside the four priority filters are not parity-certified.
- `backdrop-filter` is not a backdrop sampling implementation.
- A single filter group exceeding 4 Mpx is painted unfiltered; σ beyond 8 px is clamped. Fractional drop-shadow offsets, some transformed and viewport-edge extents, and color-space subtleties do not claim full equivalence.
- `clip-path: inset() round`, rounded descendant overflow clipping, and inset/spread/multiple box-shadow pixel differences from the phase-1 ledger remain follow-up targets.
- Animated painting and scrolled sticky/fixed behavior were not part of the static effect pass.

## Verification contract

| Check | Author result | Maintainer result |
| --- | --- | --- |
| Rust analytic pixel tests (opacity, four filters, list atomicity, premultiplied colour, tall-page crop, blending, circle) | NOT_RUN | PENDING |
| Four-filter timing print (informational, non-gating) | NOT_RUN | PENDING |
| 21 screen fixtures | NOT_RUN | PENDING |
| All existing probes and effects probes | NOT_RUN | PENDING |
| WPT, including change review for newly DOWN cases | NOT_RUN | PENDING |
| CI | NOT_RUN | PENDING |

Only this revision's implementation is described above. No new parity percentages or CI outcomes are asserted for this revision.
