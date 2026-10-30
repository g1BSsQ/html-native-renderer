# Atomic inline soft-wrap boundary plan

## Required behavior

CSS Text Module Level 3, §5.1, [Line Breaking Details](https://www.w3.org/TR/css-text-3/#line-break-details), defines the following:

- Replaced elements and other atomic inlines have soft-wrap opportunities immediately before and after their margin edges, including next to U+00A0 NO-BREAK SPACE.
- Except for U+00A0, no soft-wrap opportunity is permitted directly between an atomic inline and an adjacent Unicode GL, WJ, or ZWJ-class character. The restriction applies in either order and must follow the Unicode line-break classification rather than a list of special test strings.
- Ordinary spaces, punctuation, and atomic-to-atomic boundaries retain their applicable soft-wrap opportunities. `word-break: keep-all` does not suppress the atomic boundaries.
- `white-space: nowrap` / disabled text wrapping prevents these soft-wrap opportunities, including the U+00A0 exception.

The tested family exercises U+00A0 (001–002); U+202F (003–004); U+2060 (005–006); ordinary text (007); `keep-all` (008); punctuation (009); U+200D (010–011); U+FEFF (012–013); U+180E (014–015); U+034F (016–017); U+2007 (018–019); U+2011 (020–021); U+0F08 (022–023); U+0F0C (024–025); and U+0F12 (026–027). The two `nowrap` variants check that no automatic breaks arise in unwrapped text.

## Evidence

At the pinned base, the line-breaking category has 54 failures; the atomic-inline family has 22 failures and five passes. In 003, an atomic inline is separated from preceding U+202F despite the required no-break boundary. In 010, an atomic inline is separated from following U+200D. The visual evidence reports 258 px of height in the first case versus 158 px in its reference.

## Ownership and limitations

The inline-content builder in `src/layout/construct.rs` emits atomic in-flow boxes and text through distinct builder methods. The inline layout in `src/layout/inline.rs` invokes the unvendored line breaker. The pinned breaker unconditionally offers a post-box wrap opportunity while processing in-flow boxes; it also decides overflow before a box. The builder's text-boundary override does not control these inline-box decisions. Inserting joiner glyphs into the DOM-generated text would alter user text, offsets, shaping, and other break opportunities, so it is not a general correction.

No source implementation, dependency pin, test fixture, expectation, threshold, or baseline is changed in this proposal.

## Required implementation

1. Obtain an unvendored text-layout version or API that models each in-flow atomic inline at a character position (for example U+FFFC) and exposes independent pre-box and post-box soft-wrap decisions. Avoid copying a text-layout implementation into this repository.
2. At inline-content construction, derive the adjacent Unicode line-breaking classes from *final shaped/collapsed text* and the atomic box boundaries. Preserve U+00A0 as an explicit exception; do not equate all Unicode spaces or all format characters.
3. Set the boundary decisions for both directions using the common ancestor's wrapping mode; leave out-of-flow boxes unchanged. Preserve intrinsic sizing, float handling, bidi, line-height, and position/hit-testing behavior.
4. Add tests that assert actual line counts rather than merely successful rendering. Use a constrained inline width and an atomic inline whose width makes the pair overflow. Check both neighbor orders.

## Planned regression tests

- For each U+202F, U+2060 and U+200D, assert that an inline-block plus the adjacent character stays on a single line even when their combined advance exceeds the container. Exercise both `text + atomic` and `atomic + text` directions.
- In paired control cases using the same dimensions, assert two lines when U+00A0 or an ordinary space creates a valid wrap boundary.
- Add representative U+FEFF, U+2007, U+2011 and punctuation cases, plus a row of atomic chips, a replaced image, and `inline-flex`.
- Assert one line under disabled wrapping, including two neighboring atomic inlines.
- Keep each regression test paired with at least one prohibited-boundary assertion so it fails on the pinned base. Verify red-on-base, green-after-change, then run all existing tests without changing their expectations.

## Expected changes and exclusions

The prohibited-boundary cases now failing in 003–027 are expected to improve once the underlying boundary decisions are supported. Existing passing allowed-break and disabled-wrap cases must remain passing. The exact number of flipped tests is **not** established by this plan; geometry, shaping and additional controls can independently cause differences.

This is a documentation-only proposal. No test has been added or executed, no existing failure has been fixed, and no future run is claimed to have passed.
