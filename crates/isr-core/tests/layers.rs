// Copyright 2026 g1BSsQ. Licensed under Apache-2.0.
use isr_core::{Document, LayerFallback, LayerFallbackReason, LayerKind,
    LayerTransform, RenderOptions};

fn doc(html: &str) -> Document {
    let mut doc = Document::new(html, RenderOptions {
        width: 100, ..RenderOptions::default()
    }).unwrap();
    doc.set_viewport(0.0, 80).unwrap();
    doc
}

fn pixel(buffer: &[u8], x: usize, y: usize) -> [u8; 4] {
    let offset = (y * 100 + x) * 4;
    buffer[offset..offset + 4].try_into().unwrap()
}

#[test]
fn identity_matches_normal_paint_byte_for_byte() {
    let html = include_str!("../../../fixtures/layers/basic.html");
    let mut original = doc(html);
    let expected = original.paint();
    let mut layered = doc(html);
    let node = layered.element_by_id("card").unwrap();
    layered.promote(node).unwrap();
    assert_eq!(layered.composite(), expected);
    assert!(layered.layers().iter().any(|layer| layer.kind == LayerKind::Promoted));
}

#[test]
fn translation_moves_exact_opaque_pixels() {
    let mut layered = doc(include_str!("../../../fixtures/layers/basic.html"));
    let id = layered.promote(layered.element_by_id("card").unwrap()).unwrap();
    layered.set_layer_transform(id, LayerTransform::translate((15.0, 10.0))).unwrap();
    let bitmap = layered.composite();
    assert_eq!(pixel(&bitmap, 2, 2), [255, 255, 255, 255]);
    assert_eq!(pixel(&bitmap, 18, 13), [255, 0, 0, 255]);
    assert_eq!(pixel(&bitmap, 34, 29), [255, 0, 0, 255]);
    assert_eq!(pixel(&bitmap, 35, 30), [255, 255, 255, 255]);
}

#[test]
fn opacity_blends_a_promoted_group() {
    let mut layered = doc(include_str!("../../../fixtures/layers/basic.html"));
    let id = layered.promote(layered.element_by_id("card").unwrap()).unwrap();
    layered.set_layer_opacity(id, 0.5).unwrap();
    let bitmap = layered.composite();
    let p = pixel(&bitmap, 10, 10);
    assert_eq!(p[0], 255);
    assert!((127..=128).contains(&p[1]));
    assert!((127..=128).contains(&p[2]));
    assert_eq!(p[3], 255);
}

#[test]
fn later_sibling_remains_above_a_moving_layer() {
    let mut layered = doc(include_str!("../../../fixtures/layers/overlap.html"));
    let id = layered.promote(layered.element_by_id("card").unwrap()).unwrap();
    layered.set_layer_transform(id, LayerTransform::translate((10.0, 0.0))).unwrap();
    let bitmap = layered.composite();
    assert_eq!(pixel(&bitmap, 35, 10), [0, 0, 255, 255]);
    assert_eq!(pixel(&bitmap, 15, 10), [255, 0, 0, 255]);
}

#[test]
fn ancestor_rounded_overflow_is_applied_after_motion() {
    let mut layered = doc(include_str!("../../../fixtures/layers/overflow.html"));
    let id = layered.promote(layered.element_by_id("card").unwrap()).unwrap();
    layered.set_layer_transform(id, LayerTransform::translate((14.0, 0.0))).unwrap();
    let bitmap = layered.composite();
    assert_eq!(pixel(&bitmap, 45, 20), [255, 255, 255, 255]);
    assert_eq!(pixel(&bitmap, 20, 20), [255, 0, 0, 255]);
    assert_eq!(pixel(&bitmap, 38, 1), [255, 255, 255, 255]);
}

#[test]
fn filter_ancestor_has_specific_fallback_reason() {
    let mut layered = doc(include_str!("../../../fixtures/layers/filter.html"));
    let err = layered.promote(layered.element_by_id("card").unwrap()).unwrap_err();
    let reason = err.downcast_ref::<LayerFallback>().unwrap().0;
    assert_eq!(reason, LayerFallbackReason::UnsupportedAncestorEffect);
    assert!(layered.layers().is_empty());
}

#[test]
fn motion_has_no_resolve_or_raster_work() {
    let mut layered = doc(include_str!("../../../fixtures/layers/basic.html"));
    let id = layered.promote(layered.element_by_id("card").unwrap()).unwrap();
    let _ = layered.composite();
    let before = layered.layer_counters();
    for frame in 0..120 {
        layered.set_layer_transform(id,
            LayerTransform::translate((frame as f64 / 4.0, 0.0))).unwrap();
        layered.set_layer_opacity(id, 0.75).unwrap();
        let _ = layered.composite();
    }
    assert_eq!(layered.layer_counters(), before);
}

#[test]
fn promoted_subtree_damage_rerasterizes_no_base_segment() {
    let mut layered = doc(include_str!("../../../fixtures/layers/basic.html"));
    let card = layered.element_by_id("card").unwrap();
    layered.promote(card).unwrap();
    let before = layered.layer_counters();
    layered.toggle_class(card, "active").unwrap();
    layered.update();
    let after = layered.layer_counters();
    assert_eq!(after.layer_rasters - before.layer_rasters, 1);
    assert_eq!(after.segment_rasters, before.segment_rasters);
    assert_eq!(pixel(&layered.composite(), 10, 10), [0, 136, 0, 255]);
}
