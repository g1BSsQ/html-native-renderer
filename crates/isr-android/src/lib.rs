// Copyright 2026 g1BSsQ. Licensed under Apache-2.0.
// JNI boundary: one call, one owned result, no native state retained.
use isr_core::RenderOptions;
use jni::{
    JNIEnv,
    objects::{JByteArray, JObject, JObjectArray, JString, JValue},
    sys::{jfloat, jint, jobject},
};
use std::panic::{AssertUnwindSafe, catch_unwind};

const MAX_PIXEL_COUNT: u64 = 16_000_000;
const MAX_HTML_BYTES: usize = 4 * 1024 * 1024;
const MAX_FONTS: i32 = 8;
const MAX_FONT_BYTES: i32 = 8 * 1024 * 1024;
const MAX_TOTAL_FONT_BYTES: usize = 16 * 1024 * 1024;

// Layout JSON remains in CSS pixels; only the RGBA bitmap is scaled.
// Nearest-neighbour scaling is intentional for this first surface.
fn scaled_rgba(
    rgba: Vec<u8>,
    source_width: usize,
    source_height: usize,
    width: usize,
    height: usize,
    scale: f64,
) -> Vec<u8> {
    if width == source_width && height == source_height {
        // Reuse the core's owned output instead of allocating another full RGBA copy.
        return rgba;
    }
    let mut output = vec![0u8; width * height * 4];
    for y in 0..height {
        let src_y = ((y as f64 / scale).floor() as usize).min(source_height - 1);
        for x in 0..width {
            let src_x = ((x as f64 / scale).floor() as usize).min(source_width - 1);
            let source = (src_y * source_width + src_x) * 4;
            let dest = (y * width + x) * 4;
            output[dest..dest + 4].copy_from_slice(&rgba[source..source + 4]);
        }
    }
    output
}

