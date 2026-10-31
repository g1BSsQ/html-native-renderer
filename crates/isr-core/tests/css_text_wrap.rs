use isr_core::{render, RenderOptions};

fn geometry(html: &str, id: &str) -> (f64, f64) {
    let image = render(
        html,
        RenderOptions {
            width: 320,
            system_fonts: true,
            ..RenderOptions::default()
        },
    )
    .expect("test should render");
    let el = image.elements.iter().find(|el| el.id == id).expect("element");
    (el.y, el.height)
}

fn line_height(style: &str, content: &str) -> f64 {
    let html = format!(
        r#"<style>body{{margin:0;font:20px/24px monospace}}#sample{{width:26px}}</style>
        <div id="sample"><span style="display:contents;{style}">{content}</span></div>"#
    );
    geometry(&html, "sample").1
}

#[test]
fn boxless_wrapper_forwards_word_break_to_line_boxes() {
    let lines = line_height("word-break:break-all", "abcdef");
    assert!(lines >= 48.0, "break-all must form multiple line boxes: {lines}");
}

#[test]
fn boxless_wrapper_forwards_overflow_wrap_to_line_boxes() {
    let lines = line_height("overflow-wrap:anywhere", "abcdef");
    assert!(lines >= 48.0, "anywhere must wrap an otherwise unbreakable label: {lines}");
}

#[test]
fn boxless_wrapper_forwards_line_break_anywhere() {
    let lines = line_height("line-break:anywhere", "abcdef");
    assert!(lines >= 48.0, "line-break:anywhere must form multiple lines: {lines}");
}

#[test]
fn boxless_wrapper_forwards_letter_spacing_into_break_measurement() {
    let html = r#"<style>body{margin:0;font:20px/24px monospace}#sample{width:40px}</style>
        <div id="sample"><span style="display:contents;letter-spacing:20px">A A</span></div>"#;
    let (_, height) = geometry(html, "sample");
    assert!(height >= 48.0, "additional glyph advances must wrap at the space: {height}");
}

#[test]
fn regression_guard_preserved_newlines_remain_forced_breaks() {
    let lines = line_height("white-space:pre-wrap", "a\nb");
    assert!((lines - 48.0).abs() < 1.0, "preserved newline must keep two lines: {lines}");
}
