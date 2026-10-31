// Copyright 2026 g1BSsQ. Licensed under Apache-2.0.
// Explicit retained raster layers and ordered transparent page segments.
use std::{collections::HashSet, fmt, sync::Arc};
use anyrender::{PaintScene as _, recording::RenderCommand, recording::Scene, render_to_buffer};
use anyrender_vello_cpu::VelloCpuImageRenderer;
use blitz_dom::{NodeId, NodeData, util::Color};
use blitz_html::HtmlDocument;
use peniko::{Fill, kurbo::{Affine, Point, Rect, Shape as _}};
use crate::geometry;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LayerId(pub u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayerKind {
    BaseSegment,
    Promoted,
}

/// A clip fixed in page coordinates, applied after the promoted transform.
#[derive(Clone, Debug)]
pub struct LayerClip {
    pub rect_css: (f64, f64, f64, f64),
    pub radius_css: f64,
}

#[derive(Clone, Debug)]
pub struct Layer {
    pub id: LayerId,
    pub kind: LayerKind,
    /// Premultiplied RGBA8. The bitmap is cropped to nontransparent device pixels.
    pub bitmap: Arc<Vec<u8>>,
    pub width_px: u32,
    pub height_px: u32,
    pub origin_px: (i32, i32),
    pub bounds_css: (f64, f64, f64, f64),
    /// Local 2D affine transform around bounds_css's border-box top-left.
    pub transform: Affine,
    pub opacity: f32,
    pub paint_order: usize,
    pub clips: Vec<LayerClip>,
}

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct LayerCounters {
    pub style_layout_resolves: u64,
    pub layer_rasters: u64,
    pub segment_rasters: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayerFallbackReason {
    MissingBox,
    NonStackingContext,
    NestedPromotion,
    UnsupportedAncestorEffect,
    UnsupportedClip,
    UnsupportedOwnEffect,
}

#[derive(Debug)]
pub struct LayerFallback(pub LayerFallbackReason);
impl fmt::Display for LayerFallback {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "layer fallback: {:?}", self.0)
    }
}
impl std::error::Error for LayerFallback {}

#[derive(Clone)]
pub(crate) struct Promotion {
    pub id: LayerId,
    pub node: NodeId,
    pub bounds: (f64, f64, f64, f64),
    pub clips: Vec<LayerClip>,
}

#[derive(Default)]
pub(crate) struct LayerState {
    pub promotions: Vec<Promotion>,
    pub frames: Vec<Layer>,
    pub counters: LayerCounters,
    next_id: u64,
    pub original_valid: bool,
}

fn css_number(value: &str) -> Option<f64> {
    let parse = |part: &str| {
        if part == "0" { return Some(0.0); }
        part.strip_suffix("px")?.parse::<f64>().ok()
            .filter(|v| v.is_finite() && *v >= 0.0)
    };
    let mut parts = value.split_whitespace();
    let first = parse(parts.next()?)?;
    if let Some(second) = parts.next() {
        if parse(second)? != first { return None; }
    }
    parts.next().is_none().then_some(first)
}

fn style(doc: &HtmlDocument, id: NodeId, property: &str) -> String {
    doc.resolved_style_value(id, property)
}

pub(crate) fn border_box(doc: &HtmlDocument, id: NodeId)
    -> Option<(f64, f64, f64, f64)>
{
    let b = geometry::element_box(doc, doc.get_node(id)?)?;
    (b.width > 0.0 && b.height > 0.0
        && [b.x, b.y, b.width, b.height].iter().all(|n| n.is_finite()))
        .then_some((b.x, b.y, b.width, b.height))
}

pub(crate) fn ancestor_of(doc: &HtmlDocument, root: NodeId, mut child: NodeId) -> bool {
    loop {
        if root == child { return true; }
        let Some(node) = doc.get_node(child) else { return false };
        let Some(parent) = node.parent else { return false };
        child = parent;
    }
}