fn render_inner<'a>(
    env: &mut JNIEnv<'a>,
    html: JString<'a>,
    width_css_px: jint,
    scale: jfloat,
    fonts: JObjectArray<'a>,
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

    // Only JNI byte[] data is passed to the core: no paths or system fonts.
    // The limits are checked before each font is copied out of the JVM.
    let count = env.get_array_length(&fonts)
        .map_err(|e| format!("reading font list: {e}"))?;
    if count > MAX_FONTS {
        return Err(format!("at most {MAX_FONTS} fonts are supported"));
    }
    let mut font_bytes = Vec::with_capacity(count as usize);
    let mut total_bytes = 0usize;
    for index in 0..count {
        let font_object = env.get_object_array_element(&fonts, index)
            .map_err(|e| format!("reading font {index}: {e}"))?;
        if font_object.is_null() {
            return Err(format!("font {index} is null"));
        }
        let font = JByteArray::from(font_object);
        let size = env.get_array_length(&font)
            .map_err(|e| format!("reading font {index} length: {e}"))?;
        if size == 0 || size > MAX_FONT_BYTES {
            return Err(format!("font {index} must contain 1..={MAX_FONT_BYTES} bytes"));
        }
        total_bytes = total_bytes
            .checked_add(size as usize).ok_or("total font byte count overflow")?;
        if total_bytes > MAX_TOTAL_FONT_BYTES {
            return Err("font list exceeds the 16 MiB limit".into());
        }
        font_bytes.push(env.convert_byte_array(&font)
            .map_err(|e| format!("reading font {index} bytes: {e}"))?);
    }

    // Default network restrictions remain unchanged. SVG may still read
    // platform font configuration inside the pinned core (documented).
    let rendered = isr_core::render(
        &html,
        RenderOptions {
            width: width_css_px as u32,
            fonts: font_bytes,
            ..RenderOptions::default()
        },
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
    let pixels = scaled_rgba(
        rendered.rgba, rendered.width as usize, rendered.height as usize,
        pixel_width, pixel_height, factor,
    );
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

/// Kotlin: NativeRenderer.render(html: String, widthCssPx: Int, scale: Float,
///     fonts: Array<ByteArray>).
/// Any Rust error (including an unwind) becomes a managed Java exception.
#[unsafe(no_mangle)]
pub extern "system" fn Java_org_htmlnativerenderer_demo_NativeRenderer_render<'a>(
    mut env: JNIEnv<'a>,
    _receiver: JObject<'a>,
    html: JString<'a>,
    width_css_px: jint,
    scale: jfloat,
    fonts: JObjectArray<'a>,
) -> jobject {
    match catch_unwind(AssertUnwindSafe(|| {
        render_inner(&mut env, html, width_css_px, scale, fonts)
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

// These cases run on the host without JNI, an Android device or a Java VM.
#[cfg(test)]
mod tests {
    use super::scaled_rgba;

    fn source() -> Vec<u8> {
        // 3 x 2, row-major RGBA: distinct opaque colours at all six pixels.
        vec![
            10, 20, 30, 255, 40, 50, 60, 255, 70, 80, 90, 255,
            100, 110, 120, 255, 130, 140, 150, 255, 200, 30, 40, 255,
        ]
    }

    fn pixel(rgba: &[u8], width: usize, x: usize, y: usize) -> &[u8] {
        let offset = (y * width + x) * 4;
        &rgba[offset..offset + 4]
    }

    #[test]
    fn nearest_neighbour_at_scale_two() {
        let (width, height) = (6, 4); // round(3 * 2), round(2 * 2)
        let rgba = scaled_rgba(source(), 3, 2, width, height, 2.0);
        assert_eq!(rgba.len(), width * height * 4);
        assert_eq!(pixel(&rgba, width, 0, 0), &[10, 20, 30, 255]);
        assert_eq!(pixel(&rgba, width, 5, 3), &[200, 30, 40, 255]);
        assert_eq!(pixel(&rgba, width, 2, 0), &[40, 50, 60, 255]);
    }

    #[test]
    fn nearest_neighbour_at_fractional_scale_two_point_six_two_five() {
        let scale: f64 = 2.625;
        let width = (3.0_f64 * scale).round() as usize;
        let height = (2.0_f64 * scale).round() as usize;
        assert_eq!((width, height), (8, 5)); // noninteger dimensions round
        let rgba = scaled_rgba(source(), 3, 2, width, height, scale);
        assert_eq!(rgba.len(), width * height * 4);
        assert_eq!(pixel(&rgba, width, 0, 0), &[10, 20, 30, 255]);
        // floor(7 / 2.625) = 2, floor(4 / 2.625) = 1.
        assert_eq!(pixel(&rgba, width, 7, 4), &[200, 30, 40, 255]);
        assert_eq!(pixel(&rgba, width, 3, 2), &[40, 50, 60, 255]);
    }
}

// An additive benchmark-only JNI entry point. The normal render ABI is untouched.
fn bench_inner<'a>(
    env: &mut JNIEnv<'a>,
    width_css_px: jint,
    height_css_px: jint,
    density: jfloat,
    fonts: JObjectArray<'a>,
    warmup: jint,
    iterations: jint,
) -> Result<JString<'a>, String> {
    if !(1..=8192).contains(&width_css_px) {
        return Err("widthCssPx must be in 1..=8192".into());
    }
    if !(1..=32768).contains(&height_css_px) {
        return Err("heightCssPx must be in 1..=32768".into());
    }
    if !density.is_finite() || !(0.25..=4.0).contains(&density) {
        return Err("density must be finite and in 0.25..=4.0".into());
    }
    if !(0..=1000).contains(&warmup) || !(1..=10000).contains(&iterations) {
        return Err("invalid benchmark sample counts".into());
    }
    let count = env.get_array_length(&fonts)
        .map_err(|e| format!("reading font list: {e}"))?;
    if count > MAX_FONTS {
        return Err(format!("at most {MAX_FONTS} fonts are supported"));
    }
    let mut owned_fonts = Vec::with_capacity(count as usize);
    let mut total = 0usize;
    for index in 0..count {
        let object = env.get_object_array_element(&fonts, index)
            .map_err(|e| format!("reading font {index}: {e}"))?;
        if object.is_null() { return Err(format!("font {index} is null")); }
        let bytes = JByteArray::from(object);
        let size = env.get_array_length(&bytes)
            .map_err(|e| format!("reading font {index} length: {e}"))?;
        if size == 0 || size > MAX_FONT_BYTES {
            return Err(format!("font {index} must contain 1..={MAX_FONT_BYTES} bytes"));
        }
        total = total.checked_add(size as usize).ok_or("font bytes overflow")?;
        if total > MAX_TOTAL_FONT_BYTES {
            return Err("font list exceeds the 16 MiB limit".into());
        }
        owned_fonts.push(env.convert_byte_array(&bytes)
            .map_err(|e| format!("reading font {index} bytes: {e}"))?);
    }
    let records = isr_bench::run_all_viewport(
        width_css_px as u32, height_css_px as u32, density, &owned_fonts,
        warmup as usize, iterations as usize,
    ).map_err(|e| format!("interaction benchmark: {e}"))?;
    env.new_string(records.join("\n"))
        .map_err(|e| format!("creating benchmark JSON lines: {e}"))
}

/// Kotlin: NativeRenderer.bench(widthCssPx, heightCssPx, density, fonts, warmup, iterations).
/// The returned value contains exactly one JSON line per page.
#[unsafe(no_mangle)]
pub extern "system" fn Java_org_htmlnativerenderer_demo_NativeRenderer_bench<'a>(
    mut env: JNIEnv<'a>,
    _receiver: JObject<'a>,
    width_css_px: jint,
    height_css_px: jint,
    density: jfloat,
    fonts: JObjectArray<'a>,
    warmup: jint,
    iterations: jint,
) -> jni::sys::jstring {
    match catch_unwind(AssertUnwindSafe(|| {
        bench_inner(&mut env, width_css_px, height_css_px, density, fonts, warmup, iterations)
    })) {
        Ok(Ok(lines)) => lines.into_raw(),
        result => {
            let message = match result {
                Ok(Err(e)) => e,
                Err(_) => "native benchmark panicked".to_owned(),
                Ok(Ok(_)) => unreachable!(),
            };
            if !env.exception_check().unwrap_or(true) {
                let _ = env.throw_new("java/lang/RuntimeException", message);
            }
            std::ptr::null_mut()
        }
    }
}


// Retain input state on the calling thread. The demo's single worker owns
// the document; no DOM or painting state crosses the native thread boundary.
thread_local! {
    static INPUT_DOCUMENT: std::cell::RefCell<Option<isr_core::Document>> =
        const { std::cell::RefCell::new(None) };
}

fn input_fonts(env: &mut JNIEnv<'_>, fonts: JObjectArray<'_>) -> Result<Vec<Vec<u8>>, String> {
    let count = env.get_array_length(&fonts).map_err(|e| e.to_string())?;
    if count > MAX_FONTS { return Err("too many font arrays".into()); }
    let mut out = Vec::with_capacity(count as usize);
    let mut total = 0usize;
    for i in 0..count {
        let object = env.get_object_array_element(&fonts, i).map_err(|e| e.to_string())?;
        if object.is_null() { return Err("font array is null".into()); }
        let array = JByteArray::from(object);
        let size = env.get_array_length(&array).map_err(|e| e.to_string())?;
        if size == 0 || size > MAX_FONT_BYTES { return Err("invalid font byte count".into()); }
        total = total.checked_add(size as usize).ok_or("font byte count overflow")?;
        if total > MAX_TOTAL_FONT_BYTES { return Err("too many font bytes".into()); }
        out.push(env.convert_byte_array(&array).map_err(|e| e.to_string())?);
    }
    Ok(out)
}

fn input_open_inner<'a>(
    env: &mut JNIEnv<'a>,
    html: JString<'a>,
    width_css_px: jint,
    scale: jfloat,
    fonts: JObjectArray<'a>,
) -> Result<JObject<'a>, String> {
    if !(1..=8192).contains(&width_css_px) { return Err("invalid CSS width".into()); }
    if !scale.is_finite() || !(0.25..=4.0).contains(&scale) {
        return Err("invalid scale".into());
    }
    let html: String = env.get_string(&html).map_err(|e| e.to_string())?.into();
    if html.len() > MAX_HTML_BYTES { return Err("HTML exceeds input limit".into()); }
    let fonts = input_fonts(env, fonts)?;
    let mut document = isr_core::Document::new_scaled(
        &html,
        RenderOptions { width: width_css_px as u32, fonts, ..RenderOptions::default() },
        scale,
    ).map_err(|e| e.to_string())?;
    let (w, h) = document.bitmap_dimensions();
    if u64::from(w) * u64::from(h) > MAX_PIXEL_COUNT {
        return Err("input bitmap exceeds pixel limit".into());
    }
    // The complete document is the viewport for this scrollable bitmap:
    // later pressed-state mutations use the retained partial-damage path.
    let document_height = document.dimensions().1;
    document.set_viewport(0.0, document_height).map_err(|e| e.to_string())?;
    let pixels = document.paint();
    let byte_array = env.byte_array_from_slice(&pixels).map_err(|e| e.to_string())?;
    let empty_layout = env.new_string("{}").map_err(|e| e.to_string())?;
    let pixel_object = JObject::from(byte_array);
    let layout_object = JObject::from(empty_layout);
    let result = env.new_object(
        "org/htmlnativerenderer/demo/RenderResult", "(II[BLjava/lang/String;)V",
        &[JValue::Int(w as jint), JValue::Int(h as jint),
          JValue::Object(&pixel_object), JValue::Object(&layout_object)],
    ).map_err(|e| e.to_string())?;
    INPUT_DOCUMENT.with(|state| *state.borrow_mut() = Some(document));
    Ok(result)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_htmlnativerenderer_demo_NativeRenderer_inputOpen<'a>(
    mut env: JNIEnv<'a>, _receiver: JObject<'a>, html: JString<'a>,
    width_css_px: jint, scale: jfloat, fonts: JObjectArray<'a>,
) -> jobject {
    match catch_unwind(AssertUnwindSafe(|| {
        input_open_inner(&mut env, html, width_css_px, scale, fonts)
    })) {
        Ok(Ok(result)) => result.into_raw(),
        failed => {
            let message = match failed {
                Ok(Err(e)) => e, Err(_) => "input initialization panicked".into(),
                Ok(Ok(_)) => unreachable!(),
            };
            if !env.exception_check().unwrap_or(true) {
                let _ = env.throw_new("java/lang/RuntimeException", message);
            }
            std::ptr::null_mut()
        }
    }
}

