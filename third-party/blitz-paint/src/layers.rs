// Modified from Blitz 92b11f1b: support backdrop blend layers without changing normal clips.
use anyrender::{Filter, PaintScene};
use kurbo::{Affine, Shape};
use peniko::{Fill, Mix};
use std::{cell::Cell, sync::Arc};

const LAYER_LIMIT: u32 = 1024;

#[derive(Default)]
pub(crate) struct LayerManager {
    layers_used: Cell<u32>,
    layer_depth: Cell<u32>,
    layers_wanted: Cell<u32>,

    #[allow(unused)] // Only used for debugging. Enabled as required.
    layer_depth_used: Cell<u32>,
}

impl LayerManager {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn maybe_with_layer<S: PaintScene, F: FnOnce(&mut S)>(
        &self,
        scene: &mut S,
        condition: bool,
        opacity: f32,
        transform: Affine,
        shape: &impl Shape,
        filter: Option<Arc<Filter>>,
        backdrop_filter: Option<Arc<Filter>>,
        paint_layer: F,
    ) {
        let layer_used = self.maybe_push_layer(
            scene,
            condition,
            opacity,
            transform,
            shape,
            filter,
            backdrop_filter,
        );
        paint_layer(scene);
        self.maybe_pop_layer(scene, layer_used);
    }

    /// Blend layers use the same accounting as normal layers. The normal path
    /// delegates unchanged so effect-free pages retain their existing commands.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn maybe_with_blend_layer<S: PaintScene, F: FnOnce(&mut S)>(
        &self,
        scene: &mut S,
        condition: bool,
        blend: Mix,
        opacity: f32,
        transform: Affine,
        shape: &impl Shape,
        filter: Option<Arc<Filter>>,
        backdrop_filter: Option<Arc<Filter>>,
        paint_layer: F,
    ) {
        if blend == Mix::Normal {
            self.maybe_with_layer(
                scene, condition, opacity, transform, shape, filter,
                backdrop_filter, paint_layer,
            );
            return;
        }

        let pushed = if condition && self.layers_used.get() < LAYER_LIMIT {
            scene.push_layer(
                Fill::NonZero, blend, opacity, transform, shape, filter, backdrop_filter,
            );
            self.layers_used.update(|x| x + 1);
            self.layer_depth.update(|x| x + 1);
            self.layer_depth_used.update(|x| x.max(self.layer_depth_used.get()));
            true
        } else {
            false
        };
        paint_layer(scene);
        self.maybe_pop_layer(scene, pushed);
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn maybe_push_layer(
        &self,
        scene: &mut impl PaintScene,
        condition: bool,
        opacity: f32,
        transform: Affine,
        shape: &impl Shape,
        filter: Option<Arc<Filter>>,
        backdrop_filter: Option<Arc<Filter>>,
    ) -> bool {
        if !condition {
            return false;
        }
        self.layers_wanted.update(|x| x + 1);

        // Check if clips are above limit
        let layers_available = self.layers_used.get() <= LAYER_LIMIT;
        if !layers_available {
            return false;
        }

        // Actually push the layer
        if opacity == 1.0 && filter.is_none() && backdrop_filter.is_none() {
            scene.push_clip_layer(Fill::NonZero, transform, shape);
        } else {
            scene.push_layer(
                Fill::NonZero,
                Mix::Normal,
                opacity,
                transform,
                shape,
                filter,
                backdrop_filter,
            );
        };

        // Update accounting
        self.layers_used.update(|x| x + 1);
        self.layer_depth.update(|x| x + 1);
        self.layer_depth.update(|x| x.max(self.layer_depth.get()));

        true
    }

    pub(crate) fn maybe_pop_layer(&self, scene: &mut impl PaintScene, condition: bool) {
        if condition {
            scene.pop_layer();
            self.layer_depth.update(|x| x - 1);
        }
    }
}
