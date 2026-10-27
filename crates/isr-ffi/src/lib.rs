// Copyright 2026 g1BSsQ. Licensed under Apache-2.0.
// Platform-neutral, caller-owned C ABI for a single offscreen document render.
use isr_core::{RenderOptions, render};
use std::{
    ffi::{CString, c_char},
    panic::{AssertUnwindSafe, catch_unwind},
    ptr, slice,
};

const OK: i32 = 0;
const INVALID_ARGUMENT: i32 = 1;
const RENDER_ERROR: i32 = 2;
const PANIC: i32 = 3;
const MAX_PIXEL_BYTES: usize = 128 * 1024 * 1024;

/// All pointers are owned by the library. The caller must call isr_free once
/// after every isr_render call with a non-null out pointer, even on error.
/// This structure must not be copied and freed twice.
#[repr(C)]
pub struct IsrOutput {
    /// RGBA8, premultiplied alpha, row-major, width * 4 bytes per row.
    pub pixels: *mut u8,
    pub pixels_len: usize,
    /// Pixel dimensions after scaling (not CSS-pixel dimensions).
    pub width: u32,
    pub height: u32,
    /// NUL-terminated UTF-8 JSON in isr-core's existing layout() schema.
    pub element_boxes_json: *mut c_char,
    /// NUL-terminated UTF-8 message on nonzero status, otherwise NULL.
    pub error_message: *mut c_char,
}

impl Default for IsrOutput {
    fn default() -> Self {
        Self {
            pixels: ptr::null_mut(),
            pixels_len: 0,
            width: 0,
            height: 0,
            element_boxes_json: ptr::null_mut(),
            error_message: ptr::null_mut(),
        }
    }
}

fn c_message(value: &str) -> *mut c_char {
    // Error strings can originate outside this crate and may contain NUL.
    CString::new(value.replace('\0', " ")).expect("NUL was removed").into_raw()
}

fn render_document(
    html: &[u8],
    width_css_px: u32,
    scale: f32,
) -> Result<IsrOutput, (i32, String)> {
    if !(1..=8192).contains(&width_css_px) || !scale.is_finite() || !(1.0..=3.0).contains(&scale) {
        return Err((INVALID_ARGUMENT, "width must be 1..=8192 CSS px; scale must be finite and 1..=3".into()));
    }
    let html = std::str::from_utf8(html)
        .map_err(|e| (INVALID_ARGUMENT, format!("HTML is not valid UTF-8: {e}")))?;

    // A fresh default denies system font discovery and accepts no host font
    // files. isr-core already refuses non-data: subresources and never
    // executes document scripts.
    let options = RenderOptions { width: width_css_px, ..RenderOptions::default() };
    let rendered = render(html, options)
        .map_err(|e| (RENDER_ERROR, format!("render failed: {e}")))?;
    let layout = serde_json::to_string(&rendered.layout())
        .map_err(|e| (RENDER_ERROR, format!("layout JSON failed: {e}")))?;
    let json = CString::new(layout)
        .map_err(|e| (RENDER_ERROR, format!("layout JSON contains NUL: {e}")))?;

    let source_width = rendered.width as usize;
    let source_height = rendered.height as usize;
    let expected_source_bytes = source_width.checked_mul(source_height)
        .and_then(|n| n.checked_mul(4))
        .ok_or((RENDER_ERROR, "source image dimensions overflow".into()))?;
    if rendered.rgba.len() != expected_source_bytes {
        return Err((RENDER_ERROR, "renderer returned an unexpected RGBA8 buffer length".into()));
    }

    // isr-core currently paints at scale 1. Resample deterministically here
    // instead of changing the shared engine. This is NOT high-DPI repainting.
    let width = ((rendered.width as f64) * f64::from(scale)).round() as u32;
    let height = ((rendered.height as f64) * f64::from(scale)).round() as u32;
    let destination_width = width as usize;
    let destination_height = height as usize;
    let bytes = destination_width.checked_mul(destination_height)
        .and_then(|n| n.checked_mul(4))
        .filter(|n| *n <= MAX_PIXEL_BYTES)
        .ok_or((RENDER_ERROR, "scaled image exceeds 128 MiB output limit".into()))?;
    let mut rgba = vec![0_u8; bytes];

    for y in 0..destination_height {
        let source_y = y * source_height / destination_height;
        for x in 0..destination_width {
            let source_x = x * source_width / destination_width;
            let input = (source_y * source_width + source_x) * 4;
            let output = (y * destination_width + x) * 4;
            let alpha = u16::from(rendered.rgba[input + 3]);
            for component in 0..3 {
                // Core's opaque white canvas normally yields alpha == 255;
                // enforce a premultiplied-alpha contract for every pixel.
                rgba[output + component] =
                    ((u16::from(rendered.rgba[input + component]) * alpha + 127) / 255) as u8;
            }
            rgba[output + 3] = alpha as u8;
        }
    }

    let pixels_len = rgba.len();
    let pixels = Box::into_raw(rgba.into_boxed_slice()) as *mut u8;
    Ok(IsrOutput {
        pixels, pixels_len, width, height,
        element_boxes_json: json.into_raw(),
        error_message: ptr::null_mut(),
    })
}