fn event_json(document: &isr_core::Document, event: isr_core::GestureEvent) -> serde_json::Value {
    use isr_core::{GestureEvent as G, SwipeDirection as D};
    let kind = match event {
        G::Press(_) => "Press", G::Release(_) => "Release",
        G::Tap(_) => "Tap", G::DoubleTap(_) => "DoubleTap",
        G::LongPress(_) => "LongPress", G::Swipe { .. } => "Swipe",
        G::DragStart(_) => "DragStart", G::DragMove { .. } => "DragMove",
        G::DragEnd { .. } => "DragEnd", G::DragCancel(_) => "DragCancel",
        G::Pan { .. } => "Pan",
    };
    let target = document.attribute(event.target(), "id").unwrap_or_default();
    let mut out = serde_json::json!({"type":kind, "target":target});
    match event {
        G::Swipe { direction, velocity_css_per_s, .. } => {
            let direction = match direction {
                D::Left => "left", D::Right => "right", D::Up => "up", D::Down => "down",
            };
            out["direction"] = serde_json::json!(direction);
            out["velocity_css_per_s"] = serde_json::json!(velocity_css_per_s);
        }
        G::DragMove { dx_css, dy_css, .. } => {
            out["dx_css"] = serde_json::json!(dx_css);
            out["dy_css"] = serde_json::json!(dy_css);
        }
        G::DragEnd { dx_css, dy_css, velocity, .. } => {
            out["dx_css"] = serde_json::json!(dx_css);
            out["dy_css"] = serde_json::json!(dy_css);
            out["velocity"] = serde_json::json!(velocity);
        }
        G::Pan { dy_css, .. } => {
            out["dy_css"] = serde_json::json!(dy_css);
        }
        _ => {}
    }
    out
}

