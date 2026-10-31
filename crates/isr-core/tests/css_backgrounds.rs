// Copyright 2026 g1BSsQ. Licensed under Apache-2.0.
use isr_core::{render, RenderOptions};

fn draw(html: &str) -> isr_core::Rendered {
    render(html, RenderOptions {
        width:160, system_fonts:false, ..RenderOptions::default()
    }).expect("background paint fixture should render")
}

fn pixel(image: &isr_core::Rendered, x: u32, y: u32) -> [u8; 4] {
    assert!(x < image.width && y < image.height);
    let offset = ((y * image.width + x) * 4) as usize;
    image.rgba[offset..offset + 4].try_into().unwrap()
}

#[test]
fn sized_linear_gradient_uses_its_own_image_center() {
    // CSS Images 3 §3.1: the line runs through the sized image, not its element.
    let image = draw(include_str!("../../../fixtures/css-backgrounds/gradient-sized-angle.html"));
    assert_eq!(pixel(&image, 10, 10), [255, 0, 0, 255]);
    assert_eq!(pixel(&image, 30, 10), [0, 0, 255, 255]);
    assert_eq!(pixel(&image, 80, 10), [255, 255, 255, 255]);
}

#[test]
fn circular_farthest_corner_uses_the_diagonal_distance() {
    // CSS Images 3 §3.2: circle extent is the distance to the farthest corner.
    let image = draw(include_str!("../../../fixtures/css-backgrounds/gradient-farthest-corner.html"));
    assert_eq!(pixel(&image, 50, 100), [255, 0, 0, 255]);
    assert_eq!(pixel(&image, 50, 2), [255, 0, 0, 255]);
    assert_eq!(pixel(&image, 1, 1), [0, 0, 255, 255]);
}

#[test]
fn round_repeat_resolves_position_from_the_adjusted_tile_size() {
    // CSS Backgrounds 3 §2.4 and §2.6: rounding precedes position resolution.
    let image = draw(include_str!("../../../fixtures/css-backgrounds/gradient-round-position.html"));
    assert_eq!(pixel(&image, 65, 10), [0, 0, 255, 255]);
    assert_eq!(pixel(&image, 99, 10), [0, 0, 255, 255]);
    assert_eq!(pixel(&image, 10, 25), [255, 255, 255, 255]);
}

#[test]
fn inset_shadows_observe_front_to_back_order_and_skip_transparency() {
    // CSS Backgrounds 3 §6.1: the first shadow paints above the second.
    let image = draw(include_str!("../../../fixtures/css-backgrounds/inset-shadow-order.html"));
    assert_eq!(pixel(&image, 3, 30), [255, 0, 0, 255]);
    assert_eq!(pixel(&image, 12, 30), [0, 0, 255, 255]);
    assert_eq!(pixel(&image, 3, 110), [0, 0, 255, 255]);
}

#[test]
fn unblurred_spread_preserves_elliptical_corner_radii() {
    // CSS Backgrounds 3 §6.1 and §4.2: spread offsets each corner contour.
    let image = draw(include_str!("../../../fixtures/css-backgrounds/outset-elliptical-spread.html"));
    assert_eq!(pixel(&image, 30, 12), [255, 255, 255, 255]);
    assert_eq!(pixel(&image, 70, 12), [0, 0, 255, 255]);
}

// Regression guard: this geometry was correct on main. A positive spread
// outsets straight edges without rounding any corner whose radius is zero.
// In particular, a zero-sized source with positive spread stays a rectangle.
#[test]
fn sharp_corner_spread_and_empty_source_remain_rectangular() {
    // CSS Backgrounds 3 §6.1.1.
    let image = draw(include_str!("../../../fixtures/css-backgrounds/outset-sharp-spread-guard.html"));
    assert_eq!(pixel(&image, 32, 12), [0, 0, 255, 255]);
    assert_eq!(pixel(&image, 35, 8), [255, 255, 255, 255]);
    assert_eq!(pixel(&image, 84, 115), [0, 0, 255, 255]);
    assert_eq!(pixel(&image, 91, 115), [255, 255, 255, 255]);
    assert_eq!(pixel(&image, 22, 182), [0, 255, 0, 255]);
    assert_eq!(pixel(&image, 58, 218), [0, 255, 0, 255]);
}
