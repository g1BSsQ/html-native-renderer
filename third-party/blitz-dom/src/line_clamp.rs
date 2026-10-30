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

fn has_columns(node: &Node) -> bool {
    ["native-columns", "native-column-count", "native-column-width"]
        .into_iter()
        .filter_map(|name| property(node, name))
        .any(|value| value.split_ascii_whitespace().any(|part| part != "auto"))
}

pub fn for_node(node: &Node) -> Option<Clamp> {
    // In a column container, continuation cannot collapse the content.
    if has_columns(node) {
        return None;
    }

    let shorthand = property(node, "native-clamp").unwrap_or_default();
    let ellipse = property(node, "native-ellipsis").unwrap_or_default();

    if let Some(value) = shorthand.strip_prefix("standard ") {
        return positive_lines(value).map(|lines| Clamp {
            lines,
            legacy: false,
            ellipsis: ellipse != "none",
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
        ellipsis: ellipse != "none",
    })
}
