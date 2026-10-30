// Copyright 2026 g1BSsQ. Licensed under Apache-2.0.
use isr_core::{Document, RenderOptions};

enum Mutation<'a> { Class(&'a str), Text(&'a str) }

// Include actual glyph ink in every viewport regression.
fn options() -> RenderOptions {
    RenderOptions {
        width: 392,
        fonts: vec![include_bytes!(
            "../../../android/demo/app/src/main/assets/Roboto-Regular.ttf"
        ).to_vec()],
        ..RenderOptions::default()
    }
}

fn assert_viewport_parity_for_path(
    page: &str, before: &str, after: &str, change: Mutation<'_>,
    expected_path: &str,
) {
    let options = options();
    let mut retained = Document::new_scaled(before, options.clone(), 2.75).unwrap();
    let target = retained.element_by_id("probe").unwrap();
    let (_, y, _, h) = retained.element_bounds(target).unwrap();
    let scroll = (y + h / 2.0 - 140.0).max(0.0);
    retained.set_viewport(scroll, 280).unwrap();
    match change {
        Mutation::Class(class) => retained.toggle_class(target, class).unwrap(),
        Mutation::Text(text) => retained.set_text(target, text).unwrap(),
    }
    let timing = retained.update();
    assert_eq!(timing.paint_path, expected_path, "page={page}");
    assert!(timing.damage_px > 0, "page={page}: visible mutation must damage pixels");
    let result = retained.paint();
    let mut fresh = Document::new_scaled(after, options, 2.75).unwrap();
    fresh.set_viewport(scroll, 280).unwrap();
    assert_eq!(retained.bitmap_dimensions(), fresh.bitmap_dimensions());
    let expected = fresh.paint();
    let first_diff = result.iter().zip(&expected)
        .position(|(actual, fresh)| actual != fresh);
    if let Some(byte) = first_diff {
        let width = retained.bitmap_dimensions().0 as usize;
        let x = (byte / 4) % width;
        let y = (byte / 4) / width;
        let channel = byte % 4;
        let pixel = (byte / 4) * 4;
        // A fresh full repaint of the retained final state isolates crop
        // rasterization from possible incremental layout differences.
        retained.set_viewport(scroll, 280).unwrap();
        let full_retained = retained.paint();
        panic!(
            "viewport damage must equal a fresh viewport paint: page={page}, \
             first differing pixel=({x}, {y}), channel={channel}, \
             actual={:?}, expected={:?}, damage_rect_px={:?}, damage_px={}, path={}, \
             retained_full_matches_fresh={}, retained_full_pixel={:?}",
            &result[pixel..pixel + 4], &expected[pixel..pixel + 4],
            timing.damage_rect_px, timing.damage_px, timing.paint_path,
            full_retained == expected, &full_retained[pixel..pixel + 4],
        );
    }
    assert_eq!(result.len(), expected.len(), "page={page}: viewport pixel count");
}

fn assert_viewport_parity(page: &str, before: &str, after: &str, change: Mutation<'_>) {
    assert_viewport_parity_for_path(page, before, after, change, "partial");
}

#[test]
fn three_benchmark_pages_match_fresh_viewport() {
    let form = include_str!("../../../fixtures/08-form-layout.html")
        .replace("</style>", ".value.invalid{border:3px solid #c22;background:#ffe4e4}</style>")
        .replace("<div class=\"value\">River bend</div>",
                 "<input id=\"probe\" class=\"value\" type=\"text\" value=\"River bend\">");
    let changed = form.replace("id=\"probe\" class=\"value\"",
                               "id=\"probe\" class=\"value invalid\"");
    assert_viewport_parity("form_invalid", &form, &changed, Mutation::Class("invalid"));

    let source = include_str!("../../../fixtures/02-list-rows.html");
    let mut rows = String::new();
    for index in 1..=200 {
        let id = if index == 100 { " id=\"probe\"" } else { "" };
        rows.push_str(&format!(
            "<div class=\"row item\"><div class=\"dot\"></div><div class=\"copy\">Row {index}</div><div class=\"muted\"{id}>08:00</div></div>"
        ));
    }
    let open = source.find("<main id=\"list\">").unwrap();
    let close = source.find("</main>").unwrap() + "</main>".len();
    let list = format!(
        "{}<main id=\"list\"><h1>Today</h1>{rows}</main>{}",
        &source[..open], &source[close..]
    );
    let changed = list.replace("id=\"probe\">08:00", "id=\"probe\">13:45");
    assert_viewport_parity("list_200_text", &list, &changed, Mutation::Text("13:45"));

    let gallery = include_str!("../../../fixtures/09-grid-gallery.html")
        .replace("</style>", ".tile.selected{border:4px solid #176ab3;opacity:.82}</style>")
        .replace("<div class=\"tile\">River</div>",
                 "<div id=\"probe\" class=\"tile\">River</div>");
    let changed = gallery.replace("id=\"probe\" class=\"tile\"",
                                  "id=\"probe\" class=\"tile selected\"");
    assert_viewport_parity("gallery_selected", &gallery, &changed, Mutation::Class("selected"));
}

#[test]
fn shadow_outline_and_transform_match_fresh_viewport() {
    for (page, extra) in [
        ("shadow", "box-shadow:12px -7px 14px 3px rgba(0,0,0,.65)"),
        ("outline", "outline:6px solid #1548ff;outline-offset:5px"),
        ("transform", "transform:translate(24px,18px) rotate(8deg)"),
    ] {
        let before = format!(
            "<!doctype html><style>body{{margin:0;background:white}}              #probe{{width:100px;height:70px;background:#379acd;color:white}}              .active{{{extra}}}</style><div style=\"height:90px\"></div>             <div id=\"probe\" class=\"box\">Ink and layout</div>"
        );
        let after = before.replace("id=\"probe\" class=\"box\"",
                                   "id=\"probe\" class=\"box active\"");
        assert_viewport_parity(page, &before, &after, Mutation::Class("active"));
    }
}

#[test]
fn an_offscreen_text_change_keeps_cached_pixels_intact() {
    let html = "<!doctype html><style>body{margin:0;background:white}        #probe{margin-top:1800px;color:navy}</style>        <p>Visible</p><div id=\"probe\">Before</div>";
    let mut doc = Document::new_scaled(html, options(), 2.75).unwrap();
    doc.set_viewport(0.0, 220).unwrap();
    let prior = doc.paint();
    let target = doc.element_by_id("probe").unwrap();
    doc.set_text(target, "After").unwrap();
    let timing = doc.update();
    assert_eq!(timing.paint_path, "partial");
    assert_eq!(timing.damage_px, 0);
    assert_eq!(prior, doc.paint());
}

#[test]
fn nowrap_overflow_text_growth_and_shrink_are_exact() {
    let short = r#"<!doctype html><style>
        body{margin:0;background:white}
        #probe{width:60px;height:18px;white-space:nowrap;overflow:visible;color:#123456;
               background:#deefff}
        </style><div style="height:80px"></div><div id="probe">Hi</div>"#;
    let long = short.replace(">Hi</div>", ">Twelve words should overflow this tiny text box by many pixels today</div>");
    assert_viewport_parity(
        "nowrap_grow", short, &long,
        Mutation::Text("Twelve words should overflow this tiny text box by many pixels today"),
    );
    assert_viewport_parity(
        "nowrap_shrink", &long, short, Mutation::Text("Hi"),
    );
}

#[test]
fn fixed_height_wrapped_text_overflow_is_exact() {
    let short = r#"<!doctype html><style>
        body{margin:0;background:white}
        #probe{width:110px;height:18px;overflow:visible;line-height:18px;
               color:#233f88;background:#eee}
        </style><div style="height:70px"></div><div id="probe">Hi</div>"#;
    let long = short.replace(">Hi</div>", ">Many lines of words that wrap beyond fixed height</div>");
    assert_viewport_parity(
        "wrapped_overflow_grow", short, &long,
        Mutation::Text("Many lines of words that wrap beyond fixed height"),
    );
    assert_viewport_parity(
        "wrapped_overflow_shrink", &long, short, Mutation::Text("Hi"),
    );
}

#[test]
fn text_shadow_change_uses_safe_fallback_and_matches_fresh() {
    let before = r#"<!doctype html><style>
        body{margin:0;background:white}
        #probe{margin-top:70px;color:#123456;font-size:25px}
        .active{text-shadow:0 0 20px #12b}
        </style><div id="probe">Shadow text</div>"#;
    let after = before.replace("id=\"probe\"", "id=\"probe\" class=\"active\"");
    assert_viewport_parity_for_path(
        "text_shadow", before, &after, Mutation::Class("active"), "full_viewport",
    );
}

#[test]
fn transformed_ancestor_with_shadowed_child_uses_safe_fallback() {
    let before = r#"<!doctype html><style>
        body{margin:0;background:white}
        #parent{margin:85px 0 0 110px;transform:rotate(90deg);
                transform-origin:0 0;width:130px;height:90px}
        #probe{width:60px;height:30px;background:#0af;color:#123}
        .active{box-shadow:20px 0 10px 0 rgba(0,0,0,.8)}
        </style><div id="parent"><div id="probe">Child</div></div>"#;
    let after = before.replace("id=\"probe\"", "id=\"probe\" class=\"active\"");
    assert_viewport_parity_for_path(
        "ancestor_transform_shadow", before, &after,
        Mutation::Class("active"), "full_viewport",
    );
}

#[test]
fn body_background_change_falls_back_and_matches_fresh() {
    let before = r#"<!doctype html><style>
        body{margin:0;background:white;min-height:600px}
        body.active{background:#c8dafa}
        </style><body id="probe"><p>Foreground</p></body>"#;
    let after = before.replace("id=\"probe\"", "id=\"probe\" class=\"active\"");
    assert_viewport_parity_for_path(
        "body_background", before, &after,
        Mutation::Class("active"), "full_viewport",
    );
}

#[test]
fn two_mutations_in_one_update_match_fresh_viewport() {
    let before = r#"<!doctype html><style>
        body{margin:0;background:#fff}
        .active{color:#e03;background:#cef}
        </style><div style="height:70px"></div>
        <div id="probe">Before</div><div id="second">Old</div>"#;
    let after = before
        .replace("id=\"probe\"", "id=\"probe\" class=\"active\"")
        .replace("id=\"second\">Old", "id=\"second\">Updated");
    let options = options();
    let mut retained = Document::new_scaled(before, options.clone(), 2.75).unwrap();
    retained.set_viewport(0.0, 280).unwrap();
    retained.toggle_class(retained.element_by_id("probe").unwrap(), "active").unwrap();
    retained.set_text(retained.element_by_id("second").unwrap(), "Updated").unwrap();
    let timing = retained.update();
    assert_eq!(timing.paint_path, "partial", "two_mutations: {timing:?}");
    assert!(timing.damage_px > 0);
    let mut fresh = Document::new_scaled(&after, options, 2.75).unwrap();
    fresh.set_viewport(0.0, 280).unwrap();
    assert_eq!(retained.paint(), fresh.paint(), "two batched mutations must match fresh");
}

#[test]
fn inline_reflow_repaints_trailing_text_and_shrink() {
let short = r#"<!doctype html><style>
 body{margin:0;background:white;font-size:18px}p{width:235px;margin:20px 0;line-height:26px}
 </style><p>Status: <span id="probe">ok</span> and more words</p><p>next</p>"#;
 let long = short.replace(">ok</span>", ">much longer val</span>");
 assert_viewport_parity_for_path("inline_grow", short, &long, Mutation::Text("much longer val"), "full_viewport");
 assert_viewport_parity_for_path("inline_shrink", &long, short, Mutation::Text("ok"), "full_viewport");
}

#[test]
fn html_background_with_growing_body_card_matches_fresh() {
let short = r#"<!doctype html><style>
 html{background:#ddd}body{margin:0;width:300px;padding:16px;background:#f8e8bb}
 #probe{height:50px;background:#3778c0}#probe.tall{height:150px}
 </style><div id="probe"></div>"#;
 let tall = short.replace("id=\"probe\"", "id=\"probe\" class=\"tall\"");
 assert_viewport_parity("html_background_body_growth", short, &tall, Mutation::Class("tall"));
}

#[test]
fn body_bottom_border_and_padding_growth_matches_fresh() {
let short = r#"<!doctype html><style>
 body{margin:0;width:300px;padding:16px 16px 40px;background:#c8e2aa;
 border-bottom:12px solid #1646a4}
 #probe{height:50px;background:#d65}#probe.tall{height:150px}
 </style><div id="probe"></div>"#;
 let tall = short.replace("id=\"probe\"", "id=\"probe\" class=\"tall\"");
 assert_viewport_parity("body_padding_border_growth", short, &tall, Mutation::Class("tall"));
}

#[test]
fn sibling_layout_shift_matches_fresh() {
let short = r#"<!doctype html><style>
 body{margin:0;background:white}
 #probe{height:50px;width:180px;background:#e58}#probe.tall{height:120px}
 #following{height:50px;background:#48a;color:#123}
 </style><div id="probe"></div><div id="following">Following sibling</div>"#;
 let tall = short.replace("id=\"probe\"", "id=\"probe\" class=\"tall\"");
 assert_viewport_parity("sibling_shift", short, &tall, Mutation::Class("tall"));
}

#[test]
fn shrinking_document_reclamps_scroll_and_repaints_viewport() {
let before = r#"<!doctype html><style>
 body{margin:0}#probe{height:1520px;background:#7ad}#probe.short{height:100px}
 </style><div id="probe"></div>"#;
 let after = before.replace("id=\"probe\"", "id=\"probe\" class=\"short\"");
 let mut retained = Document::new_scaled(before, options(), 2.75).unwrap();
 retained.set_viewport(1500.0, 280).unwrap();
 assert!(retained.viewport_scroll_y() > 0.0);
 retained.toggle_class(retained.element_by_id("probe").unwrap(), "short").unwrap();
 let timing = retained.update();
 assert_eq!(timing.paint_path, "full_viewport", "scroll change invalidates cached viewport");
 assert_eq!(retained.dimensions(), (392, 800));
 assert!(retained.viewport_scroll_y() <= 520.0);
 let mut fresh = Document::new_scaled(&after, options(), 2.75).unwrap();
 fresh.set_viewport(retained.viewport_scroll_y(), 280).unwrap();
 assert_eq!(retained.paint(), fresh.paint(), "reclamped viewport must be exact");
}

#[test]
fn hit_test_after_scrolling_uses_document_pixel_coordinates() {
let mut html = String::from(
 "<!doctype html><style>body{margin:0}.row{height:42px;background:#ddd}</style>"
 );
 for row in 0..40 {
     html.push_str(&format!("<div id=\"r{row}\" class=\"row\"></div>"));
 }
 let mut document = Document::new(&html, options()).unwrap();
 document.set_viewport(900.0, 400).unwrap();
 let actual = document.hit_test(10.0, 1000.0)
     .and_then(|node| document.attribute(node, "id"));
 assert_eq!(actual.as_deref(), Some("r23"));
}
