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

pub fn for_node(node: &Node) -> Option<Clamp> {
    let standard = property(node, "native-lines").unwrap_or_default();
    let max_lines = property(node, "native-max-lines").unwrap_or_default();
    let legacy_lines = property(node, "native-legacy-lines").unwrap_or_default();
    let ellipse = property(node, "native-ellipsis").unwrap_or_default();

    if let Some(lines) = positive_lines(&standard).or_else(|| positive_lines(&max_lines)) {
        return Some(Clamp {
            lines,
            legacy: false,
            ellipsis: ellipse != "none",
        });
    }

    if property(node, "native-legacy-display").as_deref() == Some("yes")
        && property(node, "native-legacy-orient").as_deref() == Some("vertical")
    {
        return positive_lines(&legacy_lines).map(|lines| Clamp {
            lines,
            legacy: true,
            ellipsis: true,
        });
    }

    None
}
