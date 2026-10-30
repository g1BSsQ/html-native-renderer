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

fn is_green(image: &Rendered, x: usize, y: usize) -> bool {
    let offset = (y * image.width as usize + x) * 4;
    let c = &image.rgba[offset..offset + 4];
    c[1] > c[0].saturating_add(40)
        && c[1] > c[2].saturating_add(40) && c[3] > 0
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

#[test]
fn internal_table_solid_borders_paint_on_all_four_collapsed_edges() {
    for &part in PARTS {
        let reference = image(part, "", true);
        let decorated = image(part, "border:4px solid green", true);
        // Paint is checked independently: collapsed borders can change table sizing.
        let mut pixels = Vec::new();
        for y in 0..decorated.height as usize {
            for x in 0..decorated.width as usize {
                if is_green(&decorated, x, y) { pixels.push((x, y)); }
            }
        }
        assert!(pixels.len() >= 100, "missing collapsed border for {part}");
        let x0 = pixels.iter().map(|p| p.0).min().unwrap();
        let x1 = pixels.iter().map(|p| p.0).max().unwrap();
        let y0 = pixels.iter().map(|p| p.1).min().unwrap();
        let y1 = pixels.iter().map(|p| p.1).max().unwrap();
        let mid_x = (x0 + x1) / 2;
        let mid_y = (y0 + y1) / 2;
        for (x, y) in [(mid_x, y0), (mid_x, y1), (x0, mid_y), (x1, mid_y)] {
            assert!(is_green(&decorated, x, y), "missing edge at ({x},{y}) for {part}");
        }
        assert!(!reference.rgba.chunks_exact(4).any(|p| p[1] > p[0].saturating_add(40)
                        && p[1] > p[2].saturating_add(40)),
                "reference unexpectedly green for {part}");
    }
}
