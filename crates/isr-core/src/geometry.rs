// Copyright 2026 g1BSsQ. Licensed under Apache-2.0.
// CSSOM export bridges layoutless table parts and SVG descendants.
use blitz_dom::{BaseDocument, Node, NodeData};
use peniko::kurbo::{BezPath, Rect, Shape as _};

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
pub(crate) fn element_box(doc: &BaseDocument, node: &Node) -> Option<BoxRect> {
    match tag(node) {
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
    }
}
