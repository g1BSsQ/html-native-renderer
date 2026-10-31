// Copyright 2026 g1BSsQ. Licensed under Apache-2.0.
// Raw input coordinates and distances are in document CSS pixels.
use crate::NodeRef;
use smallvec::SmallVec;

/// Movement must exceed 8 CSS px to leave the tap/long-press region.
pub const TOUCH_SLOP_CSS_PX: f64 = 8.0;
/// An unmoved press becomes long-press at 500 ms.
pub const LONG_PRESS_MS: u64 = 500;
/// A second tap may start within 300 ms of the preceding release.
pub const DOUBLE_TAP_MS: u64 = 300;
/// Second tap must start within 24 CSS px of the preceding tap.
pub const DOUBLE_TAP_SLOP_CSS_PX: f64 = 24.0;
/// Minimum average displacement velocity for a swipe, in CSS px/s.
pub const SWIPE_MIN_VELOCITY_CSS_PER_S: f64 = 600.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointerPhase { Down, Move, Up, Cancel }

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PointerEvent {
    pub pointer_id: i64,
    pub phase: PointerPhase,
    pub x_css: f64,
    pub y_css: f64,
    pub time_ms: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SwipeDirection { Left, Right, Up, Down }

/// Delta values are relative to Down except Pan, which is incremental.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GestureEvent {
    Press(NodeRef),
    Release(NodeRef),
    Tap(NodeRef),
    DoubleTap(NodeRef),
    LongPress(NodeRef),
    Swipe { target: NodeRef, direction: SwipeDirection, velocity_css_per_s: f64 },
    DragStart(NodeRef),
    DragMove { target: NodeRef, dx_css: f64, dy_css: f64 },
    DragEnd { target: NodeRef, dx_css: f64, dy_css: f64, velocity: f64 },
    DragCancel(NodeRef),
    Pan { target: NodeRef, dy_css: f64 },
}

impl GestureEvent {
    pub fn target(self) -> NodeRef {
        match self {
            Self::Press(n) | Self::Release(n) | Self::Tap(n)
            | Self::DoubleTap(n) | Self::LongPress(n)
            | Self::DragStart(n) | Self::DragCancel(n) => n,
            Self::Swipe { target, .. } | Self::DragMove { target, .. }
            | Self::DragEnd { target, .. } | Self::Pan { target, .. } => target,
        }
    }
}

/// Up to four events are stored inline; Move does not allocate in steady state.
pub type GestureEvents = SmallVec<[GestureEvent; 4]>;

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct GestureFlags {
    pub declared: bool,
    pub link: bool,
    pub tap: bool,
    pub doubletap: bool,
    pub longpress: bool,
    pub swipe_left: bool,
    pub swipe_right: bool,
    pub swipe_up: bool,
    pub swipe_down: bool,
    pub drag: bool,
}
impl GestureFlags {
    pub fn from_attrs(gesture: Option<&str>, drag: Option<&str>) -> Self {
        let mut out = Self {
            declared: gesture.is_some() || drag.is_some_and(|s| !s.is_empty()),
            drag: drag.is_some_and(|s| !s.is_empty()),
            ..Self::default()
        };
        for token in gesture.unwrap_or("").split_whitespace() {
            match token {
                "tap" => out.tap = true,
                "doubletap" => out.doubletap = true,
                "longpress" => out.longpress = true,
                "swipe-left" => out.swipe_left = true,
                "swipe-right" => out.swipe_right = true,
                "swipe-up" => out.swipe_up = true,
                "swipe-down" => out.swipe_down = true,
                _ => {}
            }
        }
        out
    }
    pub fn swipe(self, direction: SwipeDirection) -> bool {
        match direction {
            SwipeDirection::Left => self.swipe_left,
            SwipeDirection::Right => self.swipe_right,
            SwipeDirection::Up => self.swipe_up,
            SwipeDirection::Down => self.swipe_down,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ActivePointer {
    pub id: i64,
    pub target: NodeRef,
    pub flags: GestureFlags,
    pub x0: f64,
    pub y0: f64,
    pub x: f64,
    pub y: f64,
    pub down_ms: u64,
    pub moved: bool,
    pub dragging: bool,
    pub long_pressed: bool,
}
impl ActivePointer {
    pub fn distance_squared(self, x: f64, y: f64) -> f64 {
        (x - self.x0).powi(2) + (y - self.y0).powi(2)
    }
    pub fn velocity(self, x: f64, y: f64, time_ms: u64) -> f64 {
        let elapsed = time_ms.saturating_sub(self.down_ms);
        if elapsed == 0 { 0.0 } else {
            self.distance_squared(x, y).sqrt() * 1000.0 / elapsed as f64
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct LastTap {
    pub target: NodeRef,
    pub x: f64,
    pub y: f64,
    pub time_ms: u64,
}

#[derive(Default)]
pub(crate) struct Recognizer {
    pub active: Option<ActivePointer>,
    pub last_tap: Option<LastTap>,
    pub suppress_until_up: bool,
}
