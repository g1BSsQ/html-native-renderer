// Copyright 2026 g1BSsQ. Licensed under Apache-2.0.
use super::{render, RenderOptions, Rendered};
use std::time::Instant;

fn page(content: &str) -> Rendered {
    render(
        &format!("<!doctype html><html><head><style>body{{margin:0}}</style></head><body>{content}</body></html>"),
        RenderOptions { width: 96, ..RenderOptions::default() },
    ).expect("render succeeds")
}

fn rgb(image: &Rendered, x: u32, y: u32) -> [u8; 3] {
    let idx = ((y * image.width + x) * 4) as usize;
    [image.rgba[idx], image.rgba[idx + 1], image.rgba[idx + 2]]
}

fn near(actual: [u8; 3], expected: [u8; 3], tolerance: u8) {
    for (index, (&a, &b)) in actual.iter().zip(expected.iter()).enumerate() {
        assert!(
            (a as i16 - b as i16).unsigned_abs() <= tolerance as u16,
            "channel {index}: actual {actual:?}, expected {expected:?} ±{tolerance}",
        );
    }
}

#[test]
fn grayscale_tile_is_luma() {
    let result = page(r#"<div style="background:#ff0000;width:70px;height:70px;filter:grayscale(1)"></div>"#);
    near(rgb(&result, 35, 35), [54, 54, 54], 2);
    near(rgb(&result, 85, 35), [255, 255, 255], 1);
}

#[test]
fn brightness_halves_every_channel() {
    let result = page(r#"<div style="background:rgb(200 120 80);width:70px;height:70px;filter:brightness(0.5)"></div>"#);
    near(rgb(&result, 35, 35), [100, 60, 40], 2);
}

#[test]
fn filter_function_order_is_css_order() {
    let result = page(r#"<div style="background:red;width:70px;height:70px;filter:grayscale(1) brightness(0.5)"></div>"#);
    near(rgb(&result, 35, 35), [27, 27, 27], 2);
}

#[test]
fn opacity_flattens_overlapping_children_and_keeps_overflow() {
    let result = page(r#"
      <div style="position:relative;left:20px;width:50px;height:45px;opacity:.5">
        <div style="position:absolute;left:-15px;top:0;width:40px;height:30px;background:red"></div>
        <div style="position:absolute;left:10px;top:0;width:40px;height:30px;background:blue"></div>
      </div>"#);
    // The two children overlap before the parent is composited once.
    near(rgb(&result, 35, 15), [127, 127, 255], 4);
    // This child is outside the parent's border box, but inside its group.
    near(rgb(&result, 10, 15), [255, 127, 127], 4);
}

#[test]
fn multiply_uses_backdrop_and_white_is_identity() {
    let white = page(r#"<div style="width:60px;height:50px;background:rgb(200 100 50);mix-blend-mode:multiply"></div>"#);
    near(rgb(&white, 25, 25), [200, 100, 50], 2);
    let backdrop = page(r#"
      <div style="position:absolute;top:0;left:0;width:60px;height:50px;background:yellow"></div>
      <div style="position:absolute;top:0;left:0;width:60px;height:50px;background:cyan;mix-blend-mode:multiply"></div>"#);
    near(rgb(&backdrop, 25, 25), [0, 255, 0], 3);
}

#[test]
fn screen_uses_backdrop() {
    let result = page(r#"
      <div style="position:absolute;top:0;left:0;width:60px;height:50px;background:rgb(128 0 0)"></div>
      <div style="position:absolute;top:0;left:0;width:60px;height:50px;background:rgb(0 0 128);mix-blend-mode:screen"></div>"#);
    near(rgb(&result, 25, 25), [128, 0, 128], 3);
}

#[test]
fn drop_shadow_is_derived_from_subtree_alpha() {
    let result = page(r#"<div style="width:45px;height:35px;filter:drop-shadow(20px 0px 0px black)">
      <div style="width:10px;height:10px;background:red"></div>
    </div>"#);
    near(rgb(&result, 25, 5), [0, 0, 0], 3);
    // There is no paint at this location, even though it is inside the
    // translated border box. The shadow must remain transparent here.
    near(rgb(&result, 45, 25), [255, 255, 255], 2);
}

#[test]
fn blur_softens_subtree_boundary() {
    let result = page(r#"<div style="position:absolute;left:20px;top:20px;width:40px;height:40px;background:black;filter:blur(4px)"></div>"#);
    let boundary = rgb(&result, 19, 40);
    assert!(boundary[0] > 0 && boundary[0] < 255, "blur boundary: {boundary:?}");
    near(rgb(&result, 40, 40), [0, 0, 0], 3);
    near(rgb(&result, 0, 0), [255, 255, 255], 2);
}

#[test]
fn circle_percentage_uses_normalized_diagonal() {
    let result = page(r#"<div style="position:absolute;left:10px;top:10px;width:80px;height:40px;background:blue;clip-path:circle(40% at center)"></div>"#);
    // 40% of hypot(80,40)/sqrt(2) = 25.3px. Using 80px as the
    // percentage basis instead would incorrectly paint at x=20.
    near(rgb(&result, 20, 30), [255, 255, 255], 2);
    near(rgb(&result, 30, 30), [0, 0, 255], 2);
}

#[test]
fn four_filter_page_time_is_reported_not_gated() {
    let html = r#"
      <div style="height:40px;width:64px;background:red;filter:drop-shadow(3px 3px 2px black)"></div>
      <div style="height:40px;width:64px;background:green;filter:blur(2px)"></div>
      <div style="height:40px;width:64px;background:blue;filter:grayscale(1)"></div>
      <div style="height:40px;width:64px;background:orange;filter:brightness(.5)"></div>
    "#;
    let start = Instant::now();
    let image = page(html);
    eprintln!("four-filter page render: {:.3} ms (non-gating)", start.elapsed().as_secs_f64() * 1_000.0);
    assert_eq!(image.width, 96);
}
