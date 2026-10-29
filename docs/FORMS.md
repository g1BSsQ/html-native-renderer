# Static form controls

## Scope

This branch implements a static, script-free representation of common form
controls. The control remains a normal CSS box and participates in block,
inline, flex, grid and table layout. No focus, editing, selection popup,
submission, validation or accessibility interaction is implemented here.

Implemented static paths (verified by current-head CI):

- `input` text/email/number/password/search/date; attributes `size`,
  `value` and `placeholder`. Password values are masked. The default
  submission/action types render their labels.
- Checkbox and radio boxes, checked marks, disabled accent; range track/thumb,
  bounded value, and color-input #RRGGBB swatch.
- `textarea` rows/cols, initial text and placeholder; `select` single
  selected option, `size`-based height, optgroup traversal and arrow.
- Text and nested inline content inside `button`; fieldset/legend/label
  continue to use normal CSS layout and UA defaults.
- Common UA padding, borders, backgrounds and disabled styling.
  `appearance:none` suppresses the custom painted glyphs/arrow/track while
  preserving the control's CSS box and author styles.

This is a narrow static-rendering addition. It does not grant the in-memory
renderer filesystem or network access or enable system fonts by default.

## Current-head measurements

At `bd36eadb`, render CI passed (run `40801709222`) and WPT versus main
was **+45 PASS / -36 DOWN** (run `40801709265`). The `08-form-layout`
and `form-row-flex` probes both scored **0.9994** mean box IoU. All other
measured probe scores are recorded in the PR body.

## General inline-edge geometry and whitespace

Non-atomic inline spans contribute physical left/right margin, border, and
padding to line advance and intrinsic sizing. Percentages and `calc()` edges
resolve against the containing block for used layout and a zero percent basis
for intrinsic sizing. Edge markers do not enlarge text-content rectangles.
Collapsible whitespace at decorated inline line boundaries is trimmed
without removing spaces between ordinary or generated inline boxes.

## Known gaps and measurement risks

- `::first-line` edges use the element's style rather than fragment-specific
  font-relative values. The line-breaking out-of-flow mismatch is still
  under investigation.
- No focused RTL or wrapped clone tests yet; the nowrap min-content
  override is root-scoped.
- With vertical writing-mode support off, inline edges read physical
  left/right rather than logical start/end.
- Form-control baseline offsets and fixed size coefficients are
  default-size calibrations, not general font metrics.

- The default input character-cell width uses a bounded font-size
  approximation rather than querying the exact font's `0` glyph advance;
  intrinsic widths must be calibrated from Chromium measurements on the
  CI Linux font stack. No claim of matched box sizes is made.
- The select listbox currently paints one selected label; it does not
  paint all visible options and optgroup labels within `size>1` lists.
  It does not handle multiple selection, scrolling or dropdown opening.
- Date inputs show their raw value rather than a locale-formatted field
  with calendar affordance; search clear buttons and number steppers are
  not painted.
- Native disabled and accent colors, text baselines and fieldset legend
  borders are approximate and need comparison evidence.
- Author CSS can replace default sizing/appearance. This change does not
  implement form validation, submission, editable content or keyboard input.

## Verification

Render CI at `bd36eadb`: **PASS**, run `40801709222`. WPT against main:
**+45 PASS / -36 DOWN**, run `40801709265`. The review changes after
that measured head require new CI verification. WPT selection, harness,
baselines, thresholds, expectations, and exclusions are unchanged.
