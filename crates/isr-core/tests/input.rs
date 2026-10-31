// Copyright 2026 g1BSsQ. Licensed under Apache-2.0.
use isr_core::{
    Document, GestureEvent as G, PointerEvent, PointerPhase as P, RenderOptions,
    SwipeDirection as D,
};
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
};

thread_local! {
    static TRACK: Cell<bool> = const { Cell::new(false) };
    static COUNT: Cell<usize> = const { Cell::new(0) };
}
struct CountingAllocator;
#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        TRACK.try_with(|flag| { if flag.get() {
            let _ = COUNT.try_with(|count| count.set(count.get() + 1));
        }}).ok();
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        TRACK.try_with(|flag| { if flag.get() {
            let _ = COUNT.try_with(|count| count.set(count.get() + 1));
        }}).ok();
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        TRACK.try_with(|flag| { if flag.get() {
            let _ = COUNT.try_with(|count| count.set(count.get() + 1));
        }}).ok();
        unsafe { System.realloc(ptr, layout, size) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
}

fn doc() -> Document {
    Document::new(include_str!("../../../fixtures/input/gestures.html"),
        RenderOptions { width: 360, ..Default::default() }).unwrap()
}
fn e(id: i64, phase: P, x: f64, y: f64, ms: u64) -> PointerEvent {
    PointerEvent { pointer_id: id, phase, x_css: x, y_css: y, time_ms: ms }
}
fn id(doc: &Document, g: G) -> String {
    doc.attribute(g.target(), "id").unwrap_or_default()
}
fn tap(doc: &mut Document, x: f64, y: f64, down: u64, up: u64) -> Vec<G> {
    let down_events = doc.pointer(e(1, P::Down, x, y, down));
    assert_eq!(down_events.len(), 1);
    assert!(matches!(down_events[0], G::Press(_)));
    doc.pointer(e(1, P::Up, x, y, up)).into_iter().collect()
}

#[test]
fn tap_doubletap_window_and_distance() {
    let mut d = doc();
    let t = d.element_by_id("tap").unwrap();
    let first = tap(&mut d, 25.0, 25.0, 0, 50);
    assert!(first.contains(&G::Tap(t)));
    assert!(!first.contains(&G::DoubleTap(t)));
    let second = tap(&mut d, 25.0, 25.0, 250, 270);
    assert!(second.contains(&G::DoubleTap(t)));
    let third = tap(&mut d, 25.0, 25.0, 650, 670);
    assert!(!third.contains(&G::DoubleTap(t)));
    let outside = tap(&mut d, 25.0, 25.0, 991, 1010);
    assert!(!outside.contains(&G::DoubleTap(t))); // 321 ms since release
    let _ = tap(&mut d, 25.0, 25.0, 1360, 1380);
    let distant = tap(&mut d, 50.0, 25.0, 1400, 1420);
    assert!(!distant.contains(&G::DoubleTap(t))); // 25 CSS px
    let mut boundary = doc();
    let b = boundary.element_by_id("tap").unwrap();
    let _ = tap(&mut boundary, 25.0, 25.0, 0, 50);
    assert!(tap(&mut boundary, 25.0, 25.0, 350, 370)
        .contains(&G::DoubleTap(b))); // second Down exactly 300 ms after Up
    let _ = tap(&mut boundary, 25.0, 25.0, 800, 820);
    assert!(!tap(&mut boundary, 25.0, 25.0, 1121, 1141)
        .contains(&G::DoubleTap(b))); // second Down 301 ms after Up
}

