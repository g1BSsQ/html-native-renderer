// Copyright 2026 g1BSsQ. Licensed under Apache-2.0.
// JNI boundary: one call, one owned result, no native state retained.
use isr_core::{RenderOptions, Rendered};
use jni::{
    JNIEnv,
    objects::{JObject, JString, JValue},
    sys::{jfloat, jint, jobject},
};
use std::panic::{AssertUnwindSafe, catch_unwind};

const MAX_PIXEL_COUNT: u64 = 16_000_000;
const MAX_HTML_BYTES: usize = 4 * 1024 * 1024;

// Layout JSON remains in CSS pixels; only the RGBA bitmap is scaled.
// Nearest-neighbour scaling is intentional for this first surface.
fn scaled_rgba(rendered: &Rendered, width: usize, height: usize, scale: f64) -> Vec<u8> {
    if width == rendered.width as usize && height == rendered.height as usize {
        return rendered.rgba.clone();
    }
    let source_width = rendered.width as usize;
    let source_height = rendered.height as usize;
    let mut output = vec![0u8; width * height * 4];
    for y in 0..height {
        let src_y = ((y as f64 / scale).floor() as usize).min(source_height - 1);
        for x in 0..width {
            let src_x = ((x as f64 / scale).floor() as usize).min(source_width - 1);
            let source = (src_y * source_width + src_x) * 4;
            let dest = (y * width + x) * 4;
            output[dest..dest + 4].copy_from_slice(&rendered.rgba[source..source + 4]);
        }
    }
    output
}

fn render_inner<'a>(
    env: &mut JNIEnv<'a>,
    html: JString<'a>,
    width_css_px: jint,
    scale: jfloat,
) -> Result<JObject<'a>, String> {
    if !(1..=8192).contains(&width_css_px) {
        return Err("widthCssPx must be in 1..=8192".into());
    }
    if !scale.is_finite() || !(0.25..=4.0).contains(&scale) {
        return Err("scale must be finite and in 0.25..=4.0".into());
    }
    let html: String = env.get_string(&html)
        .map_err(|e| format!("reading HTML string: {e}"))?.into();
    if html.len() > MAX_HTML_BYTES {
        return Err("HTML document exceeds the 4 MiB input limit".into());
    }

    // Never opt in to installed fonts or supply filesystem paths. isr-core
    // accepts only data: resource URLs; see the documented SVG caveat.
    let rendered = isr_core::render(
        &html,
        RenderOptions { width: width_css_px as u32, ..RenderOptions::default() },
    ).map_err(|e| format!("rendering HTML: {e}"))?;

    let factor = f64::from(scale);
    let pixel_width = (f64::from(rendered.width) * factor).round().max(1.0) as usize;
    let pixel_height = (f64::from(rendered.height) * factor).round().max(1.0) as usize;
    let pixel_count = (pixel_width as u64)
        .checked_mul(pixel_height as u64).ok_or("bitmap dimensions overflow")?;
    if pixel_count > MAX_PIXEL_COUNT {
        return Err(format!("bitmap exceeds {MAX_PIXEL_COUNT} pixels"));
    }
    let json = serde_json::to_string(&rendered.layout())
        .map_err(|e| format!("serializing element boxes: {e}"))?;
    let pixels = scaled_rgba(&rendered, pixel_width, pixel_height, factor);
    let pixel_array = env.byte_array_from_slice(&pixels)
        .map_err(|e| format!("allocating RGBA byte array: {e}"))?;
    let json_string = env.new_string(json)
        .map_err(|e| format!("allocating layout JSON string: {e}"))?;
    let pixel_object = JObject::from(pixel_array);
    let json_object = JObject::from(json_string);
    env.new_object(
        "org/htmlnativerenderer/demo/RenderResult",
        "(II[BLjava/lang/String;)V",
        &[
            JValue::Int(pixel_width as jint),
            JValue::Int(pixel_height as jint),
            JValue::Object(&pixel_object),
            JValue::Object(&json_object),
        ],
    ).map_err(|e| format!("constructing RenderResult: {e}"))
}

/// Kotlin: NativeRenderer.render(html: String, widthCssPx: Int, scale: Float).
/// Any Rust error (including an unwind) becomes a managed Java exception.
#[unsafe(no_mangle)]
pub extern "system" fn Java_org_htmlnativerenderer_demo_NativeRenderer_render<'a>(
    mut env: JNIEnv<'a>,
    _receiver: JObject<'a>,
    html: JString<'a>,
    width_css_px: jint,
    scale: jfloat,
) -> jobject {
    match catch_unwind(AssertUnwindSafe(|| {
        render_inner(&mut env, html, width_css_px, scale)
    })) {
        Ok(Ok(result)) => result.into_raw(),
        result => {
            let message = match result {
                Ok(Err(e)) => e,
                Err(_) => "native renderer panicked".to_owned(),
                Ok(Ok(_)) => unreachable!(),
            };
            // JNI calls may themselves leave an exception pending. Preserve it
            // rather than attempting to throw a second exception.
            if !env.exception_check().unwrap_or(true) {
                let _ = env.throw_new("java/lang/RuntimeException", message);
            }
            std::ptr::null_mut()
        }
    }
}
