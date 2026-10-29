// Copyright 2026 g1BSsQ. Licensed under Apache-2.0.
// CSSOM export bridges layoutless table parts and SVG descendants.
use blitz_dom::{BaseDocument, Node, NodeData};
use peniko::kurbo::{Affine, BezPath, Rect, Shape as _};
use parley::{BreakReason, PositionedLayoutItem};

#[derive(Clone, Copy, Debug)]
pub(crate) struct BoxRect { pub x: f64, pub y: f64, pub width: f64, pub height: f64 }
impl BoxRect {
    fn union(self, other: Self) -> Self {
        let x = self.x.min(other.x);
        let y = self.y.min(other.y);
        Self { x, y,
            width: (self.x + self.width).max(other.x + other.width) - x,
            height: (self.y + self.height).max(other.y + other.height) - y }
    }
}
fn native_box(doc: &BaseDocument, node: &Node) -> Option<BoxRect> {
    doc.get_client_bounding_rect(node.id)
        .map(|b| BoxRect { x: b.x, y: b.y, width: b.width, height: b.height })
}
fn tag(node: &Node) -> Option<&str> {
    match &node.data { NodeData::Element(d) => Some(&*d.name.local), _ => None }
}
fn attr<'a>(node: &'a Node, name: &str) -> Option<&'a str> {
    match &node.data {
        NodeData::Element(d) => d.attrs.iter()
            .find(|a| &*a.name.local == name).map(|a| a.value.as_str()),
        _ => None,
    }
}
fn number(node: &Node, name: &str, default: f64) -> f64 {
    attr(node, name).and_then(|s| s.parse().ok()).unwrap_or(default)
}
fn descendant_cells(doc: &BaseDocument, node: &Node) -> Option<BoxRect> {
    let mut bounds: Option<BoxRect> = None;
    for id in &node.children {
        if let Some(child) = doc.get_node(*id) {
            let b = if matches!(tag(child), Some("td" | "th")) {
                native_box(doc, child)
            } else if matches!(tag(child), Some("tr" | "thead" | "tbody" | "tfoot")) {
                descendant_cells(doc, child)
            } else { None };
            if let Some(b) = b.filter(|b| b.width > 0.0 && b.height > 0.0) {
                bounds = Some(bounds.map_or(b, |prev| prev.union(b)));
            }
        }
    }
    bounds
}
fn svg_shape(doc: &BaseDocument, node: &Node) -> Option<BoxRect> {
    let raw = match tag(node)? {
        "circle" => {
            let r = number(node, "r", 0.0).max(0.0);
            let cx = number(node, "cx", 0.0);
            let cy = number(node, "cy", 0.0);
            if r <= 0.0 { return None; }
            Rect::new(cx-r, cy-r, cx+r, cy+r)
        }
        "path" => BezPath::from_svg(attr(node, "d")?).ok()?.bounding_box(),
        _ => return None,
    };
    let mut parent = node.parent;
    while let Some(id) = parent {
        let p = doc.get_node(id)?;
        if tag(p) == Some("svg") {
            let viewport = native_box(doc, p)?;
            if viewport.width <= 0.0 || viewport.height <= 0.0 { return None; }
            let numbers: Vec<f64> = attr(p, "viewBox")
                .or_else(|| attr(p, "viewbox"))
                .unwrap_or("")
                .split(|c: char| c == ',' || c.is_ascii_whitespace())
                .filter(|s| !s.is_empty())
                .filter_map(|s| s.parse().ok()).collect();
            let (vx, vy, vw, vh) = if numbers.len() == 4 && numbers[2] > 0.0 && numbers[3] > 0.0 {
                (numbers[0], numbers[1], numbers[2], numbers[3])
            } else { (0.0, 0.0, viewport.width, viewport.height) };
            // Default preserveAspectRatio="xMidYMid meet".
            let sx = viewport.width / vw;
            let sy = viewport.height / vh;
            let scale = sx.min(sy);
            let ox = viewport.x + (viewport.width - vw * scale) / 2.0 - vx * scale;
            let oy = viewport.y + (viewport.height - vh * scale) / 2.0 - vy * scale;
            return Some(BoxRect { x: ox + raw.x0 * scale, y: oy + raw.y0 * scale,
                width: raw.width() * scale, height: raw.height() * scale });
        }
        parent = p.parent;
    }
    None
}