impl LayerState {
    pub fn is_active(&self) -> bool { !self.promotions.is_empty() }

    pub(crate) fn can_update_only_promoted(
        &self, doc: &HtmlDocument, touched: &HashSet<NodeId>,
    ) -> bool {
        !touched.is_empty()
            && touched.iter().all(|id| self.promotions.iter()
                .any(|p| ancestor_of(doc, p.node, *id)))
            && self.promotions.iter().all(|p| border_box(doc, p.node) == Some(p.bounds))
    }

    pub(crate) fn validate(
        &self, doc: &HtmlDocument, node_id: NodeId,
    ) -> Result<Promotion, LayerFallback> {
        let node = doc.get_node(node_id).ok_or(LayerFallback(LayerFallbackReason::MissingBox))?;
        if !matches!(&node.data, NodeData::Element(_)) {
            return Err(LayerFallback(LayerFallbackReason::MissingBox));
        }
        let bounds = border_box(doc, node_id)
            .ok_or(LayerFallback(LayerFallbackReason::MissingBox))?;
        if node.stacking_context.is_none() {
            return Err(LayerFallback(LayerFallbackReason::NonStackingContext));
        }
        if self.promotions.iter().any(|p| ancestor_of(doc, p.node, node_id)
            || ancestor_of(doc, node_id, p.node))
        {
            return Err(LayerFallback(LayerFallbackReason::NestedPromotion));
        }
        // Effects that require a common offscreen backdrop cannot be
        // separated into independent paint-order segments.
        if style(doc, node_id, "filter") != "none"
            || style(doc, node_id, "backdrop-filter") != "none"
            || style(doc, node_id, "mix-blend-mode") != "normal"
            || style(doc, node_id, "mask-image") != "none"
        {
            return Err(LayerFallback(LayerFallbackReason::UnsupportedOwnEffect));
        }
        let mut clips = Vec::new();
        let mut parent = node.parent;
        while let Some(id) = parent {
            let ancestor = doc.get_node(id)
                .ok_or(LayerFallback(LayerFallbackReason::MissingBox))?;
            if ancestor.primary_styles().is_some() {
                if style(doc, id, "opacity").parse::<f32>().ok() != Some(1.0)
                    || style(doc, id, "filter") != "none"
                    || style(doc, id, "backdrop-filter") != "none"
                    || style(doc, id, "mix-blend-mode") != "normal"
                    || style(doc, id, "mask-image") != "none"
                    || ancestor.transform().is_some()
                {
                    return Err(LayerFallback(LayerFallbackReason::UnsupportedAncestorEffect));
                }
                if style(doc, id, "clip-path") != "none" {
                    return Err(LayerFallback(LayerFallbackReason::UnsupportedClip));
                }
                let overflow_x = style(doc, id, "overflow-x");
                let overflow_y = style(doc, id, "overflow-y");
                if overflow_x != "visible" || overflow_y != "visible" {
                    if overflow_x != overflow_y {
                        return Err(LayerFallback(LayerFallbackReason::UnsupportedClip));
                    }
                    let rect = border_box(doc, id)
                        .ok_or(LayerFallback(LayerFallbackReason::MissingBox))?;
                    let border = ancestor.final_layout().border;
                    let rect = (rect.0 + border.left as f64, rect.1 + border.top as f64,
                        rect.2 - (border.left + border.right) as f64,
                        rect.3 - (border.top + border.bottom) as f64);
                    let radii = ["border-top-left-radius", "border-top-right-radius",
                        "border-bottom-right-radius", "border-bottom-left-radius"];
                    let parsed = radii.map(|key| css_number(&style(doc, id, key)));
                    if parsed.iter().any(Option::is_none)
                        || parsed.iter().any(|x| *x != parsed[0])
                    {
                        return Err(LayerFallback(LayerFallbackReason::UnsupportedClip));
                    }
                    let radius = (parsed[0].unwrap() - border.left.min(border.top) as f64).max(0.0);
                    clips.push(LayerClip { rect_css: rect, radius_css: radius });
                }
            }
            parent = ancestor.parent;
        }
        Ok(Promotion {
            id: LayerId(self.next_id + 1), node: node_id, bounds, clips,
        })
    }

