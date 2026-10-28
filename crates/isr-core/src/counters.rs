// Copyright 2026 g1BSsQ. Licensed under Apache-2.0.
// Static CSS counter resolution for generated pseudo-element text.
// The second style/layout pass is deliberately script-free.
use blitz_dom::{BaseDocument, Node, NodeData, NodeId, UserAgentStylesheet};
use std::collections::{BTreeMap, BTreeSet};

// Track the originating element so ancestor scopes outlive local resets while
// sibling-created counters can be replaced at the next sibling.
#[derive(Clone, Copy)]
struct Counter { value: i64, reversed: bool, origin: Option<NodeId> }
#[derive(Clone, Default)]
struct Counters { values: BTreeMap<String, Vec<Counter>> }

impl Counters {
    fn current(&self, name: &str) -> i64 {
        self.values.get(name).and_then(|v| v.last()).map(|v| v.value).unwrap_or(0)
    }
    #[cfg(test)]
    fn reset(&mut self, name: &str, value: i64) {
        self.values.entry(name.to_owned()).or_default()
            .push(Counter { value, reversed: false, origin: None });
    }
    // Return whether this reset instantiated a new stack entry.
    fn reset_scoped(&mut self, doc: &BaseDocument, id: NodeId,
                    name: &str, value: i64, reversed: bool) -> bool {
        let previous = self.values.get(name).and_then(|v| v.last())
            .and_then(|v| v.origin);
        let parent = doc.get_node(id).and_then(|n| n.parent);
        if let Some(old) = previous {
            // A new reset replaces one originating at the same element or a
            // preceding sibling; it cannot shadow an inherited ancestor
            // counter for the siblings that follow.
            if old == id || (parent.is_some()
                && doc.get_node(old).and_then(|n| n.parent) == parent)
            {
                *self.values.get_mut(name).unwrap().last_mut().unwrap() =
                    Counter { value, reversed, origin: Some(id) };
                return false;
            }
        }
        self.values.entry(name.to_owned()).or_default()
            .push(Counter { value, reversed, origin: Some(id) });
        true
    }
    // Descendants can update an inherited counter's value, but cannot
    // extend the set of counter instances visible to their parent's siblings.
    fn restore_scope(&mut self, mut scope: Counters) {
        for (name, inherited) in &mut scope.values {
            if let Some(current) = self.values.get(name) {
                for counter in inherited {
                    if let Some(value) = current.iter().find(|v| v.origin == counter.origin) {
                        counter.value = value.value;
                    }
                }
            }
        }
        *self = scope;
    }
    // End a nested reversed list's counter at its boundary. A normal
    // list-item reset can still extend to a following sibling, while a
    // reversed nested list must not renumber the outer list's later items.
    fn finish_scope(&mut self, inherited: &Counters, id: NodeId) {
        for (name, previous) in &inherited.values {
            if let Some(current) = self.values.get_mut(name) {
                if current.len() > previous.len()
                    && current.last().is_some_and(|entry| {
                        entry.origin == Some(id)
                            && entry.reversed == (name == "list-item")
                    })
                {
                    current.truncate(previous.len());
                }
            }
        }
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
            .or_insert_with(|| vec![Counter { value: 0, reversed: false, origin: None }])
            .last_mut().unwrap().value = value;
    }
    fn increment(&mut self, name: &str, by: i64) {
        let next = self.current(name).saturating_add(by);
        self.set(name, next);
    }
    #[cfg(test)]
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
        "none" => String::new(),
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
fn html_integer(value: &str) -> Option<i64> {
    let text = value.trim_start_matches(|ch: char| ch.is_ascii_whitespace());
    let (sign, digits) = if let Some(rest) = text.strip_prefix('-') {
        (-1_i128, rest)
    } else if let Some(rest) = text.strip_prefix('+') {
        (1_i128, rest)
    } else { (1_i128, text) };
    let digits = digits.as_bytes().iter().take_while(|b| b.is_ascii_digit()).count();
    if digits == 0 { return None; }
    let magnitude = text.chars().skip(if sign == -1 || text.starts_with('+') { 1 } else { 0 })
        .take(digits).collect::<String>().parse::<i128>().ok()?;
    Some((magnitude * sign).clamp(i64::MIN as i128, i64::MAX as i128) as i64)
}
fn counter_attr(doc: &BaseDocument, id: NodeId, name: &str) -> Option<i64> {
    let node = doc.get_node(id)?;
    match &node.data {
        NodeData::Element(el) => el.attrs.iter()
            .find(|a| &*a.name.local == name)
            .and_then(|a| html_integer(a.value.as_str())),
        _ => None,
    }
}
// Only the author-reset name matters when applying HTML list attributes.
// Reset, increment, and set values are read from the computed style.
struct CounterContext {
    author_resets: BTreeMap<NodeId, BTreeSet<String>>,
}
impl CounterContext {
    fn new(doc: &BaseDocument) -> Self {
        let mut author_resets: BTreeMap<NodeId, BTreeSet<String>> = BTreeMap::new();
        for owner in doc.stylesheet_owner_nodes() {
            let count = doc.stylesheet_rule_count(owner, &[]).unwrap_or(0);
            for index in 0..count {
                let path = [index];
                let Some(info) = doc.stylesheet_rule_info(owner, &path) else { continue };
                if info.interface != "CSSStyleRule" { continue; }
                let Some((value, _)) = doc.stylesheet_rule_style_get_property(
                    owner, &path, "counter-reset"
                ) else { continue };
                let names: Vec<_> = reset_declarations(&value).into_iter()
                    .map(|(name, _, _)| name).collect();
                if names.is_empty() { continue; }
                if let Some((_, selector)) = info.attributes.iter()
                    .find(|(name, _)| *name == "selectorText")
                {
                    for part in selector.split(',') {
                        if let Ok(matches) = doc.query_selector_all(part.trim()) {
                            for id in matches {
                                author_resets.entry(id).or_default().extend(names.iter().cloned());
                            }
                        }
                    }
                }
            }
        }
        fn collect_inline(doc: &BaseDocument, node: &Node,
                          resets: &mut BTreeMap<NodeId, BTreeSet<String>>) {
            if let NodeData::Element(el) = &node.data {
                if let Some(style) = el.attrs.iter()
                    .find(|attr| &*attr.name.local == "style")
                    .map(|attr| attr.value.as_str())
                {
                    let value = doc.style_attr_get_property(style, "counter-reset");
                    resets.entry(node.id).or_default().extend(
                        reset_declarations(&value).into_iter().map(|(name, _, _)| name)
                    );
                }
            }
            for &child in &node.children {
                if let Some(child) = doc.get_node(child) {
                    collect_inline(doc, child, resets);
                }
            }
        }
        collect_inline(doc, doc.root_node(), &mut author_resets);
        Self { author_resets }
    }
    fn has_author_reset(&self, id: NodeId, name: &str) -> bool {
        self.author_resets.get(&id).is_some_and(|names| names.contains(name))
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
    if !matches!(style(doc, id, "display").as_str(), "contents")
        && id != creator && reset_declarations(&style(doc, id, "counter-reset"))
        .iter().any(|(reset_name, _, _)| reset_name == name)
    {
        return false;
    }

    let contents = style(doc, id, "display") == "contents";
    let increments = if contents { Vec::new() } else {
        declarations(&style(doc, id, "counter-increment"), 1)
    };
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
    let html_value = if !contents && name == "list-item"
        && node.element_data().is_some_and(|el| &*el.name.local == "li")
    {
        counter_attr(doc, id, "value")
    } else { None };
    let explicit_set = if contents { None } else {
        declarations(&style(doc, id, "counter-set"), 0)
            .into_iter().filter(|(n, _)| n == name).map(|(_, v)| v).last()
    };
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
            if reset_declarations(&style(doc, sibling, "counter-reset"))
                .iter().any(|(reset_name, _, _)| reset_name == name)
            {
                break;
            }
            if scan_reversed(doc, sibling, creator, name, &mut seed) { break; }
        }
    }
    seed.initial()
}

