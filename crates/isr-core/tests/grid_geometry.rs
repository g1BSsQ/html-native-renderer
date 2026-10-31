// Geometry checks for grid content alignment, measured in CSS pixels.
use isr_core::{render, RenderOptions, Rendered};

fn sample(container_rules: &str) -> Rendered {
    let html = format!(
        r#"<!doctype html>
        <style>
          html, body {{ margin: 0; padding: 0; }}
          #grid {{
            display: grid;
            width: 220px;
            grid-template-columns: 30px 70px;
            grid-template-rows: 20px;
            flex-direction: column;
            {container_rules}
          }}
        </style>
        <div id="grid"><div id="first"></div><div id="second"></div></div>"#
    );
    render(&html, RenderOptions {
        width: 300,
        ..RenderOptions::default()
    }).unwrap()
}

fn assert_box(result: &Rendered, id: &str, x: f64, width: f64) {
    let element = result.elements.iter().find(|element| element.id == id).unwrap();
    assert!((element.x - x).abs() < 0.1, "{id} x: got {}, expected {x}", element.x);
    assert!((element.width - width).abs() < 0.1,
        "{id} width: got {}, expected {width}", element.width);
    assert!((element.y).abs() < 0.1, "{id} y: got {}", element.y);
    assert!((element.height - 20.0).abs() < 0.1,
        "{id} height: got {}", element.height);
}

#[test]
fn grid_justify_content_right_ignores_flex_direction() {
    // CSS Grid 1 §10.5: physical right aligns the track collection on the right.
    let result = sample("justify-content: right; direction: ltr;");
    assert_box(&result, "first", 120.0, 30.0);
    assert_box(&result, "second", 150.0, 70.0);
}

#[test]
fn grid_justify_content_left_in_rtl_ignores_flex_direction() {
    // CSS Grid 1 §10.5: physical left is independent of the inline start side.
    let result = sample("justify-content: left; direction: rtl;");
    assert_box(&result, "first", 70.0, 30.0);
    assert_box(&result, "second", 0.0, 70.0);
}

#[test]
fn regression_guard_grid_tracks_and_gap_unchanged() {
    // Guard: repeat/minmax/fr distribution and gaps retain their existing geometry.
    let result = sample(
        "grid-template-columns: repeat(2, minmax(0, 1fr)); column-gap: 20px; justify-content: start;"
    );
    assert_box(&result, "first", 0.0, 100.0);
    assert_box(&result, "second", 120.0, 100.0);
}

fn box_by_id<'a>(result: &'a Rendered, id: &str) -> &'a isr_core::Element {
    result.elements.iter().find(|element| element.id == id).unwrap()
}

fn containing_block_case(property: &str, position: &str) -> Rendered {
    let html = format!(
        r#"<!doctype html>
        <style>html, body {{ margin:0; padding:0; }}</style>
        <div style="position:relative; width:300px; height:180px; margin:40px">
          <div id="grid" style="display:grid; width:220px; height:80px;
            margin-left:35px; {property}">
            <div id="badge" style="position:{position}; right:0; top:0;
              width:12px; height:10px"></div>
          </div>
        </div>"#
    );
    render(&html, RenderOptions {
        width: 400,
        ..RenderOptions::default()
    }).unwrap()
}

fn assert_badge_at_grid_corner(result: &Rendered) {
    let grid = box_by_id(result, "grid");
    let badge = box_by_id(result, "badge");
    assert!((grid.x - 75.0).abs() < 0.1, "grid x: {}", grid.x);
    assert!((grid.y - 40.0).abs() < 0.1, "grid y: {}", grid.y);
    assert!((badge.x - (grid.x + grid.width - 12.0)).abs() < 0.1,
        "badge x: {}, grid: {:?}", badge.x, (grid.x, grid.width));
    assert!((badge.y - grid.y).abs() < 0.1,
        "badge y: {}, grid y: {}", badge.y, grid.y);
    assert!((badge.width - 12.0).abs() < 0.1);
    assert!((badge.height - 10.0).abs() < 0.1);
}

#[test]
fn regression_guard_grid_transform_claims_absolute_containing_block() {
    assert_badge_at_grid_corner(&containing_block_case(
        "transform:translateZ(0);", "absolute"));
}

#[test]
fn regression_guard_grid_will_change_claims_fixed_containing_block() {
    assert_badge_at_grid_corner(&containing_block_case(
        "will-change:transform;", "fixed"));
}

#[test]
fn regression_guard_grid_filter_claims_fixed_containing_block() {
    assert_badge_at_grid_corner(&containing_block_case(
        "filter:blur(0px);", "fixed"));
}

#[test]
fn regression_guard_grid_justify_content_left_ltr() {
    let result = sample("justify-content:left; direction:ltr;");
    assert_box(&result, "first", 0.0, 30.0);
    assert_box(&result, "second", 30.0, 70.0);
}

#[test]
fn regression_guard_grid_justify_content_right_rtl() {
    let result = sample("justify-content:right; direction:rtl;");
    assert_box(&result, "first", 190.0, 30.0);
    assert_box(&result, "second", 120.0, 70.0);
}

#[test]
fn regression_guard_grid_justify_content_right_with_flex_row() {
    // Flex direction does not influence grid alignment in either direction.
    let result = sample("justify-content:right; direction:ltr; flex-direction:row;");
    assert_box(&result, "first", 120.0, 30.0);
    assert_box(&result, "second", 150.0, 70.0);
}

#[test]
fn regression_guard_grid_space_between_still_distributes_tracks() {
    let result = sample("justify-content:space-between; direction:ltr;");
    assert_box(&result, "first", 0.0, 30.0);
    assert_box(&result, "second", 150.0, 70.0);
}
