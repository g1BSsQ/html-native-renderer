// Copyright 2026 g1BSsQ. Licensed under Apache-2.0.
// Engine-backed CSS motion diagnosis and regressions. No custom timeline or
// authored-style parser is used in these tests.
use isr_core::{Document, RenderOptions};

fn page(html: &str) -> Document {
    let mut page = Document::new_scaled(
        html,
        RenderOptions { width: 392, ..Default::default() },
        2.75,
    ).unwrap();
    page.set_viewport(0.0, 360).unwrap();
    page
}

fn opacity(page: &Document, id: &str) -> f64 {
    let node = page.element_by_id(id).unwrap();
    page.computed_style(node, "opacity").parse().unwrap()
}

fn approx(value: f64, expected: f64) {
    assert!((value - expected).abs() < 0.02, "computed {value}, expected {expected}");
}

fn static_pixel(x: f64, y: f64, alpha: f64, rotation: f64) -> Vec<u8> {
    page(&format!(r#"<!doctype html><style>
        html,body{{margin:0;background:white}}
        #probe{{margin:32px;width:100px;height:70px;background:#287cc7}}
        </style><div id="probe" style="transform:translate({x}px,{y}px) rotate({rotation}deg);
        opacity:{alpha}"></div>"#)).paint()
}

#[test]
fn engine_resolves_delay_fill_direction_iterations_steps_and_bezier() {
    let html = r#"<!doctype html><style>
        #probe{opacity:.8;animation:move 100ms steps(4,end) 50ms 2 alternate both}
        @keyframes move{from{opacity:0}to{opacity:1}}
        </style><div id=probe></div>"#;
    let mut p = page(html);
    for (time, expected) in [
        (0.0, 0.0), (50.0, 0.0), (75.0, 0.25),
        (100.0, 0.5), (150.0, 1.0), (175.0, 0.75),
        (250.0, 0.0), (300.0, 0.0)
    ] {
        p.tick(time);
        approx(opacity(&p, "probe"), expected);
    }

    let mut curve = page(r#"<style>
        #probe{animation:fade 100ms cubic-bezier(.42,0,.58,1) both}
        @keyframes fade{from{opacity:0}to{opacity:1}}
        </style><div id=probe></div>"#);
    curve.tick(50.0);
    approx(opacity(&curve, "probe"), 0.5);
}

#[test]
fn engine_starts_transition_at_mutation_clock_after_idling() {
    let mut p = page(r#"<style>
        #probe{opacity:.2;transition:opacity 100ms linear}
        #probe.active{opacity:1}
        </style><div id=probe></div>"#);
    p.set_clock(1000.0);
    let id = p.element_by_id("probe").unwrap();
    p.toggle_class(id, "active").unwrap();
    p.update_at(1000.0);
    p.tick(1050.0);
    approx(opacity(&p, "probe"), 0.6);
    p.tick(1100.0);
    approx(opacity(&p, "probe"), 1.0);
}

#[test]
fn engine_reversal_uses_shortening_factor_without_inline_style_leaks() {
    let mut p = page(r#"<style>
        #probe{opacity:.2;transition:opacity 100ms linear}
        #probe.active{opacity:1}
        </style><div id=probe style="color:red"></div>"#);
    let id = p.element_by_id("probe").unwrap();
    let original = p.attribute(id, "style");
    p.toggle_class(id, "active").unwrap();
    p.update_at(0.0);
    p.tick(50.0);
    approx(opacity(&p, "probe"), 0.6);
    p.toggle_class(id, "active").unwrap();
    p.update_at(50.0);
    p.tick(75.0);
    approx(opacity(&p, "probe"), 0.4);
    p.tick(100.0);
    approx(opacity(&p, "probe"), 0.2);
    assert_eq!(p.attribute(id, "style"), original);
}

#[test]
fn engine_class_adds_and_removes_keyframes() {
    let mut p = page(r#"<style>
        #probe{opacity:.8}
        #probe.active{animation:arrive 100ms linear both}
        @keyframes arrive{from{opacity:0}to{opacity:1}}
        </style><div id=probe></div>"#);
    let id = p.element_by_id("probe").unwrap();
    p.toggle_class(id, "active").unwrap();
    p.update_at(0.0);
    p.tick(25.0);
    approx(opacity(&p, "probe"), 0.25);
    p.toggle_class(id, "active").unwrap();
    p.update_at(25.0);
    p.tick(50.0);
    approx(opacity(&p, "probe"), 0.8);
}

#[test]
fn engine_implicit_keyframes_and_two_animations_compose_by_property() {
    let mut implicit = page(r#"<style>
        #probe{opacity:.8;animation:blip 100ms linear both}
        @keyframes blip{50%{opacity:0}}
        </style><div id=probe></div>"#);
    for (t, v) in [(0.0,.8),(25.0,.4),(50.0,0.0),(75.0,.4),(100.0,.8)] {
        implicit.tick(t);
        approx(opacity(&implicit, "probe"), v);
    }

    let mut both = page(r#"<!doctype html><style>
        html,body{margin:0;background:white}
        #probe{margin:32px;width:100px;height:70px;background:#287cc7;
            animation:fade 100ms linear both, slide 100ms linear both}
        @keyframes fade{from{opacity:0}to{opacity:1}}
        @keyframes slide{from{transform:translateX(20px)}to{transform:translateX(0px)}}
        </style><div id=probe></div>"#);
    both.tick(50.0);
    approx(opacity(&both, "probe"), 0.5);
    assert_eq!(both.paint(), static_pixel(10.0, 0.0, 0.5, 0.0));
}

#[test]
fn engine_interpolates_zero_unit_translate_and_decomposed_rotate() {
    let mut translation = page(r#"<!doctype html><style>
        html,body{margin:0;background:white}
        #probe{margin:32px;width:100px;height:70px;background:#287cc7;
            animation:move 100ms linear both}
        @keyframes move{from{transform:translateY(0)}to{transform:translateY(20px)}}
        </style><div id=probe></div>"#);
    translation.tick(50.0);
    assert_eq!(translation.paint(), static_pixel(0.0, 10.0, 1.0, 0.0));

    let mut rotation = page(r#"<!doctype html><style>
        html,body{margin:0;background:white}
        #probe{margin:32px;width:100px;height:70px;background:#287cc7;
            animation:turn 100ms linear both}
        @keyframes turn{from{transform:rotate(0deg)}to{transform:rotate(90deg)}}
        </style><div id=probe></div>"#);
    rotation.tick(50.0);
    assert_eq!(rotation.paint(), static_pixel(0.0, 0.0, 1.0, 45.0));
}

#[test]
fn engine_prefers_reduced_motion_media_is_specific_and_not_unconditionally_applied() {
    let mut p = page(r#"<style>
        #probe{animation:fade 100ms linear both}
        @keyframes fade{from{opacity:0}to{opacity:1}}
        @media (prefers-reduced-motion:reduce){
            #probe{animation-duration:.01ms !important}
        }
        </style><div id=probe></div>"#);
    p.tick(25.0);
    approx(opacity(&p, "probe"), .25);
    p.set_reduced_motion(true);
    p.tick(26.0);
    approx(opacity(&p, "probe"), 1.0);
}

#[test]
fn engine_motion_ticks_do_not_construct_layout() {
    let mut p = page(r#"<style>
        #probe{width:100px;height:70px;animation:fade 100ms linear both}
        @keyframes fade{from{opacity:0}to{opacity:1}}
        </style><div id=probe></div>"#);
    let before = p.layout_pass_count();
    for t in [0.0,25.0,50.0,75.0,100.0] {
        let (times, _) = p.tick(t);
        assert_eq!(times.layout_us, 0);
        assert_eq!(p.layout_pass_count(), before);
    }
}

#[test]
fn pixel_idle_then_mutate_tracks_mutation_clock() {
    let mut p = page(r#"<!doctype html><style>
        html,body{margin:0;background:white}
        #probe{margin:32px;width:100px;height:70px;background:#287cc7;
            opacity:.2;transition:opacity 100ms linear}
        #probe.active{opacity:1}
        </style><div id=probe></div>"#);
    let id = p.element_by_id("probe").unwrap();
    p.set_clock(1000.0);
    p.toggle_class(id, "active").unwrap();
    p.update_at(1000.0);
    p.tick(1050.0);
    assert_eq!(p.paint(), static_pixel(0.0,0.0,0.6,0.0));
    p.tick(1100.0);
    assert_eq!(p.paint(), static_pixel(0.0,0.0,1.0,0.0));
}

#[test]
fn pixel_transition_reverses_with_shortening_and_leaves_inline_style_alone() {
    let mut p = page(r#"<!doctype html><style>
        html,body{margin:0;background:white}
        #probe{margin:32px;width:100px;height:70px;background:#287cc7;
            opacity:.2;transition:opacity 100ms linear}
        #probe.active{opacity:1}
        </style><div id=probe style="color:red"></div>"#);
    let id = p.element_by_id("probe").unwrap();
    let style_before = p.attribute(id, "style");
    p.toggle_class(id, "active").unwrap();
    p.update_at(0.0);
    p.tick(50.0);
    assert_eq!(p.paint(), static_pixel(0.0,0.0,0.6,0.0));
    p.toggle_class(id, "active").unwrap();
    p.update_at(50.0);
    p.tick(75.0);
    assert_eq!(p.paint(), static_pixel(0.0,0.0,0.4,0.0));
    p.tick(100.0);
    assert_eq!(p.paint(), static_pixel(0.0,0.0,0.2,0.0));
    assert_eq!(p.attribute(id, "style"), style_before);
}

#[test]
fn pixel_host_style_mutation_during_transition_is_not_lost() {
    let mut p = page(r#"<!doctype html><style>
        html,body{margin:0;background:white}
        #probe{margin:32px;width:100px;height:70px;background:#287cc7;
            opacity:.2;transition:opacity 100ms linear}
        #probe.active{opacity:1}
        </style><div id=probe></div>"#);
    let id = p.element_by_id("probe").unwrap();
    p.toggle_class(id, "active").unwrap();
    p.update_at(0.0);
    p.tick(50.0);
    p.set_attribute(id, "style", "opacity:.4").unwrap();
    p.update_at(50.0);
    p.tick(150.0);
    assert_eq!(p.attribute(id, "style").as_deref(), Some("opacity:.4"));
    assert_eq!(p.paint(), static_pixel(0.0,0.0,0.4,0.0));
}

#[test]
fn pixel_class_introduces_and_removes_animation() {
    let mut p = page(r#"<!doctype html><style>
        html,body{margin:0;background:white}
        #probe{margin:32px;width:100px;height:70px;background:#287cc7;opacity:.8}
        #probe.active{animation:fade 100ms linear both}
        @keyframes fade{from{opacity:0}to{opacity:1}}
        </style><div id=probe></div>"#);
    let id = p.element_by_id("probe").unwrap();
    p.toggle_class(id, "active").unwrap();
    p.update_at(0.0);
    p.tick(25.0);
    assert_eq!(p.paint(), static_pixel(0.0,0.0,0.25,0.0));
    p.toggle_class(id, "active").unwrap();
    p.update_at(25.0);
    p.tick(50.0);
    assert_eq!(p.paint(), static_pixel(0.0,0.0,0.8,0.0));
}

#[test]
fn pixel_reduced_motion_query_preserves_normal_duration_when_not_requested() {
    let mut p = page(r#"<!doctype html><style>
        html,body{margin:0;background:white}
        #probe{margin:32px;width:100px;height:70px;background:#287cc7;
            animation:fade 100ms linear both}
        @keyframes fade{from{opacity:0}to{opacity:1}}
        @media (prefers-reduced-motion:reduce){
            #probe{animation-duration:.01ms !important}
        }
        </style><div id=probe></div>"#);
    p.tick(25.0);
    assert_eq!(p.paint(), static_pixel(0.0,0.0,0.25,0.0));
    p.set_reduced_motion(true);
    p.tick(26.0);
    assert_eq!(p.paint(), static_pixel(0.0,0.0,1.0,0.0));
}

#[test]
fn pixel_partial_property_keyframes_interpolate_across_omitted_keyframe() {
    let mut p = page(r#"<!doctype html><style>
        html,body{margin:0;background:white}
        #probe{margin:32px;width:100px;height:70px;background:#287cc7;
            animation:partial 100ms linear both}
        @keyframes partial{
            0%{opacity:0;transform:translateX(0px)}
            50%{opacity:.5}
            100%{opacity:1;transform:translateX(20px)}
        }
        </style><div id=probe></div>"#);
    for (time,x,alpha) in [(25.0,5.0,.25),(50.0,10.0,.5),(75.0,15.0,.75)] {
        p.tick(time);
        assert_eq!(p.paint(),static_pixel(x,0.0,alpha,0.0));
    }
}

#[test]
fn engine_author_cascade_handles_not_hover_and_compound_media_without_scraping() {
    let mut p=page(r#"<style>
        #probe{opacity:.3}
        #probe:hover{opacity:.9}
        #probe:not(.active){opacity:.4}
        @media (prefers-reduced-motion:reduce) and (min-width:400px) {
            #probe{opacity:.1 !important}
        }
        </style><div id=probe></div>"#);
    approx(opacity(&p,"probe"),.4);
    p.set_reduced_motion(true);
    p.tick(0.0);
    // The 392px viewport does not satisfy the remaining width condition.
    approx(opacity(&p,"probe"),.4);
}
