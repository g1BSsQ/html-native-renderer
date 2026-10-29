# Static form controls

## Scope

This branch implements a static, script-free representation of common form
controls. The control remains a normal CSS box and participates in block,
inline, flex, grid and table layout. No focus, editing, selection popup,
submission, validation or accessibility interaction is implemented here.

Implemented static paths:

- `input` text/email/number/password/search/date; attributes `size`,
  `value` and `placeholder`. Password values are masked. The default
  submission/action types render their labels.
- Checkbox and radio boxes, checked marks, disabled accent; range track/thumb,
  bounded value, and color-input #RRGGBB swatch.
- `textarea` rows/cols, initial text and placeholder; `select` single
  selected option, `size`-based height, optgroup traversal and arrow.
- Text and nested inline content inside `button`; labels retain CSS
  layout. A block fieldset with a rendered in-flow, non-floated legend uses
  special block-start border/legend layout; without that legend, an all-inline
  fieldset retains normal inline layout and text runs. The border is painted
  centered on the legend with a notch across the legend's border box.
- Common UA padding, borders, backgrounds and disabled styling.
  `appearance:none` suppresses the custom painted glyphs/arrow/track while
  preserving the control's CSS box and author styles.

This is a narrow static-rendering addition. It does not grant the in-memory
renderer filesystem or network access or enable system fonts by default.

## General inline-edge geometry and whitespace

Non-atomic inline spans contribute physical left/right margin, border, and
padding to line advance and intrinsic sizing. Percentages and `calc()` edges
resolve against the containing block for used layout and a zero percent basis
for intrinsic sizing. Edge markers do not enlarge text-content rectangles.
Collapsible whitespace at decorated inline line boundaries is trimmed
without removing spaces between ordinary or generated inline boxes.

## Fieldset and native-control rendering

In a block fieldset, only the first direct in-flow, non-floated legend
that generates a layout box participates in special legend positioning.
Without a rendered legend, a legend-less all-inline fieldset retains
ordinary inline text layout. The block-start border stroke is centred on
the legend's block-axis centre, with a notch across the legend width.
Following content starts after the larger of the legend margin-box height
and the block-start border width, followed by padding. A declared or
parent-supplied definite fieldset height is preserved. Auto-height
fieldset measurements reserve the shared border/legend space consistently
for ordinary block, flex, and grid parents.

Checkbox, radio and range inputs use a baseline at their bottom content
edge when native appearance is enabled; native color inputs use a
reference-calibrated baseline subtracting all vertical padding and border
insets. None of these native-choice baselines depend on the default
13.33px text-input font-size gate. With `appearance:none`, atomic
controls use margin-box alignment.

Rendering reference:
https://html.spec.whatwg.org/multipage/rendering.html#the-fieldset-and-legend-elements

## Known gaps and measurement risks

- `::first-line` edges use the element's style rather than fragment-specific
  font-relative values. The line-breaking out-of-flow mismatch is still
  under investigation.
- No focused RTL or wrapped clone tests yet; the nowrap min-content
  override is root-scoped. `unbroken_ascii_word` only handles all-letter
  ASCII words: a hyphen, digit, or non-ASCII letter adjacent to an inline
  edge can still introduce an artificial min-content break.
- With vertical writing-mode support off, inline edges read physical
  left/right rather than logical start/end.
- Form-control baseline offsets and fixed size coefficients are
  default-size calibrations, not general font metrics.

- The default input character-cell width uses a bounded font-size
  approximation rather than querying the exact font's `0` glyph advance;
  intrinsic widths need calibration against the CI reference font stack;
  this is not an exact glyph-metric guarantee.
- The select listbox currently paints one selected label; it does not
  paint all visible options and optgroup labels within `size>1` lists.
  It does not handle multiple selection, scrolling or dropdown opening.
- Date inputs show their raw value rather than a locale-formatted field
  with calendar affordance; search clear buttons and number steppers are
  not painted.
- Native disabled/accent colors and ordinary text-input baselines remain
  approximate. A measured default text-input top edge is 1px too high.
  Rendered legends are centered over the block-start fieldset border; the
  top region reserves the larger of the border width and legend margin height.
  Floated and out-of-flow legends are excluded from this special handling.
- Fieldset legend geometry is incomplete for floated siblings that follow the
  rendered legend (these siblings are not shifted with other content) and for
  a legend that is not the first child of the fieldset.
- The legend's nonzero top margin is dropped by the layout offset, while
  border painting uses the legend border-box height. Default UA legends have
  no margin, but authored margins can expose a discrepancy.
- Author CSS can replace default sizing/appearance. This change does not
  implement form validation, submission, editable content or keyboard input.
