# Effects paint — K6 phase 2 head ledger

Base: main at `91427f00`. Branch: `k6-effects-paint`.

**Author execution: NOT_RUN.** The head's pixel, fixture, timing, and WPT results are not measured here. The 21-screen gate and all probe values require CI measurement. Do not interpret code changes or test assertions as passed comparisons.

## Painted by this head

| Priority | Effect | Head implementation | Analytic test |
| --- | --- | --- | --- |
| 1 | Group opacity | An effect layer encloses the element's background, borders, text, and descendants, extending its clip to scrollable descendant overflow. A single opacity value composites the group above its backdrop. | Overlapping, partially overflowing children |
| 2 | `grayscale()` | Luma matrix on rasterized subtree pixels; backdrop unchanged. | Red tile → equal luma channels ±2 |
| 2 | `brightness()` | Component transfer on rasterized subtree pixels. | All source channels halved ±2 |
| 2 | `blur()` | Separable, alpha-aware Gaussian pass on subtree pixels; the layer includes filter expansion. | Softened boundary and unchanged distant background |
| 2 | `drop-shadow()` | Blur and offset painted alpha, then composite shadow beneath source graphic; not a shadow of the element's border box. | Shadow present only where alpha was painted |
| 2 | Filter function lists | Apply supported filter graph nodes sequentially in authored CSS order. | Grayscale then brightness has expected composed result |
| 3 | `mix-blend-mode: multiply` | Group blends against the previously painted backdrop at its stacking position. | White backdrop identity; yellow × cyan = green |
| 3 | `mix-blend-mode: screen` | Group blends against the previously painted backdrop at its stacking position. | Red and blue backdrop/channel combination |
| 4 | `clip-path: circle(<percentage>)` | Percentage radius uses the normalised diagonal of its reference box, rather than only its width. | Radius geometry check added in pixel tests |

Existing clip-path inset/polygon and rounded overflow clipping paths are retained without a new claim of pixel parity. Existing box-shadow painting remains in place; the phase-1 comparison showed residual inset, spread and multiple-layer pixel differences.

## Path and resource limits

- **No-effect pages use the unchanged direct painter.** CSS filter pages use recorded offscreen groups; supported filters apply to the painted subtree, not the already-painted backdrop.
- Blur standard deviation is limited to 8 device pixels (24-pixel Gaussian support radius) and uses two separable passes with scratch buffers allocated per pass, not per pixel.
- Drop-shadow blurs only an alpha plane; its signed offsets are bounded by the image dimensions and quantised to whole device pixels.
- Filter rasterization is bounded to pages with at most 1,048,576 pixels. Above that size, effects remain unpainted rather than allocating unbounded surfaces. This is a deliberate fallback, not full CSS support.
- A non-gating Rust test measures and prints one four-filter page render duration. **Time: NOT_RUN.**

## Known gaps

- Other blend modes (`overlay`, `darken`, `lighten`, `color-dodge`, `color-burn`, `hard-light`, `soft-light`, `difference`, `exclusion`, `hue`, `saturation`, `color`, `luminosity`, `plus-lighter`) still paint with normal blend.
- URL-based filters, non-linear component transfers, and unsupported filter graph primitives remain unpainted. Color matrix and linear component-transfer functions are processed generically, but functions outside the four priority effects are not parity-certified.
- `backdrop-filter` is not a backdrop sampling implementation.
- Oversized filter surfaces, blur beyond the bounded radius, fractional drop-shadow offsets, some transformed and viewport-edge filter extents, and color-space subtleties do not claim full equivalence.
- `clip-path: inset() round`, rounded descendant overflow clipping, and inset/spread/multiple box-shadow pixel differences from the phase-1 ledger remain follow-up targets.
- Animated painting and scrolled sticky/fixed behavior were not part of the static effect pass.

## Verification contract

| Check | Author result | Maintainer result |
| --- | --- | --- |
| Rust analytic pixel tests (opacity, four filters, ordered list, blending, circle) | NOT_RUN | PENDING |
| Four-filter timing print (informational, non-gating) | NOT_RUN | PENDING |
| 21 screen fixtures | NOT_RUN | PENDING |
| All existing probes and effects probes | NOT_RUN | PENDING |
| WPT, including change review for newly DOWN cases | NOT_RUN | PENDING |
| CI | NOT_RUN | PENDING |

Only the head implementation is described here; no earlier measured metrics or head parity percentages are asserted.