    pub(crate) fn add(&mut self, promotion: Promotion) -> LayerId {
        self.next_id = promotion.id.0;
        let id = promotion.id;
        self.promotions.push(promotion);
        id
    }

    pub(crate) fn remove(&mut self, id: LayerId) -> bool {
        let before = self.promotions.len();
        self.promotions.retain(|p| p.id != id);
        self.promotions.len() != before
    }

    pub(crate) fn update_transform(&mut self, id: LayerId, transform: Affine)
        -> Result<(), &'static str>
    {
        if !transform.as_coeffs().iter().all(|x| x.is_finite()) {
            return Err("nonfinite layer transform");
        }
        let layer = self.frames.iter_mut().find(|l| l.id == id && l.kind == LayerKind::Promoted)
            .ok_or("unknown layer")?;
        layer.transform = transform;
        Ok(())
    }

    pub(crate) fn update_opacity(&mut self, id: LayerId, opacity: f32)
        -> Result<(), &'static str>
    {
        if !opacity.is_finite() || !(0.0..=1.0).contains(&opacity) {
            return Err("layer opacity must be in 0..=1");
        }
        let layer = self.frames.iter_mut().find(|l| l.id == id && l.kind == LayerKind::Promoted)
            .ok_or("unknown layer")?;
        layer.opacity = opacity;
        Ok(())
    }

    pub(crate) fn rebuild(
        &mut self, doc: &mut HtmlDocument, scale: f32, width: u32, height: u32,
        changed_only: Option<&HashSet<NodeId>>,
    ) {
        if self.promotions.is_empty() {
            self.frames.clear();
            return;
        }
        let marks: Vec<(NodeId, usize)> = self.promotions.iter()
            .enumerate().map(|(i, p)| (p.node, i)).collect();
        let mut scene = Scene::new();
        scene.fill(
            Fill::NonZero, Affine::IDENTITY, Color::WHITE, None,
            &Rect::new(0.0, 0.0, width as f64, height as f64),
        );
        blitz_paint::paint_scene_marked(&mut scene, doc, f64::from(scale), width, height, &marks);
        let pieces = split(scene);
        let old = std::mem::take(&mut self.frames);
        let mut frames = Vec::with_capacity(pieces.len());
        for (position, (mark, piece)) in pieces.into_iter().enumerate() {
            let promotion = mark.and_then(|i| self.promotions.get(i));
            let id = promotion.map_or(LayerId(u64::MAX - position as u64), |p| p.id);
            let kind = if promotion.is_some() { LayerKind::Promoted } else { LayerKind::BaseSegment };
            let unchanged = changed_only.is_some_and(|changed| {
                match promotion {
                    None => true,
                    Some(p) => !changed.iter().any(|node| ancestor_of(doc, p.node, *node)),
                }
            });
            if unchanged {
                if let Some(mut cached) = old.iter().find(|l| l.id == id).cloned() {
                    cached.paint_order = position;
                    frames.push(cached);
                    continue;
                }
            }
            let (bitmap, origin_px, width_px, height_px) = raster_crop(&piece, width, height);
            if kind == LayerKind::Promoted { self.counters.layer_rasters += 1; }
            else { self.counters.segment_rasters += 1; }
            let existing = old.iter().find(|l| l.id == id);
            frames.push(Layer {
                id, kind, bitmap: Arc::new(bitmap), origin_px, width_px, height_px,
                bounds_css: promotion.map_or((0.0, 0.0, width as f64 / scale as f64,
                    height as f64 / scale as f64), |p| p.bounds),
                transform: existing.map_or(Affine::IDENTITY, |l| l.transform),
                opacity: existing.map_or(1.0, |l| l.opacity),
                paint_order: position,
                clips: promotion.map_or_else(Vec::new, |p| p.clips.clone()),
            });
        }
        self.frames = frames;
        if changed_only.is_some() { self.original_valid = false; }
    }

