// Copyright 2026 g1BSsQ. Licensed under Apache-2.0.
// Paint invalidation for the retained, viewport-sized raster cache.
use std::collections::{HashMap, HashSet};
use blitz_dom::{BaseDocument, Node, NodeData, NodeId};
use peniko::kurbo::{Rect, Vec2};
use crate::geometry;

#[derive(Clone)]
pub(crate) struct PaintNode {
    rect: Option<Rect>,
    style_key: usize,
    root_background: bool,
    // Stable computed paint properties, unlike style allocation identity or
    // layout height, which can change when only a descendant is restyled.
    root_paint: Option<Vec<String>>,
    unbounded: bool,
    parent: Option<NodeId>,
}

pub(crate) type Snapshot = HashMap<NodeId, PaintNode>;

fn ink_bounds(doc: &BaseDocument, node: &Node) -> Option<Rect> {
    let b = geometry::element_box(doc, node)?;
    let mut box_rect = Rect::new(b.x, b.y, b.x + b.width, b.y + b.height);
    if !box_rect.is_finite() {
        return None;
    }
    let mut ink = box_rect;
    if let Some(style) = node.primary_styles() {
        let effects = style.get_effects();
        for shadow in effects.box_shadow.0.iter().filter(|shadow| !shadow.inset) {
            let spread = shadow.spread.px() as f64;
            let blur = shadow.base.blur.px() as f64;
            let pad = (spread + 3.0 * blur).max(0.0);
            let shifted = box_rect.inflate(pad, pad) + Vec2::new(
                shadow.base.horizontal.px() as f64,
                shadow.base.vertical.px() as f64,
            );
            ink = ink.union(shifted);
        }
        let outline = style.get_outline();
        let pad = (outline.outline_offset.to_f64_px()
            + outline.outline_width.0.to_f64_px()).max(0.0);
        ink = ink.union(box_rect.inflate(pad, pad));
    }
    // Include antialiasing and glyph overhang. All fractional CSS-to-device
    // boundaries are rounded outward again when making the raster rectangle.
    box_rect = ink.inflate(8.0, 8.0);
    box_rect.is_finite().then_some(box_rect)
}

fn visit(doc: &BaseDocument, node: &Node, result: &mut Snapshot) {
    if matches!(&node.data, NodeData::Element(_)) {
        let style = node.primary_styles();
        let effects = style.as_ref().map(|s| s.get_effects());
        let unbounded = effects.is_some_and(|e| {
            !e.filter.0.is_empty() || !e.backdrop_filter.0.is_empty()
                || (node.transform().is_some()
                    && (!e.box_shadow.0.is_empty()
                        || doc.resolved_style_value(node.id, "outline-style") != "none"))
        });
        let root_background = matches!(
            &node.data,
            NodeData::Element(data) if matches!(&*data.name.local, "html" | "body")
        );
        let root_paint = root_background.then(|| {
            [
                "background-color", "background-image", "background-position",
                "background-size", "background-repeat", "background-attachment",
                "background-clip", "background-origin", "color", "visibility",
                "border-top", "border-right", "border-bottom", "border-left",
                "box-shadow", "outline", "opacity", "filter", "backdrop-filter",
                "transform", "clip-path", "mix-blend-mode", "overflow-x", "overflow-y",
            ]
            .iter()
            .map(|property| doc.resolved_style_value(node.id, property))
            .collect()
        });
        result.insert(node.id, PaintNode {
            rect: ink_bounds(doc, node),
            style_key: style.map(|s| &**s as *const _ as usize).unwrap_or(0),
            root_background,
            root_paint,
            unbounded,
            parent: node.parent,
        });
    }
    for &child in &node.children {
        if let Some(child) = doc.get_node(child) {
            visit(doc, child, result);
        }
    }
}

pub(crate) fn capture(doc: &BaseDocument) -> Snapshot {
    let mut result = HashMap::new();
    visit(doc, doc.root_node(), &mut result);
    result
}

/// Return None when a mutation cannot be bounded safely; otherwise return
/// the union in document CSS pixels. A zero-sized union is valid.
fn in_subtree(snapshot: &Snapshot, mut id: NodeId, roots: &HashSet<NodeId>) -> bool {
    loop {
        if roots.contains(&id) {
            return true;
        }
        let Some(parent) = snapshot.get(&id).and_then(|node| node.parent) else {
            return false;
        };
        id = parent;
    }
}

pub(crate) fn damage(
    before: &Snapshot,
    after: &Snapshot,
    touched: &HashSet<NodeId>,
) -> Option<Rect> {
    // Root/body rectangles can grow or shift when descendants relayout.
    // Those bounds, or a reallocated but equivalent computed style, are not
    // evidence that the viewport-wide background changed. Explicit root/body
    // mutations and changed computed paint properties still require fallback.
    for id in before.keys().chain(after.keys()) {
        let old = before.get(id);
        let new = after.get(id);
        if old.is_some_and(|item| item.root_background)
            || new.is_some_and(|item| item.root_background)
        {
            if touched.contains(id)
                || old.map(|item| &item.root_paint) != new.map(|item| &item.root_paint)
            {
                return None;
            }
        }
    }

    let mut ids = touched.clone();
    // A changed ancestor may alter descendant ink even when the child's
    // layout rectangle and computed style allocation are unchanged.
    for id in before.keys().chain(after.keys()) {
        if in_subtree(before, *id, touched) || in_subtree(after, *id, touched) {
            ids.insert(*id);
        }
    }
    for (id, new) in after {
        if new.root_background {
            continue;
        }
        match before.get(id) {
            Some(old) if old.style_key == new.style_key && old.rect == new.rect => {}
            _ => { ids.insert(*id); }
        }
    }
    for (id, old) in before {
        if !old.root_background && !after.contains_key(id) {
            ids.insert(*id);
        }
    }

    // Filters and backdrop operations may sample pixels beyond individual
    // element bounds, including when an overlapping item is the mutator.
    if before.values().any(|item| item.unbounded)
        || after.values().any(|item| item.unbounded)
    {
        return None;
    }

    let mut union: Option<Rect> = None;
    for id in ids {
        for item in [before.get(&id), after.get(&id)].into_iter().flatten() {
            if item.root_background || item.unbounded {
                return None;
            }
            let Some(rect) = item.rect else { return None };
            union = Some(union.map_or(rect, |previous| previous.union(rect)));
        }
    }
    Some(union.unwrap_or(Rect::ZERO))
}
