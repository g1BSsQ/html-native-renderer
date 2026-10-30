// Copyright 2026 g1BSsQ. Licensed under Apache-2.0.
use isr_core::{render, RenderOptions};

fn draw(html: &str) -> isr_core::Rendered {
    render(html, RenderOptions {
        system_fonts: true, width: 320, ..RenderOptions::default()
    }).expect("test document should render")
}

fn box_height(r: &isr_core::Rendered, id: &str) -> f64 {
    r.elements.iter().find(|el| el.id == id).unwrap().height
}

fn dark_pixels(r: &isr_core::Rendered, x0: u32, x1: u32, y0: u32, y1: u32) -> usize {
    let mut pixels = 0;
    for y in y0..y1 {
        for x in x0..x1 {
            let i = ((y * r.width + x) * 4) as usize;
            if r.rgba[i] < 150 && r.rgba[i + 1] < 150 && r.rgba[i + 2] < 150 {
                pixels += 1;
            }
        }
    }
    pixels
}

fn red_pixels(r: &isr_core::Rendered) -> usize {
    r.rgba.chunks_exact(4).filter(|p| p[0] > 180 && p[1] < 90 && p[2] < 90).count()
}

#[test]
fn standard_clamp_shortens_five_lines_and_paints_marker_not_third_line() {
    let html = r#"<style>
        body{margin:0;background:white;font:20px/24px monospace}
        #clamp{line-clamp:2}
        .later{color:red}
        </style>
        <div id="clamp">A<br>B<br><span class="later">C<br>D<br>E</span></div>
        <div id="after">Following</div>"#;
    let r = draw(html);
    assert!((box_height(&r, "clamp") - 48.0).abs() < 1.0,
        "five source lines must consume two 24px lines");
    let after = r.elements.iter().find(|e| e.id == "after").unwrap();
    assert!((after.y - 48.0).abs() < 2.0, "following flow must move up: {after:?}");
    assert!(dark_pixels(&r, 0, 13, 0, 48) > 12, "the first two lines must not be blank");
    assert!(dark_pixels(&r, 13, 38, 24, 48) > 2,
        "ellipsis must be painted after the second line");
    assert_eq!(red_pixels(&r), 0, "third-line and later glyphs cannot paint");

    // The negative control belongs to the same regression: it should retain
    // all five lines and should not acquire a synthetic marker.
    let none = draw(r#"<style>body{margin:0;background:white;font:20px/24px monospace}
        #clamp{line-clamp:none}</style>
        <div id="clamp">A<br>B<br>C<br>D<br>E</div>"#);
    assert!((box_height(&none, "clamp") - 120.0).abs() < 1.0);
    assert!(dark_pixels(&none, 0, 13, 96, 120) > 2, "the fifth line must paint");
    assert_eq!(dark_pixels(&none, 13, 38, 24, 48), 0,
        "there must be no synthetic marker on line two");
}

#[test]
fn legacy_recipe_clamps_only_with_vertical_box() {
    let style = "body{margin:0;background:white;font:20px/24px monospace}";
    let first = format!(r#"<style>{style}</style><div id="clamp" style="display:-webkit-box;-webkit-box-orient:vertical;-webkit-line-clamp:2;overflow:hidden">A<br>B<br>C<br>D<br>E</div>"#);
    let r = draw(&first);
    assert!((box_height(&r, "clamp") - 48.0).abs() < 1.0);
    assert!(dark_pixels(&r, 13, 38, 24, 48) > 2, "legacy marker must paint");

    let no_orient = format!(r#"<style>{style}</style><div id="clamp" style="display:-webkit-box;-webkit-line-clamp:2">A<br>B<br>C<br>D<br>E</div>"#);
    let unqualified = draw(&no_orient);
    assert!(box_height(&unqualified, "clamp") > 100.0);
}

#[test]
fn nested_blocks_count_toward_one_shared_limit() {
    let html = r#"<style>body{margin:0;background:white;font:20px/24px monospace}
        #clamp{line-clamp:2}
        .later{color:red}
        </style>
        <div id="clamp"><div>A</div><div>B<br><span class="later">C<br>D<br>E</span></div></div>
        <div id="after">Following</div>"#;
    let r = draw(html);
    assert!((box_height(&r, "clamp") - 48.0).abs() < 2.0,
        "a nested block's first line must be the second counted line");
    assert!(dark_pixels(&r, 0, 13, 0, 48) > 12, "visible lines must paint");
    assert!(dark_pixels(&r, 13, 38, 24, 48) > 2, "nested marker must paint");
    assert_eq!(red_pixels(&r), 0);
}
