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
    assert!((value - expected).abs() < 0.0001, "computed {value}, expected {expected}");
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
    for (t, v) in [(0.0,0.8),(25.0,0.4),(50.0,0.0),(75.0,0.4),(100.0,0.8)] {
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
    approx(opacity(&p, "probe"), 0.25);
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
    for (time,x,alpha) in [(25.0,5.0,0.25),(50.0,10.0,0.5),(75.0,15.0,0.75)] {
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
    approx(opacity(&p,"probe"),0.4);
    p.set_reduced_motion(true);
    p.tick(0.0);
    // The 392px viewport does not satisfy the remaining width condition.
    approx(opacity(&p,"probe"),0.4);
}

#[test]
fn animated_owner_uses_damage_raster_pseudo_effect_fallback() {
    let mut p = page(r#"<!doctype html><style>
        html,body{margin:0;background:white}
        #probe{position:relative;margin:32px;width:100px;height:70px;
            background:#287cc7;animation:fade 100ms linear both}
        #probe::before{content:"";position:absolute;left:0;top:0;
            width:100px;height:70px;box-shadow:0 0 12px red}
        @keyframes fade{from{opacity:0}to{opacity:1}}
        </style><div id=probe></div>"#);
    let before = p.paint();
    let layout_before = p.layout_pass_count();
    let (timings, _) = p.tick(50.0);
    assert_eq!(p.layout_pass_count(), layout_before);
    assert_eq!(timings.paint_path, "full_viewport",
        "pseudo effects extend outside the owner's bounded ink");
    assert!(timings.damage_px > 0);
    assert_ne!(p.paint(), before, "the opacity sample must repaint");
    let expected = page(r#"<!doctype html><style>
        html,body{margin:0;background:white}
        #probe{position:relative;margin:32px;width:100px;height:70px;
            background:#287cc7;opacity:.5}
        #probe::before{content:"";position:absolute;left:0;top:0;
            width:100px;height:70px;box-shadow:0 0 12px red}
        </style><div id=probe></div>"#);
    assert_eq!(p.paint(), expected.paint(), "sampled opacity must match static paint");
}

#[test]
fn reduced_motion_settles_transition_without_more_frames() {
    let mut p = page(r#"<style>
        #probe{opacity:.2;transition:opacity 100ms linear}
        #probe.active{opacity:1}
        </style><div id=probe></div>"#);
    let target = p.element_by_id("probe").unwrap();
    p.set_reduced_motion(true);
    p.toggle_class(target, "active").unwrap();
    p.update_at(0.0);
    let (_, more) = p.tick(25.0);
    assert_eq!(opacity(&p, "probe"), 1.0);
    assert!(!more);
}

#[test]
fn reduced_motion_settles_each_animation_independently() {
    let mut p = page(r#"<style>
        #short{opacity:.3;animation:fade 100ms linear both}
        #late{opacity:.8;animation:fade 100ms linear 800ms both}
        #loop{opacity:.2;animation:fade 100ms linear infinite both}
        @keyframes fade{from{opacity:0}to{opacity:1}}
        </style><div id=short></div><div id=late></div><div id=loop></div>"#);
    p.set_reduced_motion(true);
    let (_, more) = p.tick(0.0);
    assert!(!more);
    approx(opacity(&p, "short"), 1.0);
    approx(opacity(&p, "late"), 1.0);
    approx(opacity(&p, "loop"), 1.0);
}

#[test]
fn non_compositor_background_transition_updates_computed_style_and_pixels() {
    let base = r#"<!doctype html><style>
        html,body{margin:0;background:white}
        #probe{margin:32px;width:100px;height:70px;background:red;
            transition:background-color 100ms linear}
        #probe.active{background:blue}
        </style><div id=probe></div>"#;
    let mut p = page(base);
    let id = p.element_by_id("probe").unwrap();
    p.toggle_class(id, "active").unwrap();
    p.update_at(0.0);
    p.tick(100.0);
    assert_eq!(p.computed_style(id, "background-color"), "rgb(0, 0, 255)");
    let expected = page(&base.replace("id=probe", "id=probe class=active"));
    assert_eq!(p.paint(), expected.paint());
}

#[test]
fn non_compositor_width_transition_resolves_layout_before_paint() {
    let base = r#"<!doctype html><style>
        html,body{margin:0;background:white}
        #probe{margin:32px;width:100px;height:70px;background:#287cc7;
            transition:width 100ms linear}
        #probe.active{width:200px}
        </style><div id=probe></div>"#;
    let mut p = page(base);
    let id = p.element_by_id("probe").unwrap();
    p.toggle_class(id, "active").unwrap();
    p.update_at(0.0);
    let count = p.layout_pass_count();
    let (timings, _) = p.tick(50.0);
    assert!(p.layout_pass_count() > count, "width motion must perform layout");
    assert!(timings.layout_us > 0);
    assert_eq!(p.computed_style(id, "width"), "150px");
    let expected = page(&base.replace(
        "transition:width 100ms linear", "width:150px"));
    assert_eq!(p.paint(), expected.paint());
}

#[test]
fn reduced_motion_finite_fill_modes_and_alternate_direction() {
    let mut p = page(r#"<style>
        #forward{opacity:.8;animation:fade 100ms 2 alternate forwards}
        #none{opacity:.8;animation:fade 100ms linear none}
        @keyframes fade{from{opacity:0}to{opacity:1}}
        </style><div id=forward></div><div id=none></div>"#);
    p.set_reduced_motion(true);
    let (_, more) = p.tick(0.0);
    assert!(!more);
    assert_eq!(opacity(&p, "forward"), 0.0);
    assert_eq!(opacity(&p, "none"), 0.8);
}
