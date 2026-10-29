// Copyright 2026 g1BSsQ. Licensed under Apache-2.0.
use isr_core::{Document, RenderOptions, render};

fn parity(before: String, after: String) {
    let opts = RenderOptions { width: 412, ..Default::default() };
    let mut document = Document::new(&before, opts.clone()).unwrap();
    let target = document.element_by_id("probe").expect("interaction target");
    document.toggle_class(target, "active").unwrap();
    let times = document.update();
    assert!(times.total_us() >= times.paint_us);
    let pixels = document.paint();
    let fresh = render(&after, opts).unwrap();
    assert_eq!(document.dimensions(), (fresh.width, fresh.height));
    assert_eq!(pixels, fresh.rgba, "retained repaint differs from fresh render");
}

#[test]
fn toggling_class_matches_fresh_render_for_three_fixtures() {
    // Fixture 1: form grid, with an interaction target inserted in memory.
    let form = include_str!("../../../fixtures/08-form-layout.html")
        .replace("</style>", ".active{border:4px solid #f00}</style>")
        .replace("<div class=\"value\">River bend</div>",
                 "<div id=\"probe\" class=\"value\">River bend</div>");
    parity(
        form.clone(),
        form.replace("id=\"probe\" class=\"value\"",
                     "id=\"probe\" class=\"value active\""),
    );

    // Fixture 2: gallery tile, with a change to border and opacity.
    let gallery = include_str!("../../../fixtures/09-grid-gallery.html")
        .replace("</style>", ".tile.active{border:4px solid #000;opacity:.7}</style>")
        .replace("<div class=\"tile\">River</div>",
                 "<div id=\"probe\" class=\"tile\">River</div>");
    parity(
        gallery.clone(),
        gallery.replace("id=\"probe\" class=\"tile\"",
                        "id=\"probe\" class=\"tile active\""),
    );

    // Fixture 3: list row, including a font-size change that affects layout.
    let list = include_str!("../../../fixtures/02-list-rows.html")
        .replace("</style>", ".copy.active{font-size:22px;color:#922}</style>")
        .replace("<div class=\"copy\">Morning walk</div>",
                 "<div id=\"probe\" class=\"copy\">Morning walk</div>");
    parity(
        list.clone(),
        list.replace("id=\"probe\" class=\"copy\"",
                     "id=\"probe\" class=\"copy active\""),
    );
}

#[test]
fn hit_test_respects_paint_order_visibility_pointer_events_and_gaps() {
    let html = include_str!("../../../fixtures/interaction/hit-order.html");
    let document = Document::new(html, RenderOptions::default()).unwrap();
    let checks = [
        (40.0, 40.0, Some("inner")),   // nested child
        (110.0, 100.0, Some("left")),  // overlay is pointer-events:none
        (180.0, 50.0, Some("right")), // hidden box must not intercept
        (135.0, 60.0, Some("board")), // gap between siblings
        (200.0, 180.0, Some("board")), // beneath the overlay
        (-1.0, 10.0, None),          // outside the document
    ];
    for (x, y, expected) in checks {
        let id = document.hit_test(x, y)
            .and_then(|node| document.attribute(node, "id"));
        assert_eq!(id.as_deref(), expected, "hit at ({x}, {y})");
    }
}

#[test]
fn text_and_attribute_mutations_update_retained_tree() {
    let html = "<!doctype html><style>#probe{color:red}</style><div id=\"probe\">old</div>";
    let mut doc = Document::new(html, RenderOptions::default()).unwrap();
    let node = doc.element_by_id("probe").unwrap();
    doc.set_text(node, "new").unwrap();
    doc.set_attribute(node, "title", "updated").unwrap();
    assert_eq!(doc.attribute(node, "title").as_deref(), Some("updated"));
    doc.update();
    assert_eq!(
        doc.paint(),
        render(
            "<!doctype html><style>#probe{color:red}</style><div id=\"probe\" title=\"updated\">new</div>",
            RenderOptions::default()
        ).unwrap().rgba
    );
}
