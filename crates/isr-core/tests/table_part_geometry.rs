// Copyright 2026 g1BSsQ. Licensed under Apache-2.0.
use isr_core::{render, RenderOptions, Rendered};

fn page(markup: &str) -> Rendered {
    let html = format!(r#"<style>
        html,body {{ margin:0; background:white; }}
        .cell {{ display:table-cell; width:48px; height:32px; background:#ff8000 }}
    </style>{markup}"#);
    render(&html, RenderOptions {
        width:300, system_fonts:false, ..RenderOptions::default()
    }).unwrap()
}

fn element<'a>(r: &'a Rendered, id: &str) -> &'a isr_core::Element {
    r.elements.iter().find(|e| e.id == id)
        .unwrap_or_else(|| panic!("missing element {id}"))
}

fn green_pixels(r: &Rendered) -> usize {
    r.rgba.chunks_exact(4).filter(|p| {
        p[1] > p[0].saturating_add(30)
            && p[1] > p[2].saturating_add(30)
            && p[3] > 0
    }).count()
}

fn orange_pixels(r: &Rendered) -> usize {
    r.rgba.chunks_exact(4).filter(|p| {
        p[0] > 175 && p[1] > 50 && p[1] < 180 && p[2] < 100 && p[3] > 0
    }).count()
}

fn part_table(display: &str, extra: &str) -> Rendered {
    let cell = r#"<div id="a" class="cell"></div><div id="b" class="cell"></div>"#;
    let parts = match display {
        "table-row-group" | "table-header-group" | "table-footer-group" => {
            format!(r#"<div id="part" style="display:{display};{extra}">
                 <div id="row" style="display:table-row">{cell}</div></div>"#)
        }
        "table-row" =>
            format!(r#"<div id="part" style="display:table-row;{extra}">{cell}</div>"#),
        "table-column-group" => format!(
            r#"<div id="part" style="display:table-column-group;{extra}">
              <div style="display:table-column"></div><div style="display:table-column"></div>
              </div><div id="row" style="display:table-row">{cell}</div>"#
        ),
        "table-column" => format!(
            r#"<div id="part" style="display:table-column;{extra}"></div>
              <div style="display:table-column"></div>
              <div id="row" style="display:table-row">{cell}</div>"#
        ),
        _ => panic!("unknown table part"),
    };
    page(&format!(r#"<div id="table" style="display:table;
                border-collapse:collapse;table-layout:fixed;width:96px">{parts}</div>"#))
}

#[test]
fn internal_parts_have_nonempty_track_geometry() {
    // The baseline exports every table-part div as (0,0,0,0).
    for display in [
        "table-row-group", "table-header-group", "table-footer-group",
        "table-row", "table-column-group", "table-column"
    ] {
        let r = part_table(display, "");
        assert_eq!(r.elements.iter().filter(|e| e.id == "a" || e.id == "b").count(), 2,
            "both cells must be present: {display}");
        let p = element(&r, "part");
        assert!(p.width >= 40.0 && p.height >= 30.0,
            "missing track geometry for {display}: {p:?}");
        assert!(orange_pixels(&r) >= 1000, "blank table: {display}");
    }
}

#[test]
fn internal_margins_and_padding_do_not_expand_tracks() {
    for display in [
        "table-row-group", "table-header-group", "table-footer-group",
        "table-row", "table-column-group", "table-column"
    ] {
        let baseline = part_table(display, "");
        let decorated = part_table(display, "margin:40px;padding:50px");
        assert!(element(&baseline, "part").width > 0.0
            && element(&baseline, "part").height > 0.0,
            "non-vacuous internal part geometry: {display}");
        for id in ["part", "a", "b"] {
            let lhs = element(&baseline, id);
            let rhs = element(&decorated, id);
            assert_eq!((lhs.x, lhs.y, lhs.width, lhs.height),
                       (rhs.x, rhs.y, rhs.width, rhs.height),
                       "{display} {id}");
        }
        assert!(orange_pixels(&baseline) > 1000);
        assert_eq!(baseline.rgba, decorated.rgba, "{display}");
    }
}

#[test]
fn uncontested_collapsed_solid_border_paints_the_four_shared_edges() {
    // CSS 2.1 §17.6.2: only one source can own each grid-line segment.
    for display in [
        "table-row-group", "table-header-group", "table-footer-group", "table-row",
        "table-column-group", "table-column"
    ] {
        let r = part_table(display, "border:4px solid green");
        assert_eq!(r.elements.iter().filter(|e| e.id == "a" || e.id == "b").count(), 2);
        assert!(orange_pixels(&r) >= 1000, "empty cells in {display}");
        assert!(green_pixels(&r) >= 80, "missing collapsed border for {display}");
    }
}

#[test]
fn empty_cells_do_not_erase_the_collapsed_border_strip() {
    let r = page(r#"<div id="table" style="display:table;table-layout:fixed;
        border-collapse:collapse;width:96px">
        <div id="group" style="display:table-row-group;border-bottom:3px solid green">
            <div id="row" style="display:table-row">
                <div id="a" style="display:table-cell"></div>
                <div id="b" style="display:table-cell"></div>
            </div>
        </div></div>"#);
    assert_eq!(r.elements.iter().filter(|e| e.id == "a" || e.id == "b").count(), 2);
    assert!(green_pixels(&r) >= 200, "expected visible 96px by 3px strip");
    assert!(element(&r, "table").height >= 2.5);
}

#[test]
fn caption_has_layout_geometry_and_paints_its_border() {
    // CSS 2.1 §17.4: the caption is a block outside the table grid.
    let r = page(r#"<div id="table" style="display:table">
       <div id="caption" style="display:table-caption;width:96px;height:96px;
           border-bottom:4px solid green;background:#ff8000"></div>
       <div style="display:table-row"><div id="cell" style="display:table-cell"></div></div>
       </div>"#);
    assert_eq!(r.elements.iter().filter(|e| e.id == "caption").count(), 1);
    assert_eq!(r.elements.iter().filter(|e| e.id == "cell").count(), 1);
    let c = element(&r, "caption");
    assert!(c.width >= 96.0 && c.height >= 100.0, "{c:?}");
    assert!(element(&r, "table").height >= c.height);
    assert!(orange_pixels(&r) > 1000, "blank caption");
    assert!(green_pixels(&r) >= 250, "missing caption border");
}

#[test]
fn caption_margins_and_padding_participate_in_wrapper_layout() {
    let base = page(r#"<div style="display:table;width:160px">
       <div id="caption" style="display:table-caption;width:100px;height:40px;
            background:#ff8000"></div>
       <div style="display:table-row"><div class="cell" id="cell"></div></div>
       </div>"#);
    let decorated = page(r#"<div style="display:table;width:160px">
       <div id="caption" style="display:table-caption;width:100px;height:40px;
            margin:10px;padding:12px;border:2px solid green;background:#ff8000"></div>
       <div style="display:table-row"><div class="cell" id="cell"></div></div>
       </div>"#);
    assert_eq!(base.elements.iter().filter(|e| e.id == "cell").count(), 1);
    assert_eq!(decorated.elements.iter().filter(|e| e.id == "cell").count(), 1);
    assert!(orange_pixels(&base) > 300);
    assert!(orange_pixels(&decorated) > 300);
    let plain = element(&base, "caption");
    let styled = element(&decorated, "caption");
    assert!(styled.width >= plain.width + 25.0, "caption padding/border ignored");
    assert!(styled.height >= plain.height + 25.0, "caption padding/border ignored");
    assert!(element(&decorated, "cell").y > element(&base, "cell").y,
       "caption margin/padding must move the table grid");
}

#[test]
fn bottom_caption_is_placed_after_the_grid() {
    let r = page(r#"<div id="table" style="display:table;width:100px">
        <div id="caption" style="display:table-caption;caption-side:bottom;
             width:100px;height:40px;background:#ff8000"></div>
        <div id="row" style="display:table-row">
             <div id="cell" style="display:table-cell;width:100px;height:32px;
                  background:green"></div>
        </div></div>"#);
    assert_eq!(r.elements.iter().filter(|e| e.id == "row").count(), 1);
    assert_eq!(r.elements.iter().filter(|e| e.id == "cell").count(), 1);
    let cell = element(&r, "cell");
    let caption = element(&r, "caption");
    assert!(cell.height > 20.0 && caption.height > 30.0,
        "both boxes must have real geometry");
    assert!(caption.y >= cell.y + cell.height - 1.0,
        "bottom caption overlaps the table grid: cell={cell:?}, caption={caption:?}");
    assert!(green_pixels(&r) > 500 && orange_pixels(&r) > 500,
        "the grid and caption must both be painted");
}
