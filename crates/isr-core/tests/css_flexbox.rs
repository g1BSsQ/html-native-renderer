// Copyright 2026 g1BSsQ. Licensed under Apache-2.0.
use isr_core::{render, RenderOptions};
use blitz_dom::{BaseDocument, DocumentConfig, Node, NodeData};
use blitz_html::HtmlDocument;
use blitz_traits::shell::{ColorScheme, Viewport};

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

// Read the actual anonymous flex boxes, which are not DOM elements and are
// therefore absent from the public geometry export. At font-size:0, their
// text advance and border-box width are exactly zero CSS pixels.
fn anonymous_item_boxes(contents: &str) -> Vec<(f64, f64)> {
    let html = format!(
        r#"<style>body{{margin:0}}#row{{display:flex;column-gap:20px;font-size:0}}</style>
           <div id=row>{contents}<div style="flex:none;width:10px;height:10px"></div></div>"#
    );
    let mut doc = HtmlDocument::from_html(&html, DocumentConfig {
        viewport: Some(Viewport::new(500, 800, 1.0, ColorScheme::Light)),
        ..Default::default()
    });
    doc.resolve(0.0);

    fn find_row<'a>(doc: &'a BaseDocument, node: &'a Node) -> Option<&'a Node> {
        if node.attr(blitz_dom::local_name!("id")) == Some("row") {
            return Some(node);
        }
        node.children.iter().find_map(|id|
            doc.get_node(*id).and_then(|child| find_row(doc, child))
        )
    }
    let row = find_row(&doc, doc.root_node()).expect("flex container");
    let children = row.layout_children.borrow();
    children.as_ref().expect("constructed flex children").iter().filter_map(|id| {
        let node = doc.get_node(*id)?;
        matches!(&node.data, NodeData::AnonymousBlock(_)).then_some((
            node.final_layout().location.x as f64,
            node.final_layout().size.width as f64,
        ))
    }).collect()
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
    let input = "a<span style='display:none'></span>b";
    assert_marker_at_one_gap(input);
    assert_eq!(anonymous_item_boxes(input), vec![(0.0, 0.0)],
        "one zero-width anonymous flex item must contain both text runs");
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

// Regression guard: ignored hidden children and the intervening whitespace
// do not split the adjacent non-whitespace text runs.
#[test]
fn guard_hidden_elements_around_whitespace_keep_one_anonymous_item() {
    let input = "a<span style='display:none'></span> <span style='display:none'></span>b";
    assert_marker_at_one_gap(input);
    assert_eq!(anonymous_item_boxes(input), vec![(0.0, 0.0)]);
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
