// Copyright 2026 g1BSsQ. Licensed under Apache-2.0.
// Static CSS counter resolution for generated pseudo-element text.
// The second style/layout pass is deliberately script-free.
use blitz_dom::{BaseDocument, Node, NodeData, NodeId, UserAgentStylesheet};
use std::collections::BTreeMap;

#[derive(Default)]
struct Counters { values: BTreeMap<String, Vec<i64>> }

impl Counters {
    fn current(&self, name: &str) -> i64 {
        self.values.get(name).and_then(|v| v.last()).copied().unwrap_or(0)
    }
    fn reset(&mut self, name: &str, value: i64) {
        self.values.entry(name.to_owned()).or_default().push(value);
    }
    fn set(&mut self, name: &str, value: i64) {
        *self.values.entry(name.to_owned()).or_insert_with(|| vec![0]).last_mut().unwrap() = value;
    }
    fn increment(&mut self, name: &str, by: i64) {
        let next = self.current(name).saturating_add(by);
        self.set(name, next);
    }
    fn pop(&mut self, name: &str) {
        if let Some(values) = self.values.get_mut(name) {
            values.pop();
            if values.is_empty() { self.values.remove(name); }
        }
    }
}

fn declarations(value: &str, default: i64) -> Vec<(String, i64)> {
    if value.is_empty() || value == "none" { return Vec::new(); }
    let words: Vec<&str> = value.split_whitespace().collect();
    let mut result = Vec::new();
    let mut i = 0;
    while i < words.len() {
        let name = words[i];
        if name == "none" { break; }
        i += 1;
        let mut amount = default;
        if i < words.len() {
            if let Ok(v) = words[i].parse::<i64>() {
                amount = v;
                i += 1;
            }
        }
        result.push((name.to_owned(), amount));
    }
    result
}

fn format_counter(value: i64, kind: &str) -> String {
    let negative = value < 0;
    let absolute = value.unsigned_abs();
    match kind.trim() {
        "decimal-leading-zero" => {
            if negative { format!("-{:02}", absolute) } else { format!("{:02}", absolute) }
        }
        "lower-alpha" | "upper-alpha" if value > 0 => {
            let mut n = absolute;
            let mut text = String::new();
            while n > 0 {
                n -= 1;
                text.insert(0, (b'a' + (n % 26) as u8) as char);
                n /= 26;
            }
            if kind.trim() == "upper-alpha" { text.to_ascii_uppercase() } else { text }
        }
        "lower-roman" | "upper-roman" if (1..=3999).contains(&value) => {
            let mut n = value;
            let mut result = String::new();
            for (amount, glyph) in [(1000,"m"),(900,"cm"),(500,"d"),(400,"cd"),
                (100,"c"),(90,"xc"),(50,"l"),(40,"xl"),(10,"x"),(9,"ix"),(5,"v"),(4,"iv"),(1,"i")] {
                while n >= amount { n -= amount; result.push_str(glyph); }
            }
            if kind.trim() == "upper-roman" { result.to_ascii_uppercase() } else { result }
        }
        "disc" => "•".to_owned(),
        "circle" => "◦".to_owned(),
        "square" => "▪".to_owned(),
        _ => value.to_string(),
    }
}