// The initial closed layout has no boxes for the hidden children. The caller
// separately measures them with details temporarily opened, while retaining
// the original closed layout and capture height.
pub(crate) fn hidden_in_details(doc: &BaseDocument, node: &Node) -> bool {
    let mut current = node;
    while let Some(parent_id) = current.parent {
        let Some(parent) = doc.get_node(parent_id) else { break };
        if tag(parent) == Some("details") && attr(parent, "open").is_none() {
            let first_summary = parent.children.iter().copied().find(|&id|
                doc.get_node(id).is_some_and(|n| tag(n) == Some("summary")));
            if first_summary != Some(current.id) { return true; }
        }
        current = parent;
    }
    false
}

// A <br> is flattened to an explicit Parley line break and therefore has no
// Taffy border box. Map it back to the line it terminates. The line already
// contains the correct shaped advances and block metrics.
fn br_box(doc: &BaseDocument, node: &Node) -> Option<BoxRect> {
    let root = node.inline_root_ancestor()?;
    let text = root.element_data()?.inline_layout_data.as_ref()?;
    let layout = &text.layout;
    let scale = layout.scale() as f64;
    let root_rect = native_box(doc, root)?;
    let inset = root.unrounded_layout();
    let left = (inset.border.left + inset.padding.left) as f64;
    let top = (inset.border.top + inset.padding.top + text.block_offset) as f64;

    fn order(doc: &BaseDocument, id: blitz_dom::NodeId, root: blitz_dom::NodeId,
             target: blitz_dom::NodeId, count: &mut usize, preserve: bool) -> bool {
        if id == target { return true; }
        let Some(node) = doc.get_node(id) else { return false; };
        // An inline-block has its own independent Parley layout. Neither its
        // <br> nodes nor its preserved text breaks belong to this inline root.
        if id != root && node.flags.is_inline_root() { return false; }
        if let NodeData::Text(data) = &node.data {
            if preserve { *count += data.content.chars().filter(|c| *c == '\n').count(); }
            return false;
        }
        let preserve = if tag(node).is_some() {
            matches!(doc.resolved_style_value(id, "white-space-collapse").as_str(),
                "preserve" | "preserve-breaks" | "break-spaces")
        } else { preserve };
        if tag(node) == Some("br") { *count += 1; }
        for &child in &node.children {
            if order(doc, child, root, target, count, preserve) { return true; }
        }
        false
    }
    let mut ordinal = 0;
    if !order(doc, root.id, root.id, node.id, &mut ordinal, false) { return None; }
    let line = layout.lines().filter(|line| line.break_reason() == BreakReason::Explicit)
        .nth(ordinal)?;
    let metrics = line.metrics();
    // A line break has zero advance, but occupies the font content area
    // (baseline minus ascent through descent), not the full line-height box.
    let mut right = 0.0_f64;
    for item in line.items() {
        let edge = match item {
            PositionedLayoutItem::GlyphRun(run) => (run.offset() + run.advance()) as f64,
            PositionedLayoutItem::InlineBox(b) => (b.x + b.width) as f64,
        };
        right = right.max(edge);
    }
    Some(BoxRect {
        x: root_rect.x + left + right / scale,
        y: root_rect.y + top + metrics.content_block_min_coord as f64 / scale,
        width: 0.0,
        height: (metrics.content_block_max_coord - metrics.content_block_min_coord) as f64 / scale,
    })
}