unsafe fn render_entry(
    html_utf8: *const u8,
    len: usize,
    width_css_px: u32,
    scale: f32,
    out: *mut IsrOutput,
) -> i32 {
    if out.is_null() {
        // There is nowhere to return an error string in this case.
        return INVALID_ARGUMENT;
    }
    // The caller must supply a writable, properly aligned output. It need
    // not initialize the struct. Previous allocations must be freed first.
    unsafe { ptr::write(out, IsrOutput::default()) };
    if len > isize::MAX as usize || (len != 0 && html_utf8.is_null()) {
        unsafe { (*out).error_message = c_message("invalid HTML pointer or length") };
        return INVALID_ARGUMENT;
    }
    let html = if len == 0 { &[] } else { unsafe { slice::from_raw_parts(html_utf8, len) } };
    match render_document(html, width_css_px, scale) {
        Ok(result) => {
            unsafe { ptr::write(out, result) };
            OK
        }
        Err((status, message)) => {
            unsafe { (*out).error_message = c_message(&message) };
            status
        }
    }
}

/// Returns 0 on success, 1 for invalid arguments, 2 for rendering failure,
/// or 3 when a panic was caught. On failure, out.error_message is allocated.
/// The caller supplies valid memory for input and output for this call.
/// No Rust panic is allowed to unwind through the C ABI.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn isr_render(
    html_utf8: *const u8,
    len: usize,
    width_css_px: u32,
    scale: f32,
    out: *mut IsrOutput,
) -> i32 {
    match catch_unwind(AssertUnwindSafe(|| unsafe {
        render_entry(html_utf8, len, width_css_px, scale, out)
    })) {
        Ok(status) => status,
        Err(_) => {
            if !out.is_null() {
                // render_entry initializes out before doing fallible work.
                // This also discards partially constructed error messages.
                let _ = catch_unwind(AssertUnwindSafe(|| unsafe {
                    (*out).error_message = c_message("renderer panicked");
                }));
            }
            PANIC
        }
    }
}

unsafe fn free_entry(out: *mut IsrOutput) {
    if out.is_null() { return; }
    let previous = unsafe { std::mem::take(&mut *out) };
    if !previous.pixels.is_null() {
        let array = ptr::slice_from_raw_parts_mut(previous.pixels, previous.pixels_len);
        unsafe { drop(Box::from_raw(array)) };
    }
    if !previous.element_boxes_json.is_null() {
        unsafe { drop(CString::from_raw(previous.element_boxes_json)) };
    }
    if !previous.error_message.is_null() {
        unsafe { drop(CString::from_raw(previous.error_message)) };
    }
}

/// Releases all output allocations and zeros the struct. Safe to call twice
/// on the same output struct (but not on copies of it). NULL is accepted.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn isr_free(out: *mut IsrOutput) {
    let _ = catch_unwind(AssertUnwindSafe(|| unsafe { free_entry(out) }));
}