    pub(crate) fn composite(&self, scale: f32, width: u32, height: u32,
        original: &[u8]) -> Vec<u8>
    {
        if self.original_valid && !original.is_empty()
            && self.frames.iter().all(|l| l.kind == LayerKind::BaseSegment
                || (l.transform == Affine::IDENTITY && l.opacity == 1.0))
        {
            return original.to_vec();
        }
        let mut output = vec![0u8; width as usize * height as usize * 4];
        for layer in &self.frames {
            blend_layer(&mut output, width, height, layer, scale);
        }
        output
    }
}

fn boundary(commands: &[RenderCommand], index: usize) -> Option<(usize, bool)> {
    let RenderCommand::PushClipLayer(layer) = commands.get(index)? else { return None };
    if !matches!(commands.get(index + 1), Some(RenderCommand::PopLayer)) {
        return None;
    }
    let bounds = layer.clip.bounding_box();
    let ending = if bounds.x0 == -9_000_002.0 { true }
        else if bounds.x0 == -9_000_001.0 { false }
        else { return None };
    if bounds.y0 < 0.0 || bounds.y0.fract() != 0.0 { return None; }
    Some((bounds.y0 as usize, ending))
}

fn split(scene: Scene) -> Vec<(Option<usize>, Scene)> {
    let mut result = Vec::new();
    let mut stack: Vec<RenderCommand> = Vec::new();
    let mut current: Vec<RenderCommand> = Vec::new();
    let mut current_mark = None;
    let mut i = 0;
    let flush = |result: &mut Vec<(Option<usize>, Scene)>,
        current: &mut Vec<RenderCommand>, mark, stack: &[RenderCommand]| {
        if mark.is_none() {
            current.extend((0..stack.len()).map(|_| RenderCommand::PopLayer));
        }
        result.push((mark, Scene { tolerance: 0.1, commands: std::mem::take(current) }));
    };
    while i < scene.commands.len() {
        if let Some((mark, ending)) = boundary(&scene.commands, i) {
            flush(&mut result, &mut current, current_mark, &stack);
            current_mark = if ending { None } else { Some(mark) };
            // The promoted image must not bake in ancestor clips: those
            // remain fixed in page coordinates and are applied at composite.
            if current_mark.is_none() {
                current.extend(stack.iter().cloned());
            }
            i += 2;
            continue;
        }
        let cmd = scene.commands[i].clone();
        match &cmd {
            RenderCommand::PushLayer(_) | RenderCommand::PushClipLayer(_) =>
                stack.push(cmd.clone()),
            RenderCommand::PopLayer => { stack.pop(); }
            _ => {}
        }
        current.push(cmd);
        i += 1;
    }
    flush(&mut result, &mut current, current_mark, &stack);
    result
}

fn raster_crop(scene: &Scene, width: u32, height: u32) -> (Vec<u8>, (i32, i32), u32, u32) {
    let rgba = render_to_buffer::<VelloCpuImageRenderer, _>(
        |target| target.append_scene(scene.clone(), Affine::IDENTITY), width, height,
    );
    let mut min_x = width;
    let mut min_y = height;
    let mut max_x = 0;
    let mut max_y = 0;
    for y in 0..height {
        for x in 0..width {
            if rgba[((y * width + x) * 4 + 3) as usize] != 0 {
                min_x = min_x.min(x);
                min_y = min_y.min(y);
                max_x = max_x.max(x + 1);
                max_y = max_y.max(y + 1);
            }
        }
    }
    if min_x >= max_x || min_y >= max_y {
        return (Vec::new(), (0, 0), 0, 0);
    }
    let out_width = max_x - min_x;
    let out_height = max_y - min_y;
    let mut cropped = vec![0u8; out_width as usize * out_height as usize * 4];
    for y in 0..out_height {
        let from = (((y + min_y) * width + min_x) * 4) as usize;
        let to = (y * out_width * 4) as usize;
        let bytes = out_width as usize * 4;
        cropped[to..to + bytes].copy_from_slice(&rgba[from..from + bytes]);
    }
    (cropped, (min_x as i32, min_y as i32), out_width, out_height)
}