fn input_reply<'a>(
    env: &mut JNIEnv<'a>, events: serde_json::Value, pixels: Option<Vec<u8>>,
) -> Result<JObject<'a>, String> {
    let json = env.new_string(events.to_string()).map_err(|e| e.to_string())?;
    let json_object = JObject::from(json);
    let pixel_object = if let Some(pixels) = pixels {
        JObject::from(env.byte_array_from_slice(&pixels).map_err(|e| e.to_string())?)
    } else {
        JObject::null()
    };
    env.new_object(
        "org/htmlnativerenderer/demo/InputResult", "(Ljava/lang/String;[B)V",
        &[JValue::Object(&json_object), JValue::Object(&pixel_object)],
    ).map_err(|e| e.to_string())
}

fn input_pointer_inner<'a>(
    env: &mut JNIEnv<'a>, pointer_id: jni::sys::jlong, phase: jint,
    x_css: jni::sys::jdouble, y_css: jni::sys::jdouble, time_ms: jni::sys::jlong,
) -> Result<JObject<'a>, String> {
    use isr_core::{PointerEvent, PointerPhase};
    let phase = match phase {
        0 => PointerPhase::Down, 1 => PointerPhase::Move,
        2 => PointerPhase::Up, 3 => PointerPhase::Cancel,
        _ => return Err("unknown pointer phase".into()),
    };
    let (events, pixels) = INPUT_DOCUMENT.with(|state| {
        let mut slot = state.borrow_mut();
        let doc = slot.as_mut().ok_or("input document has not been opened")?;
        let events = doc.pointer(PointerEvent {
            pointer_id, phase, x_css, y_css, time_ms: time_ms.max(0) as u64,
        });
        let repaint = events.iter().any(|e| matches!(
            e, isr_core::GestureEvent::Press(_) | isr_core::GestureEvent::Release(_)
        ));
        let json = serde_json::Value::Array(
            events.into_iter().map(|event| event_json(doc, event)).collect()
        );
        let pixels = if repaint { doc.update(); Some(doc.paint()) } else { None };
        Ok::<_, String>((json, pixels))
    })?;
    input_reply(env, events, pixels)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_htmlnativerenderer_demo_NativeRenderer_inputPointer<'a>(
    mut env: JNIEnv<'a>, _receiver: JObject<'a>,
    pointer_id: jni::sys::jlong, phase: jint,
    x_css: jni::sys::jdouble, y_css: jni::sys::jdouble, time_ms: jni::sys::jlong,
) -> jobject {
    match catch_unwind(AssertUnwindSafe(|| {
        input_pointer_inner(&mut env, pointer_id, phase, x_css, y_css, time_ms)
    })) {
        Ok(Ok(result)) => result.into_raw(),
        failed => {
            let message = match failed {
                Ok(Err(e)) => e, Err(_) => "input dispatch panicked".into(),
                Ok(Ok(_)) => unreachable!(),
            };
            if !env.exception_check().unwrap_or(true) {
                let _ = env.throw_new("java/lang/RuntimeException", message);
            }
            std::ptr::null_mut()
        }
    }
}

