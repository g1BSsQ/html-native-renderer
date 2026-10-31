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
    scroll_css: f64, viewport_css: u32,
) {
    let mut retained = Document::new_scaled(before, options(), SCALE).unwrap();
    retained.set_viewport(scroll_css, viewport_css).unwrap();
    let unchanged = retained.paint();
    let target = retained.element_by_id(id).expect("mutation target");
    retained.toggle_class(target, class).unwrap();
    let t = retained.update();
    assert_eq!(t.paint_path, "partial", "{id}: unexpected fallback");
    let (x, y, w, h) = t.damage_rect_px.expect("damage rectangle");
    assert!(w > 0 && h > 0);
    let (rw, rh) = retained.bitmap_dimensions();
    assert!(x + w <= rw && y + h <= rh);
    assert!(t.damage_px < u64::from(rw) * u64::from(rh),
        "{id}: test must exercise a damage-sized output");
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
    check_partial(&before, &after, "probe", "selected", 0.0, 280);
}

#[test]
fn grid_flex_and_opacity_match_full() {
    let before = include_str!("../../../fixtures/damage-raster/paint-contexts.html");
    for id in ["grid-probe", "flex-probe", "opacity-probe"] {
        let after = before.replace(
            &format!("id=\"{id}\" class=\"item\""),
            &format!("id=\"{id}\" class=\"item active\""),
        );
        check_partial(before, &after, id, "active", 0.0, 650);
    }
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
    check_partial(before, &after, "probe", "active", 400.0, 290);
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
    check_partial(before, &after, "probe", "active", 0.0, 320);
}
