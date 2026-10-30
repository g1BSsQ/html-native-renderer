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
// Table sections, rows, columns and column groups are represented as tracks
// in the grid rather than as standalone layout children. Export their actual
// track rectangles, including for author-created table parts on <div>s.
// CSS 2.1 §§17.2, 17.5; margins/padding do not expand these rectangles.
fn internal_table_part_box(doc: &BaseDocument, node: &Node) -> Option<BoxRect> {
    use blitz_dom::node::SpecialElementData;
    // Export traverses all DOM elements, including unstyled head and
    // display:none subtrees. Read only the primary style already computed
    // during layout; querying resolved styles here can reenter the styler.
    let row_part = match node.table_part_axis()? {
        blitz_dom::node::TablePartAxis::Row => true,
        blitz_dom::node::TablePartAxis::Column => false,
    };

    let mut parent = node.parent;
    while let Some(parent_id) = parent {
        let ancestor = doc.get_node(parent_id)?;
        if let Some(el) = ancestor.element_data() {
            if let SpecialElementData::TableRoot(context) = &el.special_data {
                let grid = context.computed_grid_info.borrow();
                let grid = grid.as_ref()?;
                let table_rect = native_box(doc, ancestor)?;
                let x = &grid.columns.positions;
                let y = &grid.rows.positions;
                if x.is_empty() || y.is_empty() { return None; }
                let range = |indices: Vec<usize>, rows: bool| -> Option<(f64, f64)> {
                    let positions = if rows { y.as_slice() } else { x.as_slice() };
                    let mut min = f64::INFINITY;
                    let mut max = f64::NEG_INFINITY;
                    for index in indices {
                        let line = positions.get(index)?;
                        min = min.min(line.start.min(line.end) as f64);
                        max = max.max(line.start.max(line.end) as f64);
                    }
                    min.is_finite().then_some((min, max))
                };
                let all_x: Vec<_> = (0..x.len()).collect();
                let all_y: Vec<_> = (0..y.len()).collect();
                let (xmin, xmax, ymin, ymax) = if row_part {
                    let indices: Vec<_> = context.rows.iter().enumerate()
                        .filter(|(_, row)| row.node_id == node.id || row.group == Some(node.id))
                        .map(|(index, _)| index).collect();
                    let (xmin, xmax) = range(all_x, false)?;
                    let (ymin, ymax) = range(indices, true)?;
                    (xmin, xmax, ymin, ymax)
                } else {
                    let indices: Vec<_> = context.columns.iter().enumerate()
                        .filter(|(_, col)| col.node_id == node.id || col.group == Some(node.id))
                        .map(|(index, _)| index).collect();
                    let (xmin, xmax) = range(indices, false)?;
                    let (ymin, ymax) = range(all_y, true)?;
                    (xmin, xmax, ymin, ymax)
                };
                return Some(BoxRect {
                    x: table_rect.x + xmin, y: table_rect.y + ymin,
                    width: xmax - xmin, height: ymax - ymin,
                });
            }
        }
        parent = ancestor.parent;
    }
    None
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

// Non-atomic inline elements have no independent layout box. Keep the
// upstream union of horizontal fragments, but report the vertical border box
// formed by the element's own font content area and its vertical edges.
// Descendant glyphs and atomic controls affect which lines have fragments,
// not the font metrics of the element itself.
fn inline_content_box(doc: &BaseDocument, node: &Node) -> Option<BoxRect> {
    fn belongs_to(doc: &BaseDocument, mut id: blitz_dom::NodeId, ancestor: blitz_dom::NodeId) -> bool {
        loop {
            if id == ancestor { return true; }
            let Some(parent) = doc.get_node(id).and_then(|n| n.parent) else {
                return false;
            };
            id = parent;
        }
    }

    fn length_px(value: &str, percent_basis: f64) -> f64 {
        let value = value.trim();
        let resolved = value.strip_suffix("px")
            .and_then(|px| px.trim().parse::<f64>().ok())
            .or_else(|| value.strip_suffix('%')
                .and_then(|percent| percent.trim().parse::<f64>().ok())
                .map(|percent| percent * percent_basis / 100.0));
        resolved.unwrap_or(0.0).max(0.0)
    }

    if !node.is_non_atomic_inline() { return None; }
    let root = node.inline_root_ancestor()?;
    let text = root.element_data()?.inline_layout_data.as_ref()?;
    let scale = text.layout.scale() as f64;
    if scale <= 0.0 { return None; }
    let root_rect = native_box(doc, root)?;
    let inset = root.unrounded_layout();
    let origin_y = root_rect.y
        + (inset.border.top + inset.padding.top + text.block_offset) as f64;

    // The inline fragment builder owns horizontal edges and control advances.
    let horizontal = doc.node_client_rects(node.id)
        .into_iter().map(|b| BoxRect {
            x: b.x, y: b.y, width: b.width, height: b.height,
        }).reduce(BoxRect::union);
    let font_size = node.primary_styles()?.clone_font_size().used_size().px() as f64;
    if font_size <= 0.0 { return horizontal; }

    let own_family = doc.resolved_style_value(node.id, "font-family");
    // Prefer a shaped run in the same family; use a run from another family
    // only if no matching face appears anywhere in this inline root. Scale
    // that font's actual ascent and descent to the element's own font size.
    let mut first_run = None;
    let mut family_run = None;
    for line in text.layout.lines() {
        for item in line.items() {
            let PositionedLayoutItem::GlyphRun(run) = item else { continue };
            let size = run.run().font_size() as f64;
            if size <= 0.0 { continue; }
            let metrics = run.run().font_metrics();
            // An inline element's content rect excludes external line gap:
            // round the font ascent and descent independently in device pixels.
            // Its containing line box still uses the rounded gap for spacing.
            let candidate = (
                (metrics.ascent as f64 * font_size * scale / size).round(),
                (metrics.descent as f64 * font_size * scale / size).round(),
            );
            first_run.get_or_insert(candidate);
            if doc.resolved_style_value(run.style().brush.id, "font-family") == own_family {
                family_run.get_or_insert(candidate);
            }
        }
    }
    // No glyph at all: the line still needs an inline font strut, not the
    // control's border box or the full line box. Use an em-sized fallback if
    // no shaped font metrics are available in this layout.
    // The shaped metrics and em fallback are device pixels; baselines below
    // are CSS pixels. Convert at the point where the strut is applied.
    let (strut_ascent, strut_descent) =
        family_run.or(first_run).unwrap_or((
            (font_size * 0.8 * scale).round(),
            (font_size * 0.2 * scale).round(),
        ));

    let content_width = (root_rect.width
        - (inset.border.left + inset.border.right
            + inset.padding.left + inset.padding.right) as f64).max(0.0);
    let top_edge = length_px(&doc.resolved_style_value(node.id, "padding-top"), content_width)
        + length_px(&doc.resolved_style_value(node.id, "border-top-width"), content_width);
    let bottom_edge = length_px(&doc.resolved_style_value(node.id, "padding-bottom"), content_width)
        + length_px(&doc.resolved_style_value(node.id, "border-bottom-width"), content_width);

    let mut top = f64::INFINITY;
    let mut bottom = f64::NEG_INFINITY;
    for line in text.layout.lines() {
        let baseline = line.metrics().baseline as f64 / scale;
        let mut has_fragment = false;
        let mut own_top = f64::INFINITY;
        let mut own_bottom = f64::NEG_INFINITY;
        for item in line.items() {
            match item {
                PositionedLayoutItem::GlyphRun(run) => {
                    let owner = run.style().brush.id;
                    if !belongs_to(doc, owner, node.id) { continue; }
                    has_fragment = true;
                    if owner != node.id { continue; }
                    let metrics = run.run().font_metrics();
                    own_top = own_top.min(baseline - (metrics.ascent as f64).round() / scale);
                    own_bottom = own_bottom.max(baseline + (metrics.descent as f64).round() / scale);
                }
                PositionedLayoutItem::InlineBox(inline_box) => {
                    let id = blitz_dom::NodeId::from_u64(inline_box.id);
                    if doc.get_node(id).is_some() && belongs_to(doc, id, node.id) {
                        has_fragment = true;
                    }
                }
            }
        }
        if !has_fragment { continue; }
        let line_top = if own_top.is_finite() {
            own_top
        } else {
            baseline - strut_ascent / scale
        };
        let line_bottom = if own_bottom.is_finite() {
            own_bottom
        } else {
            baseline + strut_descent / scale
        };
        top = top.min(line_top - top_edge);
        bottom = bottom.max(line_bottom + bottom_edge);
    }

    // Empty inline elements have no glyph or control fragments. Their own
    // strut still provides a nonzero content height at the first line baseline.
    if !top.is_finite() || !bottom.is_finite() {
        let baseline = text.layout.lines().next()?.metrics().baseline as f64 / scale;
        top = baseline - strut_ascent / scale - top_edge;
        bottom = baseline + strut_descent / scale + bottom_edge;
    }
    if bottom <= top { return horizontal; }
    let x = horizontal.map_or(
        root_rect.x + (inset.border.left + inset.padding.left) as f64,
        |rect| rect.x,
    );
    Some(BoxRect {
        x,
        y: origin_y + top,
        width: horizontal.map_or(0.0, |rect| rect.width),
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
        Some("thead" | "tbody" | "tfoot" | "tr") =>
            descendant_cells(doc, node).or_else(|| native_box(doc, node)),
        Some("circle" | "path") => svg_shape(doc, node).or_else(|| native_box(doc, node)),
        _ if node.is_non_atomic_inline() =>
            inline_content_box(doc, node).or_else(|| native_box(doc, node)),
        _ => internal_table_part_box(doc, node).or_else(|| native_box(doc, node)),
    }?;
    Some(transformed_bounds(doc, node, original))
}
