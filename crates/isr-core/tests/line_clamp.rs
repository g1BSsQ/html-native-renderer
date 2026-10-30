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

#[test]
fn later_legacy_shorthand_wins_without_a_legacy_box() {
    // Both shorthands write the same longhands. The last declaration
    // takes precedence even if its legacy box condition is absent.
    let base = "body{margin:0} #clamp{font:16px/32px serif;white-space:pre}";
    let lines = "Line 1\nLine 2\nLine 3\nLine 4\nLine 5";
    let legacy_last = format!(
        "<style>{base} #clamp{{line-clamp:4;-webkit-line-clamp:4}}</style><div id=\"clamp\">{lines}</div>"
    );
    let r = draw(&legacy_last);
    assert!((box_height(&r, "clamp") - 160.0).abs() < 1.0,
        "later legacy shorthand must not clamp a non-legacy box");

    let standard_last = format!(
        "<style>{base} #clamp{{-webkit-line-clamp:4;line-clamp:4}}</style><div id=\"clamp\">{lines}</div>"
    );
    let standard = draw(&standard_last);
    assert!((box_height(&standard, "clamp") - 128.0).abs() < 1.0,
        "later standard shorthand must clamp the same five lines");
}

#[test]
fn multicol_container_does_not_collapse_its_lines() {
    // A column container keeps its full block extent rather than using
    // the normal clamp continuation mode.
    let lines = "<p>Line 1</p><p>Line 2</p><p>Line 3</p><p>Line 4</p><p>Line 5</p><p>Line 6</p><p>Line 7</p><p>Line 8</p><p>Line 9</p>";
    let style = "body{margin:0;font:16px/18px sans-serif} p{margin:0}";
    let columns = format!(
        "<style>{style} #clamp{{line-clamp:2;columns:3;background:yellow}}</style><div id=\"clamp\">{lines}</div>"
    );
    let r = draw(&columns);
    assert!((box_height(&r, "clamp") - 162.0).abs() < 1.0,
        "nine lines inside a column container must remain visible");

    let auto = format!(
        "<style>{style} #clamp{{line-clamp:2;columns:auto}}</style><div id=\"clamp\">{lines}</div>"
    );
    let control = draw(&auto);
    assert!((box_height(&control, "clamp") - 36.0).abs() < 1.0,
        "auto columns must not disable an ordinary clamp");
}

#[test]
fn scrollable_descendant_starts_an_independent_line_budget() {
    // The five lines belong to a child with its own formatting context.
    // The outer legacy box must not count or truncate those child lines.
    let base = "body{margin:0} .clamp{display:-webkit-box;-webkit-box-orient:vertical;-webkit-line-clamp:3;font:16px/32px monospace;white-space:pre;overflow:hidden}";
    let lines = "Line 1\nLine 2\nLine 3\nLine 4\nLine 5";
    let scrollable = format!(
        "<style>{base} .child{{overflow:hidden}}</style><div class=\"clamp\" id=\"clamp\"><div class=\"child\">{lines}</div></div>"
    );
    let r = draw(&scrollable);
    assert!((box_height(&r, "clamp") - 160.0).abs() < 1.0,
        "the scrollable child must keep all five lines");

    let normal = format!(
        "<style>{base} .child{{overflow:visible}}</style><div class=\"clamp\" id=\"clamp\"><div class=\"child\">{lines}</div></div>"
    );
    let control = draw(&normal);
    assert!((box_height(&control, "clamp") - 96.0).abs() < 1.0,
        "a normal block child must still contribute to the outer line budget");
}

#[test]
fn block_clamp_uses_current_padding_and_border() {
    let html = r#"<style>body{margin:0;font:20px/24px monospace}</style><div id="clamp" style="line-clamp:2;padding:10px;border:2px solid"><div>A</div><div>B</div><div>C</div><div>D</div><div>E</div></div>"#;
    let r = draw(html);
    assert!((box_height(&r, "clamp") - 72.0).abs() < 2.0,
        "two 24px lines and 24px of current root padding/border");
}

#[test]
fn block_clamp_respects_definite_height_and_min_height() {
    let blocks = "<div>A</div><div>B</div><div>C</div><div>D</div><div>E</div>";
    let style = "body{margin:0;font:20px/24px monospace}";
    let definite = format!(
        r#"<style>{style}</style><div id="clamp" style="line-clamp:2;height:200px">{blocks}</div>"#
    );
    let r = draw(&definite);
    assert!((box_height(&r, "clamp") - 200.0).abs() < 1.0,
        "clamp cannot shrink an explicit block height");

    let minimum = format!(
        r#"<style>{style}</style><div id="clamp" style="line-clamp:2;min-height:140px">{blocks}</div>"#
    );
    let r = draw(&minimum);
    assert!((box_height(&r, "clamp") - 140.0).abs() < 1.0,
        "the used min-height is a floor for the clamp");
}

#[test]
fn text_and_selectors_are_not_rewritten_by_clamp_normalization() {
    let html = r#"<style>
        body{margin:0;font:20px/24px monospace}
        .columns:last-child{margin:0}
        .line-clamp:hover{color:red}
        td::before{content:"Columns: "}
        </style>
        <div id="visible">Columns: x</div>
        <code id="example">-webkit-line-clamp: 2</code>"#;
    let r = draw(html);
    assert!((box_height(&r, "visible") - 24.0).abs() < 2.0);
    assert!(dark_pixels(&r, 0, 60, 0, 24) > 8,
        "ordinary visible text must remain intact");
}
