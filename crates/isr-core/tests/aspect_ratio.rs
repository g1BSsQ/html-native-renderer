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

#[test]
fn block_ratio_uses_border_box_and_content_box() {
    assert_box(r#"<style>body{margin:0}#box{width:120px;box-sizing:border-box;
        padding:10px;border:5px solid;aspect-ratio:2/1}</style>
        <div id=box></div>"#, "box", 120.0, 60.0);
    assert_box(r#"<style>body{margin:0}#box{width:120px;box-sizing:content-box;
        padding:10px;border:5px solid;aspect-ratio:2/1}</style>
        <div id=box></div>"#, "box", 150.0, 90.0);
}

#[test]
fn block_min_max_transfer_before_ratio_dependent_size() {
    let html = r#"<style>body{margin:0}
        #capped{width:300px;max-width:100px;aspect-ratio:2/1}
        #floored{width:10px;min-width:100px;aspect-ratio:2/1}</style>
        <div id=capped></div><div id=floored></div>"#;
    assert_box(html, "capped", 100.0, 50.0);
    assert_box(html, "floored", 100.0, 50.0);
}

#[test]
fn automatic_minimum_protects_content_unless_overflow_clips() {
    let html = r#"<style>body{margin:0}.tile{width:100px;aspect-ratio:1/1}
        #clipped{overflow:hidden}</style>
        <div class=tile id=visible><div style="height:180px"></div></div>
        <div class=tile id=clipped><div style="height:180px"></div></div>"#;
    assert_box(html, "visible", 100.0, 180.0);
    assert_box(html, "clipped", 100.0, 100.0);
}

#[test]
fn flex_item_stretched_cross_size_transfers_through_ratio() {
    let html = r#"<style>body{margin:0}
        #container{display:flex;align-items:stretch;height:100px}
        #item{aspect-ratio:2/1;min-width:0;flex:none}</style>
        <div id=container><div id=item></div></div>"#;
    assert_box(html, "item", 200.0, 100.0);
}

#[test]
fn grid_item_in_definite_track_derives_block_size() {
    let html = r#"<style>body{margin:0}
        #container{display:grid;grid-template-columns:160px}
        #item{aspect-ratio:2/1;justify-self:stretch;align-self:start}</style>
        <div id=container><div id=item></div></div>"#;
    assert_box(html, "item", 160.0, 80.0);
}

#[test]
fn replaced_auto_ratio_prefers_natural_ratio() {
    let html = r#"<style>body{margin:0}#image{width:120px;aspect-ratio:auto 1/1}</style>
        <img id=image src="data:image/svg+xml,%3Csvg%20xmlns='http://www.w3.org/2000/svg'%20width='80'%20height='40'%3E%3C/svg%3E">"#;
    assert_box(html, "image", 120.0, 60.0);
}
