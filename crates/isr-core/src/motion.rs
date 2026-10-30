// Copyright 2026 g1BSsQ. Licensed under Apache-2.0.
// A deliberately small, explicit-clock sampler for paint-only CSS motion.
use std::collections::HashMap;
use blitz_dom::{BaseDocument, Node, NodeData, NodeId};

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Sample {
    pub transform: Option<String>,
    pub opacity: Option<f64>,
}

#[derive(Clone, Debug)]
pub(crate) struct Frame {
    offset: f64,
    transform: Option<String>,
    opacity: Option<f64>,
}

#[derive(Clone, Debug)]
pub(crate) struct Track {
    pub node: NodeId,
    frames: Vec<Frame>,
    duration: f64,
    delay: f64,
    iterations: f64,
    direction: String,
    fill: String,
    ease: String,
    base: Sample,
}

#[derive(Clone, Debug)]
pub(crate) struct Transition {
    pub node: NodeId,
    pub property: String,
    pub from: String,
    pub to: String,
    pub start: f64,
    pub duration: f64,
    pub delay: f64,
    pub ease: String,
}

fn balanced_end(source: &str, open: usize) -> Option<usize> {
    let mut depth = 0;
    for (offset, c) in source[open..].char_indices() {
        if c == '{' { depth += 1; }
        if c == '}' {
            depth -= 1;
            if depth == 0 { return Some(open + offset); }
        }
    }
    None
}

fn declarations(css: &str) -> (Option<String>, Option<f64>) {
    let mut transform = None;
    let mut opacity = None;
    for entry in css.split(';') {
        let Some((key, value)) = entry.split_once(':') else { continue };
        let value = value.trim();
        match key.trim().to_ascii_lowercase().as_str() {
            "transform" => transform = Some(value.to_owned()),
            "opacity" => opacity = value.parse::<f64>().ok().map(|v| v.clamp(0.0, 1.0)),
            _ => {}
        }
    }
    (transform, opacity)
}

fn parse_frames(body: &str) -> Vec<Frame> {
    let mut frames = Vec::new();
    let mut at = 0;
    while let Some(rel) = body[at..].find('{') {
        let open = at + rel;
        let Some(close) = balanced_end(body, open) else { break };
        let labels = &body[at..open];
        let (transform, opacity) = declarations(&body[open + 1..close]);
        for label in labels.split(',') {
            let key = label.trim();
            let offset = match key {
                "from" => Some(0.0),
                "to" => Some(1.0),
                _ => key.strip_suffix('%').and_then(|s| s.trim().parse::<f64>().ok())
                    .map(|v| v / 100.0).filter(|v| (0.0..=1.0).contains(v)),
            };
            if let Some(offset) = offset {
                frames.push(Frame { offset, transform: transform.clone(), opacity });
            }
        }
        at = close + 1;
    }
    frames.sort_by(|a, b| a.offset.total_cmp(&b.offset));
    frames
}

/// The stylesheet's keyframe names and declarations are read without altering
/// the one-shot stylesheet or resolving a new layout.
pub(crate) fn keyframes(html: &str) -> HashMap<String, Vec<Frame>> {
    let mut result = HashMap::new();
    let mut lower = html.to_ascii_lowercase();
    let mut offset = 0;
    while let Some(open_rel) = lower[offset..].find("<style") {
        let start = offset + open_rel;
        let Some(tag_end_rel) = lower[start..].find('>') else { break };
        let text_start = start + tag_end_rel + 1;
        let Some(end_rel) = lower[text_start..].find("</style") else { break };
        let text_end = text_start + end_rel;
        let css = &html[text_start..text_end];
        let css_lower = css.to_ascii_lowercase();
        let mut pos = 0;
        while let Some(index_rel) = css_lower[pos..].find("@keyframes") {
            let index = pos + index_rel + "@keyframes".len();
            let Some(open_rel) = css[index..].find('{') else { break };
            let open = index + open_rel;
            let name = css[index..open].trim();
            let Some(close) = balanced_end(css, open) else { break };
            if !name.is_empty() {
                result.insert(name.to_owned(), parse_frames(&css[open + 1..close]));
            }
            pos = close + 1;
        }
        offset = text_end + "</style".len();
        if offset >= lower.len() { break; }
    }
    lower.clear();
    result
}

