// Copyright 2026. Licensed under Apache-2.0.
use isr_core::{render, RenderOptions};

fn assert_box(html: &str, id: &str, width: f64, height: f64) {
    let output = render(html, RenderOptions {
        width: 500, ..RenderOptions::default()
    }).unwrap();
    assert!(output.blocked_resources.is_empty());
    let el = output.elements.iter().find(|el| el.id == id).unwrap();
    assert!((el.width - width).abs() < 0.15, "{id}: width {} != {width}", el.width);
    assert!((el.height - height).abs() < 0.15, "{id}: height {} != {height}", el.height);
}

// The automatic minimum must be allowed to exceed the preferred aspect-ratio
// size, after converting between content and border boxes.
#[test]
fn block_content_box_ratio_respects_content_automatic_minimum() {
    let html = r#"<style>body{margin:0}
        #box{width:60px;aspect-ratio:2/1;padding:10px;border:10px solid green}
        </style><div id=box><div style="height:60px"></div></div>"#;
    assert_box(html, "box", 100.0, 100.0);
}

#[test]
fn block_border_box_ratio_respects_content_automatic_minimum() {
    let html = r#"<style>body{margin:0}
        #box{box-sizing:border-box;width:100px;aspect-ratio:2/1;
        padding:10px;border:10px solid green}
        </style><div id=box><div style="height:60px"></div></div>"#;
    assert_box(html, "box", 100.0, 100.0);
}

// A transferred maximum cannot suppress the content-based automatic minimum.
#[test]
fn max_width_transfer_does_not_cap_content_automatic_minimum() {
    let html = r#"<style>body{margin:0}
        #box{width:200px;max-width:100px;aspect-ratio:2/1}
        </style><div id=box><div style="height:100px"></div></div>"#;
    assert_box(html, "box", 100.0, 100.0);
}

const NATURAL_RATIO_IMAGE: &str =
    "data:image/svg+xml,%3Csvg%20xmlns='http://www.w3.org/2000/svg'%20width='80'%20height='40'%3E%3C/svg%3E";

#[test]
fn replaced_auto_ratio_prefers_natural_ratio() {
    let html = format!(r#"<style>body{{margin:0}}#image{{width:120px;aspect-ratio:auto 1/1}}</style>
        <img id=image src="{NATURAL_RATIO_IMAGE}">"#);
    assert_box(&html, "image", 120.0, 60.0);
}

// A stretched flex cross size feeds natural-ratio transfer into the main size.
#[test]
fn flex_stretched_cross_size_transfers_natural_auto_ratio() {
    let html = format!(r#"<style>body{{margin:0}}
        #row{{display:flex;height:120px;align-items:stretch}}
        #image{{aspect-ratio:auto 1/1;min-width:0;flex:none}}
        </style><div id=row><img id=image src="{NATURAL_RATIO_IMAGE}"></div>"#);
    assert_box(&html, "image", 240.0, 120.0);
}

#[test]
fn grid_definite_track_uses_natural_auto_ratio() {
    let html = format!(r#"<style>body{{margin:0}}
        #grid{{display:grid;grid-template-columns:120px}}
        #image{{aspect-ratio:auto 1/1;width:100%;align-self:start}}
        </style><div id=grid><img id=image src="{NATURAL_RATIO_IMAGE}"></div>"#);
    assert_box(&html, "image", 120.0, 60.0);
}

// Guard the already-correct margin-collapsing behavior. The positive bottom
// margin of a zero-height child is not its parent’s content minimum size.
#[test]
fn collapsed_child_margins_do_not_enlarge_ratio_sized_parent() {
    let html = r#"<style>body{margin:0}</style>
        <div style="width:100px;height:25px;margin-bottom:-200px"></div>
        <div id=box style="width:100px;aspect-ratio:2/1;margin:0">
          <div style="width:100px;margin-top:50px;margin-bottom:200px"></div>
        </div>
        <div style="width:100px;height:25px"></div>"#;
    assert_box(html, "box", 100.0, 50.0);
}
