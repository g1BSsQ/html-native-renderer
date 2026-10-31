// Copyright 2026 g1BSsQ. Licensed under Apache-2.0.
// Retained interaction surface; the one-shot render path remains independent.
use crate::{DataOnly, RenderOptions, Result, counters, damage_paint, document_config, fonts, geometry};
use anyrender::{ImageRenderer as _, PaintScene as _, recording::Scene, render_to_buffer};
use rayon::prelude::*;
use anyrender_vello_cpu::VelloCpuImageRenderer;
use blitz_dom::{LocalName, Namespace, Node, NodeData, NodeId, QualName, util::Color};
use blitz_html::HtmlDocument;
use peniko::{Fill, kurbo::{Affine, Rect}};
use std::collections::HashSet;
use serde::Serialize;
use std::sync::{Arc, OnceLock, atomic::Ordering};
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
    /// True if rasterized into the original viewport-sized target rather
    /// than a compact damage target. This is independent of paint_path:
    /// a bounded partial update can still require a viewport-sized raster.
    pub viewport_target: bool,
    /// Active raster workers for this update (one for the serial path).
    pub raster_threads: u32,
    /// Number of independently rasterized horizontal regions.
    pub strips: u32,
    /// Partial-only drawing-command generation (including glyph/path encoding).
    pub encode_us: u64,
    /// Partial-only CPU raster and backend flush/maintenance, excluding encode.
    pub raster_us: u64,
    /// No GPU/readback exists in the CPU backend; always zero.
    pub readback_us: u64,
    /// Row copies from the compact damage target into retained RGBA.
    pub blit_us: u64,
    /// Partial-only renderer creation/resizing and reusable buffer preparation.
    pub target_setup_us: u64,
}

#[derive(Clone, Copy, Default)]
struct PaintStages {
    encode_us: u64,
    raster_us: u64,
    readback_us: u64,
    blit_us: u64,
    target_setup_us: u64,
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
    // Reuse the renderer and output allocation across damage-only updates.
    partial_renderer: Option<VelloCpuImageRenderer>,
    partial_dimensions: (u32, u32),
    partial_rgba: Vec<u8>,
    dirty: bool,
    viewport_height: Option<u32>,
    scroll_y_css: f64,
    before: Option<damage_paint::Snapshot>,
    touched: HashSet<NodeId>,
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

// Four clipped full-viewport strip rasters measured slower than serial on
// the target device (form paint p50: 23.1 ms vs 12.5 ms on the baseline).
// Do not schedule them in production until a subrect-aware backend wins a
// controlled benchmark. An explicit opt-in keeps the parity experiment
// available without penalizing ordinary interaction frames.
const EXPERIMENTAL_PARALLEL_MIN_PIXELS: u64 = 300_000;
const DEFAULT_PARALLEL_MIN_PIXELS: u64 = u64::MAX;
const MAX_RASTER_THREADS: usize = 4;
// The CPU rasterizer's fine tiles are 4×4 device pixels. We align compact
// target origins and extents to a conservative 16-pixel multiple of that
// grid; 16 is not the fine-tile size and does not alone guarantee parity.
// This may pad past a viewport's right/bottom edge. The reported damage
// bounds remain unchanged; only internal raster dimensions grow.
const CPU_RASTER_ALIGNMENT_PX: u32 = 16;

fn aligned_raster_target(
    x: u32, y: u32, width: u32, height: u32,
) -> (u32, u32, u32, u32) {
    let tile = CPU_RASTER_ALIGNMENT_PX;
    let x0 = (x / tile) * tile;
    let y0 = (y / tile) * tile;
    let x1 = (x + width).div_ceil(tile) * tile;
    let y1 = (y + height).div_ceil(tile) * tile;
    (x0, y0, x1 - x0, y1 - y0)
}

fn parallel_threshold() -> u64 {
    static THRESHOLD: OnceLock<u64> = OnceLock::new();
    *THRESHOLD.get_or_init(|| {
        if std::env::var("ISR_EXPERIMENTAL_RASTER_STRIPS").as_deref() == Ok("1") {
            EXPERIMENTAL_PARALLEL_MIN_PIXELS
        } else {
            DEFAULT_PARALLEL_MIN_PIXELS
        }
    })
}

fn raster_pool() -> &'static rayon::ThreadPool {
    static POOL: OnceLock<rayon::ThreadPool> = OnceLock::new();
    POOL.get_or_init(|| {
        let cores = std::thread::available_parallelism().map_or(1, |n| n.get());
        rayon::ThreadPoolBuilder::new()
            .num_threads(cores.min(MAX_RASTER_THREADS))
            .build()
            .expect("raster worker pool")
    })
}

