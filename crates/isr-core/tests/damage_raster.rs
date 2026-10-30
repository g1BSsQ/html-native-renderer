// Copyright 2026 g1BSsQ. Licensed under Apache-2.0.
use isr_core::{Document, RenderOptions};

const SCALE: f64 = 2.75;

fn options() -> RenderOptions {
    RenderOptions {
        width: 392,
        fonts: vec![include_bytes!(
            "../../../android/demo/app/src/main/assets/Roboto-Regular.ttf"
        ).to_vec()],
        ..RenderOptions::default()
    }
}

fn paint_only_is_local(before: &str, after: &str, id: &str, height: u32) {
    let mut doc = Document::new_scaled(before, options(), SCALE as f32).unwrap();
    let probe = doc.element_by_id(id).unwrap();
    let box_before = doc.element_bounds(probe).unwrap();
    doc.set_viewport(0.0, height).unwrap();
    doc.toggle_class(probe, if id == "probe" { "selected" } else { "active" }).unwrap();
    let timing = doc.update();
    let box_after = doc.element_bounds(probe).unwrap();
    assert_eq!(box_before, box_after, "paint-only border box must not move");
    assert_eq!(timing.paint_path, "partial");
    let (x, y, w, h) = box_after;
    let (dx, dy, dw, dh) = timing.damage_rect_px.unwrap();
    // The snapshot expands ink by 8 CSS px for glyph antialiasing and
    // overhang. Allow one additional CSS pixel for device rounding.
    let left = ((x - 9.0) * SCALE).floor().max(0.0);
    let top = ((y - 9.0) * SCALE).floor().max(0.0);
    let right = ((x + w + 9.0) * SCALE).ceil();
    let bottom = ((y + h + 9.0) * SCALE).ceil();
    assert!(
        dx as f64 >= left && dy as f64 >= top
            && (dx + dw) as f64 <= right && (dy + dh) as f64 <= bottom,
        "{id}: damage {:?} escaped local ink ({left}, {top}, {right}, {bottom})",
        timing.damage_rect_px,
    );
    let mut fresh = Document::new_scaled(after, options(), SCALE as f32).unwrap();
    fresh.set_viewport(0.0, height).unwrap();
    assert_eq!(doc.paint(), fresh.paint(), "{id}: stale pixels");
}

#[test]
fn gallery_selected_only_invalidates_its_tile() {
    let before = include_str!("../../../fixtures/09-grid-gallery.html")
        .replace("</style>", ".tile.selected{border:4px solid #176ab3;opacity:.82}</style>")
        .replace("<div class=\"tile\">River</div>",
                 "<div id=\"probe\" class=\"tile\">River</div>");
    let after = before.replace("id=\"probe\" class=\"tile\"",
                               "id=\"probe\" class=\"tile selected\"");
    paint_only_is_local(&before, &after, "probe", 280);
}

#[test]
fn paint_only_toggle_stays_local_inside_grid_flex_and_opacity() {
    let before = include_str!("../../../fixtures/damage-raster/paint-contexts.html");
    for id in ["grid-probe", "flex-probe", "opacity-probe"] {
        let after = before.replace(
            &format!("id=\"{id}\" class=\"item\""),
            &format!("id=\"{id}\" class=\"item active\""),
        );
        assert_ne!(before, after, "fixture must actually change");
        paint_only_is_local(before, &after, id, 650);
    }
}

// GUARD: the auto-height input really changes layout; damage cannot be
// narrowed to the input just because it was the explicit mutation target.
#[test]
fn auto_height_border_growth_keeps_shifted_siblings_damaged() {
    let before = include_str!("../../../fixtures/08-form-layout.html")
        .replace("</style>", ".value.invalid{border:3px solid #c22;background:#ffe4e4}</style>")
        .replace("<div class=\"value\">River bend</div>",
                 "<input id=\"probe\" class=\"value\" type=\"text\" value=\"River bend\">")
        .replace("<div class=\"field\"><div class=\"label\">Distance</div>",
                 "<div id=\"following\" class=\"field\"><div class=\"label\">Distance</div>");
    let after = before.replace("id=\"probe\" class=\"value\"",
                               "id=\"probe\" class=\"value invalid\"");
    let mut doc = Document::new_scaled(&before, options(), SCALE as f32).unwrap();
    let probe = doc.element_by_id("probe").unwrap();
    let following = doc.element_by_id("following").unwrap();
    let old_top = doc.element_bounds(following).unwrap().1;
    doc.set_viewport(0.0, 500).unwrap();
    doc.toggle_class(probe, "invalid").unwrap();
    let t = doc.update();
    let (fx, fy, fw, fh) = doc.element_bounds(following).unwrap();
    assert!(fy > old_top, "input height growth must shift next grid row");
    let (x, y, w, h) = t.damage_rect_px.unwrap();
    assert!(x as f64 <= fx * SCALE && (x + w) as f64 >= (fx + fw) * SCALE - 1.0);
    assert!(y as f64 <= old_top * SCALE && (y + h) as f64 >= (fy + fh) * SCALE - 1.0);
    let mut fresh = Document::new_scaled(&after, options(), SCALE as f32).unwrap();
    fresh.set_viewport(0.0, 500).unwrap();
    assert_eq!(doc.paint(), fresh.paint());
}
