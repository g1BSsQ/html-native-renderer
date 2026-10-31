// Copyright 2026 g1BSsQ. Licensed under Apache-2.0.
// Return immutable layer rasters; transform-only motion stays with the host.
use isr_core::{Document, LayerKind, RenderOptions};
use jni::{
    JNIEnv,
    objects::{JByteArray, JObject, JObjectArray, JString, JValue},
    sys::{jfloat, jint, jobjectArray},
};
use std::panic::{catch_unwind, AssertUnwindSafe};

fn render_layers<'a>(
    env: &mut JNIEnv<'a>,
    html: JString<'a>,
    width_css_px: jint,
    scale: jfloat,
    fonts: JObjectArray<'a>,
    identifiers: JObjectArray<'a>,
) -> Result<JObjectArray<'a>, String> {
    if !(1..=8192).contains(&width_css_px) {
        return Err("invalid document width".into());
    }
    if !scale.is_finite() || !(0.25..=4.0).contains(&scale) {
        return Err("invalid document scale".into());
    }
    let html: String = env.get_string(&html)
        .map_err(|e| format!("reading document: {e}"))?.into();
    if html.len() > 4 * 1024 * 1024 {
        return Err("document input is too large".into());
    }
    let count = env.get_array_length(&fonts)
        .map_err(|e| format!("reading fonts: {e}"))?;
    if count > 8 { return Err("too many fonts".into()); }
    let mut font_bytes = Vec::with_capacity(count as usize);
    let mut total = 0usize;
    for index in 0..count {
        let object = env.get_object_array_element(&fonts, index)
            .map_err(|e| format!("reading font: {e}"))?;
        if object.is_null() { return Err("null font".into()); }
        let array = JByteArray::from(object);
        let size = env.get_array_length(&array)
            .map_err(|e| format!("reading font size: {e}"))?;
        if size <= 0 || size > 8 * 1024 * 1024 {
            return Err("invalid font size".into());
        }
        total += size as usize;
        if total > 16 * 1024 * 1024 { return Err("font budget exceeded".into()); }
        font_bytes.push(env.convert_byte_array(&array)
            .map_err(|e| format!("reading font bytes: {e}"))?);
    }

    let mut document = Document::new_scaled(
        &html, RenderOptions {
            width: width_css_px as u32,
            fonts: font_bytes,
            ..RenderOptions::default()
        }, scale,
    ).map_err(|e| format!("document layout: {e}"))?;
    document.set_viewport(0.0, 800).map_err(|e| format!("viewport: {e}"))?;
    let id_count = env.get_array_length(&identifiers)
        .map_err(|e| format!("reading promoted elements: {e}"))?;
    if id_count > 16 { return Err("too many promoted elements".into()); }
    for index in 0..id_count {
        let id = env.get_object_array_element(&identifiers, index)
            .map_err(|e| format!("reading element identifier: {e}"))?;
        if id.is_null() { return Err("null element identifier".into()); }
        let id = JString::from(id);
        let id: String = env.get_string(&id)
            .map_err(|e| format!("reading identifier: {e}"))?.into();
        let node = document.element_by_id(&id)
            .ok_or_else(|| format!("element not found: {id}"))?;
        // Unsupported groups use the original bitmap host path instead.
        if document.promote(node).is_err() {
            return env.new_object_array(0,
                "org/htmlnativerenderer/demo/LayerBitmap", JObject::null())
                .map_err(|e| format!("creating empty layer list: {e}"));
        }
    }
    let layers = document.layers();
    let output = env.new_object_array(layers.len() as i32,
        "org/htmlnativerenderer/demo/LayerBitmap", JObject::null())
        .map_err(|e| format!("creating layer list: {e}"))?;
    for (index, layer) in layers.iter().enumerate() {
        let pixels = env.byte_array_from_slice(layer.bitmap.as_slice())
            .map_err(|e| format!("copying layer pixels: {e}"))?;
        let pixels = JObject::from(pixels);
        let clips = layer.clips.iter().map(|clip| {
            serde_json::json!({
                "x": clip.rect_css.0 * f64::from(scale),
                "y": clip.rect_css.1 * f64::from(scale),
                "w": clip.rect_css.2 * f64::from(scale),
                "h": clip.rect_css.3 * f64::from(scale),
                "r": clip.radius_css * f64::from(scale)
            })
        }).collect::<Vec<_>>();
        let clips = env.new_string(serde_json::to_string(&clips)
            .map_err(|e| format!("serializing clips: {e}"))?)
            .map_err(|e| format!("allocating clips: {e}"))?;
        let clips = JObject::from(clips);
        let layer_kind = if layer.kind == LayerKind::Promoted { 1 } else { 0 };
        let object = env.new_object(
            "org/htmlnativerenderer/demo/LayerBitmap",
            "(JIIIII[BLjava/lang/String;FF)V",
            &[
                JValue::Long(layer.id.0 as i64),
                JValue::Int(layer_kind),
                JValue::Int(layer.width_px as i32),
                JValue::Int(layer.height_px as i32),
                JValue::Int(layer.origin_px.0),
                JValue::Int(layer.origin_px.1),
                JValue::Object(&pixels),
                JValue::Object(&clips),
                JValue::Float(layer.bounds_css.0 as f32 * scale),
                JValue::Float(layer.bounds_css.1 as f32 * scale),
            ],
        ).map_err(|e| format!("constructing layer: {e}"))?;
        env.set_object_array_element(&output, index as i32, &object)
            .map_err(|e| format!("publishing layer: {e}"))?;
    }
    Ok(output)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_htmlnativerenderer_demo_NativeRenderer_renderLayers<'a>(
    mut env: JNIEnv<'a>,
    _receiver: JObject<'a>,
    html: JString<'a>,
    width_css_px: jint,
    scale: jfloat,
    fonts: JObjectArray<'a>,
    identifiers: JObjectArray<'a>,
) -> jobjectArray {
    match catch_unwind(AssertUnwindSafe(|| render_layers(
        &mut env, html, width_css_px, scale, fonts, identifiers,
    ))) {
        Ok(Ok(result)) => result.into_raw(),
        result => {
            let message = match result {
                Ok(Err(message)) => message,
                Err(_) => "layer rendering panicked".to_string(),
                Ok(Ok(_)) => unreachable!(),
            };
            if !env.exception_check().unwrap_or(true) {
                let _ = env.throw_new("java/lang/RuntimeException", message);
            }
            std::ptr::null_mut()
        }
    }
}
