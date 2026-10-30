// Copyright 2026 g1BSsQ. Licensed under Apache-2.0.
// Retained interaction surface; the one-shot render path remains independent.
use crate::{DataOnly, RenderOptions, Result, counters, damage_paint, document_config, fonts, geometry, motion};
use anyrender::{PaintScene as _, render_to_buffer};
use anyrender_vello_cpu::VelloCpuImageRenderer;
use blitz_dom::{LocalName, Namespace, Node, NodeData, NodeId, QualName, util::Color};
use blitz_html::HtmlDocument;
use peniko::{Fill, kurbo::{Affine, Rect}};
use std::collections::{HashMap, HashSet};
use style::stylesheets::Origin;
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
    /// Number of device pixels invalidated by this update.
    pub damage_px: u64,
    /// Invalidated device-pixel rectangle (x, y, width, height).
    /// None denotes an unbounded repaint or no pending update.
    pub damage_rect_px: Option<(u32, u32, u32, u32)>,
    /// "partial", "full_viewport", or "full_document" without a viewport.
    pub paint_path: &'static str,
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
    /// Device pixels per CSS pixel for vector scene painting.
    scale: f32,
    bitmap_width: u32,
    bitmap_height: u32,
    system_fonts: bool,
    fonts: Vec<Vec<u8>>,
    _net: Arc<DataOnly>,
    rgba: Vec<u8>,
    dirty: bool,
    viewport_height: Option<u32>,
    scroll_y_css: f64,
    before: Option<damage_paint::Snapshot>,
    touched: HashSet<NodeId>,
    motion_tracks: Vec<motion::Track>,
    motion_definitions: HashMap<String, Vec<motion::Frame>>,
    motion_style_nodes: Vec<NodeId>,
    motion_style_sources: Vec<String>,
    motion_media_dirty: bool,
    motion_transitions: Vec<motion::Transition>,
    motion_saved_inline: HashMap<NodeId, Option<String>>,
    motion_applied: HashMap<NodeId, motion::Sample>,
    pending_transition_origins: HashMap<NodeId, motion::Sample>,
    motion_clock_ms: f64,
    reduced_motion: bool,
}

fn height_of(doc: &blitz_dom::BaseDocument) -> u32 {
    fn walk(doc: &blitz_dom::BaseDocument, node: &Node, scroll: f64, max_bottom: &mut f64) {
        if matches!(&node.data, NodeData::Element(_)) {
            if let Some(rect) = geometry::element_box(doc, node) {
                // CSSOM rectangles are viewport-relative. Convert each
                // bottom edge back to document space before taking the max;
                // the current scroll position must not inflate the height
                // after content shrinks above the viewport.
                *max_bottom = (*max_bottom).max(rect.y + rect.height + scroll);
            }
        }
        for &child in &node.children {
            if let Some(child) = doc.get_node(child) {
                walk(doc, child, scroll, max_bottom);
            }
        }
    }
    let mut max_bottom = 800.0_f64;
    walk(doc, doc.root_node(), doc.viewport_scroll().y, &mut max_bottom);
    max_bottom.ceil().clamp(800.0, 32768.0) as u32
}

fn name(local: &str) -> QualName {
    QualName::new(None, Namespace::from(""), LocalName::from(local))
}

impl Document {
    /// Parse, style and lay out once, retaining all DOM and layout state.
    pub fn new(html: &str, opts: RenderOptions) -> Result<Self> {
        Self::new_scaled(html, opts, 1.0)
    }

