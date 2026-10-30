// Copyright 2026 g1BSsQ. Licensed under Apache-2.0.
use isr_core::{Document, RenderOptions};

fn options() -> RenderOptions {
    RenderOptions { width: 392, ..RenderOptions::default() }
}

fn retained(html: &str) -> Document {
    let mut page = Document::new_scaled(html, options(), 2.75).unwrap();
    page.set_viewport(0.0, 360).unwrap();
    page
}

fn keyframe_page(extra: &str) -> String {
    format!(r#"<!doctype html><style>
        html,body{{margin:0;background:white}}
        #probe{{margin:32px;width:100px;height:70px;background:#287cc7;
            animation:arrive 100ms linear both;{extra}}}
        @keyframes arrive{{from{{transform:translateX(20px);opacity:0}}
            to{{transform:translateX(0px);opacity:1}}}}
        </style><div id="probe"></div>"#)
}

fn static_frame(x: f64, opacity: f64) -> Vec<u8> {
    retained(&format!(r#"<!doctype html><style>
        html,body{{margin:0;background:white}}
        #probe{{margin:32px;width:100px;height:70px;background:#287cc7;
            transform:translateX({x}px);opacity:{opacity}}}
        </style><div id="probe"></div>"#)).paint()
}

#[test]
fn motion_tick_matches_static_viewport_at_all_phases_without_layout() {
    let mut page = retained(&keyframe_page(""));
    let bounds = page.element_bounds(page.element_by_id("probe").unwrap()).unwrap();
    for (time, x, alpha) in [
        (0.0, 20.0, 0.0), (25.0, 15.0, 0.25),
        (50.0, 10.0, 0.5), (100.0, 0.0, 1.0),
        (150.0, 0.0, 1.0),
    ] {
        let (timings, more) = page.tick(time);
        assert_eq!(timings.layout_us, 0, "layout on tick at {time}");
        assert_eq!(timings.paint_path, "partial", "unbounded tick at {time}");
        assert_eq!(page.paint(), static_frame(x, alpha), "pixel mismatch at {time}");
        if time < 100.0 { assert!(more); } else { assert!(!more); }
    }
    // At the end of the animation its paint transform returns to the
    // original position, without changing the measured layout box height.
    let updated = page.element_bounds(page.element_by_id("probe").unwrap()).unwrap();
    assert_eq!(bounds.2, updated.2);
    assert_eq!(bounds.3, updated.3);
}

#[test]
fn delay_backwards_and_forward_fill_are_sampled() {
    let html = keyframe_page("animation-delay:50ms;");
    let mut page = retained(&html);
    for (time, x, alpha) in [
        (0.0, 20.0, 0.0), (50.0, 20.0, 0.0),
        (75.0, 15.0, 0.25), (150.0, 0.0, 1.0),
        (220.0, 0.0, 1.0),
    ] {
        let (timings, _) = page.tick(time);
        assert_eq!(timings.layout_us, 0);
        assert_eq!(timings.paint_path, "partial");
        assert_eq!(page.paint(), static_frame(x, alpha), "delay/fill at {time}");
    }
}

#[test]
fn alternate_direction_and_steps_easing() {
    let html = keyframe_page("animation:arrive 100ms steps(4,end) 2 alternate both;");
    let mut page = retained(&html);
    for (time, x, alpha) in [
        (0.0, 20.0, 0.0), (25.0, 15.0, 0.25),
        (50.0, 10.0, 0.5), (100.0, 0.0, 1.0),
        (125.0, 5.0, 0.75), (200.0, 20.0, 0.0),
        (240.0, 20.0, 0.0),
    ] {
        let (timings, _) = page.tick(time);
        assert_eq!(timings.layout_us, 0);
        assert_eq!(timings.paint_path, "partial");
        assert_eq!(page.paint(), static_frame(x, alpha), "steps/alternate at {time}");
    }
}

#[test]
fn class_mutation_starts_opacity_transition() {
    let html = r#"<!doctype html><style>
        html,body{margin:0;background:white}
        #probe{margin:24px;width:110px;height:70px;background:#c83c72;
            opacity:.2;transition:opacity 100ms linear}
        #probe.selected{opacity:1}
        </style><div id="probe"></div>"#;
    let static_page = |opacity| retained(&format!(r#"<!doctype html><style>
        html,body{{margin:0;background:white}}
        #probe{{margin:24px;width:110px;height:70px;background:#c83c72;
            opacity:{opacity}}}
        </style><div id="probe"></div>"#)).paint();
    let mut page = retained(html);
    let node = page.element_by_id("probe").unwrap();
    page.toggle_class(node, "selected").unwrap();
    page.update();
    let (at_half, more) = page.tick(50.0);
    assert_eq!(at_half.layout_us, 0);
    assert_eq!(at_half.paint_path, "partial");
    assert!(more);
    assert_eq!(page.paint(), static_page(0.6));
    let (at_end, more) = page.tick(100.0);
    assert_eq!(at_end.layout_us, 0);
    assert!(!more);
    assert_eq!(page.paint(), static_page(1.0));
}

#[test]
fn reduced_motion_jumps_to_final_frame_without_layout() {
    let mut page = retained(&keyframe_page(""));
    page.set_reduced_motion(true);
    let (timings, more) = page.tick(0.0);
    assert_eq!(timings.layout_us, 0);
    assert_eq!(timings.paint_path, "partial");
    assert!(!more);
    assert_eq!(page.paint(), static_frame(0.0, 1.0));
}