#[test]
fn movement_slop_is_strict_and_pan_is_incremental() {
    for (dx, should_tap) in [(7.9, true), (8.1, false)] {
        let mut d = doc();
        let t = d.element_by_id("tap").unwrap();
        d.pointer(e(1, P::Down, 20.0, 20.0, 0));
        d.pointer(e(1, P::Move, 20.0 + dx, 20.0, 20));
        let up = d.pointer(e(1, P::Up, 20.0 + dx, 20.0, 30));
        assert_eq!(up.contains(&G::Tap(t)), should_tap, "dx={dx}");
    }
    let mut d = doc();
    let t = d.element_by_id("plain").unwrap();
    d.pointer(e(1, P::Down, 20.0, 260.0, 0));
    assert!(d.pointer(e(1, P::Move, 20.0, 267.9, 10)).is_empty());
    let move_events = d.pointer(e(1, P::Move, 20.0, 280.0, 20));
    assert_eq!(move_events.len(), 1);
    assert!(matches!(move_events[0], G::Pan { target, dy_css }
        if target == t && (dy_css - 12.1).abs() < 1e-9));
    let up = d.pointer(e(1, P::Up, 20.0, 280.0, 30));
    assert!(!up.iter().any(|g| matches!(g, G::Tap(_))));
}

#[test]
fn longpress_tick_is_one_shot_and_cancels_tap() {
    let mut d = doc();
    let hold = d.element_by_id("hold").unwrap();
    d.pointer(e(1, P::Down, 150.0, 30.0, 100));
    assert!(d.tick(599).is_empty());
    assert_eq!(d.tick(600).as_slice(), &[G::LongPress(hold)]);
    assert!(d.tick(800).is_empty());
    let up = d.pointer(e(1, P::Up, 150.0, 30.0, 810));
    assert!(up.contains(&G::Release(hold)));
    assert!(!up.iter().any(|event| matches!(event, G::Tap(_) | G::LongPress(_))));
    d.pointer(e(1, P::Down, 150.0, 30.0, 900));
    assert!(d.pointer(e(1, P::Up, 150.0, 30.0, 1400))
        .contains(&G::LongPress(hold)));
}

#[test]
fn swipe_directions_and_velocity_threshold() {
    for (x, y, end_x, end_y, direction) in [
        (40.0, 150.0, 0.0, 150.0, D::Left),
        (40.0, 150.0, 100.0, 150.0, D::Right),
        (40.0, 150.0, 40.0, 90.0, D::Up),
        (40.0, 150.0, 40.0, 220.0, D::Down),
    ] {
        let mut d = doc();
        let s = d.element_by_id("swipe").unwrap();
        d.pointer(e(1, P::Down, x, y, 10));
        d.pointer(e(1, P::Move, end_x, end_y, 40));
        let result = d.pointer(e(1, P::Up, end_x, end_y, 60));
        assert!(result.iter().any(|g| matches!(g, G::Swipe {
            target, direction: dir, velocity_css_per_s
        } if *target == s && *dir == direction && *velocity_css_per_s >= 600.0)));
    }
    let mut d = doc();
    d.pointer(e(1, P::Down, 40.0, 150.0, 0));
    d.pointer(e(1, P::Move, 0.0, 150.0, 500));
    assert!(!d.pointer(e(1, P::Up, 0.0, 150.0, 800))
        .iter().any(|g| matches!(g, G::Swipe { .. })));
}

#[test]
fn drag_precedes_swipe_and_is_cancelled_on_second_pointer() {
    let mut d = doc();
    let target = d.element_by_id("drag").unwrap();
    d.pointer(e(1, P::Down, 150.0, 150.0, 0));
    let first = d.pointer(e(1, P::Move, 165.0, 160.0, 20));
    assert!(first.contains(&G::DragStart(target)));
    assert!(first.contains(&G::DragMove {
        target, dx_css: 15.0, dy_css: 10.0,
    }));
    let end = d.pointer(e(1, P::Up, 180.0, 160.0, 40));
    assert!(end.iter().any(|g| matches!(g, G::DragEnd {
        target: t, dx_css: 30.0, dy_css: 10.0, velocity: v
    } if *t == target && *v > 0.0)));
    assert!(!end.iter().any(|g| matches!(g, G::Swipe { .. })));
    d.pointer(e(1, P::Down, 150.0, 150.0, 100));
    d.pointer(e(1, P::Move, 170.0, 150.0, 110));
    let cancel = d.pointer(e(2, P::Down, 170.0, 150.0, 115));
    assert!(cancel.contains(&G::DragCancel(target)));
    assert!(d.pointer(e(1, P::Up, 170.0, 150.0, 130)).is_empty());
    assert!(d.pointer(e(2, P::Up, 170.0, 150.0, 135)).is_empty());
    assert!(d.attribute(target, "data-pressed").is_none());
    let canceled = {
        d.pointer(e(1, P::Down, 150.0, 150.0, 200));
        d.pointer(e(1, P::Move, 170.0, 150.0, 220));
        d.pointer(e(1, P::Cancel, 170.0, 150.0, 230))
    };
    assert!(canceled.contains(&G::DragCancel(target)));
    assert!(!canceled.iter().any(|g| matches!(g, G::DragEnd { .. } | G::Tap(_))));
}

