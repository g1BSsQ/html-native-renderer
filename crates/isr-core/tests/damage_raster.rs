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
    // Expected to fail on the previous allocation-identity rule:
    // grid and flex can invalidate the whole parent or a restyled sibling.
    // These have not been executed on the base commit; this is a code-level
    // expectation, not a measured base result.
    // GUARD (opacity-probe): without a sibling tile, the base implementation
    // may already have kept the damage local; retain parity coverage.
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

#[test]
fn untouched_pseudo_sibling_repaints_when_checked_state_changes() {
    let before = r#"<!doctype html><html><head><style>
        body{margin:0;background:white}
        #switch{position:absolute;top:12px;left:12px}
        label{display:block;margin:42px 20px;width:220px;height:80px;background:#eee}
        label::before{content:"";display:inline-block;width:48px;height:48px;
            margin:8px;background:#d24a3a;border:2px solid #222}
        #switch:checked + label::before{background:#28a368}
    </style></head><body>
        <input id="switch" type="checkbox"><label id="target">Checked indicator</label>
    </body></html>"#;
    let after = before.replace("type=\"checkbox\"", "type=\"checkbox\" checked=\"true\"");
    let mut doc = Document::new_scaled(before, options(), SCALE as f32).unwrap();
    doc.set_viewport(0.0, 220).unwrap();
    let initial = doc.paint();
    let input = doc.element_by_id("switch").unwrap();
    doc.set_attribute(input, "checked", "true").unwrap();
    let timing = doc.update();
    assert!(timing.damage_px > 0);
    let mut fresh = Document::new_scaled(&after, options(), SCALE as f32).unwrap();
    fresh.set_viewport(0.0, 220).unwrap();
    let expected = fresh.paint();
    assert_ne!(initial, expected, "the pseudo-element must visibly change");
    assert_eq!(doc.paint(), expected, "untouched label::before retained stale ink");
}

#[test]
fn untouched_sibling_outline_offset_restyle_repaints() {
    let before = r#"<!doctype html><html><head><style>
        body{margin:0;background:white}
        #switch{width:100px;height:32px;margin:12px;background:#ddd}
        #target{margin:25px 40px;width:180px;height:70px;
            background:#ebded0;outline:4px solid #196bad;outline-offset:0px}
        #switch.active + #target{outline-offset:18px}
    </style></head><body>
        <div id="switch"></div><div id="target"><span>Outline target</span></div>
    </body></html>"#;
    let after = before.replace("id=\"switch\"", "id=\"switch\" class=\"active\"");
    let mut doc = Document::new_scaled(before, options(), SCALE as f32).unwrap();
    let target = doc.element_by_id("target").unwrap();
    let old_box = doc.element_bounds(target).unwrap();
    doc.set_viewport(0.0, 260).unwrap();
    let initial = doc.paint();
    let trigger = doc.element_by_id("switch").unwrap();
    doc.toggle_class(trigger, "active").unwrap();
    let timing = doc.update();
    assert_eq!(doc.element_bounds(target).unwrap(), old_box);
    assert!(timing.damage_px > 0);
    let mut fresh = Document::new_scaled(&after, options(), SCALE as f32).unwrap();
    fresh.set_viewport(0.0, 260).unwrap();
    let expected = fresh.paint();
    assert_ne!(initial, expected, "outline offset must visibly change");
    assert_eq!(doc.paint(), expected, "untouched sibling outline retained stale ink");
}

#[test]
fn pseudo_sibling_box_shadow_removal_repaints_beyond_owner_bounds() {
    let before = r#"<!doctype html><html><head><style>
        body{margin:0;background:#fff}
        #switch{position:absolute;top:12px;left:12px}
        label{display:block;margin:60px 35px;width:180px;height:72px;
            background:#e4e8ef}
        label::before{content:"";display:inline-block;width:32px;height:32px;
            margin:12px;background:#246bab;box-shadow:0 0 24px red}
        #switch:checked + label::before{box-shadow:none}
    </style></head><body>
        <input id="switch" type="checkbox"><label id="target">Shadow marker</label>
    </body></html>"#;
    let after = before.replace("type=\"checkbox\"", "type=\"checkbox\" checked=\"true\"");
    let mut retained = Document::new_scaled(before, options(), SCALE as f32).unwrap();
    retained.set_viewport(0.0, 260).unwrap();
    let old_pixels = retained.paint();
    let trigger = retained.element_by_id("switch").unwrap();
    retained.set_attribute(trigger, "checked", "true").unwrap();
    let timing = retained.update();
    assert_eq!(timing.paint_path, "full_viewport",
        "a generated box shadow cannot be bounded by its owner's ink");
    let mut fresh = Document::new_scaled(&after, options(), SCALE as f32).unwrap();
    fresh.set_viewport(0.0, 260).unwrap();
    let expected = fresh.paint();
    assert_ne!(old_pixels, expected, "pseudo box shadow must visibly disappear");
    assert_eq!(retained.paint(), expected,
        "removed pseudo box-shadow left stale pixels outside owner bounds");
}
