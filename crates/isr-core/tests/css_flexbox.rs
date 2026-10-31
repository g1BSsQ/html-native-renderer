// Copyright 2026 g1BSsQ. Licensed under Apache-2.0.
use isr_core::{render, RenderOptions};

fn marker_geometry(contents: &str) -> (f64, f64, f64, f64) {
    // Zero-size glyphs remove font-metric dependencies while leaving both
    // non-whitespace text runs as genuine anonymous flex items.
    // The following item exposes any extra anonymous-item gap exactly.
    let html = format!(
        r#"<style>
            body{{margin:0}}
            #row{{display:flex;align-items:flex-start;column-gap:20px;font-size:0}}
            #marker{{flex:none;width:10px;height:10px}}
        </style><div id=row>{contents}<div id=marker></div></div>"#
    );
    let output = render(
        &html,
        RenderOptions { width: 500, ..RenderOptions::default() },
    ).unwrap();
    let marker = output.elements.iter().find(|el| el.id == "marker").unwrap();
    (marker.x, marker.y, marker.width, marker.height)
}

fn assert_marker_at_one_gap(contents: &str) {
    let actual = marker_geometry(contents);
    let expected = (20.0, 0.0, 10.0, 10.0);
    assert!(
        (actual.0 - expected.0).abs() < 0.1
            && (actual.1 - expected.1).abs() < 0.1
            && (actual.2 - expected.2).abs() < 0.1
            && (actual.3 - expected.3).abs() < 0.1,
        "expected marker x/y/width/height {expected:?}, got {actual:?}"
    );
}

// CSS Flexbox 1 §4: a hidden child generates no flex item and must not
// separate adjacent text runs. Before the fix, it closes the first
// anonymous item, so two visible items precede the marker: x = 40px.
#[test]
fn hidden_element_does_not_split_anonymous_flex_item() {
    assert_marker_at_one_gap("a<span style='display:none'></span>b");
}

// The same rule applies when display:contents hoists the hidden child.
// On main, hoisting closes the current anonymous item, giving x = 40px.
#[test]
fn hidden_descendant_in_contents_does_not_split_anonymous_flex_item() {
    assert_marker_at_one_gap(
        "a<span style='display:contents'><i style='display:none'></i></span>b"
    );
}

// Regression guard: a contiguous anonymous flex item has one gap before
// the following explicit item. This behavior already passes on main.
#[test]
fn guard_contiguous_anonymous_flex_item_has_one_gap() {
    assert_marker_at_one_gap("ab");
}

// Regression guard: ordinary explicit flex items still wrap into new lines.
#[test]
fn guard_explicit_flex_items_wrap() {
    let html = r#"<style>body{margin:0}
        #row{display:flex;flex-wrap:wrap;width:90px}
        #a,#b{flex:none;width:80px;height:20px}
        </style><div id=row><div id=a></div><div id=b></div></div>"#;
    let output = render(
        html,
        RenderOptions { width: 500, ..RenderOptions::default() },
    ).unwrap();
    let item = |id| output.elements.iter().find(|el| el.id == id).unwrap();
    assert!((item("a").x - 0.0).abs() < 0.1);
    assert!((item("a").y - 0.0).abs() < 0.1);
    assert!((item("b").x - 0.0).abs() < 0.1);
    assert!((item("b").y - 20.0).abs() < 0.1);
}
