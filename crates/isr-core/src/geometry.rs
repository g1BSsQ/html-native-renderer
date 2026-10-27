// Copyright 2026 g1BSsQ. Licensed under Apache-2.0.
// CSSOM export bridges layoutless table parts and SVG descendants.
use blitz_dom::{BaseDocument, Node, NodeData, util::Color};
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
    match &node.data { NodeData::Element(d) => Some(d.name.local.as_ref()), _ => None }
}
fn attr<'a>(node: &'a Node, name: &str) -> Option<&'a str> {
    match &node.data {
        NodeData::Element(d) => d.attrs.iter()
            .find(|a| a.name.local.as_ref() == name).map(|a| a.value.as_ref()),
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
fn parse_color(css: &str) -> Option<Color> {
    let s = css.trim();
    if let Some(hex) = s.strip_prefix('#') {
        if hex.len() == 6 {
            let n = u32::from_str_radix(hex, 16).ok()?;
            return Some(Color::from_rgb8((n >> 16) as u8, (n >> 8) as u8, n as u8));
        }
    }
    let inner = s.strip_prefix("rgb(")?.strip_suffix(')')?;
    let v: Vec<u8> = inner.split(',').map(|s| s.trim().parse::<u8>()).collect::<std::result::Result<_,_>>().ok()?;
    if v.len() == 3 { Some(Color::from_rgb8(v[0], v[1], v[2])) } else { None }
}
fn separator_for_cell(doc: &BaseDocument, cell: &Node, out: &mut Vec<(Rect, Color)>) {
    if !matches!(tag(cell), Some("td" | "th")) { return; }
    if doc.resolved_style_value(cell.id, "border-bottom-style") != "solid" { return; }
    let width = doc.resolved_style_value(cell.id, "border-bottom-width")
        .trim_end_matches("px").parse::<f64>().ok().unwrap_or(0.0);
    let color = parse_color(&doc.resolved_style_value(cell.id, "border-bottom-color"));
    if let (Some(b), Some(color)) = (native_box(doc, cell), color) {
        if width > 0.0 && b.width > 0.0 {
            out.push((Rect::new(b.x, b.y + b.height - width / 2.0,
                b.x + b.width, b.y + b.height + width / 2.0), color));
        }
    }
}
fn visit_table_cells(doc: &BaseDocument, node: &Node, out: &mut Vec<(Rect, Color)>) {
    for id in &node.children {
        if let Some(child) = doc.get_node(*id) {
            if tag(child) == Some("table") { continue; } // Nested tables paint independently.
            separator_for_cell(doc, child, out);
            visit_table_cells(doc, child, out);
        }
    }
}
fn visit_tables(doc: &BaseDocument, node: &Node, out: &mut Vec<(Rect, Color)>) {
    if tag(node) == Some("table") &&
        doc.resolved_style_value(node.id, "border-collapse") == "collapse" {
        visit_table_cells(doc, node, out);
    }
    for id in &node.children {
        if let Some(child) = doc.get_node(*id) { visit_tables(doc, child, out); }
    }
}
pub(crate) fn collapsed_row_separators(doc: &BaseDocument) -> Vec<(Rect, Color)> {
    let mut lines = Vec::new();
    visit_tables(doc, doc.root_node(), &mut lines);
    lines
}
