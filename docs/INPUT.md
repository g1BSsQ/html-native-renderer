# Retained document input

The retained `Document` accepts raw single-pointer samples and emits gesture
events with stable DOM targets. Recognition is deterministic: only the
timestamps supplied in `PointerEvent` and `tick` affect time-based outcomes.
There is no wall-clock read, timer thread, transform, scroll physics, or
separate input DOM.

## Coordinates and target selection

`PointerEvent { pointer_id, phase, x_css, y_css, time_ms }` uses document CSS
pixels for both coordinates, not bitmap/device pixels. The caller converts
surface coordinates by dividing by the vector paint scale and adding any
vertical viewport scroll offset. `time_ms` is a monotonic millisecond count
from the caller's clock; it need not start at zero.

Only Down calls the existing paint-order `hit_test`. The selected target is
the nearest ancestor-or-self with a `data-gesture` attribute or a nonempty
`data-drag` value. A target can declare:

```html
<div data-gesture="tap doubletap longpress swipe-left swipe-right swipe-up swipe-down"></div>
<div data-drag="move" data-gesture="tap"></div>
```

Unknown tokens are ignored. `data-drag` wins over swipe and pan on that
element. Declaration-free content emits Press, Release and vertical Pan;
a declaration-free link with `href` also emits Tap. A declared element
emits only its requested tap/double-tap/long-press/swipe gestures, plus
Press/Release and drag events when `data-drag` is nonempty.
Selection stays fixed from Down to Up/Cancel, even when moving outside the
element. Hidden and non-hit-testable content follows the engine's existing
visibility and pointer-event rules.

## Recognizer constants

| Name | Value | Meaning |
|---|---:|---|
| `TOUCH_SLOP_CSS_PX` | 8 CSS px | Movement strictly greater than this cancels tap/long-press and starts drag or pan |
| `LONG_PRESS_MS` | 500 ms | Minimum duration while within slop |
| `DOUBLE_TAP_MS` | 300 ms | Maximum interval from previous Up to next Down, inclusive |
| `DOUBLE_TAP_SLOP_CSS_PX` | 24 CSS px | Maximum distance between previous tap Up and next Down, inclusive |
| `SWIPE_MIN_VELOCITY_CSS_PER_S` | 600 CSS px/s | Minimum average Down-to-Up speed |

A swipe requires displacement beyond the 8 CSS px slop and velocity at
least 600 CSS px/s. The dominant displacement axis determines the direction.
DragMove and DragEnd deltas are measured from Down; Pan uses the incremental
vertical delta since the previous Move when vertical motion dominates. DragEnd velocity and swipe velocity
use total displacement divided by Down-to-Up elapsed time. With zero elapsed
time velocity is zero.

## API and rendering

```rust
use isr_core::{Document, PointerEvent, PointerPhase};

let events = document.pointer(PointerEvent {
    pointer_id: 1,
    phase: PointerPhase::Down,
    x_css: 30.0, y_css: 40.0, time_ms: 100,
});
let timing = document.update(); // One incremental resolve/repaint for feedback
let long_press = document.tick(600);
```

`Document::pointer` and `Document::tick(time_ms)` return
`GestureEvents`, an inline-capacity small vector. Move only uses inline
event storage, no hit test and no per-Move heap allocation. The host chooses
what each event means; gesture events do not directly mutate layout.
On Press the target receives the `data-pressed` attribute. On Release,
Cancel or a second-pointer cancellation it is cleared through the same
mutation path used by `set_attribute`. An author can style
`[data-pressed]` for feedback. Batch the mutations and call `update()`
once before reading the refreshed `paint()` pixels. No change is made to
pseudo-class activation logic.

`tick` may be called with a later timestamp even without Move to deliver
LongPress once. A second Down while one pointer is active cancels that
gesture, sends DragCancel if dragging, removes pressed feedback and prevents
the interrupted sequence from producing Tap or DragEnd. Cancel never emits
Tap or DragEnd.

The demo has an input asset page and sends Down, Move, Up, Cancel and
secondary Down samples through the native boundary on one worker thread.
It logs the returned events and refreshes the bitmap for pressed feedback.
A vertical Pan is available for host scrolling.

## Known limits

Input recognizes only one pointer; it has no multi-touch gestures.
There is no gesture arbitration with nested scroll containers, focus or
text-entry dispatch. Pan is a signal for the host, not scroll physics.
DragMove reports displacement but does not move painted pixels or create
layers. Animating or compositing dragged content requires a separate change.
The native demo retains its document on the worker thread that opened it;
input methods must be called on that same thread.
