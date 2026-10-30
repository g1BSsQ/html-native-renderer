// Copyright 2026 g1BSsQ. Licensed under Apache-2.0.
// Paint invalidation for the retained, viewport-sized raster cache.
use std::collections::{HashMap, HashSet};
use blitz_dom::{BaseDocument, Node, NodeData, NodeId};
use peniko::kurbo::{Rect, Vec2};
use crate::geometry;

#[derive(Clone)]
pub(crate) struct PaintNode {
    rect: Option<Rect>,
    border_box: Option<Rect>,
    style_key: usize,
    // Container paint properties distinguish an actual style change from
    // a newly allocated but visually identical computed style.
    container_paint: Option<Vec<String>>,
    root_background: bool,
    inline_level: bool,
    mixed_text: bool,
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
    // The CSSOM border box excludes long unwrapped text and content that
    // continues past an explicitly sized box. The resolver's overflow is
    // relative to the untransformed border-box origin in device coordinates.
    // Include both old and new overflow via the captured snapshots.
    //
    // For transformed ink, the node-local overflow cannot safely be combined
    // with an already-transformed CSSOM box. Such cases fall back below.
    if let Some(origin) = doc.get_client_bounding_rect(node.id) {
        let scale = doc.viewport().scale_f64();
        let overflow = node.scrollable_overflow();
        let scroll = node.scroll_offset();
        let local = Rect::new(
            origin.x + overflow.x0 / scale - scroll.x,
            origin.y + overflow.y0 / scale - scroll.y,
            origin.x + overflow.x1 / scale - scroll.x,
            origin.y + overflow.y1 / scale - scroll.y,
        );
        if !local.is_finite() {
            return None;
        }
        ink = ink.union(local);
        // Taffy's scrollable overflow includes line boxes that exceed fixed
        // dimensions; keep it as an additional bound for CSS text layout.
        let flow = node.final_layout().scrollable_overflow_rect;
        let layout = Rect::new(
            origin.x + f64::from(flow.left) - scroll.x,
            origin.y + f64::from(flow.top) - scroll.y,
            origin.x + f64::from(flow.right) - scroll.x,
            origin.y + f64::from(flow.bottom) - scroll.y,
        );
        if !layout.is_finite() {
            return None;
        }
        ink = ink.union(layout);
    }
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

// Shadows/outlines are expanded in CSS axes. Once an ancestor transforms the
// coordinate system, applying those pads around the transformed border box
// can undercount ink (e.g. a 3x scale or 90-degree rotation). Be conservative.
fn has_transformed_ancestor(doc: &BaseDocument, node: &Node) -> bool {
    let mut parent = node.parent;
    while let Some(id) = parent {
        let Some(ancestor) = doc.get_node(id) else { break };
        if ancestor.transform().is_some() {
            return true;
        }
        parent = ancestor.parent;
    }
    false
}

// Parent boxes may be restyled by incremental layout even when they paint
// identically. Only skip that allocation churn when paint-relevant computed
// properties and geometry are stable. Share values for identical styles.
const CONTAINER_PAINT_PROPERTIES: &[&str] = &[
    "background-color", "background-image", "background-position",
    "background-size", "background-repeat", "background-attachment",
    "background-clip", "background-origin", "color", "visibility",
    "border-top", "border-right", "border-bottom", "border-left",
    "border-radius", "box-shadow", "outline", "opacity", "filter",
    "backdrop-filter", "transform", "transform-origin", "clip-path",
    "mix-blend-mode", "isolation", "overflow-x", "overflow-y",
    "text-shadow", "text-decoration-line", "text-decoration-color",
    "text-decoration-style", "text-decoration-thickness",
    "text-transform", "text-indent", "letter-spacing", "word-spacing",
    "font-family", "font-size", "font-weight", "font-style",
    "line-height", "content", "list-style-type", "list-style-image",
    "object-fit", "object-position", "mask-image", "mask-position",
    "mask-size", "mask-repeat", "appearance", "accent-color",
];

fn visit(
    doc: &BaseDocument, node: &Node, result: &mut Snapshot,
    cached_paint: &mut HashMap<usize, Vec<String>>,
) {
    if matches!(&node.data, NodeData::Element(_)) {
        let style = node.primary_styles();
        let effects = style.as_ref().map(|s| s.get_effects());
        let has_shadow = effects.is_some_and(|e| !e.box_shadow.0.is_empty());
        // Never request on-demand computed styles for an unstyled/display:none
        // node here. resolved_style_value() would call
        // resolve_undisplayed_style(), creating/dropping a Stylo traversal
        // context outside the resolver's LAYOUT thread state.
        let has_outline = style.as_ref().is_some_and(|_| {
            doc.resolved_style_value(node.id, "outline-style") != "none"
        });
        let has_text_shadow = style.as_ref().is_some_and(|_| {
            doc.resolved_style_value(node.id, "text-shadow") != "none"
        });
        // The painter uses this already resolved marker layout, too.
        let outside_marker = node.element_data()
            .and_then(|data| data.list_item_data.as_deref())
            .is_some_and(|item| matches!(
                &item.position, blitz_dom::node::ListItemLayoutPosition::Outside(_)
            ));
        let transformed = node.transform().is_some() || has_transformed_ancestor(doc, node);
        let overflow = node.scrollable_overflow();
        let size = node.final_layout().size;
        let scale = doc.viewport().scale_f64();
        let extends_box = overflow.x0 < 0.0 || overflow.y0 < 0.0
            || overflow.x1 > f64::from(size.width) * scale
            || overflow.y1 > f64::from(size.height) * scale;
        let unbounded = effects.is_some_and(|e|
            !e.filter.0.is_empty() || !e.backdrop_filter.0.is_empty()
        ) || has_text_shadow || outside_marker
            || (transformed && (has_shadow || has_outline || extends_box));
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
        let style_key = style.as_ref().map(|s| &***s as *const _ as usize).unwrap_or(0);
        let container_paint = (style_key != 0 && !root_background
            && node.children.iter().any(|&id| doc.get_node(id).is_some_and(|child| {
                matches!(&child.data, NodeData::Element(_))
            })))
            .then(|| cached_paint.entry(style_key).or_insert_with(|| {
                CONTAINER_PAINT_PROPERTIES.iter()
                    .map(|property| doc.resolved_style_value(node.id, property))
                    .collect()
            }).clone());
        let border_box = geometry::element_box(doc, node).map(|b| {
            Rect::new(b.x, b.y, b.x + b.width, b.y + b.height)
        });
        result.insert(node.id, PaintNode {
            rect: ink_bounds(doc, node),
            border_box,
            style_key,
            container_paint,
            root_background,
            // Non-atomic spans always participate in the enclosing line.
            // Atomic inline boxes must also invalidate neighboring line text
            // when siblings share that inline formatting context. Keeping the
            // shared-content check excludes a standalone form input in a
            // grid item (its change remains safely bounded).
            inline_level: node.is_non_atomic_inline()
                || (style.as_ref().is_some_and(|_| {
                    doc.resolved_style_value(node.id, "display").starts_with("inline")
                }) && node.parent
                    .and_then(|id| doc.get_node(id))
                    .is_some_and(|parent| parent.children.iter().any(|&id| {
                        if id == node.id { return false; }
                        doc.get_node(id).is_some_and(|sibling| {
                            (matches!(&sibling.data, NodeData::Text(_))
                                && !sibling.is_whitespace_node())
                                || (sibling.primary_styles().is_some()
                                    && doc.resolved_style_value(sibling.id, "display")
                                        .starts_with("inline"))
                        })
                    }))),
            // Direct bare text has no PaintNode. If a sibling block reflows,
            // its ink can move even while this element's box stays fixed.
            mixed_text: node.children.iter().any(|&id| {
                doc.get_node(id).is_some_and(|child| {
                    matches!(&child.data, NodeData::Text(_))
                        && !child.is_whitespace_node()
                })
            }),
            root_paint,
            unbounded,
            parent: node.parent,
        });
    }
    for &child in &node.children {
        if let Some(child) = doc.get_node(child) {
            visit(doc, child, result, cached_paint);
        }
    }
}

pub(crate) fn capture(doc: &BaseDocument) -> Snapshot {
    let mut result = HashMap::new();
    let mut cached_paint = HashMap::new();
    visit(doc, doc.root_node(), &mut result, &mut cached_paint);
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
    // Explicit root/body mutations or changed root paint properties can
    // affect the viewport canvas, so they require a full repaint.
    // Root/body geometry changes alone must still damage their old/new ink:
    // borders, padding and backgrounds do not necessarily cover the viewport.
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
        match before.get(id) {
            Some(old) if old.root_background && new.root_background => {
                if old.rect != new.rect {
                    ids.insert(*id);
                }
            }
            // A stable container can have newly aggregated scroll overflow
            // solely because a descendant repainted. Its own border box and
            // computed paint remain stable; the changed descendant is already
            // included in ids (with its old and new ink).
            Some(old) if old.border_box == new.border_box
                && ((old.rect == new.rect && old.style_key == new.style_key)
                    || (old.container_paint.is_some()
                        && old.container_paint == new.container_paint)) => {}
            _ => { ids.insert(*id); }
        }
    }
    for id in before.keys() {
        if !after.contains_key(id) {
            ids.insert(*id);
        }
    }

    // Bare text nodes are not represented in snapshots. A changed block
    // sibling can move that text without changing its parent's rectangle or
    // style. Include both old/new parent ink when it has direct text.
    let mut text_parents = Vec::new();
    for id in &ids {
        for node in [before.get(id), after.get(id)].into_iter().flatten() {
            if let Some(parent) = node.parent {
                if before.get(&parent).is_some_and(|item| item.mixed_text)
                    || after.get(&parent).is_some_and(|item| item.mixed_text)
                {
                    text_parents.push(parent);
                }
            }
        }
    }
    ids.extend(text_parents);

    // A changed inline-level element can reflow text on either side without
    // restyling their separate paint nodes. Check both explicit mutations
    // and style/rectangle changes discovered by the snapshot diff.
    if ids.iter().any(|id| {
        before.get(id).is_some_and(|item| item.inline_level)
            || after.get(id).is_some_and(|item| item.inline_level)
    }) {
        return None;
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
            if item.unbounded {
                return None;
            }
            let Some(rect) = item.rect else { return None };
            union = Some(union.map_or(rect, |previous| previous.union(rect)));
        }
    }
    Some(union.unwrap_or(Rect::ZERO))
}