fn within_clips(x: f64, y: f64, clips: &[LayerClip]) -> bool {
    clips.iter().all(|clip| {
        let (left, top, width, height) = clip.rect_css;
        let right = left + width;
        let bottom = top + height;
        if x < left || x >= right || y < top || y >= bottom { return false; }
        let r = clip.radius_css.min(width / 2.0).min(height / 2.0);
        if r <= 0.0 { return true; }
        let cx = if x < left + r { left + r }
            else if x > right - r { right - r } else { x };
        let cy = if y < top + r { top + r }
            else if y > bottom - r { bottom - r } else { y };
        (x - cx).powi(2) + (y - cy).powi(2) <= r * r
    })
}

fn blend_layer(target: &mut [u8], width: u32, height: u32, layer: &Layer, scale: f32) {
    if layer.width_px == 0 || layer.height_px == 0 || layer.opacity <= 0.0 { return; }
    let scale = f64::from(scale);
    let (bx, by, _, _) = layer.bounds_css;
    let transform = if layer.kind == LayerKind::Promoted {
        Affine::scale(scale) * Affine::translate((bx, by)) * layer.transform
            * Affine::translate((-bx, -by)) * Affine::scale(1.0 / scale)
    } else { Affine::IDENTITY };
    let source = Rect::new(layer.origin_px.0 as f64, layer.origin_px.1 as f64,
        layer.origin_px.0 as f64 + layer.width_px as f64,
        layer.origin_px.1 as f64 + layer.height_px as f64);
    let dest = transform.transform_rect_bbox(source);
    let x0 = dest.x0.floor().max(0.0) as u32;
    let x1 = dest.x1.ceil().min(width as f64) as u32;
    let y0 = dest.y0.floor().max(0.0) as u32;
    let y1 = dest.y1.ceil().min(height as f64) as u32;
    if x1 <= x0 || y1 <= y0 { return; }
    if !transform.determinant().is_finite() || transform.determinant() == 0.0 {
        return;
    }
    let inverse = transform.inverse();
    for y in y0..y1 {
        for x in x0..x1 {
            if !within_clips((x as f64 + 0.5) / scale, (y as f64 + 0.5) / scale,
                &layer.clips) { continue; }
            let p = inverse * Point::new(x as f64 + 0.5, y as f64 + 0.5);
            let sx = p.x.floor() as i32 - layer.origin_px.0;
            let sy = p.y.floor() as i32 - layer.origin_px.1;
            if sx < 0 || sy < 0 || sx >= layer.width_px as i32
                || sy >= layer.height_px as i32 { continue; }
            let src = ((sy as u32 * layer.width_px + sx as u32) * 4) as usize;
            let dst = ((y * width + x) * 4) as usize;
            let alpha = (layer.bitmap[src + 3] as f32 * layer.opacity).round() as u32;
            if alpha == 0 { continue; }
            let inv_alpha = 255 - alpha;
            for channel in 0..3 {
                let src_color = (layer.bitmap[src + channel] as f32 * layer.opacity).round() as u32;
                target[dst + channel] = (src_color
                    + (target[dst + channel] as u32 * inv_alpha + 127) / 255)
                    .min(255) as u8;
            }
            target[dst + 3] = (alpha
                + (target[dst + 3] as u32 * inv_alpha + 127) / 255).min(255) as u8;
        }
    }
}
