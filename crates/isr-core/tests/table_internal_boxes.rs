// Copyright 2026 g1BSsQ. Licensed under Apache-2.0.
use isr_core::{render, RenderOptions, Rendered};

const PARTS: &[&str] = &[
    "table-row-group", "table-header-group", "table-footer-group",
    "table-row", "table-column-group", "table-column",
];

fn image(part: &str, property: &str, collapse: bool) -> Rendered {
    let cells = r#"<div class="cell" id="a"></div><div class="cell" id="b"></div>"#;
    let part_markup = match part {
        "table-row-group" | "table-header-group" | "table-footer-group" => {
            format!(r#"<div style="display:{part};{property}"><div style="display:table-row">{cells}</div></div>"#)
        }
        "table-row" => format!(r#"<div style="display:table-row;{property}">{cells}</div>"#),
        "table-column-group" => format!(
            r#"<div style="display:table-column-group;{property}"><div style="display:table-column"></div><div style="display:table-column"></div></div><div style="display:table-row">{cells}</div>"#
        ),
        "table-column" => format!(
            r#"<div style="display:table-column;{property}"></div><div style="display:table-column"></div><div style="display:table-row">{cells}</div>"#
        ),
        _ => panic!("unknown display"),
    };
    let collapse = if collapse { "collapse" } else { "separate" };
    let html = format!(r#"<style>
        html,body {{ margin:0; background:white }}
        #table {{ display:table; table-layout:fixed; border-collapse:{collapse};
                  border-spacing:0; margin:20px; width:96px }}
        .cell {{ display:table-cell; height:32px; width:48px }}
        </style><div id="table">{part_markup}</div>"#);
    render(&html, RenderOptions {
        width: 240, system_fonts: false, ..RenderOptions::default()
    }).unwrap()
}

fn cell_boxes(image: &Rendered) -> Vec<(f64, f64, f64, f64)> {
    image.elements.iter().filter(|e| e.id == "a" || e.id == "b")
        .map(|e| (e.x, e.y, e.width, e.height)).collect()
}

#[test]
fn internal_table_margins_and_padding_do_not_change_cell_geometry_or_pixels() {
    for &part in PARTS {
        for collapse in [true, false] {
            let reference = image(part, "", collapse);
            let decorated = image(part, "margin:23px;padding:31px", collapse);
            assert_eq!(cell_boxes(&decorated), cell_boxes(&reference),
                       "geometry changed for {part}, collapse={collapse}");
            assert_eq!(decorated.rgba, reference.rgba,
                       "paint changed for {part}, collapse={collapse}");
        }
    }
}

#[test]
fn internal_table_borders_do_not_paint_in_separated_model() {
    for &part in PARTS {
        let reference = image(part, "", false);
        let decorated = image(part, "border:4px solid green", false);
        assert_eq!(cell_boxes(&decorated), cell_boxes(&reference), "{part}");
        assert_eq!(decorated.rgba, reference.rgba, "{part}");
    }
}


// These are self-contained forms of the row-group-order and rules-groups
// reftests. The renderer must not add a second set of un-resolved borders
// on top of the existing table painter.
fn table_page(markup: &str, css: &str) -> Rendered {
    let html = format!(
        r#"<style>html,body{{background:white;margin:0}}{css}</style>{markup}"#
    );
    render(&html, RenderOptions {
        width: 400, system_fonts: false, ..RenderOptions::default()
    }).unwrap()
}

#[test]
fn row_group_order_preserves_the_reference_paint() {
    let css = r#"
        table { font-size:8px; border-collapse:collapse; }
        thead, tbody, tfoot { border:10px solid; }
        .first { border-color:green; }
        .second { border-color:blue; }
        .third { border-color:yellow; }
        tbody { border-color:orange; }
        td { width:50px; height:50px; }
    "#;
    // As in row-group-order.html: only the first header/footer is special;
    // later group elements retain normal document order.
    let input = r#"<table>
        <tbody><tr><td>body 1</td></tr></tbody>
        <thead class="first"><tr><td>head 1</td></tr></thead>
        <thead class="second"><tr><td>head 2</td></tr></thead>
        <tbody><tr><td>body 2</td></tr></tbody>
        <thead class="third"><tr><td>head 3</td></tr></thead>
        <tfoot class="first"><tr><td>foot 1</td></tr></tfoot>
        <tbody><tr><td>body 3</td></tr></tbody>
        <tfoot class="second"><tr><td>foot 2</td></tr></tfoot>
        <tbody><tr><td>body 4</td></tr></tbody>
        <tfoot class="third"><tr><td>foot 3</td></tr></tfoot>
    </table>"#;
    let reference = r#"<table>
        <tbody class="first"><tr><td>head 1</td></tr></tbody>
        <tbody><tr><td>body 1</td></tr></tbody>
        <tbody class="second"><tr><td>head 2</td></tr></tbody>
        <tbody><tr><td>body 2</td></tr></tbody>
        <tbody class="third"><tr><td>head 3</td></tr></tbody>
        <tbody><tr><td>body 3</td></tr></tbody>
        <tbody class="second"><tr><td>foot 2</td></tr></tbody>
        <tbody><tr><td>body 4</td></tr></tbody>
        <tbody class="third"><tr><td>foot 3</td></tr></tbody>
        <tbody class="first"><tr><td>foot 1</td></tr></tbody>
    </table>"#;
    assert_eq!(table_page(input, css).rgba, table_page(reference, css).rgba,
        "row group ordering must match the reference");
}

#[test]
fn group_rules_preserve_the_reference_paint() {
    let common = r#"
        table { margin:1em 1em 2em 1em; }
    "#;
    // rules-groups.html and its reference encode the same group boundaries
    // using a legacy table attribute and explicit collapsed group borders.
    let input = format!(r#"<style>
        #a > * {{ border-block-start:none; }}
        #b > * {{ border-block-end-color:blue; }}
        #c > * {{ border-block-end-width:5px; }}
    </style>{}"#, group_rule_tables(true));
    let reference = format!(r#"<style>
        table {{ border-collapse:collapse }}
        #a thead, #a tbody {{ border-block-end-width:1px; border-block-end-style:solid; }}
        #b thead, #b tbody {{ border-block-end:1px solid blue; }}
        #c thead, #c tbody {{ border-block-end-width:5px; border-block-end-style:solid; }}
    </style>{}"#, group_rule_tables(false));
    assert_eq!(table_page(&input, common).rgba,
               table_page(&reference, common).rgba,
               "group rules must not paint extra or misplaced edges");
}

fn group_rule_tables(use_rules: bool) -> String {
    let mut tables = String::new();
    for id in ["a", "b", "c"] {
        let rules = if use_rules { " rules=groups" } else { "" };
        tables.push_str(&format!(r#"<p>Groups</p>
            <table id="{id}"{rules}>
                <thead><tr><td>head</td></tr></thead>
                <tbody><tr><td>body</td><td>one</td></tr>
                       <tr><td>body</td><td>two</td></tr></tbody>
                <tfoot><tr><td>foot</td></tr></tfoot>
            </table>"#));
    }
    tables
}
