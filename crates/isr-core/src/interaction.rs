// Copyright 2026 g1BSsQ. Licensed under Apache-2.0.
// Retained interaction surface; the one-shot render path remains independent.
use crate::{DataOnly, RenderOptions, Result, counters, document_config, fonts, geometry};
use anyrender::{PaintScene as _, render_to_buffer};
use anyrender_vello_cpu::VelloCpuImageRenderer;
use blitz_dom::{LocalName, Namespace, Node, NodeData, NodeId, QualName, util::Color};
use blitz_html::HtmlDocument;
use peniko::{Fill, kurbo::Rect};
use serde::Serialize;
use std::sync::{Arc, atomic::Ordering};
use std::time::Instant;

/// A DOM node reference, valid only for the Document that returned it.
/// Element references survive ordinary attribute and text mutations.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NodeRef(NodeId);

/// Microseconds spent applying one batch of mutations and repainting.
/// The pinned resolver exposes a combined style/layout pass: layout_us
/// includes both phases, and style_us is zero (not independently measured).
#[derive(Clone, Copy, Debug, Default, Serialize)]
pub struct Timings {
    pub style_us: u64,
    pub layout_us: u64,
    pub paint_us: u64,
}

impl Timings {
    pub fn total_us(&self) -> u64 {
        self.style_us + self.layout_us + self.paint_us
    }
}

/// A parsed, laid-out document held across interactions.
///
/// Mutation methods mark the retained tree dirty. update() resolves its
/// damage and repaints once; paint() returns the latest RGBA8 bitmap,
/// resolving first when a mutation has not yet been updated.
pub struct Document {
    doc: HtmlDocument,
    width: u32,
    height: u32,
    system_fonts: bool,
    fonts: Vec<Vec<u8>>,
    _net: Arc<DataOnly>,
    rgba: Vec<u8>,
    dirty: bool,
}

fn height_of(doc: &blitz_dom::BaseDocument) -> u32 {
    fn walk(doc: &blitz_dom::BaseDocument, node: &Node, max_bottom: &mut f64) {
        if matches!(&node.data, NodeData::Element(_)) {
            if let Some(rect) = geometry::element_box(doc, node) {
                *max_bottom = max_bottom.max(rect.y + rect.height);
            }
        }
        for &child in &node.children {
            if let Some(child) = doc.get_node(child) {
                walk(doc, child, max_bottom);
            }
        }
    }
    let mut max_bottom = 800.0_f64;
    walk(doc, doc.root_node(), &mut max_bottom);
    max_bottom.ceil().clamp(800.0, 32768.0) as u32
}

fn name(local: &str) -> QualName {
    QualName::new(None, Namespace::from(""), LocalName::from(local))
}

impl Document {
    /// Parse, style and lay out once, retaining all DOM and layout state.
    pub fn new(html: &str, opts: RenderOptions) -> Result<Self> {
        let RenderOptions { width, system_fonts, fonts } = opts;
        if !(1..=8192).contains(&width) {
            return Err("width must be in 1..=8192".into());
        }
        let _scope = blitz_dom::util::svg_font_scope(system_fonts, &fonts);
        let net = Arc::new(DataOnly::default());
        let mut doc = HtmlDocument::from_html(
            html,
            document_config(
                width,
                Some(fonts::context(system_fonts, &fonts, html)),
                Some(net.clone()),
            ),
        );
        let mut settled = false;
        for _ in 0..64 {
            let before = net.requests.load(Ordering::SeqCst);
            doc.resolve(0.0);
            if before == net.requests.load(Ordering::SeqCst)
                && !doc.has_pending_critical_resources()
            {
                doc.resolve(0.0);
                settled = true;
                break;
            }
        }
        if !settled {
            return Err("resources did not settle after 64 passes".into());
        }
        if counters::materialize(&mut doc) {
            doc.resolve(0.0);
        }
        // The vendored damage pipeline is incremental by default, but keep
        // the choice explicit for the retained API.
        doc.set_incremental_layout(true);
        let height = height_of(&doc);
        let mut this = Self {
            doc, width, height, system_fonts, fonts, _net: net,
            rgba: Vec::new(), dirty: false,
        };
        this.repaint();
        Ok(this)
    }

    pub fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// Find an element by its HTML id; convenient for stable interaction targets.
    pub fn element_by_id(&self, id: &str) -> Option<NodeRef> {
        fn find(doc: &blitz_dom::BaseDocument, node: &Node, id: &str) -> Option<NodeId> {
            if matches!(&node.data, NodeData::Element(_))
                && node.attr(blitz_dom::local_name!("id")) == Some(id)
            {
                return Some(node.id);
            }
            for &child_id in &node.children {
                if let Some(child) = doc.get_node(child_id) {
                    if let Some(found) = find(doc, child, id) {
                        return Some(found);
                    }
                }
            }
            None
        }
        find(&self.doc, self.doc.root_node(), id).map(NodeRef)
    }

    pub fn attribute(&self, node: NodeRef, attr: &str) -> Option<String> {
        self.doc.get_node(node.0)
            .and_then(|node| node.attr(LocalName::from(attr)))
            .map(str::to_owned)
    }

