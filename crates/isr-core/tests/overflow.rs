// Copyright 2026 g1BSsQ. Licensed under Apache-2.0.
use isr_core::{render, RenderOptions};

fn draw(html: &str) -> isr_core::Rendered {
    render(
        html,
        RenderOptions {
            system_fonts: false,
            width: 160,
            ..RenderOptions::default()
        },
    ).expect("overflow fixture should render")
}

fn rgba_at(image: &isr_core::Rendered, x: u32, y: u32) -> [u8; 4] {
    assert!(x < image.width && y < image.height);
    let at = ((y * image.width + x) * 4) as usize;
    image.rgba[at..at + 4].try_into().unwrap()
}

fn red_at(image: &isr_core::Rendered, x: u32, y: u32) -> bool {
    let [r, g, b, _] = rgba_at(image, x, y);
    r > 180 && g < 100 && b < 100
}

fn blue_at(image: &isr_core::Rendered, x: u32, y: u32) -> bool {
    let [r, g, b, _] = rgba_at(image, x, y);
    r < 100 && g < 100 && b > 180
}

#[test]
fn clip_visible_axes_only_clip_the_requested_dimension() {
    let image = draw(include_str!("../../../fixtures/overflow/clip-visible-axis.html"));
    // CSS Overflow 3 §3.1 and §3.1.2. These pixels are outside the box's
    // visible axis but inside its clip axis, and must remain painted.
    assert!(red_at(&image, 35, 60), "overflow-y:visible must paint below the box");
    assert!(blue_at(&image, 120, 35), "overflow-x:visible must paint right of the box");
    assert!(!red_at(&image, 55, 25), "overflow-x:clip must clip the right side");
    assert!(!blue_at(&image, 85, 55), "overflow-y:clip must clip the bottom");
}

#[test]
fn clip_margin_extends_paint_clip_without_extending_it_indefinitely() {
    let image = draw(include_str!("../../../fixtures/overflow/clip-margin.html"));
    // CSS Overflow 3 §3.2: the 8px clip margin reveals the displaced
    // descendant, but does not make overflow:clip an unrestricted overflow.
    assert!(red_at(&image, 54, 27), "positive clip margin must reveal ink");
    assert!(!red_at(&image, 62, 27), "content beyond clip margin stays hidden");
}

#[test]
fn clip_margin_preserves_rounded_clipping() {
    let image = draw(include_str!("../../../fixtures/overflow/clip-margin.html"));
    // CSS Overflow 3 §3.1.2/§3.2: expanding the rounded clip preserves its
    // corner shape, rather than using a rectangular clipping box.
    assert!(red_at(&image, 85, 40), "expanded round edge must paint");
    assert!(!red_at(&image, 84, 14), "expanded rounded corner must stay unpainted");
}

#[test]
fn clip_margin_keeps_sharp_and_collapsed_corners_square() {
    let image = draw(include_str!("../../../fixtures/overflow/clip-margin-sharp-corners.html"));
    // The square clip starts at (22, 22); (23, 23) is inside its
    // expanded corner, which must not acquire a rounded cutout.
    assert!(red_at(&image, 23, 23), "an initially square corner must stay square");
    // The 4px border radius is exhausted by the 6px border inset.
    // The padding-box clip starts at (88, 28), and is sharp there.
    assert!(red_at(&image, 89, 29), "a collapsed padding-box radius must stay square");
}

#[test]
fn clip_margin_does_not_expand_hidden_overflow() {
    let image = draw(include_str!("../../../fixtures/overflow/clip-margin-hidden-guard.html"));
    assert!(red_at(&image, 31, 40), "hidden overflow retains inside paint");
    assert!(!red_at(&image, 26, 40), "hidden overflow ignores overflow-clip-margin");
}

#[test]
fn clip_margin_content_box_uses_content_edge() {
    let image = draw(include_str!("../../../fixtures/overflow/clip-margin-content-box.html"));
    // Content begins at x=41; margin=4 moves the clip edge to x=37,
    // not to the expanded padding or border box.
    assert!(red_at(&image, 38, 55), "content-box margin reveals nearby ink");
    assert!(!red_at(&image, 34, 55), "content-box margin must not use padding edge");
}

#[test]
fn clip_margin_border_box_uses_border_edge() {
    let image = draw(include_str!("../../../fixtures/overflow/clip-margin-border-box.html"));
    // The border starts at x=30 and the selected clip starts at x=26.
    assert!(red_at(&image, 27, 55), "border-box margin reaches beyond the border");
    assert!(!red_at(&image, 25, 55), "border-box margin has a finite outer edge");
}
