use isr_core::{render, RenderOptions, Rendered};

fn draw(html: &str) -> Rendered {
    render(
        html,
        RenderOptions {
            width: 320,
            fonts: vec![include_bytes!(
                "../../../android/demo/app/src/main/assets/Roboto-Regular.ttf"
            )
            .to_vec()],
            ..RenderOptions::default()
        },
    )
    .expect("test should render")
}

fn geometry(html: &str, id: &str) -> (f64, f64) {
    let image = draw(html);
    let el = image.elements.iter().find(|el| el.id == id).expect("element");
    (el.y, el.height)
}

fn line_height(style: &str, content: &str) -> f64 {
    let html = format!(
        r#"<style>body{{margin:0;font:20px/24px monospace}}#sample{{width:30px}}</style>
        <div id="sample"><span style="display:contents;{style}">{content}</span></div>"#
    );
    geometry(&html, "sample").1
}

fn line_count(height: f64) -> usize {
    let count = (height / 24.0).round() as usize;
    assert!(
        (height - count as f64 * 24.0).abs() < 1.0,
        "expected whole 24px line boxes, got {height}"
    );
    count
}

#[test]
fn boxless_wrapper_forwards_word_break_to_line_boxes() {
    assert_eq!(line_count(line_height("", "abcdef")), 1, "control must not wrap");
    let lines = line_count(line_height("word-break:break-all", "abcdef"));
    assert!(lines > 1, "break-all must wrap the otherwise unbreakable word: {lines}");
}

#[test]
fn boxless_wrapper_forwards_overflow_wrap_to_line_boxes() {
    assert_eq!(line_count(line_height("", "abcdef")), 1, "control must not wrap");
    let lines = line_count(line_height("overflow-wrap:anywhere", "abcdef"));
    assert!(lines > 1, "anywhere must wrap an otherwise unbreakable label: {lines}");
}

#[test]
fn boxless_wrapper_forwards_line_break_anywhere() {
    assert_eq!(line_count(line_height("", "abcdef")), 1, "control must not wrap");
    let lines = line_count(line_height("line-break:anywhere", "abcdef"));
    assert!(lines > 1, "line-break:anywhere must form multiple lines: {lines}");
}

#[test]
fn boxless_wrapper_forwards_letter_spacing_into_break_measurement() {
    let html = r#"<style>body{margin:0;font:20px/24px monospace}#sample{width:40px}</style>
        <div id="sample"><span style="display:contents;letter-spacing:20px">A A</span></div>"#;
    let (_, height) = geometry(html, "sample");
    assert_eq!(line_count(height), 2, "extra glyph advances must wrap at the space");
}

#[test]
fn nested_boxless_wrappers_use_innermost_break_value() {
    let html = r#"<style>body{margin:0;font:20px/24px monospace}
        #one,#three{width:30px}</style>
        <div id="one"><span style="display:contents;overflow-wrap:anywhere"><span style="display:contents;overflow-wrap:normal">abcdef</span></span></div>
        <div id="three"><span style="display:contents;overflow-wrap:normal"><span style="display:contents;overflow-wrap:anywhere">abcdef</span></span></div>"#;
    assert_eq!(line_count(geometry(html, "one").1), 1, "inner normal disables anywhere");
    let wrapped = line_count(geometry(html, "three").1);
    assert!(wrapped > 1, "inner anywhere must enable wrapping: {wrapped}");
}

fn count_color(image: &Rendered, predicate: impl Fn(&[u8]) -> bool) -> usize {
    image.rgba.chunks_exact(4).filter(|pixel| predicate(pixel)).count()
}

#[test]
fn regression_guard_inline_background_uses_nearest_box_ancestor_of_boxless_wrapper() {
    for (case, content) in [
        ("single", r#"<span style="display:contents">text</span>"#),
        ("nested", r#"<span style="display:contents"><span style="display:contents">text</span></span>"#),
    ] {
        let html = format!(
            r#"<style>body{{margin:0;background:white;font:20px/24px monospace}}</style>
            <span style="background:yellow">{content}</span>"#
        );
        let image = draw(&html);
        let ink_area = count_color(&image, |p| {
            p[3] > 180 && p[0] < 100 && p[1] < 100 && p[2] < 100
        });
        assert!(ink_area > 0, "{case}: glyph ink must be visible");
        let yellow = count_color(&image, |p| p[0] > 220 && p[1] > 220 && p[2] < 60);
        assert!(
            yellow > ink_area * 2,
            "{case}: highlighted area ({yellow}) must exceed twice the glyph ink area ({ink_area})"
        );
    }
}

#[test]
fn regression_guard_boxless_background_does_not_paint() {
    let html = r#"<style>body{margin:0;background:white;font:20px/24px monospace}</style>
        <div><span style="display:contents;background:red">x</span></div>"#;
    let image = draw(html);
    let red = count_color(&image, |p| p[0] > 180 && p[1] < 80 && p[2] < 80);
    assert_eq!(red, 0, "a boxless wrapper cannot paint its background");
}

fn ink_vertical_bounds(image: &Rendered) -> (usize, usize) {
    let rows: Vec<_> = image.rgba.chunks_exact(4).enumerate()
        .filter(|(_, pixel)| pixel[3] > 180 && pixel[0] < 100 && pixel[1] < 100 && pixel[2] < 100)
        .map(|(index, _)| index / image.width as usize)
        .collect();
    (*rows.iter().min().expect("text ink"), *rows.iter().max().unwrap())
}

#[test]
fn regression_guard_boxless_non_inherited_vertical_align_does_not_shift_glyphs() {
    let style = "body{margin:0;background:white;font:20px/24px monospace}";
    let plain = draw(&format!(
        r#"<style>{style}</style><div id="sample"><span style="display:contents">x</span></div>"#
    ));
    let super_text = draw(&format!(
        r#"<style>{style}</style><div id="sample"><span style="display:contents;vertical-align:super">x</span></div>"#
    ));
    assert_eq!(
        ink_vertical_bounds(&plain), ink_vertical_bounds(&super_text),
        "vertical-align on a boxless wrapper must not change the glyph baseline"
    );
    let baseline_height = plain.elements.iter().find(|el| el.id == "sample").unwrap().height;
    let super_height = super_text.elements.iter().find(|el| el.id == "sample").unwrap().height;
    assert!((baseline_height - super_height).abs() < 0.1, "line-box height must be unchanged");
}

#[test]
fn regression_guard_preserved_newlines_remain_forced_breaks() {
    let lines = line_count(line_height("white-space:pre-wrap", "a\nb"));
    assert_eq!(lines, 2, "preserved newline must keep two lines");
}
