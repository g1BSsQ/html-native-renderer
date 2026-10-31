// Copyright 2026 g1BSsQ. Licensed under Apache-2.0.
// Paint invalidation for the retained, viewport-sized raster cache.
use std::{collections::{HashMap, HashSet}, sync::Arc};
use blitz_dom::{BaseDocument, Node, NodeData, NodeId};
use peniko::kurbo::{Rect, Vec2};
use crate::geometry;
#[path = "damage_constants.rs"]
mod damage_constants;
use damage_constants::INK_INFLATE_CSS;

#[derive(Clone)]
pub(crate) struct PaintNode {
    rect: Option<Rect>,
    border_box: Option<Rect>,
    style_key: usize,
    // Pseudo boxes are not in node.children and have their own style arcs.
    // If the owner or either generated style changes, include owner ink even
    // when the owner's computed paint fingerprint is unchanged.
    has_pseudo: bool,
    pseudo_styles: [Option<usize>; 2],
    // Stable painted/computed values distinguish a real restyle from an
    // otherwise identical allocation, including element leaves with text.
    computed_paint: Option<Arc<Vec<String>>>,
    root_background: bool,
    inline_level: bool,
    mixed_text: bool,
    // Stable computed paint properties, unlike style allocation identity or
    // layout height, which can change when only a descendant is restyled.
    root_paint: Option<Vec<String>>,
    unbounded: bool,
    // Some effects require full-viewport compositing and cannot be safely
    // damage-pruned. Cache this with the snapshot to avoid extra CSS queries.
    tile_sensitive: bool,
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
    box_rect = ink.inflate(INK_INFLATE_CSS, INK_INFLATE_CSS);
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

// Resolve every enabled computed longhand, not an easy-to-miss hand-written
// paint allowlist. This includes outline-offset, text decorations, image
// sampling, text strokes, column rules, and future supported properties.
// The document API enumerates only supported names, so unsupported/typo
// properties never silently become empty-string fingerprint entries.
// Values are cached across updates while style allocation identity is stable.

// Full-viewport raster is required for paint inputs whose sampling,
// compositing or fractional coverage can depend on the CPU tile origin.
// Compute from the already-captured *complete* longhand fingerprint, not a
// second on-demand CSSOM walk. Unsupported longhands are not painted.
fn tile_sensitive_values(values: &[String]) -> bool {
    let names = blitz_dom::resolved_style_property_names();
    let value = |property: &str| -> Option<&str> {
        names.binary_search(&property).ok()
            .and_then(|idx| values.get(idx).map(String::as_str))
    };
    if let Some(opacity) = value("opacity") {
        if opacity.parse::<f32>().ok() != Some(1.0) {
            return true;
        }
    }
    for (property, neutral) in [
        ("background-image", "none"),
        ("border-image-source", "none"),
        ("mask-image", "none"),
        ("clip-path", "none"),
        ("box-shadow", "none"),
        ("text-shadow", "none"),
        ("filter", "none"),
        ("backdrop-filter", "none"),
        ("mix-blend-mode", "normal"),
        ("isolation", "auto"),
        ("transform", "none"),
        ("rotate", "none"),
        ("scale", "none"),
        ("translate", "none"),
    ] {
        if value(property).is_some_and(|actual| actual != neutral) {
            return true;
        }
    }
    false
}

fn rect_intersects(a: Rect, b: Rect) -> bool {
    a.x0 < b.x1 && a.x1 > b.x0 && a.y0 < b.y1 && a.y1 > b.y0
}

/// Use the old viewport-sized clipped target for any old/new visible effect
/// that may have a different AA, gradient or compositing result in a compact
/// tile. Consider both snapshots to cover *removed* effects. Generated pseudo
/// elements are treated as unsafe anywhere: their effect ink is not bounded
/// independently by the owner's rectangle.
pub(crate) fn needs_viewport_raster(
    before: &Snapshot, after: &Snapshot, damage: Rect,
) -> bool {
    before.values().chain(after.values()).any(|item| {
        item.tile_sensitive && (item.has_pseudo
            || item.rect.is_none_or(|ink| rect_intersects(ink, damage)))
    })
}

fn visit(
    doc: &BaseDocument, node: &Node, result: &mut Snapshot,
    cached_paint: &mut HashMap<usize, Arc<Vec<String>>>,
    previous: Option<&Snapshot>,
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
        // ::before and ::after are separate styled boxes, absent from the
        // ordinary children snapshot. Their shadows, outlines, filters,
        // text shadows and transforms may extend beyond the owner's ink.
        // Do not pretend the owner's border/overflow bounds cover them:
        // any such pseudo effect requires the existing full-viewport path.
        let pseudo_unbounded = [node.before(), node.after()].into_iter()
            .flatten()
            .any(|id| {
                let Some(pseudo) = doc.get_node(id) else { return true };
                let Some(styles) = pseudo.primary_styles() else { return true };
                let effects = styles.get_effects();
                let box_style = styles.get_box();
                let outline = doc.resolved_style_value(id, "outline-style");
                let text_shadow = doc.resolved_style_value(id, "text-shadow");
                !effects.box_shadow.0.is_empty()
                    || !effects.filter.0.is_empty()
                    || !effects.backdrop_filter.0.is_empty()
                    || outline != "none"
                    || text_shadow != "none"
                    || pseudo.transform().is_some()
                    || !box_style.transform.0.is_empty()
                    || doc.resolved_style_value(id, "rotate") != "none"
                    || doc.resolved_style_value(id, "scale") != "none"
                    || doc.resolved_style_value(id, "translate") != "none"
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
        let unbounded = pseudo_unbounded || effects.is_some_and(|e|
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
        // Generated ::before/::after nodes are not ordinary DOM children.
        // Track their style identity to invalidate an untouched owner when
        // a sibling selector changes only a pseudo's computed paint.
        let has_pseudo = node.before().is_some() || node.after().is_some();
        let pseudo_styles = [node.before(), node.after()].map(|id| {
            id.and_then(|id| doc.get_node(id))
                .and_then(|pseudo| pseudo.primary_styles())
                .map(|styles| &***styles as *const _ as usize)
        });
        // Include leaves and text-containing elements: a changed style
        // allocation on a *sibling tile* is not evidence of changed ink.
        // The explicitly touched node and its descendants are included below
        // regardless of the comparison result.
        let computed_paint = (style_key != 0 && !root_background).then(|| {
            // Snapshot capture happens after every incremental resolve, but
            // the majority of nodes retain the exact same computed style.
            // Reuse their prior painted values instead of serializing every
            // CSS property on every frame. Arc cloning is constant-time.
            // Newly allocated styles still compare by actual values, not
            // allocation identity, so harmless sibling restyles stay local.
            let unchanged = previous
                .and_then(|snapshot| snapshot.get(&node.id))
                .filter(|old| old.style_key == style_key)
                .and_then(|old| old.computed_paint.as_ref());
            if let Some(values) = unchanged {
                values.clone()
            } else {
                cached_paint.entry(style_key).or_insert_with(|| {
                    Arc::new(blitz_dom::resolved_style_property_names().iter()
                        .map(|&property| doc.resolved_style_value(node.id, property))
                        .collect())
                }).clone()
            }
        });
        // Pseudo boxes do not appear in children/primary-style snapshots.
        // Generated content has no independently bounded ink snapshot, so
        // it conservatively selects viewport-sized painting. Root/background
        // has a shorter fingerprint; check its image paint directly.
        let tile_sensitive = previous
            .and_then(|snapshot| snapshot.get(&node.id))
            .filter(|old| old.style_key == style_key
                && old.pseudo_styles == pseudo_styles)
            .map(|old| old.tile_sensitive)
            .unwrap_or_else(|| {
                has_pseudo
                    || computed_paint.as_ref()
                        .is_some_and(|values| tile_sensitive_values(values))
                    || (root_background && style.is_some()
                        && doc.resolved_style_value(node.id, "background-image") != "none")
            });
        let border_box = geometry::element_box(doc, node).map(|b| {
            Rect::new(b.x, b.y, b.x + b.width, b.y + b.height)
        });
        result.insert(node.id, PaintNode {
            rect: ink_bounds(doc, node),
            border_box,
            style_key,
            has_pseudo,
            pseudo_styles,
            computed_paint,
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
            tile_sensitive,
            parent: node.parent,
        });
    }
    for &child in &node.children {
        if let Some(child) = doc.get_node(child) {
            visit(doc, child, result, cached_paint, previous);
        }
    }
}

pub(crate) fn capture(doc: &BaseDocument) -> Snapshot {
    capture_against(doc, None)
}

/// Incremental capture reuses paint signatures for nodes whose style
/// allocation survived resolve. Only newly styled nodes need CSSOM
/// serialization, avoiding an O(elements × properties) cost per frame.
pub(crate) fn capture_against(
    doc: &BaseDocument, previous: Option<&Snapshot>,
) -> Snapshot {
    let mut result = HashMap::new();
    let mut cached_paint = HashMap::new();
    visit(doc, doc.root_node(), &mut result, &mut cached_paint, previous);
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
                // The root's scrollable overflow aggregates descendant ink.
                // A changed descendant can alter it even though the root's
                // own border box and canvas paint are identical. The actual
                // descendant is already captured by the diff below.
                if old.border_box != new.border_box
                    || ((old.mixed_text || new.mixed_text) && old.rect != new.rect)
                {
                    ids.insert(*id);
                }
            }
            // Pseudo-element paint can change without any corresponding
            // change to the owner's primary computed values. Use the old
            // conservative style-identity rule for pseudo owners, also
            // observing the anonymous pseudo nodes' own style arcs.
            Some(old) if old.has_pseudo || new.has_pseudo => {
                if old.style_key != new.style_key
                    || old.pseudo_styles != new.pseudo_styles
                    || old.rect != new.rect
                    || old.border_box != new.border_box
                {
                    ids.insert(*id);
                }
            }
            // Style allocation identity does not imply a paint change.
            // Unchanged sibling tiles can be restyled by selector invalidation.
            // For a container without direct bare text, altered aggregate
            // overflow is covered by the changed descendants' own bounds.
            Some(old) if old.border_box == new.border_box
                && ((old.style_key == new.style_key && old.rect == new.rect)
                    || (old.computed_paint.is_some()
                        && old.computed_paint == new.computed_paint
                        && ((!old.mixed_text && !new.mixed_text)
                            || old.rect == new.rect))) => {}
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

#[cfg(test)]
mod property_fingerprint_tests {
    use blitz_dom::{BaseDocument, Node, NodeId};

    fn find_probe(doc: &BaseDocument, node: &Node) -> Option<NodeId> {
        if node.attr(blitz_dom::local_name!("id")) == Some("probe") {
            return Some(node.id);
        }
        node.children.iter().find_map(|&child| {
            doc.get_node(child).and_then(|node| find_probe(doc, node))
        })
    }

    #[test]
    fn key_paint_properties_resolve_on_a_styled_element() {
        let html = r#"<html><body><div id="probe" style="
            outline:2px solid black;outline-offset:4px;
            color:rgb(12,34,56);background-color:rgb(19,23,29);
            opacity:0.5;transform:translateX(3px)">Paint</div></body></html>"#;
        let mut doc = blitz_html::HtmlDocument::from_html(
            html, crate::document_config(392, None, None),
        );
        doc.resolve(0.0);
        let probe = find_probe(&doc, doc.root_node()).expect("styled element");
        let names = blitz_dom::resolved_style_property_names();
        for property in [
            "outline-offset", "color", "background-color",
            "opacity", "transform",
        ] {
            assert!(names.contains(&property), "not enumerated: {property}");
            let value = doc.resolved_style_value(probe, property);
            assert!(!value.is_empty(),
                "{property} returned an empty resolved value on a styled element");
        }
    }
}
