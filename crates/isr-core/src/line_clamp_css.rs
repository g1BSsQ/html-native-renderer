// Copyright 2026 g1BSsQ. Licensed under Apache-2.0.
//
// Keep the clamp declarations in the cascade even on builds whose style
// property registry does not yet expose these properties. Unknown declarations
// are otherwise discarded before layout can inspect them.

// Rewrite only recognized properties at declaration boundaries. Selectors,
// strings, comments, and declaration values are never treated as property names.
fn declarations(input: &str, inline: bool) -> String {
    let b = input.as_bytes();
    let mut out = String::with_capacity(input.len());
    let mut emitted = 0usize;
    let mut i = 0usize;
    let mut depth = 0usize;
    let mut start = inline.then_some(0usize);
    let mut quote = 0u8;
    let mut comment = false;
    let mut parens = 0usize;

    while i < b.len() {
        if comment {
            if b[i] == b'*' && b.get(i + 1) == Some(&b'/') {
                comment = false;
                i += 2;
            } else { i += 1; }
            continue;
        }
        if quote != 0 {
            if b[i] == b'\\' { i = (i + 2).min(b.len()); continue; }
            if b[i] == quote { quote = 0; }
            i += 1;
            continue;
        }
        if b[i] == b'/' && b.get(i + 1) == Some(&b'*') {
            comment = true;
            i += 2;
            continue;
        }
        if b[i] == b'\'' || b[i] == b'"' {
            quote = b[i];
            i += 1;
            continue;
        }
        match b[i] {
            b'(' => parens += 1,
            b')' => parens = parens.saturating_sub(1),
            b'{' if !inline && parens == 0 => {
                depth += 1;
                start = Some(i + 1);
            }
            b'}' if !inline && parens == 0 => {
                depth = depth.saturating_sub(1);
                start = (depth != 0).then_some(i + 1);
            }
            b';' if parens == 0 => {
                start = (inline || depth != 0).then_some(i + 1);
            }
            b':' if parens == 0 => {
                if let Some(segment) = start {
                    let mut name_start = segment;
                    // CSS comments can precede the property identifier.
                    loop {
                        while name_start < i && b[name_start].is_ascii_whitespace() { name_start += 1; }
                        if b.get(name_start..name_start + 2).is_some_and(|two| two == b"/*") {
                            if let Some(end) = input[name_start + 2..i].find("*/") {
                                name_start += end + 4;
                                continue;
                            }
                        }
                        break;
                    }
                    let mut name_end = name_start;
                    while name_end < i &&
                        (b[name_end].is_ascii_alphanumeric() || b[name_end] == b'-')
                    { name_end += 1; }
                    let mut tail = name_end;
                    while tail < i && b[tail].is_ascii_whitespace() { tail += 1; }
                    if tail == i && name_end > name_start {
                        let name = &input[name_start..name_end];
                        let mapped = match name.to_ascii_lowercase().as_str() {
                            "line-clamp" => Some(("--native-clamp", Some("standard"))),
                            "-webkit-line-clamp" => Some(("--native-clamp", Some("legacy"))),
                            "-webkit-box-orient" => Some(("--native-legacy-orient", None)),
                            "max-lines" => Some(("--native-max-lines", None)),
                            "block-ellipsis" => Some(("--native-ellipsis", None)),
                            _ => None,
                        };
                        if let Some((property, kind)) = mapped {
                            out.push_str(&input[emitted..name_start]);
                            out.push_str(property);
                            out.push_str(&input[name_end..=i]);
                            if let Some(kind) = kind {
                                out.push_str(kind);
                                out.push(' ');
                            }
                            emitted = i + 1;
                        } else if name.eq_ignore_ascii_case("display") {
                            let mut value = i + 1;
                            while value < b.len() && b[value].is_ascii_whitespace() { value += 1; }
                            let legacy = ["-webkit-inline-box", "-webkit-box"]
                                .into_iter()
                                .find(|v| input[value..].starts_with(v)
                                    && b.get(value + v.len()).is_none_or(|next|
                                        !next.is_ascii_alphanumeric() && *next != b'-'));
                            if let Some(legacy) = legacy {
                                out.push_str(&input[emitted..name_start]);
                                if legacy == "-webkit-inline-box" {
                                    out.push_str("display:inline-block;--native-legacy-display:yes");
                                } else {
                                    out.push_str("display:flow-root;--native-legacy-display:yes");
                                }
                                emitted = value + legacy.len();
                                i = emitted;
                                if b.get(i) == Some(&b';') {
                                    start = Some(i + 1);
                                    i += 1;
                                    continue;
                                }
                            }
                        }
                    }
                }
                // The rest of this declaration is a value, not another name.
                start = None;
            }
            _ => {}
        }
        i += 1;
    }
    out.push_str(&input[emitted..]);
    out
}