    /// Retain a document and paint its scene at device scale, without bitmap
    /// upscaling. CSS layout and hit-test coordinates remain unscaled.
    pub fn new_scaled(html: &str, opts: RenderOptions, scale: f32) -> Result<Self> {
        if !scale.is_finite() || !(0.25..=4.0).contains(&scale) {
            return Err("scale must be finite and in 0.25..=4.0".into());
        }
        let RenderOptions { width, system_fonts, fonts } = opts;
        if !(1..=8192).contains(&width) {
            return Err("width must be in 1..=8192".into());
        }
        let _scope = blitz_dom::util::svg_font_scope(system_fonts, &fonts);
        let net = Arc::new(DataOnly::default());
        let normalized_html = super::line_clamp_css::normalize(html);
        let motion_style_sources = motion::style_texts(&normalized_html);
        let retained_html = motion::reduced_media_css(&normalized_html, false);
        let mut doc = HtmlDocument::from_html(
            &retained_html,
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
        let motion_style_nodes = motion::style_nodes(&doc);
        let motion_definitions = motion::keyframes(&normalized_html);
        let motion_tracks = motion::read_tracks(&doc, &motion_definitions, &motion_style_sources);
        let height = height_of(&doc);
        let bitmap_width = ((width as f64) * f64::from(scale)).round().max(1.0) as u32;
        let bitmap_height = ((height as f64) * f64::from(scale)).round().max(1.0) as u32;
        let this = Self {
            doc, width, height, scale, bitmap_width, bitmap_height,
            system_fonts, fonts, _net: net,
            rgba: Vec::new(), dirty: false,
            viewport_height: None, scroll_y_css: 0.0,
            before: None, touched: HashSet::new(),
            motion_tracks, motion_definitions, motion_style_nodes,
            motion_style_sources, motion_media_dirty: false,
            motion_transitions: Vec::new(),
            motion_saved_inline: HashMap::new(), motion_applied: HashMap::new(),
            pending_transition_origins: HashMap::new(),
            motion_clock_ms: 0.0, reduced_motion: false,
        };
        Ok(this)
    }

    /// CSS-pixel layout dimensions, independent of raster scale.
    pub fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// Actual dimensions of the vector-painted RGBA8 bitmap in device pixels.
    pub fn bitmap_dimensions(&self) -> (u32, u32) {
        (self.bitmap_width, self.bitmap_height)
    }

    /// Set the visible region in CSS pixels. The cached bitmap then contains
    /// only this viewport, painted at device scale. Layout remains document-wide.
    /// Omitting this call retains the old full-document bitmap behavior.
    pub fn set_viewport(&mut self, scroll_y_css: f64, height_css: u32) -> Result<()> {
        if !scroll_y_css.is_finite() || scroll_y_css < 0.0
            || !(1..=32768).contains(&height_css)
        {
            return Err("invalid viewport scroll or height".into());
        }
        if self.dirty {
            self.update();
        }
        self.viewport_height = Some(height_css);
        self.scroll_y_css = scroll_y_css.min(self.height.saturating_sub(height_css) as f64);
        self.bitmap_height =
            ((height_css as f64) * f64::from(self.scale)).round().max(1.0) as u32;
        self.doc.set_viewport_scroll(blitz_dom::Point {
            x: 0.0, y: self.scroll_y_css,
        });
        self.repaint();
        // get_client_bounding_rect() is viewport-relative. Capture after
        // scrolling so the next update compares bounds in one coordinate space,
        // matching the pixels that were just cached.
        self.before = Some(damage_paint::capture(&self.doc));
        Ok(())
    }

    /// Current top edge of the rendered viewport in document CSS pixels.
    pub fn viewport_scroll_y(&self) -> f64 {
        self.scroll_y_css
    }

    /// CSS layout bounds relative to the current viewport. Before scrolling,
    /// these can be used to position the viewport around an interaction target.
    pub fn element_bounds(&self, node: NodeRef) -> Option<(f64, f64, f64, f64)> {
        let node = self.doc.get_node(node.0)?;
        let b = geometry::element_box(&self.doc, node)?;
        Some((b.x, b.y, b.width, b.height))
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
        // hit() takes document coordinates, but element_box() returns a
        // viewport-relative CSSOM rect. Use viewport coordinates only for the
        // post-hit ancestor bounds check, not for the paint-order traversal.
        let hit = self.doc.hit(x_css as f32, y_css as f32)?;
        let y_viewport = y_css - self.scroll_y_css;
        let mut id = self.doc.nearest_non_anonymous_ancestor(hit.node_id)?;
        loop {
            let node = self.doc.get_node(id)?;
            if matches!(&node.data, NodeData::Element(_)) {
                if let Some(bounds) = geometry::element_box(&self.doc, node) {
                    if bounds.width > 0.0 && bounds.height > 0.0
                        && x_css >= bounds.x && x_css < bounds.x + bounds.width
                        && y_viewport >= bounds.y
                        && y_viewport < bounds.y + bounds.height
                    {
                        return Some(NodeRef(id));
                    }
                }
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

    fn begin_change(&mut self, node: NodeRef) {
        self.pending_transition_origins.entry(node.0).or_insert_with(
            || motion::computed_sample(&self.doc, node.0)
        );
        self.touched.insert(node.0);
    }

    /// Change or insert an HTML attribute.
    pub fn set_attribute(&mut self, node: NodeRef, attr: &str, value: &str) -> Result<()> {
        self.check_element(node)?;
        if attr.is_empty() || attr.chars().any(|c| c.is_ascii_whitespace()) {
            return Err("invalid attribute name".into());
        }
        self.begin_change(node);
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
        self.begin_change(node);
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
        self.begin_change(node);
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
                    &Rect::new(0.0, 0.0, self.bitmap_width as f64, self.bitmap_height as f64),
                );
                // Paint CSS geometry and text as vector content at device
                // scale. Never stretch an already-rasterized 1x buffer.
                blitz_paint::paint_scene(
                    scene, &mut self.doc, f64::from(self.scale),
                    self.bitmap_width, self.bitmap_height, 0, 0,
                );
            },
            self.bitmap_width, self.bitmap_height,
        );
    }

    // Keep the full viewport's raster coordinates while restricting all
    // paint commands to an integer device-pixel damage rectangle. Rasterizing
    // a separately translated tile changes the CPU rasterizer's coverage
    // rounding at fractional CSS edges; an interior pixel can differ by one
    // color channel from an otherwise identical full-viewport paint.
    //
    // A rectangular clip is a hard raster clip, not an opacity group. It
    // surrounds the original scene so ancestor backgrounds, stacking contexts
    // and nested layers are replayed unchanged at the viewport origin.
    fn repaint_rect(&mut self, x: u32, y: u32, width: u32, height: u32) {
        let viewport_width = self.bitmap_width;
        let viewport_height = self.bitmap_height;
        let tile = render_to_buffer::<VelloCpuImageRenderer, _>(
            |scene| {
                scene.push_clip_layer(
                    Fill::NonZero,
                    Affine::IDENTITY,
                    &Rect::new(
                        x as f64, y as f64,
                        (x + width) as f64, (y + height) as f64,
                    ),
                );
                scene.fill(
                    Fill::NonZero, Affine::IDENTITY, Color::WHITE, None,
                    &Rect::new(0.0, 0.0, viewport_width as f64, viewport_height as f64),
                );
                blitz_paint::paint_scene(
                    scene, &mut self.doc, f64::from(self.scale),
                    viewport_width, viewport_height, 0, 0,
                );
                scene.pop_layer();
            },
            viewport_width, viewport_height,
        );
        let row_bytes = width as usize * 4;
        let stride = viewport_width as usize * 4;
        for row in y as usize..(y + height) as usize {
            let from = row * stride + x as usize * 4;
            self.rgba[from..from + row_bytes].copy_from_slice(&tile[from..from + row_bytes]);
        }
    }

    /// Apply dirty style/layout caches, then repaint only changed viewport ink.
    /// The engine exposes one combined restyle/layout resolve operation.
    pub fn update(&mut self) -> Timings {
        if !self.dirty {
            return Timings::default();
        }
        let _scope = blitz_dom::util::svg_font_scope(self.system_fonts, &self.fonts);
        let layout_start = Instant::now();
        self.doc.resolve(0.0);
        for (id, old) in std::mem::take(&mut self.pending_transition_origins) {
            let fresh = motion::new_transitions(
                &self.doc, id, &old, self.motion_clock_ms, &self.motion_style_sources,
            );
            for next in fresh {
                self.motion_transitions.retain(|prior|
                    !(prior.node == next.node && prior.property == next.property));
                self.motion_transitions.push(next);
            }
        }
        self.height = height_of(&self.doc);
        // A mutation can shorten the document beneath an existing viewport.
        // Repositioning the viewport changes every cached pixel, regardless
        // of the element damage computed in the old scroll coordinate space.
        let scroll_changed = if let Some(viewport_height) = self.viewport_height {
            let clamped = self.scroll_y_css.min(
                self.height.saturating_sub(viewport_height) as f64
            );
            if clamped != self.scroll_y_css {
                self.scroll_y_css = clamped;
                self.doc.set_viewport_scroll(blitz_dom::Point {
                    x: 0.0, y: clamped,
                });
                true
            } else {
                false
            }
        } else {
            false
        };
        if self.viewport_height.is_none() {
            self.bitmap_height =
                ((self.height as f64) * f64::from(self.scale)).round().max(1.0) as u32;
        }
        let before = self.before.take();
        let damage = if self.viewport_height.is_some() {
            let after = damage_paint::capture(&self.doc);
            let bounds = if scroll_changed {
                None
            } else {
                before.as_ref()
                    .and_then(|old| damage_paint::damage(old, &after, &self.touched))
            };
            self.before = Some(after);
            bounds
        } else {
            None
        };
        let layout_us = layout_start.elapsed().as_micros() as u64;
        let paint_start = Instant::now();
        let (damage_px, damage_rect_px, paint_path) = if self.viewport_height.is_none() {
            self.repaint();
            (self.bitmap_width as u64 * self.bitmap_height as u64, None, "full_document")
        } else if let Some(rect) = damage {
            let scale = f64::from(self.scale);
            let x0 = (rect.x0 * scale).floor().clamp(0.0, self.bitmap_width as f64) as u32;
            let x1 = (rect.x1 * scale).ceil().clamp(0.0, self.bitmap_width as f64) as u32;
            let y0 = (rect.y0 * scale).floor()
                .clamp(0.0, self.bitmap_height as f64) as u32;
            let y1 = (rect.y1 * scale).ceil()
                .clamp(0.0, self.bitmap_height as f64) as u32;
            if x1 > x0 && y1 > y0 {
                self.repaint_rect(x0, y0, x1 - x0, y1 - y0);
            }
            (u64::from(x1.saturating_sub(x0)) * u64::from(y1.saturating_sub(y0)),
             Some((x0, y0, x1.saturating_sub(x0), y1.saturating_sub(y0))),
             "partial")
        } else {
            self.repaint();
            (self.bitmap_width as u64 * self.bitmap_height as u64, None, "full_viewport")
        };
        let paint_us = paint_start.elapsed().as_micros() as u64;
        self.touched.clear();
        self.dirty = false;
        Timings { style_us: 0, layout_us, paint_us, damage_px, damage_rect_px, paint_path }
    }

    /// Sample CSS motion at the supplied monotonic clock time (milliseconds).
    ///
    /// Call update() once after a DOM mutation before ticking. A tick never
    /// calls the combined resolve/layout pipeline: only the style traversal,
    /// cached paint transforms and old/new ink damage are updated.
    /// Returns (frame timings, whether another frame is needed).
    pub fn tick(&mut self, now_ms: f64) -> (Timings, bool) {
        if !now_ms.is_finite() || now_ms < 0.0 || self.dirty {
            return (Timings::default(), !self.motion_tracks.is_empty()
                || !self.motion_transitions.is_empty());
        }
        self.motion_clock_ms = now_ms;
        let mut desired: HashMap<NodeId, motion::Sample> = HashMap::new();
        let mut needs_frames = false;
        // Transitions are applied first; running keyframes replace them.
        for transition in &self.motion_transitions {
            let (value, more) = transition.sample(now_ms, self.reduced_motion);
            needs_frames |= more;
            let entry = desired.entry(transition.node).or_default();
            match transition.property.as_str() {
                "opacity" => entry.opacity = value.parse().ok(),
                "transform" => entry.transform = Some(value),
                _ => {}
            }
        }
        for track in &self.motion_tracks {
            let (value, more) = track.sample(now_ms, self.reduced_motion);
            needs_frames |= more;
            if let Some(sample) = value {
                let entry = desired.entry(track.node).or_default();
                if sample.opacity.is_some() { entry.opacity = sample.opacity; }
                if sample.transform.is_some() { entry.transform = sample.transform; }
            }
        }

        let mut changed: HashSet<NodeId> = HashSet::new();
        for (&id, value) in &desired {
            if self.motion_applied.get(&id) != Some(value) { changed.insert(id); }
        }
        for &id in self.motion_applied.keys() {
            if !desired.contains_key(&id) { changed.insert(id); }
        }
        if changed.is_empty() && !self.motion_media_dirty {
            self.motion_transitions.retain(|t|
                t.sample(now_ms, self.reduced_motion).1);
            return (Timings { paint_path: "partial", ..Timings::default() }, needs_frames);
        }

        let old = self.before.clone().unwrap_or_else(|| damage_paint::capture(&self.doc));
        let style_start = Instant::now();
        if !changed.is_empty() {
        for &id in &changed {
            let saved = self.motion_saved_inline.entry(id).or_insert_with(|| {
                self.doc.get_node(id).and_then(|node|
                    node.attr(blitz_dom::local_name!("style")).map(str::to_owned))
            }).clone();
            // This sampler owns transform/opacity motion for this element.
            // An active CSS transition outranks even !important author rules,
            // so suppress engine transitions in the *same* inline declaration
            // block as the sampled value. Also suppress engine keyframes when
            // we are explicitly sampling a keyframe track. Restore the original
            // inline style when the sample ends.
            let mut mutator = self.doc.mutate();
            if let Some(value) = desired.get(&id) {
                let mut inline = saved.unwrap_or_default();
                if !inline.is_empty() && !inline.trim_end().ends_with(';') {
                    inline.push(';');
                }
                if let Some(transform) = &value.transform {
                    inline.push_str("transform:");
                    inline.push_str(transform);
                    inline.push_str(" !important;");
                }
                if let Some(opacity) = value.opacity {
                    inline.push_str("opacity:");
                    inline.push_str(&opacity.to_string());
                    inline.push_str(" !important;");
                }
                // The inline declaration and transition suppression are
                // applied atomically, so a sampled property change cannot
                // start a second, engine-owned transition at this tick.
                inline.push_str("transition-property:none !important;");
                if self.motion_tracks.iter().any(|track| track.node == id) {
                    inline.push_str("animation-name:none !important;");
                }
                mutator.set_attribute(id, name("style"), &inline);
            } else if let Some(style) = saved {
                mutator.set_attribute(id, name("style"), &style);
            } else {
                mutator.clear_attribute(id, name("style"));
            }
        }
        // This deliberately stops before box construction and layout.
        // Reduced motion must also advance the style engine's own animation
        // timeline to the terminal sample, not merely our paint sampler.
        let style_time = if self.reduced_motion {
            self.motion_tracks.iter().filter_map(motion::Track::terminal_time_ms)
                .fold(now_ms, f64::max)
        } else { now_ms };
        self.doc.resolve_stylist(style_time / 1000.0);
        for &id in &changed {
            if let Some(node) = self.doc.get_node_mut(id) {
                node.set_transform(1.0);
            }
        }
        }
        let style_us = style_start.elapsed().as_micros() as u64;
        let after = damage_paint::capture(&self.doc);
        let damage = damage_paint::damage(&old, &after, &changed);
        self.before = Some(after);
        self.motion_media_dirty = false;
        self.motion_applied = desired;

        // Keep ending transitions alive until their terminal value has been
        // applied and painted in this tick (even though more is already false).
        let paint_start = Instant::now();
        let (damage_px, damage_rect_px, paint_path) = if self.viewport_height.is_none() {
            self.repaint();
            (u64::from(self.bitmap_width) * u64::from(self.bitmap_height),
                None, "full_document")
        } else if let Some(rect) = damage {
            let scale = f64::from(self.scale);
            let x0 = (rect.x0 * scale).floor().clamp(0.0, f64::from(self.bitmap_width)) as u32;
            let x1 = (rect.x1 * scale).ceil().clamp(0.0, f64::from(self.bitmap_width)) as u32;
            let y0 = (rect.y0 * scale).floor().clamp(0.0, f64::from(self.bitmap_height)) as u32;
            let y1 = (rect.y1 * scale).ceil().clamp(0.0, f64::from(self.bitmap_height)) as u32;
            if x1 > x0 && y1 > y0 {
                self.repaint_rect(x0, y0, x1 - x0, y1 - y0);
            }
            (u64::from(x1.saturating_sub(x0)) * u64::from(y1.saturating_sub(y0)),
                Some((x0, y0, x1.saturating_sub(x0), y1.saturating_sub(y0))),
                "partial")
        } else {
            self.repaint();
            (u64::from(self.bitmap_width) * u64::from(self.bitmap_height),
                None, "full_viewport")
        };
        let paint_us = paint_start.elapsed().as_micros() as u64;
        self.motion_transitions.retain(|t|
            t.sample(now_ms, self.reduced_motion).1);
        (Timings {
            style_us, layout_us: 0, paint_us, damage_px, damage_rect_px, paint_path,
        }, needs_frames)
    }

    /// Select the reduced-motion mode. Motion jumps to the final keyframe or
    /// transition target on the next tick; no layout is performed by tick().
    pub fn set_reduced_motion(&mut self, reduced: bool) {
        if self.reduced_motion == reduced { return; }
        self.reduced_motion = reduced;
        // Remove paint-time inline samples before evaluating the new media
        // rules. Otherwise a previous keyframe sample would incorrectly
        // outrank author rules that disable/reduce that animation.
        if self.motion_style_sources.iter().any(|css|
            css.to_ascii_lowercase().contains("prefers-reduced-motion")
        ) {
            for (&id, original) in &self.motion_saved_inline {
                let mut mutator = self.doc.mutate();
                if let Some(style) = original {
                    mutator.set_attribute(id, name("style"), style);
                } else {
                    mutator.clear_attribute(id, name("style"));
                }
            }
            self.motion_applied.clear();
        }
        let mut changed_styles = false;
        for (&id, css) in self.motion_style_nodes.iter().zip(&self.motion_style_sources) {
            if !css.to_ascii_lowercase().contains("prefers-reduced-motion") { continue; }
            let adjusted = motion::reduced_media_css(css, reduced);
            let sheet = self.doc.make_stylesheet(&adjusted, Origin::Author);
            self.doc.add_stylesheet_for_node(sheet, id);
            changed_styles = true;
        }
        if changed_styles {
            // This is a stylesheet recascade, *not* a box reconstruction or
            // relayout. The next tick will repaint its old/new style damage.
            self.doc.resolve_stylist(self.motion_clock_ms / 1000.0);
            // Transform matrices are a paint cache: update them after the
            // recascade without invalidating or recomputing layout boxes.
            for id in motion::element_nodes(&self.doc) {
                if let Some(node) = self.doc.get_node_mut(id) {
                    if node.element_data()
                        .and_then(|data| data.layout_data.as_ref()).is_some() {
                        node.set_transform(1.0);
                    }
                }
            }
            self.motion_tracks = motion::read_tracks(
                &self.doc, &self.motion_definitions, &self.motion_style_sources,
            );
            self.motion_media_dirty = true;
        }
    }

    /// RGBA8 pixels for the current retained document. Calling this after
    /// update() only clones the already repainted buffer (not timed).
    pub fn paint(&mut self) -> Vec<u8> {
        if self.dirty {
            self.update();
        }
        if self.rgba.is_empty() {
            self.repaint();
        }
        self.rgba.clone()
    }
}