    /// Topmost paint-order element at the CSS-pixel point. The vendored hit
    /// traversal handles stacking contexts, pointer-events, visibility and
    /// transformed descendants, and resolves anonymous/text hits to elements.
    /// Call update() after mutations before querying layout.
    pub fn hit_test(&self, x_css: f64, y_css: f64) -> Option<NodeRef> {
        if !x_css.is_finite() || !y_css.is_finite()
            || x_css < 0.0 || y_css < 0.0
            || x_css >= self.width as f64 || y_css >= self.height as f64
        {
            return None;
        }
        let hit = self.doc.hit(x_css as f32, y_css as f32)?;
        let mut id = self.doc.nearest_non_anonymous_ancestor(hit.node_id)?;
        loop {
            let node = self.doc.get_node(id)?;
            if matches!(&node.data, NodeData::Element(_)) {
                return Some(NodeRef(id));
            }
            id = node.parent?;
        }
    }

    fn check_element(&self, node: NodeRef) -> Result<()> {
        if !self.doc.get_node(node.0)
            .is_some_and(|node| matches!(&node.data, NodeData::Element(_)))
        {
            return Err("node is not an element in this document".into());
        }
        Ok(())
    }

    /// Change or insert an HTML attribute.
    pub fn set_attribute(&mut self, node: NodeRef, attr: &str, value: &str) -> Result<()> {
        self.check_element(node)?;
        if attr.is_empty() || attr.chars().any(|c| c.is_ascii_whitespace()) {
            return Err("invalid attribute name".into());
        }
        self.doc.mutate().set_attribute(node.0, name(attr), value);
        self.dirty = true;
        Ok(())
    }

    /// Add a class if absent, otherwise remove it.
    pub fn toggle_class(&mut self, node: NodeRef, class: &str) -> Result<()> {
        self.check_element(node)?;
        if class.is_empty() || class.chars().any(char::is_whitespace) {
            return Err("class must be one nonempty token".into());
        }
        let mut classes: Vec<String> = self.attribute(node, "class")
            .unwrap_or_default().split_whitespace().map(str::to_owned).collect();
        if classes.iter().any(|item| item == class) {
            classes.retain(|item| item != class);
        } else {
            classes.push(class.to_owned());
        }
        let mut mutator = self.doc.mutate();
        if classes.is_empty() {
            mutator.clear_attribute(node.0, name("class"));
        } else {
            mutator.set_attribute(node.0, name("class"), &classes.join(" "));
        }
        self.dirty = true;
        Ok(())
    }

    /// Change a text node, or replace an element's textContent.
    pub fn set_text(&mut self, node: NodeRef, text: &str) -> Result<()> {
        let old = self.doc.get_node(node.0)
            .ok_or("node is not in this document")?;
        let text_target = match &old.data {
            NodeData::Text(_) => Some(node.0),
            NodeData::Element(_) if old.children.len() == 1 => {
                let child_id = old.children[0];
                self.doc.get_node(child_id).and_then(|child| {
                    matches!(&child.data, NodeData::Text(_)).then_some(child_id)
                })
            }
            NodeData::Element(_) => None,
            _ => return Err("node cannot contain text".into()),
        };
        let mut mutator = self.doc.mutate();
        if let Some(id) = text_target {
            mutator.set_node_text(id, text);
        } else {
            let child = mutator.create_text_node(text);
            mutator.replace_children(node.0, &[child]);
        }
        self.dirty = true;
        Ok(())
    }

    fn repaint(&mut self) {
        let _scope = blitz_dom::util::svg_font_scope(self.system_fonts, &self.fonts);
        self.rgba = render_to_buffer::<VelloCpuImageRenderer, _>(
            |scene| {
                scene.fill(
                    Fill::NonZero, Default::default(), Color::WHITE,
                    Default::default(),
                    &Rect::new(0.0, 0.0, self.width as f64, self.height as f64),
                );
                blitz_paint::paint_scene(
                    scene, &mut self.doc, 1.0, self.width, self.height, 0, 0,
                );
            },
            self.width, self.height,
        );
    }

    /// Apply dirty style/layout caches and paint. The engine exposes a single
    /// resolve() operation; layout_us includes that combined style/layout pass
    /// plus height calculation. No artificial split is reported.
    pub fn update(&mut self) -> Timings {
        if !self.dirty {
            return Timings::default();
        }
        let _scope = blitz_dom::util::svg_font_scope(self.system_fonts, &self.fonts);
        let layout_start = Instant::now();
        self.doc.resolve(0.0);
        self.height = height_of(&self.doc);
        let layout_us = layout_start.elapsed().as_micros() as u64;
        let paint_start = Instant::now();
        self.repaint();
        let paint_us = paint_start.elapsed().as_micros() as u64;
        self.dirty = false;
        Timings { style_us: 0, layout_us, paint_us }
    }

    /// RGBA8 pixels for the current retained document. Calling this after
    /// update() only clones the already repainted buffer (not timed).
    pub fn paint(&mut self) -> Vec<u8> {
        if self.dirty {
            self.update();
        }
        self.rgba.clone()
    }
}