fn input_tick_inner<'a>(env: &mut JNIEnv<'a>, time_ms: jni::sys::jlong)
    -> Result<JObject<'a>, String>
{
    let events = INPUT_DOCUMENT.with(|state| {
        let mut slot = state.borrow_mut();
        let doc = slot.as_mut().ok_or("input document has not been opened")?;
        Ok::<_, String>(serde_json::Value::Array(
            doc.tick(time_ms.max(0) as u64).into_iter()
                .map(|event| event_json(doc, event)).collect()
        ))
    })?;
    input_reply(env, events, None)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_htmlnativerenderer_demo_NativeRenderer_inputTick<'a>(
    mut env: JNIEnv<'a>, _receiver: JObject<'a>, time_ms: jni::sys::jlong,
) -> jobject {
    match catch_unwind(AssertUnwindSafe(|| input_tick_inner(&mut env, time_ms))) {
        Ok(Ok(result)) => result.into_raw(),
        failed => {
            let message = match failed {
                Ok(Err(e)) => e, Err(_) => "input tick panicked".into(),
                Ok(Ok(_)) => unreachable!(),
            };
            if !env.exception_check().unwrap_or(true) {
                let _ = env.throw_new("java/lang/RuntimeException", message);
            }
            std::ptr::null_mut()
        }
    }
}