// A recorded scene is generated on the Document thread, then independently
// replayed under integer viewport-space clips. Each worker preserves the
// viewport origin and the full compositing stack; translated sub-pixmaps
// change fractional-edge coverage. This helper can also raster a tick scene.
pub(crate) fn raster_scene_strips(
    scene: &Scene,
    viewport_width: u32,
    viewport_height: u32,
    rect: (u32, u32, u32, u32),
    strip_count: usize,
) -> Vec<(u32, u32, Vec<u8>)> {
    let (x, y, width, height) = rect;
    let tasks: Vec<_> = (0..strip_count).map(|index| {
        let top = y + (u64::from(height) * index as u64 / strip_count as u64) as u32;
        let bottom = y + (u64::from(height) * (index + 1) as u64 / strip_count as u64) as u32;
        (top, bottom, scene.clone())
    }).collect();
    raster_pool().install(|| tasks.into_par_iter().map(|(top, bottom, recording)| {
        let pixels = render_to_buffer::<VelloCpuImageRenderer, _>(
            |painter| {
                painter.push_clip_layer(
                    Fill::NonZero, Affine::IDENTITY,
                    &Rect::new(x as f64, top as f64,
                        (x + width) as f64, bottom as f64),
                );
                painter.fill(
                    Fill::NonZero, Affine::IDENTITY, Color::WHITE, None,
                    &Rect::new(0.0, 0.0,
                        viewport_width as f64, viewport_height as f64),
                );
                painter.append_scene(recording, Affine::IDENTITY);
                painter.pop_layer();
            },
            viewport_width, viewport_height,
        );
        (top, bottom, pixels)
    }).collect())
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
        let mut doc = HtmlDocument::from_html(
            &normalized_html,
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
        let bitmap_width = ((width as f64) * f64::from(scale)).round().max(1.0) as u32;
        let bitmap_height = ((height as f64) * f64::from(scale)).round().max(1.0) as u32;
        let this = Self {
            doc, width, height, scale, bitmap_width, bitmap_height,
            system_fonts, fonts, _net: net,
            rgba: Vec::new(),
            partial_renderer: None, partial_dimensions: (0, 0),
            partial_rgba: Vec::new(), dirty: false,
            viewport_height: None, scroll_y_css: 0.0,
            before: None, touched: HashSet::new(),
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

    // Raster the damage rectangle in a 16px-aligned compact target. The
    // larger internal bounds preserve the CPU rasterizer's tile phase; only
    // the original (unsnapped) damage pixels are copied back. For gradients,
    // blur, opacity/compositing, transforms and generated pseudo paint that
    // intersect old/new damage, use the original viewport-origin clipped
    // scene and viewport-sized target: those effects are not proven to be
    // byte-invariant under integer translation.
    fn repaint_rect(
        &mut self, x: u32, y: u32, width: u32, height: u32,
        viewport_target: bool,
    ) -> PaintStages {
        let mut stages = PaintStages::default();
        let (target_x, target_y, target_w, target_h) = if viewport_target {
            (0, 0, self.bitmap_width, self.bitmap_height)
        } else {
            aligned_raster_target(x, y, width, height)
        };
        let setup_start = Instant::now();
        if self.partial_renderer.is_none() {
            self.partial_renderer = Some(VelloCpuImageRenderer::new(target_w, target_h));
            self.partial_dimensions = (target_w, target_h);
        } else if self.partial_dimensions != (target_w, target_h) {
            self.partial_renderer.as_mut().unwrap().resize(target_w, target_h);
            self.partial_dimensions = (target_w, target_h);
        }
        self.partial_rgba.resize(target_w as usize * target_h as usize * 4, 0);
        stages.target_setup_us = setup_start.elapsed().as_micros() as u64;

        let renderer = self.partial_renderer.as_mut().unwrap();
        renderer.reset();
        let encode = std::cell::Cell::new(0_u64);
        let raster_start = Instant::now();
        renderer.render(
            |scene| {
                let encode_start = Instant::now();
                if viewport_target {
                    // Byte-proven original partial raster: full viewport
                    // geometry, full viewport tile phase, clip only to dirty
                    // ink, and no damage-aware subtree pruning.
                    scene.push_clip_layer(
                        Fill::NonZero, Affine::IDENTITY,
                        &Rect::new(x as f64, y as f64,
                            (x + width) as f64, (y + height) as f64),
                    );
                    scene.fill(
                        Fill::NonZero, Affine::IDENTITY, Color::WHITE, None,
                        &Rect::new(0.0, 0.0,
                            self.bitmap_width as f64, self.bitmap_height as f64),
                    );
                    blitz_paint::paint_scene(
                        scene, &mut self.doc, f64::from(self.scale),
                        self.bitmap_width, self.bitmap_height, 0, 0,
                    );
                    scene.pop_layer();
                } else {
                    scene.fill(
                        Fill::NonZero, Affine::IDENTITY, Color::WHITE, None,
                        &Rect::new(0.0, 0.0, target_w as f64, target_h as f64),
                    );
                    blitz_paint::paint_scene_damage(
                        scene, &mut self.doc, f64::from(self.scale),
                        self.bitmap_width, self.bitmap_height,
                        (target_x, target_y, target_w, target_h),
                    );
                }
                encode.set(encode_start.elapsed().as_micros() as u64);
            },
            &mut self.partial_rgba,
        );
        stages.encode_us = encode.get();
        stages.raster_us =
            (raster_start.elapsed().as_micros() as u64).saturating_sub(stages.encode_us);
        // CPU writes premultiplied RGBA directly; no readback/conversion.
        stages.readback_us = 0;

        let blit_start = Instant::now();
        let source_stride = target_w as usize * 4;
        let dest_stride = self.bitmap_width as usize * 4;
        let source_x_bytes = (x - target_x) as usize * 4;
        for row in 0..height as usize {
            let src = (row + (y - target_y) as usize) * source_stride + source_x_bytes;
            let dst = (row + y as usize) * dest_stride + x as usize * 4;
            let len = width as usize * 4;
            self.rgba[dst..dst + len]
                .copy_from_slice(&self.partial_rgba[src..src + len]);
        }
        stages.blit_us = blit_start.elapsed().as_micros() as u64;
        stages
    }

    // Retain the exact serial path for small damage and use a single recorded
    // scene for large independent strips. Only the requested rows are copied
    // into the retained bitmap, so untouched pixels remain byte-identical.
    fn repaint_rect_strips(
        &mut self, x: u32, y: u32, width: u32, height: u32,
        strip_count: usize,
    ) -> PaintStages {
        if strip_count <= 1 {
            return self.repaint_rect(x, y, width, height, false);
        }
        let raster_start = Instant::now();
        let mut scene = Scene::new();
        blitz_paint::paint_scene(
            &mut scene, &mut self.doc, f64::from(self.scale),
            self.bitmap_width, self.bitmap_height, 0, 0,
        );
        let outputs = raster_scene_strips(
            &scene, self.bitmap_width, self.bitmap_height,
            (x, y, width, height), strip_count,
        );
        let stride = self.bitmap_width as usize * 4;
        let row_bytes = width as usize * 4;
        let raster_us = raster_start.elapsed().as_micros() as u64;
        let blit_start = Instant::now();
        for (top, bottom, pixels) in outputs {
            for row in top as usize..bottom as usize {
                let offset = row * stride + x as usize * 4;
                self.rgba[offset..offset + row_bytes]
                    .copy_from_slice(&pixels[offset..offset + row_bytes]);
            }
        }
        // Experimental strips pre-record the scene; keep a coarse aggregate
        // rather than misattribute recording work to a finer stage.
        PaintStages {
            raster_us, blit_us: blit_start.elapsed().as_micros() as u64,
            ..PaintStages::default()
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
        let (damage, viewport_target) = if self.viewport_height.is_some() {
            let after = damage_paint::capture_against(&self.doc, before.as_ref());
            let bounds = if scroll_changed {
                None
            } else {
                before.as_ref()
                    .and_then(|old| damage_paint::damage(old, &after, &self.touched))
            };
            // Damage-sized rasters change f32 transform composition at nonzero
            // viewport scroll, even when scroll*scale is an integer. Local
            // parity found three 1-channel AA differences in a scrolled text
            // and rounded-border probe after 16px tile alignment. Keep the
            // original viewport-origin clipped raster whenever scrolled;
            // this is a byte-parity fallback, NOT full paint invalidation.
            //
            // Also check old and new painted effects before consuming the old
            // snapshot to catch opacity/gradient removal or introduction.
            let scrolled_viewport = self.scroll_y_css != 0.0;
            let viewport_target = scrolled_viewport || bounds.zip(before.as_ref())
                .is_some_and(|(rect, old)| {
                    damage_paint::needs_viewport_raster(old, &after, rect)
                });
            self.before = Some(after);
            (bounds, viewport_target)
        } else {
            (None, false)
        };
        let layout_us = layout_start.elapsed().as_micros() as u64;
        let paint_start = Instant::now();
        let mut stages = PaintStages::default();
        let mut used_viewport_target = false;
        let (damage_px, damage_rect_px, paint_path, raster_threads, strips) = if self.viewport_height.is_none() {
            used_viewport_target = true;
            self.repaint();
            (self.bitmap_width as u64 * self.bitmap_height as u64, None, "full_document", 1, 1)
        } else if let Some(rect) = damage {
            let scale = f64::from(self.scale);
            let x0 = (rect.x0 * scale).floor().clamp(0.0, self.bitmap_width as f64) as u32;
            let x1 = (rect.x1 * scale).ceil().clamp(0.0, self.bitmap_width as f64) as u32;
            let y0 = (rect.y0 * scale).floor()
                .clamp(0.0, self.bitmap_height as f64) as u32;
            let y1 = (rect.y1 * scale).ceil()
                .clamp(0.0, self.bitmap_height as f64) as u32;
            let width = x1.saturating_sub(x0);
            let height = y1.saturating_sub(y0);
            let area = u64::from(width) * u64::from(height);
            let threads = if area >= parallel_threshold() {
                raster_pool().current_num_threads()
            } else { 1 };
            let count = if threads > 1 {
                threads.min(height as usize)
            } else { 1 };
            if width > 0 && height > 0 {
                used_viewport_target = viewport_target || count > 1;
                stages = if count == 1 && viewport_target {
                    self.repaint_rect(x0, y0, width, height, true)
                } else {
                    self.repaint_rect_strips(x0, y0, width, height, count)
                };
            }
            (area, Some((x0, y0, width, height)), "partial",
             if width > 0 && height > 0 { count as u32 } else { 1 },
             count as u32)
        } else {
            used_viewport_target = true;
            self.repaint();
            (self.bitmap_width as u64 * self.bitmap_height as u64, None, "full_viewport", 1, 1)
        };
        let paint_us = paint_start.elapsed().as_micros() as u64;
        self.touched.clear();
        self.dirty = false;
        Timings { style_us: 0, layout_us, paint_us, damage_px,
            damage_rect_px, paint_path, viewport_target: used_viewport_target,
            raster_threads, strips,
            encode_us: stages.encode_us, raster_us: stages.raster_us,
            readback_us: stages.readback_us, blit_us: stages.blit_us,
            target_setup_us: stages.target_setup_us }
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

// GUARD: native-origin clips and scene replay must not produce seams or
// shift antialiasing, including where glyphs and effects cross strip rows.
#[cfg(test)]
mod raster_strip_tests {
    use super::*;

    #[test]
    fn parallel_strips_match_serial_bytes_for_effects_and_text() {
        let html = include_str!("../../../fixtures/damage-raster/raster-effects.html")
            .replace("</style>", "#alpha.active{background:#ee367a}</style>");
        let after = html.replace("id=\"alpha\"", "id=\"alpha\" class=\"active\"");
        assert_ne!(html, after);
        let fonts = vec![include_bytes!(
            "../../../android/demo/app/src/main/assets/Roboto-Regular.ttf"
        ).to_vec()];
        let opts = RenderOptions { width: 392, fonts, ..RenderOptions::default() };
        let mut doc = Document::new_scaled(&html, opts.clone(), 2.75).unwrap();
        doc.set_viewport(0.0, 280).unwrap();
        let initial = doc.paint();
        let probe = doc.element_by_id("alpha").unwrap();
        // Exercise the real invalidation/partial-update path, not just the
        // helper under test. A fresh document is the pixel oracle.
        doc.toggle_class(probe, "active").unwrap();
        let update = doc.update();
        assert_eq!(update.paint_path, "partial");
        let mut fresh = Document::new_scaled(&after, opts, 2.75).unwrap();
        fresh.set_viewport(0.0, 280).unwrap();
        let expected = fresh.paint();
        assert_ne!(initial, expected, "fixture mutation must alter pixels");
        assert_eq!(doc.paint(), expected, "incremental update differs from fresh");
        let (vw, vh) = doc.bitmap_dimensions();
        for strips in [1, 2, 3, 8] {
            // If any strip fails to write its output, the sentinel remains
            // and must differ from the fresh full-viewport oracle.
            doc.rgba.fill(0xAA);
            doc.repaint_rect_strips(0, 0, vw, vh, strips);
            assert_eq!(doc.rgba, expected, "strip count {strips} changed pixels");
        }
    }
}

#[cfg(test)]
mod compact_target_alignment_tests {
    use super::aligned_raster_target;

    #[test]
    fn outward_grid_alignment_preserves_ink_and_tile_phase() {
        for (x, y, w, h) in [
            (14, 19, 20, 20), (0, 0, 1, 1),
            (85, 347, 528, 305), (1070, 2163, 8, 4),
        ] {
            let (tx, ty, tw, th) = aligned_raster_target(x, y, w, h);
            assert_eq!(tx % 16, 0);
            assert_eq!(ty % 16, 0);
            assert_eq!(tw % 16, 0);
            assert_eq!(th % 16, 0);
            assert!(tx <= x && ty <= y);
            assert!(tx + tw >= x + w && ty + th >= y + h);
        }
    }
}
