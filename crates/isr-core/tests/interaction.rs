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
fn overlapping_elements_follow_z_index_then_later_sibling_paint_order() {
    // Kept in memory so the checked-in probe fixture stays unchanged.
    let html = r#"<!doctype html><style>
        body{margin:0}
        #board{position:relative;width:300px;height:180px}
        .box{position:absolute;width:70px;height:70px}
        #low{left:10px;top:10px;z-index:1;background:red}
        #high{left:30px;top:30px;z-index:5;background:green}
        #early{left:130px;top:10px;background:blue}
        #late{left:150px;top:30px;background:yellow}
        </style><div id="board">
        <div class="box" id="low"></div><div class="box" id="high"></div>
        <div class="box" id="early"></div><div class="box" id="late"></div>
        </div>"#;
    let document = Document::new(html, RenderOptions::default()).unwrap();
    for (x, y, expected) in [
        (40.0, 40.0, "high"), // higher z-index beats earlier box
        (160.0, 40.0, "late"), // equal stacking level: later sibling wins
    ] {
        let actual = document.hit_test(x, y)
            .and_then(|node| document.attribute(node, "id"));
        assert_eq!(actual.as_deref(), Some(expected), "overlap at ({x}, {y})");
    }
}

#[test]
fn vector_scale_two_covers_four_times_the_solid_box_pixels() {
    let html = r#"<!doctype html><style>
        body{margin:0}
        #probe{position:absolute;top:10px;left:10px;width:30px;height:20px;
               background:#ff0000}
        </style><div id="probe"></div>"#;
    let opts = RenderOptions { width: 120, ..RenderOptions::default() };
    let mut native = Document::new(html, opts.clone()).unwrap();
    let mut scaled = Document::new_scaled(html, opts, 2.0).unwrap();
    let (css_w, css_h) = native.dimensions();
    let (one_w, one_h) = native.bitmap_dimensions();
    let (two_w, two_h) = scaled.bitmap_dimensions();
    assert_eq!((one_w, one_h), (css_w, css_h));
    assert_eq!((two_w, two_h), (one_w * 2, one_h * 2));
    let red_pixels = |rgba: &[u8]| {
        rgba.chunks_exact(4).filter(|pixel| {
            pixel[0] >= 250 && pixel[1] <= 5
                && pixel[2] <= 5 && pixel[3] >= 250
        }).count()
    };
    let one = red_pixels(&native.paint());
    let two = red_pixels(&scaled.paint());
    assert!(one > 0 && two > 0, "scaled paint must contain solid red pixels");
    assert!(
        two.abs_diff(one * 4) <= 40,
        "expected 4x solid box pixels: 1x={one}, 2x={two}"
    );
    assert_eq!(scaled.paint().len(), (two_w as usize) * (two_h as usize) * 4);
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