fn enter(doc: &BaseDocument, id: NodeId, counters: &mut Counters,
         ctx: &CounterContext) -> Vec<String> {
    let mut created = Vec::new();
    // A contents-only wrapper produces no box, so it cannot perform
    // counter operations; the traversal still processes its children.
    if style(doc, id, "display") == "contents" { return created; }
    let tag = doc.get_node(id).and_then(|node| match &node.data {
        NodeData::Element(el) => Some(el.name.local.to_string()),
        _ => None,
    });
    let ordered = tag.as_deref() == Some("ol");
    let mut resets = reset_declarations(&style(doc, id, "counter-reset"));
    // Direction comes only from the winning counter-reset entry, not from
    // ol[reversed] itself. In particular, an author "list-item" reset wins.
    let reversed_list = resets.iter().any(|(name, _, reversed)|
        name == "list-item" && *reversed);
    if ordered && reversed_list && !ctx.has_author_reset(id, "list-item") {
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
        if counters.reset_scoped(doc, id, &name, initial, reversed) {
            created.push(name);
        }
    }
    // In a forward <ol>, HTML start supplies one less than the first marker.
    // For <ol reversed>, the starting value was set above to start + 1.
    if ordered && !reversed_list && !ctx.has_author_reset(id, "list-item") {
        if let Some(start) = counter_attr(doc, id, "start") {
            if !counters.contains("list-item") {
                if counters.reset_scoped(doc, id, "list-item", 0, false) {
                    created.push("list-item".to_owned());
                }
            }
            counters.set("list-item", start.saturating_sub(1));
        }
    }
    let increments = declarations(&style(doc, id, "counter-increment"), 1);
    let has_list_item = increments.iter().any(|(name, _)| name == "list-item");
    for (name, value) in increments {
        if !counters.contains(&name) {
            if counters.reset_scoped(doc, id, &name, 0, false) {
                created.push(name.clone());
            }
        }
        counters.increment(&name, value);
    }
    // display:list-item, not the <li> tag, determines the implicit increment.
    if style(doc, id, "display").split_whitespace().any(|part| part == "list-item")
        && !has_list_item
    {
        if !counters.contains("list-item") {
            if counters.reset_scoped(doc, id, "list-item", 0, false) {
                created.push("list-item".to_owned());
            }
        }
        let step = counters.implicit_list_step();
        counters.increment("list-item", step);
    }
    // HTML li[value] behaves as a counter-set after the increment.
    if tag.as_deref() == Some("li") {
        if let Some(value) = counter_attr(doc, id, "value") {
            if !counters.contains("list-item") {
                if counters.reset_scoped(doc, id, "list-item", 0, false) {
                    created.push("list-item".to_owned());
                }
            }
            counters.set("list-item", value);
        }
    }
    // CSS Lists 3: reset -> increment -> set. Unscoped set creates a
    // counter belonging to the current sibling scope.
    for (name, value) in declarations(&style(doc, id, "counter-set"), 0) {
        if !counters.contains(&name) {
            if counters.reset_scoped(doc, id, &name, 0, false) {
                created.push(name.clone());
            }
        }
        counters.set(&name, value);
    }
    created
}
#[cfg(test)]
fn leave(counters: &mut Counters, names: Vec<String>) {
    for name in names.into_iter().rev() { counters.pop(&name); }
}
struct Generated { id: NodeId, suffix: Option<&'static str>, text: String }

fn visit_pseudo(doc: &BaseDocument, id: Option<NodeId>, suffix: &'static str,
    owner: NodeId, counters: &mut Counters, output: &mut Vec<Generated>,
    ctx: &CounterContext) {
    let Some(id) = id else { return };
    let inherited = counters.clone();
    let _created = enter(doc, id, counters, ctx);
    if let Some(text) = expand_content(&style(doc, id, "content"), counters, true) {
        output.push(Generated { id: owner, suffix: Some(suffix), text });
    }
    // A pseudo-element's new counter cannot replace the parent's instance
    // for the following real descendants. Preserve increments to inherited
    // counters, but discard pseudo-local resets.
    counters.restore_scope(inherited);
}
fn visit(doc: &BaseDocument, node: &Node, counters: &mut Counters,
         output: &mut Vec<Generated>, ctx: &CounterContext) {
    if let NodeData::Element(_) = &node.data {
        let display = style(doc, node.id, "display");
        if display == "none" { return; }
        let inherited = counters.clone();
        let _created = enter(doc, node.id, counters, ctx);
        let own_scope = counters.clone();
        let marker_value = counters.current("list-item");
        visit_pseudo(doc, node.before(), "before", node.id, counters, output, ctx);
        for &id in &node.children {
            if let Some(child) = doc.get_node(id) {
                visit(doc, child, counters, output, ctx);
            }
        }
        visit_pseudo(doc, node.after(), "after", node.id, counters, output, ctx);
        if display != "contents" {
            // Parent and previous-sibling instances survive; descendant-only
            // instances do not, even though their shared value updates do.
            counters.restore_scope(own_scope);
            counters.finish_scope(&inherited, node.id);
        }
        let list_style = style(doc, node.id, "list-style-type");
        if display.split_whitespace().any(|s| s == "list-item")
            && matches!(list_style.as_str(), "decimal" | "decimal-leading-zero"
                | "lower-alpha" | "upper-alpha" | "lower-roman" | "upper-roman")
        {
            let value = format_counter(marker_value, &list_style);
            output.push(Generated {
                id: node.id, suffix: None, text: css_quote(&format!("{value}. ")),
            });
        }
    } else {
        for &id in &node.children {
            if let Some(child) = doc.get_node(id) {
                visit(doc, child, counters, output, ctx);
            }
        }
    }
}

/// Convert counter functions to literal generated text using the computed
/// counter operations in document order, then ask the engine to shape/reflow
/// the resulting pseudo-elements in a second ordinary resolve pass.
pub(crate) fn materialize(doc: &mut BaseDocument) -> bool {
    let mut generated = Vec::new();
    let ctx = CounterContext::new(doc);
    visit(doc, doc.root_node(), &mut Counters::default(), &mut generated, &ctx);
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
            crate::document_config(412, None, None));
        doc.resolve(0.0);
        let mut counters = Counters::default();
        let ctx = CounterContext::new(&doc);
        let mut parent_scope = Vec::new();
        let mut labels = Vec::new();
        for id in ["r1", "a", "b", "r2", "c"] {
            let node = doc.get_element_by_id(id).unwrap();
            parent_scope.extend(enter(&doc, node, &mut counters, &ctx));
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
            crate::document_config(412, None, None));
        doc.resolve(0.0);
        let mut state = Counters::default();
        let node = doc.get_element_by_id("subject").unwrap();
        let ctx = CounterContext::new(&doc);
        let created = enter(&doc, node, &mut state, &ctx);
        assert_eq!(state.current("n"), 6);
        leave(&mut state, created);
        assert!(!state.contains("n"));
    }
    #[test]
    fn ordered_list_start_and_value_adjust_markers() {
        let mut doc = blitz_html::HtmlDocument::from_html(
            r#"<ol id="list" start="4"><li id="first">First</li>
            <li id="chosen" value="9">Chosen</li><li id="next">Next</li></ol>"#,
            crate::document_config(412, None, None));
        doc.resolve(0.0);
        let mut state = Counters::default();
        let ctx = CounterContext::new(&doc);
        let mut created = enter(&doc, doc.get_element_by_id("list").unwrap(), &mut state, &ctx);
        for (id, expected) in [("first", 4), ("chosen", 9), ("next", 10)] {
            created.extend(enter(&doc, doc.get_element_by_id(id).unwrap(), &mut state, &ctx));
            assert_eq!(state.current("list-item"), expected);
        }
        leave(&mut state, created);
    }
    // Collect generated marker strings without painting or pixel comparisons.
    fn generated_for(html: &str) -> Vec<Generated> {
        let mut doc = blitz_html::HtmlDocument::from_html(
            html, crate::document_config(412, None, None));
        doc.resolve(0.0);
        let mut generated = Vec::new();
        let ctx = CounterContext::new(&doc);
        visit(&doc, doc.root_node(), &mut Counters::default(), &mut generated, &ctx);
        generated
    }

    fn before_values(html: &str) -> Vec<String> {
        generated_for(html).into_iter()
            .filter(|item| item.suffix == Some("before"))
            .map(|item| item.text).collect()
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
            html, crate::document_config(412, None, None));
        doc.resolve(0.0);
        let mut generated = Vec::new();
        let ctx = CounterContext::new(&doc);
        visit(&doc, doc.root_node(), &mut Counters::default(), &mut generated, &ctx);
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
    fn reset_from_following_sibling_replaces_counter_instance() {
        // Unlike a hand-built stack, this covers DOM sibling scope and the
        // actual ::before-generated counters() values.
        let generated = generated_for(r#"<style>
            h2{counter-reset:sub}
            h3{counter-increment:sub}
            h3::before{content:counters(sub,".")}
            </style>
            <h2></h2><h3></h3><h3></h3><h2></h2><h3></h3>"#);
        let content: Vec<_> = generated.iter()
            .filter(|item| item.suffix == Some("before"))
            .map(|item| item.text.as_str()).collect();
        assert_eq!(content, [r#""1""#, r#""2""#, r#""1""#]);
    }

    #[test]
    fn computed_author_counter_reset_wins_by_specificity() {
        assert_eq!(marker_numbers(r#"<style>
            #l{counter-reset:list-item 10}
            ol{counter-reset:list-item 0}
            </style><ol id="l"><li>a<li>b</ol>"#), [11, 12]);
    }

    #[test]
    fn nonmatching_print_counter_reset_is_ignored() {
        assert_eq!(marker_numbers(r#"<style>
            @media print{body{color:red} ol{counter-reset:list-item 10}}
            </style><ol><li>a</ol>"#), [1]);
    }

    #[test]
    fn list_summary_has_zero_implicit_counter_increment() {
        assert_eq!(marker_numbers(
            "<ol><li>A</li><li><details><summary>S</summary>x</details></li><li>C</li></ol>"
        ), [1, 2, 3]);
    }

    #[test]
    fn html_list_integer_attributes_allow_space_and_trailing_text() {
        assert_eq!(marker_numbers(
            r#"<ol start="3 "><li>three<li value=" 7">seven<li value="7x">seven<li>eight</ol>"#
        ), [3, 7, 7, 8]);
    }

    #[test]
    fn author_counter_set_in_print_at_rule_does_not_apply_on_screen() {
        let generated = generated_for(r#"<style>
            p{counter-reset:n 7}
            @media print{a{} p{counter-set:n 5}}
            p::before{content:counter(n)}
            </style><p></p>"#);
        let values: Vec<_> = generated.iter()
            .filter(|item| item.suffix == Some("before"))
            .map(|item| item.text.as_str()).collect();
        assert_eq!(values, [r#""7""#]);
    }

    #[test]
    fn display_contents_skips_own_counter_operations() {
        let generated = generated_for(r#"<style>
            #outer{counter-reset:n 7}
            #ghost{display:contents;counter-reset:n 100;
                   counter-increment:n 50;counter-set:n 200}
            p::before{content:counter(n)}
            </style>
            <div id="outer"><div id="ghost"><p>Seven</p></div></div>"#);
        let values: Vec<_> = generated.iter()
            .filter(|item| item.suffix == Some("before"))
            .map(|item| item.text.as_str()).collect();
        assert_eq!(values, [r#""7""#]);
    }

    #[test]
    fn explicit_author_reversed_reset_ignores_start_attribute() {
        assert_eq!(marker_numbers(
            r#"<ol start="5" style="counter-reset:reversed(list-item)">
                <li>Three<li>Two<li>One</ol>"#), [3, 2, 1]);
    }

    #[test]
    fn counter_none_style_is_empty() {
        let mut counters = Counters::default();
        counters.reset("n", 19);
        assert_eq!(expand_content("counter(n, none)", &counters, false), Some(String::new()));
    }

    #[test]
    fn nested_counters_join_all_active_levels() {
        let mut state = Counters::default();
        state.reset("chapter", 1);
        state.reset("chapter", 2);
        state.increment("chapter", 1);
        assert_eq!(expand_content(r#"counters(chapter, ".")"#, &state, false).unwrap(), "1.3");
        assert_eq!(expand_content(r#"counters(chapter, ")")"#, &state, false).unwrap(), "1)3");
        assert_eq!(expand_content(r#"counter(chapter, decimal-leading-zero)"#, &state, false).unwrap(), "03");
        assert_eq!(format_counter(-5, "decimal-leading-zero"), "-5");
        assert_eq!(format_counter(27, "upper-alpha"), "AA");
        assert_eq!(format_counter(14, "lower-roman"), "xiv");
        assert_eq!(expand_content(r#""pre" counter(chapter) attr(data-x)"#, &state, true).unwrap(),
            r#""pre" "3" attr(data-x)"#);
    }
    #[test]
    fn scope_inherits_parent_and_preceding_sibling_value() {
        assert_eq!(before_values(r#"<style>p::before{content:counter(n)}</style>
            <div style="counter-reset:n 1">
              <p style="counter-increment:n"><span style="counter-increment:n"></span></p>
              <p style="counter-increment:n"></p>
            </div>"#), [r#""2""#, r#""4""#]);
    }
    #[test]
    fn sibling_reset_replaces_inner_counter_but_retains_ancestor() {
        // CSS 2.1 §12.4.1: a reset's scope includes following siblings and their descendants.
        assert_eq!(before_values(r#"<style>p::before{content:counters(n, ".")}</style>
            <div style="counter-reset:n 4">
              <div style="counter-reset:n 7"></div>
              <p style="counter-increment:n"></p>
              <div style="counter-reset:n 20"></div>
              <p style="counter-increment:n"></p>
            </div><p></p>"#), [r#""5""#, r#""6""#, r#""6""#]);
    }
    #[test]
    fn reset_increment_then_set_on_same_element() {
        assert_eq!(before_values(r#"<style>p::before{content:counter(n)}</style>
            <p style="counter-reset:n 4;counter-increment:n 3;counter-set:n 11"></p>"#),
            [r#""11""#]);
    }
    #[test]
    fn increment_and_set_each_instantiate_unscoped_counters() {
        assert_eq!(before_values(r#"<style>p::before{content:counter(n)}</style>
            <p style="counter-increment:n 2"></p><p></p>
            <p style="counter-set:n 9"></p><p style="counter-increment:n"></p>"#),
            [r#""2""#, r#""2""#, r#""9""#, r#""10""#]);
    }
    #[test]
    fn author_reset_of_another_name_does_not_disable_list_start() {
        assert_eq!(marker_numbers(
            r#"<ol start="5" style="counter-reset:chapter 8"><li>a<li>b</ol>"#
        ), [5, 6]);
    }
    #[test]
    fn display_none_does_not_change_counter_or_visit_children() {
        assert_eq!(before_values(r#"<style>p::before{content:counter(n)}</style>
            <div style="counter-reset:n 1"></div>
            <div style="display:none;counter-set:n 99">
              <span style="counter-increment:n 3"></span>
            </div><p></p>"#), [r#""1""#]);
    }
    #[test]
    fn computed_counter_set_obeys_cascade_and_pseudo_style() {
        assert_eq!(before_values(r#"<style>
            p{counter-set:n 3} #subject{counter-set:n 9}
            p::before{content:counter(n)}
            </style><p id="subject"></p>"#), [r#""9""#]);
        assert_eq!(before_values(r#"<style>
            p::before{counter-set:n 8;content:counter(n)}
            </style><p></p>"#), [r#""8""#]);
    }

    // The following cases mirror the counter text of the pinned reference
    // pages. They do not depend on rasterization or font metrics.
    fn scope_before(html: &str) -> Vec<String> {
        before_values(html)
    }
    fn scope_after(html: &str) -> Vec<String> {
        generated_for(html).into_iter()
            .filter(|item| item.suffix == Some("after"))
            .map(|item| item.text).collect()
    }
    fn counter_reset_sequence(kind: &str, reset: i64) -> Vec<String> {
        let html = format!(r#"<style>
            #run{{counter-reset:c}}
            span{{counter-increment:c}}
            span::before{{content:counter(c,{kind})}}
            </style><div id="run">
            <span></span><span style="counter-reset:c {reset}"></span>
            <span></span></div>"#);
        scope_before(&html)
    }
    fn counters_reset_sequence(kind: &str, reset: i64) -> Vec<String> {
        let html = format!(r#"<style>
            span::before{{content:counters(c,".",{kind})}}
            </style><div style="counter-reset:c 1">
            <span style="counter-reset:c;counter-increment:c"></span>
            <span style="counter-increment:c"></span>
            <span style="counter-reset:c {reset};counter-increment:c"></span>
            <span style="counter-increment:c"></span></div>"#);
        scope_before(&html)
    }
    #[test]
    fn wpt_counter_001_parent_instance_survives_inline_reset() {
        assert_eq!(counter_reset_sequence("decimal-leading-zero", 98),
                   [r#""01""#, r#""99""#, r#""02""#]);
    }
    #[test]
    fn wpt_counter_002_parent_instance_survives_roman_reset() {
        assert_eq!(counter_reset_sequence("lower-roman", 48),
                   [r#""i""#, r#""xlix""#, r#""ii""#]);
    }
    #[test]
    fn wpt_counter_003_parent_instance_survives_upper_roman_reset() {
        assert_eq!(counter_reset_sequence("upper-roman", 48),
                   [r#""I""#, r#""XLIX""#, r#""II""#]);
    }
    #[test]
    fn wpt_counters_001_ancestor_and_local_decimal_instances() {
        assert_eq!(counters_reset_sequence("decimal-leading-zero", 98),
                   [r#""01.01""#, r#""02""#, r#""02.99""#, r#""03""#]);
    }
    #[test]
    fn wpt_counters_002_ancestor_and_local_roman_instances() {
        assert_eq!(counters_reset_sequence("lower-roman", 48),
                   [r#""i.i""#, r#""ii""#, r#""ii.xlix""#, r#""iii""#]);
    }
    #[test]
    fn wpt_counters_003_ancestor_and_local_upper_roman_instances() {
        assert_eq!(counters_reset_sequence("upper-roman", 48),
                   [r#""I.I""#, r#""II""#, r#""II.XLIX""#, r#""III""#]);
    }
    #[test]
    fn wpt_counter_reset_increment_set_display_contents() {
        assert_eq!(scope_before(r#"<style>
            .inc{counter-increment:x}
            .contents{display:contents}
            .result::before{content:counter(x)}
            </style><div>
            <span style="counter-reset:x 6"></span>
            <span class="contents inc" style="counter-reset:x 666"></span>
            <span class="contents" style="counter-set:x 666"></span>
            <span class="inc result"></span></div>"#),
            [r#""7""#]);
    }
    #[test]
    fn wpt_li_value_reversed_009a_initials_and_nested_set() {
        assert_eq!(scope_before(r#"<style>
            ol::before,li::before{content:counters(list-item,".")}
            </style><ol reversed>
            <li style="counter-increment:list-item -2"></li>
            <li style="counter-increment:list-item -2;counter-set:list-item 5">
              <ol reversed>
                <li style="counter-increment:list-item -3"></li>
                <li style="counter-increment:list-item -1;counter-set:list-item 5"></li>
                <li style="counter-increment:list-item -8"></li>
              </ol>
            </li>
            <li style="counter-increment:list-item -2"></li></ol>"#),
            [r#""9""#,r#""7""#,r#""5""#,r#""5.9""#,
             r#""5.6""#,r#""5.5""#,r#""5.-3""#,r#""3""#]);
    }
    #[test]
    fn wpt_li_value_reversed_009b_zero_increments_before_set() {
        assert_eq!(scope_before(r#"<style>
            ol::before,li::before{content:counters(list-item,".")}
            .pure-set{counter-increment:list-item 0;counter-set:list-item 5}
            </style><ol reversed>
            <li style="counter-increment:list-item -2"></li>
            <div style="counter-increment:list-item 0"></div>
            <li class="pure-set"><ol reversed>
                <li style="counter-increment:list-item -3"></li>
                <div style="counter-increment:list-item -3"></div>
                <div style="counter-increment:list-item 1"></div>
                <div style="counter-increment:list-item 0"></div>
                <li class="pure-set"></li>
                <li style="counter-increment:list-item -8"></li>
            </ol></li>
            <div style="counter-increment:list-item 0"></div>
            <li style="counter-increment:list-item -2"></li>
            </ol>"#),
            [r#""9""#,r#""7""#,r#""5""#,r#""5.9""#,
             r#""5.6""#,r#""5.5""#,r#""5.-3""#,r#""3""#]);
    }

    // A reversed nested list followed by outer list items must resume the
    // outer counter, not the nested counter's final value.
    #[test]
    fn wpt_li_value_reversed_009a_nested_sibling_resumes_outer_list() {
        assert_eq!(scope_before(r#"<style>
            ol::before, li::before { content: counters(list-item, "."); }
            </style><ol reversed>
              <ol reversed>
                <li style="counter-increment:list-item 3;counter-set:list-item 5"></li>
                <li style="counter-increment:list-item 1"></li>
                <li style="counter-increment:list-item 8"></li>
              </ol>
              <li style="counter-increment:list-item 3"></li>
              <li style="counter-increment:list-item 1"></li>
              <li style="counter-increment:list-item 8;counter-set:list-item 5"></li>
            </ol>"#),
            [r#""-7""#, r#""-7.2""#, r#""-7.5""#, r#""-7.6""#,
             r#""-7.14""#, r#""-4""#, r#""-3""#, r#""5""#]);
    }

    #[test]
    fn wpt_li_value_reversed_009b_nested_sibling_resumes_outer_list() {
        assert_eq!(scope_before(r#"<style>
            ol::before, li::before { content: counters(list-item, "."); }
            .pure-set { counter-increment:list-item 0;counter-set:list-item 5; }
            </style><ol reversed>
              <ol reversed>
                <div style="counter-increment:list-item -1"></div>
                <div style="counter-increment:list-item 2"></div>
                <li class="pure-set"></li>
                <li style="counter-increment:list-item 1"></li>
                <li style="counter-increment:list-item 8"></li>
              </ol>
              <li style="counter-increment:list-item 3"></li>
              <li style="counter-increment:list-item 1"></li>
              <div style="counter-increment:list-item 4"></div>
              <li class="pure-set"></li>
            </ol>"#),
            [r#""-7""#, r#""-7.2""#, r#""-7.5""#, r#""-7.6""#,
             r#""-7.14""#, r#""-4""#, r#""-3""#, r#""5""#]);
    }

    #[test]
    fn wpt_counters_scope_001_pseudo_resets_are_local() {
        let html = r#"<style>
            .scope{counter-reset:c 1}
            .scope::before,.scope::after{content:counter(c)}
            .c::before{content:counter(c)}
            .one::before{counter-reset:c 2}
            .two{counter-reset:c 3}
            </style><div>
            <span class="scope"><span class="one c"><span class="c"></span></span>
              <span class="c"></span></span><span class="c"></span>
            <span class="scope"><span class="two c"><span class="c"></span></span>
              <span class="c"></span></span><span class="c"></span></div>"#;
        assert_eq!(scope_before(html),
            [r#""1""#,r#""2""#,r#""1""#,r#""1""#,r#""1""#,
             r#""1""#,r#""3""#,r#""3""#,r#""1""#,r#""1""#]);
        assert_eq!(scope_after(html), [r#""1""#,r#""1""#]);
    }
    #[test]
    fn wpt_counters_scope_002_pseudo_increments_in_tree_order() {
        let html = r#"<style>
            #scope,#reset{counter-reset:c 0}
            span::before{counter-increment:c;content:counters(c,".")}
            span::after{counter-increment:c;content:counters(c,".")}
            </style><div id="scope">
            <span><span><span id="reset"><span></span><span></span></span>
            <span><span></span></span></span></span></div>"#;
        assert_eq!(scope_before(html),
            [r#""1""#,r#""2""#,r#""2.1""#,r#""2.2""#,
             r#""2.4""#,r#""3""#,r#""4""#]);
        assert_eq!(scope_after(html),
            [r#""2.3""#,r#""2.5""#,r#""2.6""#,
             r#""5""#,r#""6""#,r#""7""#,r#""8""#]);
    }
    #[test]
    fn wpt_counters_scope_003_before_reset_does_not_shadow_body() {
        let html = r#"<style>
            body,#reset::before{counter-reset:c 0}
            span::before{counter-increment:c;content:counters(c,".")}
            span::after{counter-increment:c;content:counters(c,".")}
            </style><div><span><span id="reset"><span></span></span></span></div>"#;
        assert_eq!(scope_before(html), [r#""1""#,r#""1.1""#,r#""2""#]);
        assert_eq!(scope_after(html), [r#""3""#,r#""4""#,r#""5""#]);
    }
    #[test]
    fn wpt_counters_scope_004_reset_on_before_is_pseudo_local() {
        assert_eq!(scope_before(r#"<style>
            .reset{counter-reset:c}
            .rb::before{counter-reset:c;content:"R"}
            .use{counter-increment:c}
            .use::before{content:counters(c,".")}
            </style><div>
            <span class="reset"></span><span class="use"></span>
            <span class="reset"></span><span class="use"></span>
            <span class="rb"><span class="use"></span>
              <span class="reset"></span><span class="use"></span></span>
            </div>"#), [r#""1""#,r#""1""#,r#""2""#,r#""3""#]);
    }
    #[test]
    fn wpt_counter_set_001_computed_cascade_and_nested_scopes() {
        assert_eq!(scope_before(r#"<style>span::before{content:counters(n,".")}</style>
            <span style="counter-set:n 7"></span>
            <span style="counter-set:n"></span>
            <span style="counter-set:n 8 n 7"></span>
            <span style="counter-set:n 6;counter-increment:n 2"></span>
            <span style="counter-set:n;counter-increment:n 2"></span>
            <x style="counter-reset:n 9"><span style="counter-set:n 2"></span></x>
            <span style="counter-increment:n"></span>
            <x style="counter-reset:n 9">
              <span style="counter-set:n"></span>
              <span style="counter-set:n 2"></span>
              <x style="counter-reset:n 1"><span style="counter-set:n 5"></span></x>
            </x>
            <span style="counter-increment:n"></span>"#),
            [r#""7""#,r#""0""#,r#""7""#,r#""6""#,r#""0""#,
             r#""2""#,r#""3""#,r#""0""#,r#""2""#,r#""2.5""#,r#""3""#]);
    }

}
