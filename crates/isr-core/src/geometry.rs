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
// CSS 2.1 17.6.2: every grid edge has one winning border. Cell borders
// conflict with their neighbours and with row, row-group, and table borders;
// painting each cell independently duplicates shared edges.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BorderStyle {
    None, Hidden, Inset, Groove, Outset, Ridge, Dotted, Dashed, Solid, Double,
}
impl BorderStyle {
    fn parse(s: &str) -> Self {
        match s.trim() {
            "hidden" => Self::Hidden,
            "inset" => Self::Inset,
            "groove" => Self::Groove,
            "outset" => Self::Outset,
            "ridge" => Self::Ridge,
            "dotted" => Self::Dotted,
            "dashed" => Self::Dashed,
            "solid" => Self::Solid,
            "double" => Self::Double,
            _ => Self::None,
        }
    }
    fn priority(self) -> u8 {
        match self {
            Self::None => 0, Self::Inset => 1, Self::Groove => 2,
            Self::Outset => 3, Self::Ridge => 4, Self::Dotted => 5,
            Self::Dashed => 6, Self::Solid => 7, Self::Double => 8,
            Self::Hidden => 9,
        }
    }
}
#[derive(Clone, Copy, Debug)]
enum Side { Top, Right, Bottom, Left }
impl Side {
    fn name(self) -> &'static str {
        match self {
            Self::Top => "top", Self::Right => "right",
            Self::Bottom => "bottom", Self::Left => "left",
        }
    }
}
#[derive(Clone, Copy, Debug)]
enum Origin { Table, RowGroup, Row, Cell }
impl Origin {
    fn priority(self) -> u8 {
        match self {
            Self::Table => 0, Self::RowGroup => 1,
            Self::Row => 2, Self::Cell => 3,
        }
    }
}
#[derive(Clone, Copy, Debug)]
struct Border {
    style: BorderStyle,
    width: f64,
    color: Option<Color>,
    origin: Origin,
    // If every earlier conflict rule ties, the upper/left edge wins.
    leading: bool,
}
fn computed_border(doc: &BaseDocument, node: &Node, side: Side, origin: Origin, leading: bool) -> Border {
    let prefix = format!("border-{}-", side.name());
    let style = BorderStyle::parse(&doc.resolved_style_value(node.id, &(prefix.clone() + "style")));
    let raw_width = doc.resolved_style_value(node.id, &(prefix.clone() + "width"));
    let width = match raw_width.trim() {
        "thin" => 1.0, "medium" => 3.0, "thick" => 5.0,
        other => other.trim_end_matches("px").parse::<f64>().ok().unwrap_or(0.0).max(0.0),
    };
    // Both width and colour originate in this edge owner's resolved CSS,
    // including rows/groups/table; no fixture-specific colour or width.
    let color = parse_color(&doc.resolved_style_value(node.id, &(prefix + "color")));
    Border { style, width, color, origin, leading }
}
fn winner(candidates: &[Border]) -> Option<Border> {
    use std::cmp::Ordering;
    let best = candidates.iter().copied().max_by(|a, b| {
        // 'hidden' suppresses the entire edge, regardless of width.
        match (a.style == BorderStyle::Hidden, b.style == BorderStyle::Hidden) {
            (true, false) => return Ordering::Greater,
            (false, true) => return Ordering::Less,
            _ => {}
        }
        // 'none' always loses, regardless of width.
        match (a.style == BorderStyle::None, b.style == BorderStyle::None) {
            (true, false) => return Ordering::Less,
            (false, true) => return Ordering::Greater,
            _ => {}
        }
        a.width.total_cmp(&b.width)
            .then_with(|| a.style.priority().cmp(&b.style.priority()))
            .then_with(|| a.origin.priority().cmp(&b.origin.priority()))
            .then_with(|| a.leading.cmp(&b.leading))
    })?;
    if matches!(best.style, BorderStyle::None | BorderStyle::Hidden) || best.width <= 0.0 {
        None
    } else { Some(best) }
}
#[derive(Clone, Copy)]
struct TablePart<'a> { node: &'a Node, rect: BoxRect }
struct TableRow<'a> {
    row: TablePart<'a>,
    group: Option<TablePart<'a>>,
    cells: Vec<TablePart<'a>>,
}
fn gather_rows<'a>(
    doc: &'a BaseDocument, node: &'a Node,
    group: Option<TablePart<'a>>, rows: &mut Vec<TableRow<'a>>,
) {
    match tag(node) {
        Some("table") => return, // Do not mix nested table grids.
        Some("thead" | "tbody" | "tfoot") => {
            let group = descendant_cells(doc, node)
                .or_else(|| native_box(doc, node))
                .map(|rect| TablePart { node, rect });
            for id in &node.children {
                if let Some(child) = doc.get_node(*id) {
                    gather_rows(doc, child, group, rows);
                }
            }
        }
        Some("tr") => {
            let mut cells: Vec<_> = node.children.iter()
                .filter_map(|id| doc.get_node(*id))
                .filter(|child| matches!(tag(child), Some("td" | "th")))
                .filter_map(|cell| native_box(doc, cell).map(|rect| TablePart { node: cell, rect }))
                .filter(|part| part.rect.width > 0.0 && part.rect.height > 0.0)
                .collect();
            cells.sort_by(|a, b| a.rect.x.total_cmp(&b.rect.x));
            if let Some(rect) = cells.iter().map(|c| c.rect).reduce(BoxRect::union) {
                rows.push(TableRow {
                    row: TablePart { node, rect }, group, cells,
                });
            }
        }
        _ => {
            for id in &node.children {
                if let Some(child) = doc.get_node(*id) {
                    gather_rows(doc, child, group, rows);
                }
            }
        }
    }
}
fn covers(part: TablePart<'_>, start: f64, end: f64) -> bool {
    let (a, b) = (part.rect.x, part.rect.x + part.rect.width);
    b > start + 0.01 && a < end - 0.01
}
// CSS 2.1 17.6.2: half the collapsed width lies on each side of the
// grid line. For odd widths Chromium assigns the extra device pixel to
// the right/bottom. Snapping both ends prevents antialiased half-pixel
// stripes being painted over neighbouring cell interiors.
fn snapped_band(line: f64, css_width: f64) -> (f64, f64) {
    let width = css_width.round().max(1.0);
    ((line - width / 2.0).round(), (line + width / 2.0).round())
}
fn stroke_edge(
    out: &mut Vec<(Rect, Color)>, candidates: &[Border],
    vertical: bool, line: f64, start: f64, end: f64,
) {
    let Some(border) = winner(candidates) else { return; };
    let Some(color) = border.color else { return; };
    let (near, far) = snapped_band(line, border.width);
    let start = start.round();
    let end = end.round();
    if far <= near || end <= start { return; }
    let draw = |a: f64, b: f64, out: &mut Vec<(Rect, Color)>| {
        if a < b {
            let rect = if vertical { Rect::new(a, start, b, end) }
                       else { Rect::new(start, a, end, b) };
            out.push((rect, color));
        }
    };
    match border.style {
        BorderStyle::Double if far - near >= 3.0 => {
            let stripe = ((far - near) / 3.0).round().max(1.0);
            draw(near, near + stripe, out);
            draw(far - stripe, far, out);
        }
        BorderStyle::Dashed | BorderStyle::Dotted => {
            let thick = far - near;
            let length = if border.style == BorderStyle::Dotted { thick } else { thick * 3.0 };
            let mut offset = start;
            while offset < end {
                let finish = (offset + length).min(end);
                let rect = if vertical { Rect::new(near, offset, far, finish) }
                           else { Rect::new(offset, near, finish, far) };
                out.push((rect, color));
                offset += length * 2.0;
            }
        }
        // 'inset'/'outset' resolve as 'ridge'/'groove' for collapsed
        // border conflicts. The colour is always from computed style.
        _ => draw(near, far, out),
    }
}
fn group_differs(a: Option<TablePart<'_>>, b: Option<TablePart<'_>>) -> bool {
    a.map(|v| v.node.id) != b.map(|v| v.node.id)
}
fn paint_table_edges(doc: &BaseDocument, table: &Node, out: &mut Vec<(Rect, Color)>) {
    let mut rows = Vec::new();
    for id in &table.children {
        if let Some(child) = doc.get_node(*id) {
            gather_rows(doc, child, None, &mut rows);
        }
    }
    if rows.is_empty() { return; }
    rows.sort_by(|a, b| a.row.rect.y.total_cmp(&b.row.rect.y));
    let table_bounds = native_box(doc, table)
        .or_else(|| descendant_cells(doc, table))
        .map(|rect| TablePart { node: table, rect });

    // Horizontal grid lines: resolve the bottom of the row above against
    // the top of the row below, plus row-group/table borders at boundaries.
    for k in 0..=rows.len() {
        let above = k.checked_sub(1).and_then(|i| rows.get(i));
        let below = rows.get(k);
        let line = match (above, below) {
            (Some(a), Some(b)) =>
                (a.row.rect.y + a.row.rect.height + b.row.rect.y) / 2.0,
            (Some(a), None) => a.row.rect.y + a.row.rect.height,
            (None, Some(b)) => b.row.rect.y,
            (None, None) => continue,
        };
        let mut cuts = Vec::new();
        for row in [above, below].into_iter().flatten() {
            for c in &row.cells {
                cuts.push(c.rect.x);
                cuts.push(c.rect.x + c.rect.width);
            }
        }
        if let Some(t) = table_bounds {
            cuts.push(t.rect.x);
            cuts.push(t.rect.x + t.rect.width);
        }
        cuts.sort_by(f64::total_cmp);
        cuts.dedup_by(|a, b| (*a - *b).abs() < 0.25);
        let groups_break = group_differs(above.and_then(|r| r.group), below.and_then(|r| r.group));
        for slice in cuts.windows(2) {
            let (start, end) = (slice[0], slice[1]);
            if end - start < 0.25 { continue; }
            let mut candidates = Vec::new();
            if let Some(a) = above {
                for cell in &a.cells {
                    if covers(*cell, start, end) {
                        candidates.push(computed_border(doc, cell.node, Side::Bottom, Origin::Cell, true));
                    }
                }
                if covers(a.row, start, end) {
                    candidates.push(computed_border(doc, a.row.node, Side::Bottom, Origin::Row, true));
                }
                if groups_break {
                    if let Some(g) = a.group.filter(|g| covers(*g, start, end)) {
                        candidates.push(computed_border(doc, g.node, Side::Bottom, Origin::RowGroup, true));
                    }
                }
            }
            if let Some(b) = below {
                for cell in &b.cells {
                    if covers(*cell, start, end) {
                        candidates.push(computed_border(doc, cell.node, Side::Top, Origin::Cell, false));
                    }
                }
                if covers(b.row, start, end) {
                    candidates.push(computed_border(doc, b.row.node, Side::Top, Origin::Row, false));
                }
                if groups_break {
                    if let Some(g) = b.group.filter(|g| covers(*g, start, end)) {
                        candidates.push(computed_border(doc, g.node, Side::Top, Origin::RowGroup, false));
                    }
                }
            }
            if let Some(t) = table_bounds.filter(|t| covers(*t, start, end)) {
                if above.is_none() {
                    candidates.push(computed_border(doc, t.node, Side::Top, Origin::Table, false));
                }
                if below.is_none() {
                    candidates.push(computed_border(doc, t.node, Side::Bottom, Origin::Table, true));
                }
            }
            stroke_edge(out, &candidates, false, line, start, end);
        }
    }

    // Vertical grid lines: left/right cell borders conflict within each
    // row; the outermost edges also admit row/group/table candidates.
    for row in &rows {
        for k in 0..=row.cells.len() {
            let left = k.checked_sub(1).and_then(|i| row.cells.get(i)).copied();
            let right = row.cells.get(k).copied();
            let line = match (left, right) {
                (Some(a), Some(b)) => (a.rect.x + a.rect.width + b.rect.x) / 2.0,
                (Some(a), None) => a.rect.x + a.rect.width,
                (None, Some(b)) => b.rect.x,
                (None, None) => continue,
            };
            let mut candidates = Vec::new();
            if let Some(c) = left {
                candidates.push(computed_border(doc, c.node, Side::Right, Origin::Cell, true));
            }
            if let Some(c) = right {
                candidates.push(computed_border(doc, c.node, Side::Left, Origin::Cell, false));
            }
            if left.is_none() || right.is_none() {
                let side = if left.is_none() { Side::Left } else { Side::Right };
                let leading = right.is_none();
                candidates.push(computed_border(doc, row.row.node, side, Origin::Row, leading));
                if let Some(g) = row.group {
                    candidates.push(computed_border(doc, g.node, side, Origin::RowGroup, leading));
                }
                if let Some(t) = table_bounds {
                    candidates.push(computed_border(doc, t.node, side, Origin::Table, leading));
                }
            }
            stroke_edge(out, &candidates, true, line, row.row.rect.y,
                row.row.rect.y + row.row.rect.height);
        }
    }
}
fn visit_tables(doc: &BaseDocument, node: &Node, out: &mut Vec<(Rect, Color)>) {
    if tag(node) == Some("table") &&
        doc.resolved_style_value(node.id, "border-collapse") == "collapse" {
        paint_table_edges(doc, node, out);
    }
    for id in &node.children {
        if let Some(child) = doc.get_node(*id) { visit_tables(doc, child, out); }
    }
}
pub(crate) fn collapsed_row_separators(doc: &BaseDocument) -> Vec<(Rect, Color)> {
    // Paint only. The collapsed grid is already included in native layout;
    // these strokes must never inflate table or exported box dimensions.
    let mut lines = Vec::new();
    visit_tables(doc, doc.root_node(), &mut lines);
    lines
}
#[cfg(test)]
mod collapsed_border_tests {
    use super::*;
    fn edge(style: BorderStyle, width: f64, origin: Origin, leading: bool) -> Border {
        Border { style, width, origin, leading,
            color: Some(Color::from_rgb8(12, 34, 56)) }
    }
    #[test]
    fn hidden_suppresses_even_wide_solid() {
        assert!(winner(&[
            edge(BorderStyle::Solid, 8.0, Origin::Cell, true),
            edge(BorderStyle::Hidden, 0.0, Origin::Table, false),
        ]).is_none());
    }
    #[test]
    fn width_then_style_then_origin_then_upper_left() {
        let wide = winner(&[
            edge(BorderStyle::Solid, 1.0, Origin::Cell, true),
            edge(BorderStyle::Dotted, 2.0, Origin::Table, false),
        ]).unwrap();
        assert_eq!(wide.style, BorderStyle::Dotted);
        let solid = winner(&[
            edge(BorderStyle::Dashed, 2.0, Origin::Cell, true),
            edge(BorderStyle::Solid, 2.0, Origin::Table, false),
        ]).unwrap();
        assert_eq!(solid.style, BorderStyle::Solid);
        let cell = winner(&[
            edge(BorderStyle::Solid, 2.0, Origin::Row, true),
            edge(BorderStyle::Solid, 2.0, Origin::Cell, false),
        ]).unwrap();
        assert_eq!(cell.origin.priority(), Origin::Cell.priority());
        let upper = winner(&[
            edge(BorderStyle::Solid, 2.0, Origin::Cell, false),
            edge(BorderStyle::Solid, 2.0, Origin::Cell, true),
        ]).unwrap();
        assert!(upper.leading);
    }
    #[test]
    fn one_pixel_is_not_two_antialiased_halves() {
        assert_eq!(snapped_band(30.0, 1.0), (30.0, 31.0));
        assert_eq!(snapped_band(30.0, 2.0), (29.0, 31.0));
        assert_eq!(snapped_band(30.0, 3.0), (29.0, 32.0));
    }
}
