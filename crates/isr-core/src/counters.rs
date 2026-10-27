// Copyright 2026 g1BSsQ. Licensed under Apache-2.0.
// Static CSS counter resolution for generated pseudo-element text.
// The second style/layout pass is deliberately script-free.
use blitz_dom::{BaseDocument, Node, NodeData, NodeId, UserAgentStylesheet};
use std::collections::BTreeMap;

// Keep direction with each scoped value. The counter-set inheritance model
// remains separate and can replace this stack in a later change.
#[derive(Clone, Copy)]
struct Counter { value: i64, reversed: bool }
#[derive(Default)]
struct Counters { values: BTreeMap<String, Vec<Counter>> }

impl Counters {
    fn current(&self, name: &str) -> i64 {
        self.values.get(name).and_then(|v| v.last()).map(|v| v.value).unwrap_or(0)
    }
    fn reset(&mut self, name: &str, value: i64) {
        self.reset_direction(name, value, false);
    }
    fn reset_direction(&mut self, name: &str, value: i64, reversed: bool) {
        self.values.entry(name.to_owned()).or_default().push(Counter { value, reversed });
    }
    fn implicit_list_step(&self) -> i64 {
        if self.values.get("list-item").and_then(|v| v.last())
            .is_some_and(|v| v.reversed) { -1 } else { 1 }
    }
    fn contains(&self, name: &str) -> bool {
        self.values.get(name).is_some_and(|values| !values.is_empty())
    }
    fn set(&mut self, name: &str, value: i64) {
        self.values.entry(name.to_owned())
            .or_insert_with(|| vec![Counter { value: 0, reversed: false }])
            .last_mut().unwrap().value = value;
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

// Unlike counter-increment/set, counter-reset accepts reversed(<name>).
// A reversed counter without an explicit number gets an automatic initial
// value, calculated from increments and the first set within its scope.
fn reset_declarations(value: &str) -> Vec<(String, Option<i64>, bool)> {
    if value.is_empty() || value == "none" { return Vec::new(); }
    let words: Vec<&str> = value.split_whitespace().collect();
    let mut resets = Vec::new();
    let mut i = 0;
    while i < words.len() {
        let raw = words[i];
        if raw == "none" { break; }
        let reversed = raw.starts_with("reversed(") && raw.ends_with(')');
        let name = if reversed { &raw[9..raw.len() - 1] } else { raw };
        i += 1;
        if name.is_empty() { continue; }
        let explicit = if i < words.len() {
            if let Ok(n) = words[i].parse::<i64>() { i += 1; Some(n) } else { None }
        } else { None };
        resets.push((name.to_owned(), explicit.or_else(|| (!reversed).then_some(0)), reversed));
    }
    resets
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
            values.iter().map(|v| format_counter(v.value, style)).collect::<Vec<_>>().join(&sep)
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
// The pinned Stylo CSSOM can return an empty resolved value for counter-set.
// Retain the author's declaration in that case, using the document's original
// <style> text and selector matcher. Do not infer a set from counter-increment.
fn counter_set_value(doc: &BaseDocument, id: NodeId) -> String {
    let computed = style(doc, id, "counter-set");
    if !computed.is_empty() && computed != "none" { return computed; }

    fn declared_value(declarations: &str) -> Option<String> {
        declarations.split(';').filter_map(|part| {
            let (name, value) = part.split_once(':')?;
            (name.trim().eq_ignore_ascii_case("counter-set")).then(||
                value.trim().trim_end_matches("!important").trim().to_owned())
        }).last()
    }

    let mut selected = None;
    for sheet_id in doc.stylesheet_owner_nodes() {
        let Some(sheet) = doc.get_node(sheet_id) else { continue };
        // Keep the author order, including rules skipped by the pinned
        // engine's CSSOM support, rather than silently dropping counter-set.
        let source = sheet.text_content();
        for rule in source.split('}') {
            let Some((selector, body)) = rule.rsplit_once('{') else { continue };
            let selector = selector.trim();
            let Some(value) = declared_value(body) else { continue };
            if doc.query_selector_all(selector)
                .is_ok_and(|matches| matches.contains(&id))
            {
                selected = Some(value);
            }
        }
    }
    if let Some(node) = doc.get_node(id) {
        if let NodeData::Element(element) = &node.data {
            if let Some(inline) = element.attrs.iter()
                .find(|attr| &*attr.name.local == "style")
                .and_then(|attr| declared_value(attr.value.as_str()))
            {
                selected = Some(inline);
            }
        }
    }
    selected.unwrap_or(computed)
}

// Resolve the counter-reset entry used for scoping and direction. The pinned
// UA stylesheet contains an ordinary ol reset but lacks ol[reversed]'s
// reversed(list-item) rule. Model that UA declaration only when no author
// counter-reset overrides it; direction is thereafter taken from this entry,
// never forced by the HTML attribute in enter().
fn counter_reset_value(doc: &BaseDocument, id: NodeId) -> String {
    let computed = style(doc, id, "counter-reset");
    let Some(node) = doc.get_node(id) else { return computed };

    fn declaration(css: &str) -> Option<String> {
        css.split(';').filter_map(|part| {
            let (name, value) = part.split_once(':')?;
            name.trim().eq_ignore_ascii_case("counter-reset")
                .then(|| value.trim().trim_end_matches("!important").trim().to_owned())
        }).last()
    }

    // The pinned Stylo CSSOM might omit reversed() even when it was authored.
    // An inline author reset, including an ordinary list-item reset, has
    // precedence over the synthetic UA reversal.
    if let NodeData::Element(el) = &node.data {
        if let Some(inline) = el.attrs.iter()
            .find(|attr| &*attr.name.local == "style")
            .and_then(|attr| declaration(attr.value.as_str()))
        {
            return if inline.contains("reversed(") { inline } else { computed };
        }
    }

    let is_reversed_ol = node.element_data().is_some_and(|el|
        &*el.name.local == "ol" && node.attr(blitz_dom::local_name!("reversed")).is_some());
    if !is_reversed_ol || reset_declarations(&computed).iter()
        .any(|(name, _, reversed)| name == "list-item" && *reversed)
    {
        return computed;
    }

    // Only a reversed ol needs this UA fallback. An author stylesheet's
    // counter-reset beats ol[reversed], even when it sets the ordinary value
    // to the same value as the pinned default UA rule.
    for sheet_id in doc.stylesheet_owner_nodes() {
        let Some(sheet) = doc.get_node(sheet_id) else { continue };
        let source = sheet.text_content();
        for rule in source.split('}') {
            let Some((selector, body)) = rule.rsplit_once('{') else { continue };
            let Some(value) = declaration(body) else { continue };
            if doc.query_selector_all(selector.trim())
                .is_ok_and(|matches| matches.contains(&id))
            {
                return if value.contains("reversed(") { value } else { computed };
            }
        }
    }

    // The synthetic UA reset matches the default ol counter-reset, but uses
    // reversed direction. A later explicit author reset returns above.
    if reset_declarations(&computed).iter().any(|(name, _, _)| name == "list-item") {
        "reversed(list-item)".to_owned()
    } else {
        computed
    }
}

#[derive(Default)]
struct ReversedSeed {
    sum: i64,
    last_nonzero_increment_negated: i64,
}
impl ReversedSeed {
    fn initial(&self) -> i64 {
        self.sum.saturating_add(self.last_nonzero_increment_negated)
    }
}

// Return true when the first counter-set/value ends the automatic-initial-
// value scan. A nested reset belongs to a different scope and is skipped.
fn scan_reversed(doc: &BaseDocument, id: NodeId, creator: NodeId,
                 name: &str, seed: &mut ReversedSeed) -> bool {
    let Some(node) = doc.get_node(id) else { return false };
    if style(doc, id, "display") == "none" { return false; }
    if id != creator && reset_declarations(&counter_reset_value(doc, id))
        .iter().any(|(reset_name, _, _)| reset_name == name)
    {
        return false;
    }

    let increments = declarations(&style(doc, id, "counter-increment"), 1);
    let has_explicit = increments.iter().any(|(n, _)| n == name);
    let mut increment = increments.iter().filter(|(n, _)| n == name)
        .fold(0_i64, |total, (_, amount)| total.saturating_add(*amount));
    if name == "list-item" && !has_explicit
        && style(doc, id, "display").split_whitespace().any(|s| s == "list-item")
    {
        increment = increment.saturating_sub(1);
    }
    let negated = increment.saturating_neg();
    if negated != 0 { seed.last_nonzero_increment_negated = negated; }

    // A list item's HTML value is a counter-set operation; a CSS counter-set
    // on the same element wins, just as it does in enter().
    let html_value = if name == "list-item"
        && node.element_data().is_some_and(|el| &*el.name.local == "li")
    {
        counter_attr(doc, id, "value")
    } else { None };
    let explicit_set = declarations(&counter_set_value(doc, id), 0)
        .into_iter().filter(|(n, _)| n == name).map(|(_, v)| v).last();
    if let Some(set) = explicit_set.or(html_value) {
        seed.sum = seed.sum.saturating_add(set);
        return true;
    }
    seed.sum = seed.sum.saturating_add(negated);

    if let Some(before) = node.before() {
        if scan_reversed(doc, before, creator, name, seed) { return true; }
    }
    for &child in &node.children {
        if scan_reversed(doc, child, creator, name, seed) { return true; }
    }
    if let Some(after) = node.after() {
        if scan_reversed(doc, after, creator, name, seed) { return true; }
    }
    false
}

fn reversed_initial(doc: &BaseDocument, creator: NodeId, name: &str) -> i64 {
    let mut seed = ReversedSeed::default();
    if scan_reversed(doc, creator, creator, name, &mut seed) {
        return seed.initial();
    }
    // The reset covers descendants and following siblings, up to a sibling
    // reset that begins a new scope for the same counter name.
    if let Some(parent) = doc.get_node(creator).and_then(|node| node.parent)
        .and_then(|id| doc.get_node(id))
    {
        let mut following = false;
        for &sibling in &parent.children {
            if !following {
                following = sibling == creator;
                continue;
            }
            if reset_declarations(&counter_reset_value(doc, sibling))
                .iter().any(|(reset_name, _, _)| reset_name == name)
            {
                break;
            }
            if scan_reversed(doc, sibling, creator, name, &mut seed) { break; }
        }
    }
    seed.initial()
}

fn enter(doc: &BaseDocument, id: NodeId, counters: &mut Counters) -> Vec<String> {
    let mut created = Vec::new();
    let tag = doc.get_node(id).and_then(|node| match &node.data {
        NodeData::Element(el) => Some(el.name.local.to_string()),
        _ => None,
    });
    let ordered = tag.as_deref() == Some("ol");
    let mut resets = reset_declarations(&counter_reset_value(doc, id));
    // Direction comes only from the winning counter-reset entry, not from
    // ol[reversed] itself. In particular, an author "list-item" reset wins.
    let reversed_list = resets.iter().any(|(name, _, reversed)|
        name == "list-item" && *reversed);
    if ordered && reversed_list {
        if let Some(start) = counter_attr(doc, id, "start") {
            if let Some((_, initial, _)) = resets.iter_mut()
                .find(|(name, _, reversed)| name == "list-item" && *reversed)
            {
                if initial.is_none() { *initial = Some(start.saturating_add(1)); }
            }
        }
    }
    for (name, value, reversed) in resets {
        let initial = value.unwrap_or_else(|| reversed_initial(doc, id, &name));
        counters.reset_direction(&name, initial, reversed);
        created.push(name);
    }
    // In a forward <ol>, HTML start supplies one less than the first marker.
    // For <ol reversed>, the starting value was set above to start + 1.
    if ordered && !reversed_list {
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
        let step = counters.implicit_list_step();
        counters.increment("list-item", step);
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
    for (name, value) in declarations(&counter_set_value(doc, id), 0) {
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
// Only a genuinely nested list must restore the surrounding list counter.
// Two separate sibling lists under body must keep the existing sibling-scope
// behavior; their counter model is a different (deferred) concern.
fn nested_in_list(doc: &BaseDocument, node: &Node) -> bool {
    let mut parent = node.parent;
    while let Some(id) = parent {
        let Some(ancestor) = doc.get_node(id) else { break };
        if ancestor.element_data().is_some_and(|el|
            matches!(&*el.name.local, "ol" | "ul"))
        {
            return true;
        }
        parent = ancestor.parent;
    }
    false
}

fn visit(doc: &BaseDocument, node: &Node, counters: &mut Counters,
         output: &mut Vec<Generated>) -> Vec<String> {
    let mut created = Vec::new();
    if let NodeData::Element(_) = &node.data {
        if style(doc, node.id, "display") == "none" { return created; }
        // A nested ordered list owns its own list-item instance. Unlike a
        // root-level sibling reset, this instance must not shadow the outer
        // list when a later outer item is visited.
        let outer_list_counter = counters.contains("list-item");
        created = enter(doc, node.id, counters);
        // ::marker uses the value of the list item itself. Descendant
        // increments (including those in nested divs) cannot renumber it.
        let marker_value = counters.current("list-item");
        let display = style(doc, node.id, "display");
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
        if display == "contents" {
            // A display:contents wrapper has no box boundary for its child
            // counters; let the parent's sibling scope retain them.
            created.extend(child_counters);
        } else {
            leave(counters, child_counters);
        }
        let list_style = style(doc, node.id, "list-style-type");
        if display.split_whitespace().any(|s| s == "list-item")
            && matches!(list_style.as_str(), "decimal" | "decimal-leading-zero"
                | "lower-alpha" | "upper-alpha" | "lower-roman" | "upper-roman")
        {
            let value = format_counter(marker_value, &list_style);
            let suffix = if matches!(list_style.as_str(), "disc" | "circle" | "square") {
                " "
            } else { ". " };
            output.push(Generated {
                id: node.id, suffix: None, text: css_quote(&format!("{value}{suffix}")),
            });
        }
        if outer_list_counter
            && node.element_data().is_some_and(|el| &*el.name.local == "ol")
            && nested_in_list(doc, node)
            && created.iter().any(|name| name == "list-item")
            && display != "contents"
        {
            // The inner ol has finished: its list-item counter must not
            // escape to the following outer li. Changes from its descendants
            // (including counter-set) were applied only to that instance.
            counters.pop("list-item");
            if let Some(index) = created.iter().position(|name| name == "list-item") {
                created.remove(index);
            }
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
    // Collect generated marker strings without painting or pixel comparisons.
    fn generated_for(html: &str) -> Vec<Generated> {
        let mut doc = blitz_html::HtmlDocument::from_html(
            html, blitz_dom::DocumentConfig::default());
        doc.resolve(0.0);
        let mut generated = Vec::new();
        visit(&doc, doc.root_node(), &mut Counters::default(), &mut generated);
        generated
    }

    #[test]
    fn marker_is_sampled_before_descendant_increment() {
        let generated = generated_for(r#"<ol><li>One
            <div style="counter-increment:list-item"></div></li>
            <li>Three</li></ol>"#);
        let markers: Vec<_> = generated.iter().filter(|g| g.suffix.is_none())
            .map(|g| g.text.as_str()).collect();
        assert_eq!(markers, [r#""1. ""#, r#""3. ""#]);
    }

    #[test]
    fn reversed_list_value_sets_direction_and_auto_start() {
        let generated = generated_for(r#"<ol reversed>
            <li>Seven</li><li value="6">Six</li><li>Five</li>
            </ol>"#);
        let markers: Vec<_> = generated.iter().filter(|g| g.suffix.is_none())
            .map(|g| g.text.as_str()).collect();
        assert_eq!(markers, [r#""7. ""#, r#""6. ""#, r#""5. ""#]);
    }

    #[test]
    fn contents_wrappers_preserve_child_counter_for_following_siblings() {
        let generated = generated_for(r#"<style>
            .counted{counter-increment:section}
            .counted::before{content:counter(section)}
            </style>
            <div style="display:contents"><div class="counted">One</div></div>
            <div style="display:contents"><div class="counted">Two</div></div>
            <div style="display:contents"><div class="counted">Three</div></div>"#);
        let values: Vec<_> = generated.iter()
            .filter(|g| g.suffix == Some("before"))
            .map(|g| g.text.as_str()).collect();
        assert_eq!(values, [r#""1""#, r#""2""#, r#""3""#]);
    }

    // Generated markers are produced after child traversal. Inspect them in
    // DOM order, as the WPT reference pages do, without invoking paint.
    fn marker_numbers(html: &str) -> Vec<i64> {
        let mut doc = blitz_html::HtmlDocument::from_html(
            html, blitz_dom::DocumentConfig::default());
        doc.resolve(0.0);
        let mut generated = Vec::new();
        visit(&doc, doc.root_node(), &mut Counters::default(), &mut generated);
        let markers: BTreeMap<NodeId, String> = generated.into_iter()
            .filter(|g| g.suffix.is_none())
            .map(|g| (g.id, g.text))
            .collect();
        fn walk(doc: &BaseDocument, node: &Node, markers: &BTreeMap<NodeId, String>,
                numbers: &mut Vec<i64>) {
            if let Some(text) = markers.get(&node.id) {
                let number = text.trim_matches('"').trim().trim_end_matches('.')
                    .parse::<i64>().expect("decimal list-item marker");
                numbers.push(number);
            }
            for &id in &node.children {
                if let Some(child) = doc.get_node(id) {
                    walk(doc, child, markers, numbers);
                }
            }
        }
        let mut numbers = Vec::new();
        walk(&doc, doc.root_node(), &markers, &mut numbers);
        numbers
    }

    #[test]
    fn reversed_024_author_reset_disables_reversal() {
        assert_eq!(marker_numbers(r#"<ol reversed style="counter-reset: list-item">
            <li>One
            <li>Two
            <li>Three
            </ol>"#), [1, 2, 3]);
    }

    #[test]
    fn reversed_025_author_initial_reset_disables_reversal() {
        assert_eq!(marker_numbers(r#"<ol reversed style="counter-reset: list-item 3">
            <li>Four
            <li>Five
            <li>Six
            </ol>"#), [4, 5, 6]);
    }

    #[test]
    fn reversed_014_nested_list_does_not_modify_outer_scope() {
        assert_eq!(marker_numbers(r#"<ol reversed>
            <li>Five
            <li style="counter-increment: list-item -3">Two
            <ol reversed>
                <li>Four
                <li style="counter-increment: list-item -2">Two
                <li>One
            </ol>
            <li>One
            </ol>"#), [5, 2, 4, 2, 1, 1]);
    }

    #[test]
    fn reversed_019_nested_list_set_does_not_modify_outer_scope() {
        assert_eq!(marker_numbers(r#"<ol reversed>
            <li>Three
            <div style="counter-increment: list-item -1"></div>
            <ol reversed>
                <li>Four
                <div style="counter-set: list-item 3"></div>
                <li>Two
            </ol>
            <li>One
            </ol>"#), [3, 4, 2, 1]);
    }

    #[test]
    fn list_item_005_nested_set_changes_only_inner_scope() {
        assert_eq!(marker_numbers(r#"<ol>
            <li>One
            <div style="counter-increment: list-item 1"></div>
            <ol>
                <li>One
                <div style="counter-set: list-item -3"></div>
                <li>Minus Two
            </ol>
            <li>Three
            </ol>"#), [1, 1, -2, 3]);
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