fn tag_end(html: &str, start: usize) -> usize {
    let b = html.as_bytes();
    let mut quote = 0u8;
    let mut i = start;
    while i < b.len() {
        if quote != 0 {
            if b[i] == quote { quote = 0; }
        } else if b[i] == b'\'' || b[i] == b'"' {
            quote = b[i];
        } else if b[i] == b'>' {
            return i + 1;
        }
        i += 1;
    }
    b.len()
}

fn name_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'-' || b == b':'
}

fn tag_name(tag: &str) -> (&str, bool) {
    let bytes = tag.as_bytes();
    let mut i = 1usize;
    let closing = bytes.get(i) == Some(&b'/');
    if closing { i += 1; }
    while bytes.get(i).is_some_and(u8::is_ascii_whitespace) { i += 1; }
    let start = i;
    while bytes.get(i).is_some_and(|b| name_char(*b)) { i += 1; }
    (&tag[start..i], closing)
}

fn inline_styles(tag: &str) -> String {
    let b = tag.as_bytes();
    let mut out = String::with_capacity(tag.len());
    let mut emitted = 0;
    let mut i = 1usize;
    // Attribute names are only recognized at whitespace-separated starts.
    while i < b.len() {
        if !b[i].is_ascii_whitespace() { i += 1; continue; }
        while i < b.len() && b[i].is_ascii_whitespace() { i += 1; }
        let begin = i;
        while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'-' || b[i] == b':') {
            i += 1;
        }
        if i == begin { i += 1; continue; }
        let is_style = tag[begin..i].eq_ignore_ascii_case("style");
        let name_end = i;
        while i < b.len() && b[i].is_ascii_whitespace() { i += 1; }
        if b.get(i) != Some(&b'=') {
            // Let the next iteration see whitespace after a boolean attribute.
            i = name_end;
            continue;
        }
        i += 1;
        while i < b.len() && b[i].is_ascii_whitespace() { i += 1; }
        let quoted = b.get(i).is_some_and(|c| *c == b'\'' || *c == b'"');
        let quote = if quoted { let c = b[i]; i += 1; c } else { 0 };
        let value_start = i;
        while i < b.len() && (if quoted { b[i] != quote }
            else { !b[i].is_ascii_whitespace() && b[i] != b'>' })
        { i += 1; }
        if is_style {
            out.push_str(&tag[emitted..value_start]);
            out.push_str(&declarations(&tag[value_start..i], true));
            emitted = i;
        }
        if quoted && i < b.len() { i += 1; }
    }
    out.push_str(&tag[emitted..]);
    out
}

