# Static form controls

## Scope

This branch implements a static, script-free representation of common form
controls. The control remains a normal CSS box and participates in block,
inline, flex, grid and table layout. No focus, editing, selection popup,
submission, validation or accessibility interaction is implemented here.

Implemented paths (unverified until CI builds and measures them):

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

## Before-change Chromium probes

Base revision: `d3b896c17ae6445bf4df9a89031ad94a7c0c118f`
(`main`). The probes were authored for this branch, so baseline runs require
checking out these probes against the original engine revision. An author
run was not available; no numerical parity claim is made.

| Probe | Main IoU | Main max edge (px) | Main changed pixels (%) |
|---|---:|---:|---:|
| form-text-inputs | NOT_RUN | NOT_RUN | NOT_RUN |
| form-choice-inputs | NOT_RUN | NOT_RUN | NOT_RUN |
| form-range-color | NOT_RUN | NOT_RUN | NOT_RUN |
| form-action-inputs | NOT_RUN | NOT_RUN | NOT_RUN |
| form-textarea | NOT_RUN | NOT_RUN | NOT_RUN |
| form-select | NOT_RUN | NOT_RUN | NOT_RUN |
| form-buttons | NOT_RUN | NOT_RUN | NOT_RUN |
| form-fieldset-label | NOT_RUN | NOT_RUN | NOT_RUN |
| form-states | NOT_RUN | NOT_RUN | NOT_RUN |
| form-row-flex | NOT_RUN | NOT_RUN | NOT_RUN |
| form-row-table | NOT_RUN | NOT_RUN | NOT_RUN |
| form-appearance-none | NOT_RUN | NOT_RUN | NOT_RUN |

The columns are the existing compare tool's mean element-box IoU, maximum
edge displacement in CSS pixels and percentage of RGB pixels changed above
its threshold of 16/255. To reproduce on the same Linux host, render with
`npm run fixtures` using Chromium via Playwright and native `isr`,
then `npm run compare`. The resulting `out/report.json` and screenshots
are local artifacts. Do not mix host font stacks when interpreting results.

## Known gaps and measurement risks

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

Author build: **NOT_RUN**.

Author `cargo test`, `cargo test -p blitz-dom -p blitz-paint --lib`,
`npm test`, `npm run fixtures`, `npm run compare`, `npm run gate`:
**NOT_RUN**.

Author WPT run/delta against main: **NOT_RUN**. The existing WPT selection
includes `css/css-ui`; no WPT selection, harness, baseline, threshold,
expectation, or exclusion has been changed.
