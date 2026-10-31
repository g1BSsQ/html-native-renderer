// Copyright 2026 g1BSsQ. Licensed under Apache-2.0.
// Values are preserved as computed custom properties until the style registry
// supports native line-clamp declarations in this build.
use crate::node::Node;

#[derive(Clone, Copy)]
pub struct Clamp {
    pub lines: usize,
    pub legacy: bool,
    pub ellipsis: bool,
}

fn property(node: &Node, name: &str) -> Option<String> {
    let styles = node.primary_styles()?;
    let name = style::Atom::from(name);
    let value = styles.custom_properties().get_for_cssom(&name)?;
    Some(value.as_universal()?.css.trim().to_owned())
}

fn positive_lines(value: &str) -> Option<usize> {
    let digits = value.split_ascii_whitespace().next()?;
    let n = digits.parse::<usize>().ok()?;
    (n > 0 && n <= 1_000_000).then_some(n)
}

// CSS Overflow 4 §4.2 and §5.1: the shorthand's optional marker wins
// over the separately declared marker, and quoted strings remain text.
fn ellipsis_value(value: &str) -> Option<String> {
    match value.trim() {
        "none" | "no-ellipsis" => return None,
        "auto" | "ellipsis" => return Some("…".to_owned()),
        _ => {}
    }
    let mut parser = cssparser::Parser::new(value);
    let marker = parser.expect_string().ok()?.to_string();
    parser.is_exhausted().then_some(marker)
}

pub fn marker_text(node: &Node) -> Option<String> {
    let shorthand = property(node, "native-clamp").unwrap_or_default();
    if shorthand.starts_with("legacy ") {
        return Some("…".to_owned());
    }
    if let Some(value) = shorthand.strip_prefix("standard ") {
        // CSS token whitespace precedes the number in normal declarations.
        // Measure the token span after trimming, not from the raw value.
        let value = value.trim_start();
        let number = value.split_ascii_whitespace().next().unwrap_or("");
        let marker = value[number.len()..].trim();
        if !marker.is_empty() {
            return ellipsis_value(marker);
        }
    }
    ellipsis_value(&property(node, "native-ellipsis")
        .unwrap_or_else(|| "ellipsis".to_owned()))
}

fn has_columns(node: &Node) -> bool {
    node.primary_styles()
        .is_some_and(|style| style.get_column().is_multicol())
}

pub fn for_node(node: &Node) -> Option<Clamp> {
    // In a column container, continuation cannot collapse the content.
    if has_columns(node) {
        return None;
    }

    let shorthand = property(node, "native-clamp").unwrap_or_default();

    if let Some(value) = shorthand.strip_prefix("standard ") {
        return positive_lines(value).map(|lines| Clamp {
            lines,
            legacy: false,
            ellipsis: marker_text(node).is_some(),
        });
    }

    if let Some(value) = shorthand.strip_prefix("legacy ") {
        if property(node, "native-legacy-display").as_deref() == Some("yes")
            && property(node, "native-legacy-orient").as_deref() == Some("vertical")
        {
            return positive_lines(value).map(|lines| Clamp {
                lines,
                legacy: true,
                ellipsis: true,
            });
        }
        return None;
    }

    positive_lines(&property(node, "native-max-lines").unwrap_or_default()).map(|lines| Clamp {
        lines,
        legacy: false,
        ellipsis: marker_text(node).is_some(),
    })
}