/// Preserve markup and text exactly; transform only style-element CSS and
/// style-attribute declaration values.
pub(crate) fn normalize(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut i = 0;
    while i < html.len() {
        let Some(next) = html[i..].find('<') else {
            out.push_str(&html[i..]);
            break;
        };
        let at = i + next;
        out.push_str(&html[i..at]);
        if html[at..].starts_with("<!--") {
            let end = html[at + 4..].find("-->").map_or(html.len(), |p| at + 4 + p + 3);
            out.push_str(&html[at..end]);
            i = end;
            continue;
        }
        let end = tag_end(html, at);
        let tag = &html[at..end];
        let (name, closing) = tag_name(tag);
        if closing || !tag.starts_with('<') || tag.starts_with("<!") || tag.starts_with("<?") {
            out.push_str(tag);
            i = end;
            continue;
        }
        out.push_str(&inline_styles(tag));
        i = end;
        if name.eq_ignore_ascii_case("style")
            || ["script", "textarea", "title", "xmp", "iframe", "noembed", "noframes", "plaintext"]
                .iter().any(|raw| name.eq_ignore_ascii_case(raw))
        {
            let close = format!("</{}", name.to_ascii_lowercase());
            let close_at = html[i..].to_ascii_lowercase().find(&close).map(|p| i + p);
            let boundary = close_at.unwrap_or(html.len());
            if name.eq_ignore_ascii_case("style") {
                out.push_str(&declarations(&html[i..boundary], false));
            } else {
                out.push_str(&html[i..boundary]);
            }
            i = boundary;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{declarations, normalize};

    #[test]
    fn keeps_clamp_declarations_and_legacy_recipe() {
        let css = "display: -webkit-box; -webkit-box-orient: vertical; -webkit-line-clamp: 2; overflow: hidden; line-clamp: 3; max-lines: 4; block-ellipsis: none";
        let out = declarations(css, true);
        for part in ["display:flow-root", "--native-legacy-display:yes",
            "--native-legacy-orient: vertical", "--native-clamp:legacy  2",
            "--native-clamp:standard  3", "--native-max-lines: 4", "--native-ellipsis: none"] {
            assert!(out.contains(part), "{part}: {out}");
        }
        assert!(out.contains("overflow: hidden"));
        assert_eq!(declarations("line-clamp: none", true), "--native-clamp:standard  none");
        assert_eq!(
            declarations("line-clamp:4;-webkit-line-clamp:4", true),
            "--native-clamp:standard 4;--native-clamp:legacy 4"
        );
        assert_eq!(declarations("columns:3;column-count:2;column-width:12em", true),
            "columns:3;column-count:2;column-width:12em");
        assert!(normalize(&format!("<style>.clamp{{{css}}}</style>"))
            .contains("--native-clamp:legacy  2"));
    }

    #[test]
    fn ignores_markup_selectors_strings_and_comments() {
        for untouched in [
            "<p>Columns: x</p>",
            ".columns:last-child{margin:0}",
            ".line-clamp:hover{color:red}",
            r#"td::before{content:"line-clamp: 2"}"#,
            "/* columns: 3 */",
            "<th>Columns:</th>",
            "<code>-webkit-line-clamp: 2</code>",
            "<style>.columns:last-child{margin:0}.line-clamp:hover{color:red}</style>",
            r#"<style>td::before{content:"line-clamp: 2"}/* columns: 3 */</style>"#,
            r#"<div style="content:'line-clamp: 2'"></div>"#,
        ] {
            assert_eq!(normalize(untouched), untouched, "{untouched}");
        }
        assert_eq!(normalize("<style>.clamp{line-clamp:2}</style>"),
            "<style>.clamp{--native-clamp:standard 2}</style>");
        assert_eq!(normalize(r#"<div style="line-clamp:2"></div>"#),
            r#"<div style="--native-clamp:standard 2"></div>"#);
        assert_eq!(normalize(r#"<div hidden style="line-clamp:2"></div>"#),
            r#"<div hidden style="--native-clamp:standard 2"></div>"#);
        assert_eq!(normalize(r#"<div title="columns: 3"></div>"#),
            r#"<div title="columns: 3"></div>"#);
        assert_eq!(normalize("<!-- <style>.a{line-clamp:2}</style> -->"),
            "<!-- <style>.a{line-clamp:2}</style> -->");
    }
}
