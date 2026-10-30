// Copyright 2026 g1BSsQ. Licensed under Apache-2.0.
use isr_core::{render, RenderOptions, Rendered};

fn geometry(fixture: &str) -> Rendered {
    render(fixture, RenderOptions {
        width: 412, system_fonts: true, ..RenderOptions::default()
    }).unwrap()
}

// Layout JSON comparisons are reported to one decimal CSS pixel. Assert at
// that precision rather than replacing them with whole-pixel tolerances.
fn tenth(actual: f64, expected: f64, label: &str) {
    assert_eq!((actual * 10.0).round() as i64,
               (expected * 10.0).round() as i64,
               "{label}: expected {expected:.1} CSS px, got {actual:.5}");
}

#[test]
fn inline_badges_keep_fractional_shrink_to_fit_width_and_row_advances() {
    let result = geometry(include_str!("../../../fixtures/04-badges.html"));
    let badges: Vec<_> = result.elements.iter().filter(|e| e.tag == "span").collect();
    assert!(badges.len() >= 4);
    for (index, (x, width)) in [
        (20.0, 49.4), (81.4, 51.4),
        (144.7, 73.8), (230.5, 79.4),
    ].into_iter().enumerate() {
        tenth(badges[index].x, x, &format!("badge {index} x"));
        tenth(badges[index].width, width, &format!("badge {index} width"));
    }
}

#[test]
fn buttons_retain_previous_fractional_widths() {
    let result = geometry(include_str!("../../../fixtures/probes/form-buttons.html"));
    for (id, width) in [("plain-button", 47.1), ("nested-button", 134.5)] {
        let element = result.elements.iter().find(|e| e.id == id).unwrap();
        tenth(element.width, width, id);
    }
}

#[test]
fn collapsed_header_allocates_half_border_and_columns_keep_fractional_tracks() {
    let result = geometry(include_str!("../../../fixtures/07-table.html"));
    let head = result.elements.iter().find(|e| e.tag == "thead").unwrap();
    let body = result.elements.iter().find(|e| e.tag == "tbody").unwrap();
    let headers: Vec<_> = result.elements.iter().filter(|e| e.tag == "th").collect();
    let rows: Vec<_> = result.elements.iter().filter(|e| e.tag == "tr").collect();
    assert_eq!(headers.len(), 3);
    assert_eq!(rows.len(), 4);
    tenth(head.height, 45.5, "header height");
    tenth(body.y, 108.7, "body y");
    for (index, (x, width)) in [
        (20.0, 150.0), (170.0, 113.7), (283.7, 108.3),
    ].into_iter().enumerate() {
        tenth(headers[index].x, x, &format!("column {index} x"));
        tenth(headers[index].width, width, &format!("column {index} width"));
    }
    for (index, row) in rows.iter().skip(1).enumerate() {
        tenth(row.height, 46.0, &format!("body row {index} height"));
    }
}

#[test]
fn auto_menulists_snap_their_intrinsic_border_boxes() {
    let result = geometry(include_str!("../../../fixtures/probes/form-select.html"));
    for (id, expected) in [("single", 110.0), ("grouped", 73.0)] {
        let control = result.elements.iter().find(|e| e.id == id).unwrap();
        // Exact whole-pixel used widths. Do not change explicit or fractional
        // inline sizing to obtain these control-specific dimensions.
        assert_eq!(control.width, expected, "{id}: {control:?}");
        tenth(control.height, 19.0, &format!("{id} height"));
    }
}

#[test]
fn listbox_options_include_the_default_four_pixel_inline_end_padding() {
    let result = geometry(include_str!("../../../fixtures/probes/form-select.html"));
    let listbox = result.elements.iter()
        .find(|e| e.id == "multiple-rows").unwrap();
    let options: Vec<_> = result.elements.iter()
        .filter(|e| e.tag == "option" && e.width > 0.0).collect();
    assert_eq!(options.len(), 3, "{options:?}");
    // Widths are read to 0.1 CSS px to allow the independently measured
    // 0.02px text advance difference while detecting a missing whole pixel.
    tenth(listbox.width, 45.27, "listbox border-box width");
    for (index, option) in options.iter().enumerate() {
        tenth(option.width, 43.27, &format!("option {index} width"));
        tenth(option.height, 17.0, &format!("option {index} height"));
    }
}