pub(crate) fn csv(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0;
    let mut start = 0;
    for (i, c) in s.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            ',' if depth == 0 => {
                out.push(s[start..i].trim().to_owned());
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(s[start..].trim().to_owned());
    out
}

fn nth(list: &[String], index: usize, fallback: &str) -> String {
    if list.is_empty() { fallback.to_owned() } else { list[index % list.len()].clone() }
}

pub(crate) fn milliseconds(raw: &str) -> f64 {
    let raw = raw.trim();
    if let Some(ms) = raw.strip_suffix("ms") {
        ms.trim().parse::<f64>().unwrap_or(0.0)
    } else if let Some(s) = raw.strip_suffix('s') {
        s.trim().parse::<f64>().unwrap_or(0.0) * 1000.0
    } else { 0.0 }
}

pub(crate) fn read_tracks(doc: &BaseDocument, definitions: &HashMap<String, Vec<Frame>>) -> Vec<Track> {
    fn visit(doc: &BaseDocument, node: &Node, definitions: &HashMap<String, Vec<Frame>>, tracks: &mut Vec<Track>) {
        if matches!(&node.data, NodeData::Element(_)) && node.primary_styles().is_some() {
            let val = |property| doc.resolved_style_value(node.id, property);
            let names = csv(&val("animation-name"));
            let durations = csv(&val("animation-duration"));
            let delays = csv(&val("animation-delay"));
            let counts = csv(&val("animation-iteration-count"));
            let directions = csv(&val("animation-direction"));
            let fills = csv(&val("animation-fill-mode"));
            let eases = csv(&val("animation-timing-function"));
            let base = Sample {
                transform: Some(val("transform")),
                opacity: val("opacity").parse().ok(),
            };
            for (index, name) in names.iter().enumerate() {
                let Some(frames) = definitions.get(name).filter(|v| !v.is_empty()) else { continue };
                let duration = milliseconds(&nth(&durations, index, "0s")).max(0.0);
                let count = nth(&counts, index, "1");
                let iterations = if count == "infinite" { f64::INFINITY }
                    else { count.parse::<f64>().unwrap_or(1.0).max(0.0) };
                tracks.push(Track {
                    node: node.id, frames: frames.clone(), duration,
                    delay: milliseconds(&nth(&delays, index, "0s")),
                    iterations,
                    direction: nth(&directions, index, "normal"),
                    fill: nth(&fills, index, "none"),
                    ease: nth(&eases, index, "ease"),
                    base: base.clone(),
                });
            }
        }
        for &child in &node.children {
            if let Some(child) = doc.get_node(child) { visit(doc, child, definitions, tracks); }
        }
    }
    let mut tracks = Vec::new();
    visit(doc, doc.root_node(), definitions, &mut tracks);
    tracks
}

fn bezier_coord(t: f64, p1: f64, p2: f64) -> f64 {
    let inv = 1.0 - t;
    3.0 * inv * inv * t * p1 + 3.0 * inv * t * t * p2 + t * t * t
}

/// CSS timing functions are evaluated in progress space, not wall-clock space.
pub(crate) fn ease(function: &str, t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    let fn_name = function.trim();
    if fn_name.starts_with("steps(") && fn_name.ends_with(')') {
        let args = csv(&fn_name[6..fn_name.len() - 1]);
        let n = args.first().and_then(|x| x.parse::<f64>().ok()).unwrap_or(1.0).max(1.0);
        let pos = args.get(1).map(|s| s.trim()).unwrap_or("jump-end");
        // CSS Easing steps() changes value at the exact jump boundary.
        // Serializing a time to seconds can put an exact quarter-frame a
        // few ulps below its mathematical boundary, so snap only near integers.
        let scaled = t * n;
        let boundary = scaled.round();
        let scaled = if (scaled - boundary).abs() <= 1e-9 {
            boundary
        } else { scaled };
        let floor = scaled.floor();
        let output = match pos {
            "start" | "jump-start" => (floor + 1.0) / n,
            "jump-none" if n > 1.0 => floor.min(n - 1.0) / (n - 1.0),
            "jump-both" => (floor + 1.0) / (n + 1.0),
            _ => floor / n, // omitted position is jump-end
        };
        // The final sample is always the endpoint; jump-end explicitly
        // includes the jump at input progress 1.
        return if t == 1.0 { 1.0 } else { output.clamp(0.0, 1.0) };
    }
    if t == 0.0 || t == 1.0 { return t; }
    let points = match fn_name {
        "linear" => return t,
        "ease" => Some([0.25, 0.1, 0.25, 1.0]),
        "ease-in" => Some([0.42, 0.0, 1.0, 1.0]),
        "ease-out" => Some([0.0, 0.0, 0.58, 1.0]),
        "ease-in-out" => Some([0.42, 0.0, 0.58, 1.0]),
        _ => {
            fn_name.strip_prefix("cubic-bezier(").and_then(|s| s.strip_suffix(')')).and_then(|s| {
                let nums: Vec<_> = csv(s).iter().filter_map(|x| x.parse::<f64>().ok()).collect();
                (nums.len() == 4 && (0.0..=1.0).contains(&nums[0])
                    && (0.0..=1.0).contains(&nums[2]))
                    .then(|| [nums[0], nums[1], nums[2], nums[3]])
            })
        }
    };
    let Some([x1, y1, x2, y2]) = points else { return t };
    let (mut lo, mut hi) = (0.0, 1.0);
    for _ in 0..26 {
        let mid = (lo + hi) / 2.0;
        if bezier_coord(mid, x1, x2) < t { lo = mid; } else { hi = mid; }
    }
    bezier_coord((lo + hi) / 2.0, y1, y2)
}

fn functions(s: &str) -> Option<Vec<(String, Vec<(f64, String)>)>> {
    if s.trim() == "none" { return Some(Vec::new()); }
    let mut out = Vec::new();
    let mut rest = s.trim();
    while !rest.is_empty() {
        let open = rest.find('(')?;
        let close = rest.find(')')?;
        if close < open { return None; }
        let name = rest[..open].trim().to_ascii_lowercase();
        if !matches!(name.as_str(), "translate" | "translatex" | "translatey"
            | "scale" | "scalex" | "scaley" | "rotate" | "matrix") { return None; }
        let args = rest[open + 1..close].replace(',', " ");
        let args: Vec<_> = args.split_whitespace().map(|token| {
            let index = token.find(|c: char| c.is_ascii_alphabetic() || c == '%')
                .unwrap_or(token.len());
            let number = token[..index].parse::<f64>().ok()?;
            Some((number, token[index..].to_owned()))
        }).collect::<Option<_>>()?;
        if args.is_empty() { return None; }
        out.push((name, args));
        rest = rest[close + 1..].trim();
    }
    Some(out)
}

fn identity(name: &str, args: &[(f64, String)]) -> Vec<(f64, String)> {
    args.iter().enumerate().map(|(index, (_, unit))| {
        (if name.starts_with("scale") || (name == "matrix" && (index == 0 || index == 3)) {
            1.0
        } else { 0.0 }, unit.clone())
    }).collect()
}

/// Matching transform function lists interpolate component-by-component.
/// Nonmatching lists use a discrete step at 50%, rather than matrix decomposition.
pub(crate) fn interpolate_transform(a: &str, b: &str, p: f64) -> String {
    let (Some(mut left), Some(mut right)) = (functions(a), functions(b)) else {
        return if p < 0.5 { a } else { b }.to_owned()
    };
    if left.is_empty() && !right.is_empty() {
        left = right.iter().map(|(name, args)| (name.clone(), identity(name, args))).collect();
    }
    if right.is_empty() && !left.is_empty() {
        right = left.iter().map(|(name, args)| (name.clone(), identity(name, args))).collect();
    }
    if left.len() != right.len() || left.iter().zip(&right).any(|((an, aa), (bn, ba))| {
        an != bn || aa.len() != ba.len()
            || aa.iter().zip(ba).any(|((_, au), (_, bu))| au != bu)
    }) {
        return if p < 0.5 { a } else { b }.to_owned();
    }
    if left.is_empty() { return "none".to_owned(); }
    left.iter().zip(right).map(|((name, aa), (_, bb))| {
        let components: Vec<_> = aa.iter().zip(bb).map(|((av, unit), (bv, _))| {
            format!("{}{}", av + (bv - av) * p, unit)
        }).collect();
        format!("{}({})", name, components.join(", "))
    }).collect::<Vec<_>>().join(" ")
}

fn at(frames: &[Frame], base: &Sample, progress: f64, easing: &str) -> Sample {
    let mut completed = Vec::with_capacity(frames.len() + 2);
    if !frames.iter().any(|f| f.offset == 0.0) {
        completed.push(Frame {offset: 0.0, transform: base.transform.clone(), opacity: base.opacity});
    }
    completed.extend(frames.iter().cloned());
    if !frames.iter().any(|f| f.offset == 1.0) {
        completed.push(Frame {offset: 1.0, transform: base.transform.clone(), opacity: base.opacity});
    }
    let index = completed.partition_point(|f| f.offset < progress)
        .min(completed.len().saturating_sub(1));
    let right = &completed[index];
    let left = if index == 0 { right } else { &completed[index - 1] };
    let interval = right.offset - left.offset;
    let fraction = if interval <= 0.0 { 1.0 } else {
        ease(easing, (progress - left.offset) / interval)
    };
    let opacity = match (left.opacity.or(base.opacity), right.opacity.or(base.opacity)) {
        (Some(a), Some(b)) => Some((a + (b - a) * fraction).clamp(0.0, 1.0)),
        _ => None,
    };
    let transform = match (left.transform.as_deref().or(base.transform.as_deref()),
                            right.transform.as_deref().or(base.transform.as_deref())) {
        (Some(a), Some(b)) => Some(interpolate_transform(a, b, fraction)),
        _ => None,
    };
    Sample { transform, opacity }
}

impl Track {
    pub(crate) fn sample(&self, now: f64, reduced: bool) -> (Option<Sample>, bool) {
        if reduced {
            let reversed = matches!(self.direction.as_str(), "reverse" | "alternate-reverse");
            return (Some(at(&self.frames, &self.base,
                if reversed { 0.0 } else { 1.0 }, &self.ease)), false);
        }
        let elapsed = now - self.delay;
        let backwards = matches!(self.fill.as_str(), "backwards" | "both");
        let forwards = matches!(self.fill.as_str(), "forwards" | "both");
        if elapsed < 0.0 {
            let first_reverse = matches!(self.direction.as_str(), "reverse" | "alternate-reverse");
            return (backwards.then(|| at(&self.frames, &self.base,
                if first_reverse { 1.0 } else { 0.0 }, &self.ease)), true);
        }
        let total = self.duration * self.iterations;
        let ended = self.duration == 0.0 || self.iterations == 0.0
            || (total.is_finite() && elapsed >= total);
        if ended && !forwards { return (None, false); }
        let whole = if ended { self.iterations.max(0.0) }
            else { elapsed / self.duration };
        let index = if ended && whole.fract() == 0.0 {
            (whole - 1.0).max(0.0).floor()
        } else { whole.floor() };
        let fraction = if ended && whole.fract() == 0.0 { 1.0 } else { whole.fract() };
        let odd_iteration = index.rem_euclid(2.0) >= 1.0;
        let reverse = match self.direction.as_str() {
            "reverse" => true,
            "alternate" => odd_iteration,
            "alternate-reverse" => !odd_iteration,
            _ => false,
        };
        // Reverse directed progress first; apply the easing function to
        // that progress, including its step boundary behavior.
        let directed_progress = if reverse { 1.0 - fraction } else { fraction };
        (Some(at(&self.frames, &self.base, directed_progress, &self.ease)), !ended)
    }
}

impl Transition {
    pub(crate) fn sample(&self, now: f64, reduced: bool) -> (String, bool) {
        if reduced { return (self.to.clone(), false); }
        let time = now - self.start - self.delay;
        let progress = if self.duration <= 0.0 { 1.0 }
            else { (time / self.duration).clamp(0.0, 1.0) };
        let progress = ease(&self.ease, progress);
        let value = if self.property == "opacity" {
            match (self.from.parse::<f64>(), self.to.parse::<f64>()) {
                (Ok(a), Ok(b)) => format!("{}", a + (b - a) * progress),
                _ => self.to.clone()
            }
        } else { interpolate_transform(&self.from, &self.to, progress) };
        (value, time < self.duration)
    }
}

/// Replaces only the preference condition, preserving surrounding media
/// expressions, selector order and author cascade order. The bundled style
/// dependency treats this feature as unknown; width conditions are supported.
pub(crate) fn reduced_media_css(source: &str, reduced: bool) -> String {
    let mut result = String::with_capacity(source.len());
    let mut pos = 0;
    while let Some(rel) = source[pos..].find('(') {
        let open = pos + rel;
        let Some(end_rel) = source[open + 1..].find(')') else { break };
        let end = open + 1 + end_rel;
        let condition = source[open + 1..end].trim().to_ascii_lowercase();
        let media = condition.split_once(':').map(|(name, value)|
            (name.trim(), value.trim()));
        let enabled = match media {
            Some(("prefers-reduced-motion", "reduce")) => Some(reduced),
            Some(("prefers-reduced-motion", "no-preference")) => Some(!reduced),
            None if condition == "prefers-reduced-motion" => Some(reduced),
            _ => None,
        };
        result.push_str(&source[pos..open]);
        if let Some(enabled) = enabled {
            result.push_str(if enabled { "(min-width: 0px)" }
                else { "(min-width: 999999999px)" });
        } else {
            result.push_str(&source[open..=end]);
        }
        pos = end + 1;
    }
    result.push_str(&source[pos..]);
    result
}

/// Original stylesheet texts in source order, before preference substitution.
pub(crate) fn style_texts(html: &str) -> Vec<String> {
    let lower = html.to_ascii_lowercase();
    let mut styles = Vec::new();
    let mut pos = 0;
    while let Some(rel) = lower[pos..].find("<style") {
        let start = pos + rel;
        let Some(open_rel) = lower[start..].find('>') else { break };
        let begin = start + open_rel + 1;
        let Some(close_rel) = lower[begin..].find("</style") else { break };
        let end = begin + close_rel;
        styles.push(html[begin..end].to_owned());
        pos = end + 7;
    }
    styles
}

/// Style element IDs in DOM order (including nested or body style elements).
pub(crate) fn style_nodes(doc: &BaseDocument) -> Vec<NodeId> {
    fn walk(doc: &BaseDocument, node: &Node, result: &mut Vec<NodeId>) {
        if matches!(&node.data, NodeData::Element(_)) && node.local_name() == "style" {
            result.push(node.id);
        }
        for &id in &node.children {
            if let Some(child) = doc.get_node(id) { walk(doc, child, result); }
        }
    }
    let mut result = Vec::new();
    walk(doc, doc.root_node(), &mut result);
    result
}

fn css_words(value: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut depth = 0;
    let mut start = None;
    for (i, c) in value.char_indices() {
        if c == '(' { depth += 1; }
        if c == ')' { depth -= 1; }
        if c.is_whitespace() && depth == 0 {
            if let Some(begin) = start.take() {
                words.push(value[begin..i].to_owned());
            }
        } else if start.is_none() {
            start = Some(i);
        }
    }
    if let Some(begin) = start { words.push(value[begin..].to_owned()); }
    words
}

#[derive(Default)]
struct TransitionProperties {
    properties: Vec<String>,
    durations: Vec<String>,
    delays: Vec<String>,
    eases: Vec<String>,
}

fn from_shorthand(value: &str) -> TransitionProperties {
    let mut config = TransitionProperties::default();
    for transition in csv(value) {
        let mut property = "all".to_owned();
        let mut duration = "0s".to_owned();
        let mut delay = "0s".to_owned();
        let mut timing = "ease".to_owned();
        let mut has_duration = false;
        for word in css_words(&transition) {
            if word.ends_with("ms") || word.ends_with('s')
                && word.trim_end_matches('s').parse::<f64>().is_ok()
            {
                if !has_duration { duration = word; has_duration = true; }
                else { delay = word; }
            } else if word == "linear" || word == "ease"
                || word == "ease-in" || word == "ease-out"
                || word == "ease-in-out" || word.starts_with("steps(")
                || word.starts_with("cubic-bezier(") {
                timing = word;
            } else if word == "opacity" || word == "transform" || word == "all" {
                property = word;
            }
        }
        config.properties.push(property);
        config.durations.push(duration);
        config.delays.push(delay);
        config.eases.push(timing);
    }
    config
}

fn selector_matches(selector: &str, node: &Node) -> bool {
    let id = node.attr(blitz_dom::local_name!("id")).unwrap_or("");
    let classes = node.attr(blitz_dom::local_name!("class")).unwrap_or("");
    selector.split(',').any(|selector| {
        let last = selector.trim().split_whitespace().last().unwrap_or("");
        if last.is_empty() { return false; }
        let mut found_constraint = false;
        if let Some(start) = last.find('#') {
            let ident: String = last[start + 1..].chars()
                .take_while(|c| c.is_alphanumeric() || *c == '-' || *c == '_').collect();
            if ident.is_empty() || ident != id { return false; }
            found_constraint = true;
        }
        for part in last.split('.').skip(1) {
            let name: String = part.chars()
                .take_while(|c| c.is_alphanumeric() || *c == '-' || *c == '_').collect();
            if !name.is_empty() {
                if !classes.split_whitespace().any(|class| class == name) { return false; }
                found_constraint = true;
            }
        }
        if found_constraint { return true; }
        last == "*" || last.eq_ignore_ascii_case(node.local_name())
    })
}

fn transition_fallback(doc: &BaseDocument, id: NodeId, sources: &[String]) -> Option<TransitionProperties> {
    let node = doc.get_node(id)?;
    fn scan(css: &str, node: &Node, found: &mut Option<TransitionProperties>) {
        let mut pos = 0;
        while let Some(offset) = css[pos..].find('{') {
            let open = pos + offset;
            let Some(close) = balanced_end(css, open) else { break };
            let selector = css[pos..open].trim();
            let body = &css[open + 1..close];
            if selector.starts_with("@media") || selector.starts_with("@supports") {
                scan(body, node, found);
            } else if !selector.starts_with('@') && selector_matches(selector, node) {
                let mut config = found.take().unwrap_or_default();
                for entry in body.split(';') {
                    let Some((property, value)) = entry.split_once(':') else { continue };
                    match property.trim().to_ascii_lowercase().as_str() {
                        "transition" => config = from_shorthand(value.trim()),
                        "transition-property" => config.properties = csv(value.trim()),
                        "transition-duration" => config.durations = csv(value.trim()),
                        "transition-delay" => config.delays = csv(value.trim()),
                        "transition-timing-function" => config.eases = csv(value.trim()),
                        _ => {}
                    }
                }
                *found = Some(config);
            }
            pos = close + 1;
        }
    }
    let mut result = None;
    for css in sources { scan(css, node, &mut result); }
    result
}

pub(crate) fn new_transitions(
    doc: &BaseDocument, node: NodeId, old: &Sample, now: f64, styles: &[String],
) -> Vec<Transition> {
    let val = |name| doc.resolved_style_value(node, name);
    let mut properties = csv(&val("transition-property"));
    let mut durations = csv(&val("transition-duration"));
    let mut delays = csv(&val("transition-delay"));
    let mut eases = csv(&val("transition-timing-function"));
    // Transition-start callbacks in the style dependency are not connected.
    // The computed before/after values are authoritative; a matching source
    // declaration supplies the timing when shorthand computed timing is absent.
    if durations.iter().all(|value| milliseconds(value) <= 0.0) {
        if let Some(fallback) = transition_fallback(doc, node, styles) {
            properties = fallback.properties;
            durations = fallback.durations;
            delays = fallback.delays;
            eases = fallback.eases;
        }
    }
    let end = Sample {
        transform: Some(val("transform")),
        opacity: val("opacity").parse().ok(),
    };
    let mut transitions = Vec::new();
    for (name, from, to) in [
        ("transform", old.transform.clone(), end.transform.clone()),
        ("opacity", old.opacity.map(|x| x.to_string()), end.opacity.map(|x| x.to_string())),
    ] {
        let Some(position) = properties.iter().position(|p| p == name || p == "all") else { continue };
        let (Some(from), Some(to)) = (from, to) else { continue };
        let duration = milliseconds(&nth(&durations, position, "0s"));
        if from == to || duration <= 0.0 { continue; }
        transitions.push(Transition {
            node, property: name.into(), from, to, start: now,
            duration, delay: milliseconds(&nth(&delays, position, "0s")),
            ease: nth(&eases, position, "ease"),
        });
    }
    transitions
}

pub(crate) fn computed_sample(doc: &BaseDocument, id: NodeId) -> Sample {
    Sample {
        transform: Some(doc.resolved_style_value(id, "transform")),
        opacity: doc.resolved_style_value(id, "opacity").parse().ok(),
    }
}