#[test]
fn nested_targets_and_pointer_events_are_resolved_once() {
    let mut d = doc();
    let ancestor = d.element_by_id("ancestor").unwrap();
    let under = d.element_by_id("under").unwrap();
    let child = tap(&mut d, 25.0, 380.0, 0, 30);
    assert!(child.contains(&G::Tap(ancestor)));
    let overlay = tap(&mut d, 150.0, 380.0, 40, 60);
    assert!(overlay.contains(&G::Tap(under)));
    assert_eq!(id(&d, overlay[0]), "under");
    let hidden = d.element_by_id("hidden").unwrap();
    let down_hidden = d.pointer(e(1, P::Down, 20.0, 500.0, 70));
    assert!(down_hidden.iter().all(|event| event.target() != hidden));
    let _ = d.pointer(e(1, P::Up, 20.0, 500.0, 80));
    let link = d.element_by_id("link").unwrap();
    assert!(tap(&mut d, 150.0, 260.0, 100, 120).contains(&G::Tap(link)));
    let plain = tap(&mut d, 20.0, 260.0, 150, 170);
    assert!(!plain.iter().any(|g| matches!(g, G::Tap(_))));
    d.pointer(e(1, P::Down, 20.0, 30.0, 200));
    let moved = d.pointer(e(1, P::Move, 150.0, 150.0, 220));
    assert!(moved.is_empty()); // still original tap target, not drag target
    assert!(!d.pointer(e(1, P::Up, 150.0, 150.0, 240))
        .iter().any(|g| matches!(g, G::Tap(_) | G::DragEnd { .. })));
}

#[test]
fn pressed_is_mutated_through_one_incremental_update_per_transition() {
    let mut d = doc();
    d.set_viewport(0.0, 600).unwrap();
    let target = d.element_by_id("tap").unwrap();
    let unpressed_pixels = d.paint();
    d.pointer(e(1, P::Down, 20.0, 20.0, 0));
    assert_eq!(d.attribute(target, "data-pressed").as_deref(), Some(""));
    let _first = d.update();
    assert_ne!(d.paint(), unpressed_pixels, "pressed style must repaint");
    assert_eq!(d.update().total_us(), 0);
    d.pointer(e(1, P::Up, 20.0, 20.0, 20));
    assert!(d.attribute(target, "data-pressed").is_none());
    let _second = d.update();
    assert_eq!(d.paint(), unpressed_pixels, "release must restore pixels");
    assert_eq!(d.update().total_us(), 0);
    d.pointer(e(1, P::Down, 20.0, 20.0, 30));
    d.pointer(e(1, P::Cancel, 20.0, 20.0, 40));
    assert!(d.attribute(target, "data-pressed").is_none());
}

#[test]
fn steady_state_move_reuses_inline_event_storage_without_allocating() {
    let mut d = doc();
    d.pointer(e(1, P::Down, 20.0, 260.0, 0));
    let _ = d.pointer(e(1, P::Move, 20.0, 280.0, 1));
    // Per-thread allocation counter includes Vec/Box/format allocations.
    // Only this thread is tracked, avoiding unrelated test worker traffic.
    COUNT.with(|count| count.set(0));
    TRACK.with(|track| track.set(true));
    for t in 2..50 {
        let events = d.pointer(e(1, P::Move, 20.0, 280.0 + t as f64, t));
        assert_eq!(events.len(), 1);
    }
    TRACK.with(|track| track.set(false));
    assert_eq!(COUNT.with(Cell::get), 0, "Move must use inline event storage");
}
