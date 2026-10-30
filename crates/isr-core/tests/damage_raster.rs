// Copyright 2026 g1BSsQ. Licensed under Apache-2.0.
use isr_core::{Document, RenderOptions};

#[path = "../src/damage_constants.rs"]
mod damage_constants;
use damage_constants::INK_INFLATE_CSS;

const SCALE: f64 = 2.75;
// These paint-only fixtures have up to ~3 CSS px of glyph/flow ink beyond
// the CSSOM border box. The renderer adds INK_INFLATE_CSS on top of that ink.
const FIXTURE_INK_OVERFLOW_CSS: f64 = 3.0;

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
    // Normalize both rectangles to (left, top, right, bottom); Timings
    // supplies (x, y, width, height), while element_bounds supplies
    // (x, y, width, height) in CSS pixels.
    let damage = (
        f64::from(dx), f64::from(dy),
        f64::from(dx + dw), f64::from(dy + dh),
    );
    // An element's ink includes small glyph/flow overflow beyond its
    // border box; the renderer then inflates that ink by the shared
    // antialiasing margin before outward device-pixel rounding.
    let pad = FIXTURE_INK_OVERFLOW_CSS + INK_INFLATE_CSS;
    let local_ink = (
        ((x - pad) * SCALE).floor().max(0.0),
        ((y - pad) * SCALE).floor().max(0.0),
        ((x + w + pad) * SCALE).ceil(),
        ((y + h + pad) * SCALE).ceil(),
    );
    assert!(
        damage.0 >= local_ink.0 && damage.1 >= local_ink.1
            && damage.2 <= local_ink.2 && damage.3 <= local_ink.3,
        "{id}: damage {damage:?} escaped local ink {local_ink:?}",
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