// Split arguments at commas, leaving commas inside CSS strings untouched.
fn arguments(s: &str) -> Vec<String> {
    let mut args = Vec::new();
    let (mut start, mut quote, mut escaped) = (0, None, false);
    for (i, ch) in s.char_indices() {
        if escaped { escaped = false; continue; }
        if ch == '\\' && quote.is_some() { escaped = true; continue; }
        if Some(ch) == quote { quote = None; continue; }
        if quote.is_none() && (ch == '"' || ch == '\'') { quote = Some(ch); continue; }
        if ch == ',' && quote.is_none() {
            args.push(s[start..i].trim().to_owned()); start = i + 1;
        }
    }
    args.push(s[start..].trim().to_owned());
    args
}
fn unquote(s: &str) -> String {
    let s = s.trim();
    let s = s.strip_prefix('"').and_then(|s| s.strip_suffix('"'))
        .or_else(|| s.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')))
        .unwrap_or(s);
    let mut text = String::new();
    let mut chars = s.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' { if let Some(next) = chars.next() { text.push(next); } }
        else { text.push(ch); }
    }
    text
}
fn content_text(content: &str, counters: &Counters) -> Option<String> {
    // Other generated content (images, attr(), quote control) stays upstream.
    if !content.contains("counter(") && !content.contains("counters(") { return None; }
    let mut output = String::new();
    let mut i = 0;
    while i < content.len() {
        let tail = &content[i..];
        if let Some((multi, prefix)) = tail.starts_with("counters(").then_some((true, "counters("))
            .or_else(|| tail.starts_with("counter(").then_some((false, "counter("))) {
            if let Some(close) = tail[prefix.len()..].find(')') {
                let args = arguments(&tail[prefix.len()..prefix.len() + close]);
                if let Some(name) = args.first() {
                    let style = if multi { args.get(2) } else { args.get(1) }
                        .map(String::as_str).unwrap_or("decimal");
                    if multi {
                        let sep = args.get(1).map(|s| unquote(s)).unwrap_or_default();
                        let text = counters.values.get(name).map(|values|
                            values.iter().map(|v| format_counter(*v, style)).collect::<Vec<_>>().join(&sep)
                        ).unwrap_or_else(|| format_counter(0, style));
                        output.push_str(&text);
                    } else { output.push_str(&format_counter(counters.current(name), style)); }
                }
                i += prefix.len() + close + 1;
                continue;
            }
        }
        if tail.starts_with('"') || tail.starts_with('\'') {
            let quote = tail.chars().next().unwrap();
            let mut end = 1;
            let mut escaped = false;
            for (idx, ch) in tail[1..].char_indices() {
                end = idx + 2;
                if escaped { escaped = false; continue; }
                if ch == '\\' { escaped = true; continue; }
                if ch == quote { break; }
            }
            output.push_str(&unquote(&tail[..end]));
            i += end;
        } else {
            let ch = tail.chars().next().unwrap();
            i += ch.len_utf8();
        }
    }
    Some(output)
}
fn css_quote(s: &str) -> String {
    let mut result = String::from("\"");
    for ch in s.chars() {
        match ch {
            '\\' => result.push_str("\\\\"),
            '"' => result.push_str("\\\""),
            '\n' => result.push_str("\\A "),
            '\r' => result.push_str("\\D "),
            _ => result.push(ch),
        }
    }
    result.push('"');
    result
}
fn style(doc: &BaseDocument, id: NodeId, property: &str) -> String {
    doc.resolved_style_value(id, property)
}
fn enter(doc: &BaseDocument, id: NodeId, counters: &mut Counters) -> Vec<String> {
    let resets = declarations(&style(doc, id, "counter-reset"), 0);
    let mut names = Vec::new();
    for (name, value) in resets {
        counters.reset(&name, value);
        names.push(name);
    }
    for (name, value) in declarations(&style(doc, id, "counter-set"), 0) {
        counters.set(&name, value);
    }
    let increments = declarations(&style(doc, id, "counter-increment"), 1);
    let has_list_item = increments.iter().any(|(name, _)| name == "list-item");
    for (name, value) in increments { counters.increment(&name, value); }
    if let Some(node) = doc.get_node(id) {
        // The UA rule resets list-item on lists but does not increment li.
        if let NodeData::Element(el) = &node.data {
            if &*el.name.local == "li" && !has_list_item { counters.increment("list-item", 1); }
        }
    }
    names
}
fn leave(counters: &mut Counters, names: Vec<String>) {
    for name in names.into_iter().rev() { counters.pop(&name); }
}
struct Generated { id: NodeId, suffix: &'static str, text: String }

fn visit_pseudo(doc: &BaseDocument, id: Option<NodeId>, suffix: &'static str,
    owner: NodeId, counters: &mut Counters, output: &mut Vec<Generated>) {
    let Some(id) = id else { return };
    let names = enter(doc, id, counters);
    if let Some(text) = content_text(&style(doc, id, "content"), counters) {
        output.push(Generated { id: owner, suffix, text });
    }
    leave(counters, names);
}
fn visit(doc: &BaseDocument, node: &Node, counters: &mut Counters,
         output: &mut Vec<Generated>) {
    if let NodeData::Element(_) = &node.data {
        if style(doc, node.id, "display") == "none" { return; }
        let names = enter(doc, node.id, counters);
        visit_pseudo(doc, node.before(), "before", node.id, counters, output);
        for &id in &node.children {
            if let Some(child) = doc.get_node(id) {
                visit(doc, child, counters, output);
            }
        }
        visit_pseudo(doc, node.after(), "after", node.id, counters, output);
        leave(counters, names);
    } else {
        for &id in &node.children {
            if let Some(child) = doc.get_node(id) { visit(doc, child, counters, output); }
        }
    }
}

/// Convert counter functions to literal generated text using the computed
/// counter operations in document order, then ask the engine to shape/reflow
/// the resulting pseudo-elements in a second ordinary resolve pass.
pub(crate) fn materialize(doc: &mut BaseDocument) -> bool {
    let mut generated = Vec::new();
    visit(doc, doc.root_node(), &mut Counters::default(), &mut generated);
    if generated.is_empty() { return false; }
    let mut css = String::new();
    let mut numbered = BTreeMap::new();
    for item in &generated {
        let num = if let Some(n) = numbered.get(&item.id) { *n } else {
            let n = numbered.len();
            numbered.insert(item.id, n);
            n
        };
        css.push_str(&format!(
            "[data-isr-counter=\"k{num}\"]::{} {{ content: {} !important; }}\n",
            item.suffix, css_quote(&item.text)));
    }
    {
        let mut mutator = doc.mutate();
        for (&id, &num) in &numbered {
            mutator.set_attribute(id, blitz_dom::QualName::new(
                None, blitz_dom::Namespace::from(""), blitz_dom::LocalName::from("data-isr-counter")),
                &format!("k{num}"));
        }
    }
    doc.add_user_agent_stylesheet(UserAgentStylesheet::parse(&css));
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn nested_counters_join_all_active_levels() {
        let mut state = Counters::default();
        state.reset("chapter", 1);
        state.reset("chapter", 2);
        state.increment("chapter", 1);
        assert_eq!(content_text(r#"counters(chapter, ".")"#, &state).unwrap(), "1.3");
        assert_eq!(content_text(r#"counter(chapter, decimal-leading-zero)"#, &state).unwrap(), "03");
        assert_eq!(format_counter(27, "upper-alpha"), "AA");
        assert_eq!(format_counter(14, "lower-roman"), "xiv");
    }
}
