// Copyright 2026 g1BSsQ. Licensed under Apache-2.0.
use isr_core::{render, RenderOptions};

fn draw(style: &str, content: &str) -> isr_core::Rendered {
    let html = format!(
        "<style>body{{margin:0;background:white;font:20px/24px Roboto}} {style}</style>{content}"
    );
    render(&html, RenderOptions {
        width: 240,
        system_fonts: false,
        fonts: vec![include_bytes!(
            "../../../android/demo/app/src/main/assets/Roboto-Regular.ttf"
        ).to_vec()],
    }).expect("the test page should render")
}

fn pixel(page: &isr_core::Rendered, x: u32, y: u32) -> [u8; 4] {
    let start = ((y * page.width + x) * 4) as usize;
    page.rgba[start..start + 4].try_into().unwrap()
}

fn changed_pixels(a: &isr_core::Rendered, b: &isr_core::Rendered,
                  x0: u32, x1: u32, y0: u32, y1: u32) -> usize {
    (y0..y1).flat_map(|y| (x0..x1).map(move |x| (x, y)))
        .filter(|&(x, y)| pixel(a, x, y) != pixel(b, x, y)).count()
}

#[test]
fn single_line_text_overflow_draws_end_marker() {
    let text = r#"<div id="card">MMMMMMMMMMMMMMMMMMMM</div>"#;
    let common = "#card{width:90px;height:24px;overflow:hidden;white-space:nowrap;color:black;";
    let ellipsis = draw(&format!("{common}text-overflow:ellipsis}}"), text);
    let clip = draw(&format!("{common}text-overflow:clip}}"), text);
    assert_eq!(pixel(&ellipsis, 200, 8), [255, 255, 255, 255]);
    assert_eq!(pixel(&clip, 200, 8), [255, 255, 255, 255]);
    assert!(changed_pixels(&ellipsis, &clip, 55, 90, 0, 24) > 4,
        "an overflowing line must replace its painted end with a marker");
}

#[test]
fn standard_no_ellipsis_suppresses_only_the_marker() {
    let text = r#"<div id="card">A<br>B<br>C<br>D</div>"#;
    let no_marker = draw("#card{line-clamp:2 no-ellipsis;width:90px}", text);
    let marker = draw("#card{line-clamp:2;width:90px}", text);
    assert_eq!(pixel(&no_marker, 200, 8), [255, 255, 255, 255]);
    assert_eq!(no_marker.elements.iter().find(|e| e.id == "card").unwrap().height,
               marker.elements.iter().find(|e| e.id == "card").unwrap().height);
    assert!(changed_pixels(&no_marker, &marker, 0, 90, 24, 48) > 2,
        "no-ellipsis must preserve the clamp height without drawing a marker");
}

#[test]
fn standard_string_marker_uses_supplied_glyphs() {
    let text = r#"<div id="card">A<br>B<br>C<br>D</div>"#;
    let custom = draw(r#"#card{line-clamp:2 "+++ ";width:120px}"#, text);
    let default = draw("#card{line-clamp:2;width:120px}", text);
    assert_eq!(pixel(&custom, 200, 8), [255, 255, 255, 255]);
    assert!(changed_pixels(&custom, &default, 0, 120, 24, 48) > 2,
        "the shorthand string must be painted instead of the default marker");
}

#[test]
fn guard_standard_clamp_with_whitespace_keeps_default_marker() {
    // Guard: line-clamp-001. Both CSS spellings must paint identical pixels.
    let text = "<div id=card>Line 1\\nLine 2\\nLine 3\\nLine 4\\nLine 5</div>";
    let common = "#card{width:160px;white-space:pre;line-height:24px;";
    let compact = draw(&format!("{common}line-clamp:4}}"), text);
    let spaced = draw(&format!("{common}line-clamp: 4}}"), text);
    assert_eq!(compact.rgba, spaced.rgba,
        "CSS whitespace must not change the default clamp marker");
    assert_eq!(compact.elements.iter().find(|e| e.id == "card").unwrap().height,
        spaced.elements.iter().find(|e| e.id == "card").unwrap().height);
}

#[test]
fn guard_standard_clamp_with_abspos_keeps_default_marker() {
    // Guard: line-clamp-with-abspos-001. Positioning must not alter
    // shorthand tokenization or which line receives the default marker.
    let text = "<div id=card><div style='position:absolute;top:0;left:0;width:10px;height:10px;background:blue'></div>Line 1\\nLine 2\\nLine 3\\nLine 4\\nLine 5</div>";
    let common = "#card{width:160px;white-space:pre;line-height:24px;";
    let compact = draw(&format!("{common}line-clamp:4}}"), text);
    let spaced = draw(&format!("{common}line-clamp: 4}}"), text);
    assert_eq!(compact.rgba, spaced.rgba,
        "an out-of-flow child must not change the marker on the fourth line");
}

#[test]
fn guard_anonymous_line_uses_containing_block_overflow_rule() {
    // Guard: text-overflow-in-anonymous-block. A trailing zero-height
    // block must not change the pixels of the overflowing text line.
    let common = "#card{overflow:hidden;text-overflow:ellipsis;width:90px;white-space:pre;line-height:24px}";
    let direct = draw(common, "<div id=card>MMMMMMMMMMMMMMMMMMM</div>");
    let anonymous = draw(common,
        "<div id=card>MMMMMMMMMMMMMMMMMMM<div style='height:0'></div></div>");
    assert_eq!(direct.rgba, anonymous.rgba,
        "an anonymous wrapper uses the containing block's text-overflow");
}
