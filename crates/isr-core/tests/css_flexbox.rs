// Copyright 2026 g1BSsQ. Licensed under Apache-2.0.
use isr_core::{render, RenderOptions};

fn marker_geometry(contents: &str) -> (f64, f64) {
    let html = format!(
        r#"<style>
            body{{margin:0}}
            #row{{display:flex;flex-wrap:wrap;width:400px;font:20px Arial,sans-serif}}
            #marker{{flex:none;width:10px;height:10px}}
        </style><div id=row>{contents}<div id=marker></div></div>"#
    );
    let output = render(
        &html,
        RenderOptions { width: 500, ..RenderOptions::default() },
    ).unwrap();
    let marker = output.elements.iter().find(|el| el.id == "marker").unwrap();
    (marker.x, marker.y)
}

fn assert_same_marker_geometry(actual: (f64, f64), expected: (f64, f64)) {
    assert!(
        (actual.0 - expected.0).abs() < 0.1 &&
        (actual.1 - expected.1).abs() < 0.1,
        "flex item geometry {actual:?} differs from {expected:?}"
    );
}

// CSS Flexbox 1 §4: display:none boxes must not divide contiguous text
// into separate anonymous flex items, or discard the intervening space.
#[test]
fn hidden_element_does_not_split_anonymous_flex_item() {
    let expected = marker_geometry("two words");
    let actual = marker_geometry("two <span style='display:none'></span>words");
    assert_same_marker_geometry(actual, expected);
}

// The same flex-item rule applies through boxless display:contents wrappers.
#[test]
fn hidden_descendant_in_contents_does_not_split_anonymous_flex_item() {
    let expected = marker_geometry("two words");
    let actual = marker_geometry(
        "two <span style='display:contents'><i style='display:none'></i></span>words"
    );
    assert_same_marker_geometry(actual, expected);
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
