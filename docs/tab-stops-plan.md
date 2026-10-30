# Preserved tab rendering: implementation plan

## Constraint

The current text layout API stores preserved U+0009 in shaped text with no horizontal advance. It exposes mutable inline-box dimensions, but not the tab cluster's line-dependent advance or a hook at the point where a line breaker consumes that cluster. Sizing a replacement inline box after a line has already broken would not reliably preserve the original tab's wrapping, whitespace-sequence, hanging, or justification semantics. Do not substitute spaces or create surrogate inline boxes.

No rendering code is changed in this branch. Correct behavior requires a public per-line tab-advance facility in the text layout dependency, followed by integration in the DOM/layout crate. The existing local shaping-source override must not be expanded into a vendored layout library.

## Rules to implement

1. **Preservation and origin** — Under whitespace-preserving modes, retain U+0009 and advance the next glyph to a stop at a multiple of the tab size from the nearest block container's content-box start edge. The current inline edge, inline padding, and text-indent affect the tab's starting position but never restart the grid. [CSS Text 3 §4.1.2](https://www.w3.org/TR/css-text-3/#white-space-phase-2).
2. **Size** — An integer tab size multiplies the nearest block ancestor's resolved space advance, including letter-spacing and word-spacing. A length is an absolute advance. The computed value inherits, including lengths computed from a parent's relative units. A tab's inline style may override the size without changing the origin. [CSS Text 3 §4.2](https://www.w3.org/TR/css-text-3/#tab-size-property).
3. **Threshold** — If the distance to the next stop is less than 0.5ch, use the following stop; at exactly 0.5ch, use the nearest. This decision must use the tab's position after preceding text and inline edges have contributed to the line. [CSS Text 3 §4.1.2](https://www.w3.org/TR/css-text-3/#white-space-phase-2).
4. **Breaking and hanging** — Under `pre-wrap`, a contiguous sequence of preserved whitespace has a break after the sequence, not between its members, and trailing whitespace hangs. Under `break-spaces`, each preserved space or tab permits a break and trailing whitespace does not hang. [CSS Text 3 §4.1](https://www.w3.org/TR/css-text-3/#white-space-processing).
5. **Alignment** — Tabs do not gain justification expansion. Text-align can move a line, but does not shift the block-origin tab grid, and justified text must retain aligned tab columns across nested inlines. [CSS Text 3 §4.1.2](https://www.w3.org/TR/css-text-3/#white-space-phase-2), [CSS Text 4 §7.3](https://www.w3.org/TR/css-text-4/#text-align-property).

## Proposed integration

- Resolve the inherited tab-size from computed styles on every preserved tab, retaining distinct inline overrides.
- Expose a text-layout hook that receives each preserved tab's position on its actual line (including indent, padding, and text before the tab), the block-origin offset, the resolved tab size, and the half-`ch` minimum.
- Apply the computed advance **during line breaking**, so width constraints, intrinsic widths, line items, wrapping, glyph placement, hit testing and paint all agree. Preserve the source U+0009 and whitespace sequence identity.
- Keep tabs out of justification opportunities, then re-evaluate alignment against the fixed block-origin stops.
- Integrate without changing WPT tools, baseline data, fixtures, exclusions or paint/damage code.

## Required regression tests (not yet added)

All tests must assert following-glyph positions (not merely line counts), must fail against the specified base commit, and must pass with the implementation:

1. `a\tb`, `tab-size:8`, monospace: `b.x == 8 * space_advance`.
2. `a\tb`, `tab-size:25px`: `b.x == 25px` relative to the block content edge.
3. `a\tb`, `tab-size:8`, positive letter-spacing: `b.x == 8 * (space_advance + letter_spacing)`, with word-spacing checked separately.
4. At a position `7.3ch` on an `8ch` grid, advance to `8ch`; at `7.6ch`, advance to `16ch`.
5. With `text-indent:3ch` and leading inline padding, the next glyph remains on the block's `8ch` grid, not a grid restarted at the inline or indented start.
6. With `text-align:justify`, following-glyph columns remain at tab stops while ordinary justification still applies elsewhere.

Re-run the 32 failing cases under `css/css-text/tab-size/`, `white-space/`, `text-align/` and `text-indent/`. Expected flips once implemented: the advance/position cases and affected wrapping/justification cases can pass; the present plan-only PR flips **zero**. None of the six new unit tests has been executed or demonstrated failing on the base: they are acceptance criteria, not claimed results.

## Verification status

No implementation or tests were run. The full workspace regression suite remains unverified.