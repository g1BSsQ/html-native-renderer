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
    fn contains(&self, name: &str) -> bool {
        self.values.get(name).is_some_and(|values| !values.is_empty())
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
            if negative { format!("-{}", absolute) } else { format!("{:02}", absolute) }
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
// Return the closing parenthesis of a CSS function, respecting strings,
// escapes and nested calls (including a separator such as ")").
fn function_end(value: &str, open: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut quote = None;
    let mut escape = false;
    for (offset, ch) in value[open..].char_indices() {
        if escape { escape = false; continue; }
        if ch == '\\' { escape = true; continue; }
        if let Some(q) = quote {
            if ch == q { quote = None; }
            continue;
        }
        if ch == '"' || ch == '\'' { quote = Some(ch); continue; }
        if ch == '(' { depth += 1; }
        if ch == ')' {
            depth -= 1;
            if depth == 0 { return Some(open + offset); }
        }
    }
    None
}

fn counter_value(input: &str, counters: &Counters, multi: bool) -> Option<String> {
    let args = arguments(input);
    let name = args.first()?;
    let style = if multi { args.get(2) } else { args.get(1) }
        .map(String::as_str).unwrap_or("decimal");
    if multi {
        let sep = args.get(1).map(|s| unquote(s)).unwrap_or_default();
        Some(counters.values.get(name).map(|values|
            values.iter().map(|v| format_counter(*v, style)).collect::<Vec<_>>().join(&sep)
        ).unwrap_or_else(|| format_counter(0, style)))
    } else {
        Some(format_counter(counters.current(name), style))
    }
}

// In a generated CSS declaration, leave unrelated content items intact. The
// upstream engine can still handle string and other supported content tokens.
fn expand_content(content: &str, counters: &Counters, css: bool) -> Option<String> {
    let mut output = String::new();
    let mut i = 0;
    let mut touched = false;
    while i < content.len() {
        let tail = &content[i..];
        if tail.starts_with('"') || tail.starts_with('\'') {
            let quote = tail.chars().next().unwrap();
            let mut end = tail.len();
            let mut escaped = false;
            for (idx, ch) in tail[1..].char_indices() {
                if escaped { escaped = false; continue; }
                if ch == '\\' { escaped = true; continue; }
                if ch == quote { end = idx + 2; break; }
            }
            if css { output.push_str(&tail[..end]); }
            else { output.push_str(&unquote(&tail[..end])); }
            i += end;
            continue;
        }
        let call = tail.starts_with("counters(").then_some((true, "counters("))
            .or_else(|| tail.starts_with("counter(").then_some((false, "counter(")));
        if let Some((multi, prefix)) = call {
            let open = i + prefix.len() - 1;
            if let Some(close) = function_end(content, open) {
                if let Some(value) = counter_value(&content[open + 1..close], counters, multi) {
                    if css { output.push_str(&css_quote(&value)); }
                    else { output.push_str(&value); }
                    i = close + 1;
                    touched = true;
                    continue;
                }
            }
        }
        let ch = tail.chars().next().unwrap();
        // Preserve unknown syntax (attr(), quotes, keywords, URLs) rather than
        // silently deleting it from the replacement declaration.
        if css || !ch.is_whitespace() { output.push(ch); }
        i += ch.len_utf8();
    }
    touched.then_some(output)
}
fn content_text(content: &str, counters: &Counters) -> Option<String> {
    expand_content(content, counters, false)
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
fn counter_attr(doc: &BaseDocument, id: NodeId, name: &str) -> Option<i64> {
    let node = doc.get_node(id)?;
    match &node.data {
        NodeData::Element(el) => el.attrs.iter()
            .find(|a| &*a.name.local == name)
            .and_then(|a| a.value.parse::<i64>().ok()),
        _ => None,
    }
}
fn enter(doc: &BaseDocument, id: NodeId, counters: &mut Counters) -> Vec<String> {
    let mut created = Vec::new();
    for (name, value) in declarations(&style(doc, id, "counter-reset"), 0) {
        counters.reset(&name, value);
        created.push(name);
    }
    let tag = doc.get_node(id).and_then(|node| match &node.data {
        NodeData::Element(el) => Some(el.name.local.to_string()),
        _ => None,
    });
    // HTML ordered-list start initializes the list-item counter immediately
    // before its first item. The list itself need not have display:list-item.
    if tag.as_deref() == Some("ol") {
        if let Some(start) = counter_attr(doc, id, "start") {
            if !counters.contains("list-item") {
                counters.reset("list-item", 0);
                created.push("list-item".to_owned());
            }
            counters.set("list-item", start.saturating_sub(1));
        }
    }
    let increments = declarations(&style(doc, id, "counter-increment"), 1);
    let has_list_item = increments.iter().any(|(name, _)| name == "list-item");
    for (name, value) in increments {
        if !counters.contains(&name) {
            counters.reset(&name, 0);
            created.push(name.clone());
        }
        counters.increment(&name, value);
    }
    // display:list-item, not the <li> tag, determines the implicit increment.
    if style(doc, id, "display").split_whitespace().any(|part| part == "list-item")
        && !has_list_item
    {
        if !counters.contains("list-item") {
            counters.reset("list-item", 0);
            created.push("list-item".to_owned());
        }
        counters.increment("list-item", 1);
    }
    // HTML li[value] behaves as a counter-set after the increment.
    if tag.as_deref() == Some("li") {
        if let Some(value) = counter_attr(doc, id, "value") {
            if !counters.contains("list-item") {
                counters.reset("list-item", 0);
                created.push("list-item".to_owned());
            }
            counters.set("list-item", value);
        }
    }
    // CSS Lists 3: reset -> increment -> set. Unscoped set creates a
    // counter belonging to the current sibling scope.
    for (name, value) in declarations(&style(doc, id, "counter-set"), 0) {
        if !counters.contains(&name) {
            counters.reset(&name, 0);
            created.push(name.clone());
        }
        counters.set(&name, value);
    }
    created
}
fn leave(counters: &mut Counters, names: Vec<String>) {
    for name in names.into_iter().rev() { counters.pop(&name); }
}
struct Generated { id: NodeId, suffix: Option<&'static str>, text: String }

fn visit_pseudo(doc: &BaseDocument, id: Option<NodeId>, suffix: &'static str,
    owner: NodeId, counters: &mut Counters, output: &mut Vec<Generated>) -> Vec<String> {
    let Some(id) = id else { return Vec::new() };
    let created = enter(doc, id, counters);
    if let Some(text) = expand_content(&style(doc, id, "content"), counters, true) {
        output.push(Generated { id: owner, suffix: Some(suffix), text });
    }
    created
}
fn visit(doc: &BaseDocument, node: &Node, counters: &mut Counters,
         output: &mut Vec<Generated>) -> Vec<String> {
    let mut created = Vec::new();
    if let NodeData::Element(_) = &node.data {
        if style(doc, node.id, "display") == "none" { return created; }
        created = enter(doc, node.id, counters);
        let before = visit_pseudo(doc, node.before(), "before", node.id, counters, output);
        // Sibling counter creations remain visible to following siblings, and
        // are removed when their common parent closes, not at each child exit.
        let mut child_counters = before;
        for &id in &node.children {
            if let Some(child) = doc.get_node(id) {
                child_counters.extend(visit(doc, child, counters, output));
            }
        }
        child_counters.extend(visit_pseudo(doc, node.after(), "after", node.id, counters, output));
        leave(counters, child_counters);
        let list_style = style(doc, node.id, "list-style-type");
        if style(doc, node.id, "display").split_whitespace().any(|s| s == "list-item")
            && matches!(list_style.as_str(), "decimal" | "decimal-leading-zero"
                | "lower-alpha" | "upper-alpha" | "lower-roman" | "upper-roman")
        {
            let counter = counters.current("list-item");
            let value = format_counter(counter, &list_style);
            let suffix = if matches!(list_style.as_str(), "disc" | "circle" | "square") {
                " "
            } else { ". " };
            output.push(Generated {
                id: node.id, suffix: None, text: css_quote(&format!("{value}{suffix}")),
            });
        }
    } else {
        for &id in &node.children {
            if let Some(child) = doc.get_node(id) {
                created.extend(visit(doc, child, counters, output));
            }
        }
    }
    created
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
        if let Some(suffix) = item.suffix {
            css.push_str(&format!(
                "[data-isr-counter=\"k{num}\"]::{suffix} {{ content: {} !important; }}\n",
                item.text));
        } else {
            css.push_str(&format!(
                "[data-isr-counter=\"k{num}\"] {{ list-style-type: {} !important; }}\n",
                item.text));
        }
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
    fn sibling_resets_survive_until_parent_closes() {
        let mut doc = blitz_html::HtmlDocument::from_html(
            r#"<style>h1{counter-reset:s}h2{counter-increment:s}</style>
            <h1 id="r1"></h1><h2 id="a"></h2><h2 id="b"></h2>
            <h1 id="r2"></h1><h2 id="c"></h2>"#,
            blitz_dom::DocumentConfig::default());
        doc.resolve(0.0);
        let mut counters = Counters::default();
        let mut parent_scope = Vec::new();
        let mut labels = Vec::new();
        for id in ["r1", "a", "b", "r2", "c"] {
            let node = doc.get_element_by_id(id).unwrap();
            parent_scope.extend(enter(&doc, node, &mut counters));
            if matches!(id, "a" | "b" | "c") { labels.push(counters.current("s")); }
        }
        assert_eq!(labels, [1, 2, 1]);
        leave(&mut counters, parent_scope);
        assert!(!counters.contains("s"));
    }
    #[test]
    fn counter_set_applies_after_increment() {
        let mut doc = blitz_html::HtmlDocument::from_html(
            r#"<style>p{counter-increment:n 2;counter-set:n 6}</style>
            <p id="subject"></p>"#,
            blitz_dom::DocumentConfig::default());
        doc.resolve(0.0);
        let mut state = Counters::default();
        let node = doc.get_element_by_id("subject").unwrap();
        let created = enter(&doc, node, &mut state);
        assert_eq!(state.current("n"), 6);
        leave(&mut state, created);
        assert!(!state.contains("n"));
    }
    #[test]
    fn ordered_list_start_and_value_adjust_markers() {
        let mut doc = blitz_html::HtmlDocument::from_html(
            r#"<ol id="list" start="4"><li id="first">First</li>
            <li id="chosen" value="9">Chosen</li><li id="next">Next</li></ol>"#,
            blitz_dom::DocumentConfig::default());
        doc.resolve(0.0);
        let mut state = Counters::default();
        let mut created = enter(&doc, doc.get_element_by_id("list").unwrap(), &mut state);
        for (id, expected) in [("first", 4), ("chosen", 9), ("next", 10)] {
            created.extend(enter(&doc, doc.get_element_by_id(id).unwrap(), &mut state));
            assert_eq!(state.current("list-item"), expected);
        }
        leave(&mut state, created);
    }
    #[test]
    fn nested_counters_join_all_active_levels() {
        let mut state = Counters::default();
        state.reset("chapter", 1);
        state.reset("chapter", 2);
        state.increment("chapter", 1);
        assert_eq!(content_text(r#"counters(chapter, ".")"#, &state).unwrap(), "1.3");
        assert_eq!(content_text(r#"counters(chapter, ")")"#, &state).unwrap(), "1)3");
        assert_eq!(content_text(r#"counter(chapter, decimal-leading-zero)"#, &state).unwrap(), "03");
        assert_eq!(format_counter(-5, "decimal-leading-zero"), "-5");
        assert_eq!(format_counter(27, "upper-alpha"), "AA");
        assert_eq!(format_counter(14, "lower-roman"), "xiv");
        assert_eq!(expand_content(r#""pre" counter(chapter) attr(data-x)"#, &state, true).unwrap(),
            r#""pre" "3" attr(data-x)"#);
    }
}
