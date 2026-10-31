// Copyright 2026 g1BSsQ. Licensed under Apache-2.0.
// Byte-exact partial target parity against a new full-viewport paint.
use isr_core::{Document, RenderOptions};

const SCALE: f32 = 2.75;
fn options() -> RenderOptions {
    RenderOptions {
        width: 392,
        fonts: vec![include_bytes!(
            "../../../android/demo/app/src/main/assets/Roboto-Regular.ttf"
        ).to_vec()],
        ..RenderOptions::default()
    }
}

fn check_partial(
    before: &str, after: &str, id: &str, class: &str,
    scroll_css: f64, viewport_css: u32, expected_viewport_target: bool,
) {
    let mut retained = Document::new_scaled(before, options(), SCALE).unwrap();
    retained.set_viewport(scroll_css, viewport_css).unwrap();
    let unchanged = retained.paint();
    let target = retained.element_by_id(id).expect("mutation target");
    retained.toggle_class(target, class).unwrap();
    let t = retained.update();
    assert_eq!(t.paint_path, "partial", "{id}: unexpected invalidation");
    assert_eq!(t.viewport_target, expected_viewport_target,
        "{id}: incorrect raster target; compact path was not exercised as expected");
    let (x, y, w, h) = t.damage_rect_px.expect("damage rectangle");
    assert!(w > 0 && h > 0);
    let (rw, rh) = retained.bitmap_dimensions();
    assert!(x + w <= rw && y + h <= rh);
    assert!(t.damage_px < u64::from(rw) * u64::from(rh),
        "{id}: test must exercise a bounded invalidation");
    let mut fresh = Document::new_scaled(after, options(), SCALE).unwrap();
    fresh.set_viewport(scroll_css, viewport_css).unwrap();
    let expected = fresh.paint();
    assert_ne!(unchanged, expected, "{id}: fixture must alter painted pixels");
    assert_eq!(retained.paint(), expected, "{id}: tile/full byte mismatch");
}

#[test]
fn gallery_selected_matches_full_at_fractional_device_scale() {
    let before = include_str!("../../../fixtures/09-grid-gallery.html")
        .replace("</style>", ".tile.selected{border:4px solid #176ab3;opacity:.82}</style>")
        .replace("<div class=\"tile\">River</div>",
                 "<div id=\"probe\" class=\"tile\">River</div>");
    let after = before.replace("id=\"probe\" class=\"tile\"",
                               "id=\"probe\" class=\"tile selected\"");
    check_partial(&before, &after, "probe", "selected", 0.0, 280, true);
}

#[test]
fn grid_flex_and_opacity_match_full() {
    let before = include_str!("../../../fixtures/damage-raster/paint-contexts.html");
    for id in ["grid-probe", "flex-probe", "opacity-probe"] {
        let after = before.replace(
            &format!("id=\"{id}\" class=\"item\""),
            &format!("id=\"{id}\" class=\"item active\""),
        );
        check_partial(before, &after, id, "active", 0.0, 650, true);
    }
}

#[test]
fn form_auto_height_damage_matches_fresh_full_viewport() {
    let before = include_str!("../../../fixtures/08-form-layout.html")
        .replace("</style>", ".value.invalid{border:3px solid #c22;background:#ffe4e4}</style>")
        .replace("<div class=\"value\">River bend</div>",
                 "<input id=\"probe\" class=\"value\" type=\"text\" value=\"River bend\">");
    let after = before.replace("id=\"probe\" class=\"value\"",
                               "id=\"probe\" class=\"value invalid\"");
    check_partial(&before, &after, "probe", "invalid", 0.0, 500, false);
}

#[test]
fn scrolled_partial_target_matches_fresh_full_viewport() {
    let before = r#"<!doctype html><html><head><style>
        body{margin:0;background:#fff;font:17px sans-serif}
        .spacer{height:500px;background:#e8ecf2}
        #probe{width:180px;height:90px;margin:24px;background:#d3e7fb;
            border-radius:12px;border:2px solid #2567aa;padding:10px}
        #probe.active{background:#f0b0a9;color:#73173d}
        .tail{height:500px;background:#eef5ee}
    </style></head><body><div class="spacer">Header</div>
        <div id="probe">Scrolled target</div><div class="tail"></div>
    </body></html>"#;
    let after = before.replace("id=\"probe\"", "id=\"probe\" class=\"active\"");
    check_partial(before, &after, "probe", "active", 400.0, 290, true);
}

#[test]
fn transformed_container_partial_matches_full() {
    let before = r#"<!doctype html><html><head><style>
        body{margin:0;background:white;font:18px sans-serif}
        #parent{width:280px;height:180px;margin:24px;
            transform:translate(12.25px,6.5px);background:#f0f2fa}
        #probe{width:170px;height:80px;margin:14px;padding:10px;
            background:#bbdcef;border-radius:14px}
        #probe.active{background:#ddaa9b;color:#60121a}
    </style></head><body><div id="parent">
        <div id="probe">Shifted target</div></div></body></html>"#;
    let after = before.replace("id=\"probe\"", "id=\"probe\" class=\"active\"");
    check_partial(before, &after, "probe", "active", 0.0, 320, true);
}

#[test]
fn neutral_text_and_rounded_border_use_compact_raster_at_many_offsets() {
    // These cases intentionally contain no gradient, transform, opacity
    // layer, mask, shadow or scroll. Each MUST use a compact damage target;
    // checking only paint_path == "partial" would allow a false positive.
    // Near-right and near-bottom cases test outward 16px alignment past a
    // non-multiple-of-16 viewport edge (1078 x 2167 device pixels).
    for (left, top, width, height) in [
        (14, 12, 158, 54),
        (40, 195, 158, 54),
        (102, 514, 158, 54),
        (345, 411, 63, 56),  // right edge clipped by viewport
        (60, 752, 215, 58),  // bottom edge clipped by viewport
        (342, 750, 80, 62),  // right and bottom edges
    ] {
        let before = format!(r#"<!doctype html><html><head><style>
            *{{box-sizing:border-box}}
            body{{margin:0;background:#fff;font:18px/1.3 Roboto,sans-serif}}
            #probe{{position:absolute;left:{left}px;top:{top}px;
                width:{width}px;height:{height}px;padding:6px;
                background:#e0e9f0;color:#17304d;border:2px solid #2c5977;
                border-radius:9px}}
            #probe.active{{background:#f1c9b8;color:#7c2141;
                border-color:#ad425c}}
        </style></head><body><div id="probe">Rounded text</div></body></html>"#);
        let after = before.replace(
            "id=\"probe\"", "id=\"probe\" class=\"active\"",
        );
        check_partial(&before, &after, "probe", "active", 0.0, 788, false);
    }
}
