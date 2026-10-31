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