// Only a non-atomic inline label can use text content bounds; block and atomic
// labels must preserve their padding, border and explicitly sized boxes.
// Such an inline label can contain a control taller than its text. Its horizontal
// fragment extent includes that control, but the vertical inline-text box does
// not inherit the control's border-box height. Position the content box from
// the line baseline and shaped run font metrics; other inline elements retain
// their usual path.
fn form_label_text_box(doc: &BaseDocument, node: &Node) -> Option<BoxRect> {
    fn contains_control(doc: &BaseDocument, node: &Node) -> bool {
        node.children.iter().any(|id| {
            doc.get_node(*id).is_some_and(|child|
                matches!(tag(child), Some("input" | "textarea" | "select" | "button"))
                    || contains_control(doc, child))
        })
    }
    fn belongs_to(doc: &BaseDocument, mut id: blitz_dom::NodeId, ancestor: blitz_dom::NodeId) -> bool {
        loop {
            if id == ancestor { return true; }
            let Some(parent) = doc.get_node(id).and_then(|n| n.parent) else {
                return false;
            };
            id = parent;
        }
    }

    if tag(node) != Some("label")
        || !node.is_non_atomic_inline()
        || !contains_control(doc, node)
    {
        return None;
    }
    let root = node.inline_root_ancestor()?;
    let text = root.element_data()?.inline_layout_data.as_ref()?;
    let scale = text.layout.scale() as f64;
    if scale <= 0.0 { return None; }

    // Retain the complete inline fragment width, including any control,
    // margins and font-dependent glyph advances.
    let horizontal = native_box(doc, node)?;
    let root_rect = native_box(doc, root)?;
    let inset = root.unrounded_layout();
    let origin_y = root_rect.y
        + (inset.border.top + inset.padding.top + text.block_offset) as f64;
    let mut top = f64::INFINITY;
    let mut bottom = f64::NEG_INFINITY;
    for line in text.layout.lines() {
        for item in line.items() {
            let PositionedLayoutItem::GlyphRun(run) = item else { continue };
            if !belongs_to(doc, run.style().brush.id, node.id) { continue; }
            let metrics = run.run().font_metrics();
            let baseline = line.metrics().baseline as f64 / scale;
            top = top.min(baseline - metrics.ascent as f64 / scale);
            bottom = bottom.max(baseline + metrics.descent as f64 / scale);
        }
    }
    if !top.is_finite() || !bottom.is_finite() || bottom <= top { return None; }
    Some(BoxRect {
        x: horizontal.x,
        y: origin_y + top,
        width: horizontal.width,
        height: bottom - top,
    })
}

// Painting resolves transform-origin in Node::set_transform. Reuse those same
// matrices rather than reparsing CSS or rounding each intermediary AABB.
// CSSOM returns the AABB of the final four transformed border-box corners.
fn transformed_bounds(doc: &BaseDocument, node: &Node, rect: BoxRect) -> BoxRect {
    let mut combined = Affine::IDENTITY;
    let mut current = Some(node.id);
    while let Some(id) = current {
        let Some(n) = doc.get_node(id) else { break };
        if let Some(transform) = n.transform().as_deref() {
            if let Some(origin) = native_box(doc, n) {
                let ox = origin.x;
                let oy = origin.y;
                let around = Affine::translate((ox, oy)) * *transform
                    * Affine::translate((-ox, -oy));
                combined = around * combined;
            }
        }
        current = n.containing_block();
    }
    let bounds = combined.transform_rect_bbox(
        Rect::new(rect.x, rect.y, rect.x + rect.width, rect.y + rect.height));
    BoxRect { x: bounds.x0, y: bounds.y0,
        width: bounds.width(), height: bounds.height() }
}

pub(crate) fn element_box(doc: &BaseDocument, node: &Node) -> Option<BoxRect> {
    if hidden_in_details(doc, node) {
        return Some(BoxRect { x: 0.0, y: 0.0, width: 0.0, height: 0.0 });
    }
    let original = match tag(node) {
        Some("br") => br_box(doc, node).or_else(|| native_box(doc, node)),
        Some("label") => form_label_text_box(doc, node).or_else(|| native_box(doc, node)),
        Some("thead" | "tbody" | "tfoot" | "tr") =>
            descendant_cells(doc, node).or_else(|| native_box(doc, node)),
        Some("circle" | "path") => svg_shape(doc, node).or_else(|| native_box(doc, node)),
        _ if node.is_non_atomic_inline() => {
            // getClientRects is provided by the upstream fragment builder;
            // getBoundingClientRect is the union, not the first fragment.
            let parts = doc.node_client_rects(node.id);
            parts.iter().map(|b| BoxRect { x: b.x, y: b.y, width: b.width, height: b.height })
                .reduce(BoxRect::union).or_else(|| native_box(doc, node))
        }
        _ => native_box(doc, node),
    }?;
    Some(transformed_bounds(doc, node, original))
}
