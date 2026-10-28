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

/// Borrowed, read-only bytes from the host; valid only during the call.
/// Neither these pointers nor their contents are retained by the renderer.
#[repr(C)]
pub struct IsrBytes {
    pub data: *const u8,
    pub len: usize,
}

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
    fonts: Vec<Vec<u8>>,
) -> Result<IsrOutput, (i32, String)> {
    if !(1..=8192).contains(&width_css_px) || !scale.is_finite() || !(1.0..=3.0).contains(&scale) {
        return Err((INVALID_ARGUMENT, "width must be 1..=8192 CSS px; scale must be finite and 1..=3".into()));
    }
    let html = std::str::from_utf8(html)
        .map_err(|e| (INVALID_ARGUMENT, format!("HTML is not valid UTF-8: {e}")))?;

    // Only explicitly supplied in-memory font bytes are registered. No system
    // font discovery, paths, network resources or script execution are enabled.
    let options = RenderOptions { width: width_css_px, fonts, ..RenderOptions::default() };
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
            // The painting backend already produces premultiplied RGBA8.
            // Copy all four channels unchanged, including non-opaque alpha.
            copy_premultiplied_pixel(&rendered.rgba[input..input + 4], &mut rgba[output..output + 4]);
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

fn copy_premultiplied_pixel(source: &[u8], destination: &mut [u8]) {
    debug_assert_eq!(source.len(), 4);
    debug_assert_eq!(destination.len(), 4);
    destination.copy_from_slice(source);
}

unsafe fn render_entry(
    html_utf8: *const u8,
    len: usize,
    width_css_px: u32,
    scale: f32,
    fonts: *const IsrBytes,
    font_count: usize,
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
    if font_count > 32 || (font_count != 0 && fonts.is_null()) {
        unsafe { (*out).error_message = c_message("invalid font pointer or count (max 32)") };
        return INVALID_ARGUMENT;
    }
    let font_slices = if font_count == 0 {
        &[][..]
    } else {
        unsafe { slice::from_raw_parts(fonts, font_count) }
    };
    let mut owned_fonts = Vec::with_capacity(font_slices.len());
    let mut total_bytes = 0_usize;
    for font in font_slices {
        if font.len > isize::MAX as usize || (font.len != 0 && font.data.is_null()) {
            unsafe { (*out).error_message = c_message("invalid font byte pointer or length") };
            return INVALID_ARGUMENT;
        }
        total_bytes = match total_bytes.checked_add(font.len) {
            Some(n) if n <= 64 * 1024 * 1024 => n,
            _ => {
                unsafe { (*out).error_message = c_message("font data exceeds 64 MiB limit") };
                return INVALID_ARGUMENT;
            }
        };
        let bytes = if font.len == 0 { &[] } else { unsafe { slice::from_raw_parts(font.data, font.len) } };
        owned_fonts.push(bytes.to_vec());
    }
    let html = if len == 0 { &[] } else { unsafe { slice::from_raw_parts(html_utf8, len) } };
    match render_document(html, width_css_px, scale, owned_fonts) {
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

/// The zero-font convenience entry point. Keep its C ABI stable.
/// Returns 0 success, 1 invalid argument, 2 renderer error, 3 caught panic.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn isr_render(
    html_utf8: *const u8,
    len: usize,
    width_css_px: u32,
    scale: f32,
    out: *mut IsrOutput,
) -> i32 {
    unsafe { isr_render_with_fonts(html_utf8, len, width_css_px, scale, ptr::null(), 0, out) }
}

/// Render with zero or more borrowed, in-memory font byte slices.
/// Fonts never contain paths and are copied before the engine receives them.
/// The caller must keep all pointers valid for this call.
/// No Rust panic may unwind across the C boundary.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn isr_render_with_fonts(
    html_utf8: *const u8,
    len: usize,
    width_css_px: u32,
    scale: f32,
    fonts: *const IsrBytes,
    font_count: usize,
    out: *mut IsrOutput,
) -> i32 {
    match catch_unwind(AssertUnwindSafe(|| unsafe {
        render_entry(html_utf8, len, width_css_px, scale, fonts, font_count, out)
    })) {
        Ok(status) => status,
        Err(_) => {
            if !out.is_null() {
                // render_entry initializes out before any fallible work.
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

// Failure paths and buffer ownership are tested without running the renderer.
#[cfg(test)]
mod tests {
    use super::*;

    fn invalid(html: *const u8, len: usize, width: u32, scale: f32) {
        let mut out = IsrOutput::default();
        let code = unsafe { isr_render(html, len, width, scale, &mut out) };
        assert_eq!(code, INVALID_ARGUMENT);
        assert!(!out.error_message.is_null());
        unsafe { isr_free(&mut out) };
        assert!(out.error_message.is_null());
        assert!(out.pixels.is_null());
    }

    #[test]
    fn null_out_returns_error_without_dereferencing() {
        let code = unsafe { isr_render(ptr::null(), 0, 412, 1.0, ptr::null_mut()) };
        assert_eq!(code, INVALID_ARGUMENT);
    }

    #[test]
    fn rejects_null_html_with_nonzero_length() {
        invalid(ptr::null(), 1, 412, 1.0);
    }

    #[test]
    fn rejects_invalid_utf8() {
        invalid([0xff_u8].as_ptr(), 1, 412, 1.0);
    }

    #[test]
    fn rejects_invalid_dimensions_and_scale() {
        let html = b"<p>test</p>";
        for width in [0, 8193, u32::MAX] {
            invalid(html.as_ptr(), html.len(), width, 1.0);
        }
        for scale in [0.0, -1.0, 3.1, f32::NAN, f32::INFINITY] {
            invalid(html.as_ptr(), html.len(), 412, scale);
        }
    }

    #[test]
    fn rejects_invalid_font_slices() {
        let html = b"<p>test</p>";
        let mut out = IsrOutput::default();
        let status = unsafe {
            isr_render_with_fonts(html.as_ptr(), html.len(), 412, 1.0, ptr::null(), 1, &mut out)
        };
        assert_eq!(status, INVALID_ARGUMENT);
        unsafe { isr_free(&mut out) };
        let font = IsrBytes { data: ptr::null(), len: 1 };
        let status = unsafe {
            isr_render_with_fonts(html.as_ptr(), html.len(), 412, 1.0, &font, 1, &mut out)
        };
        assert_eq!(status, INVALID_ARGUMENT);
        unsafe { isr_free(&mut out) };
    }

    #[test]
    fn double_free_is_safe_on_one_output_struct() {
        let mut out = IsrOutput::default();
        let code = unsafe { isr_render(ptr::null(), 1, 412, 1.0, &mut out) };
        assert_eq!(code, INVALID_ARGUMENT);
        unsafe {
            isr_free(&mut out);
            isr_free(&mut out);
            isr_free(ptr::null_mut());
        }
        assert!(out.error_message.is_null());
        assert!(out.pixels.is_null());
    }

    #[test]
    fn premultiplied_channels_are_never_multiplied_twice() {
        let source = [40_u8, 20, 10, 128];
        let mut destination = [0_u8; 4];
        copy_premultiplied_pixel(&source, &mut destination);
        assert_eq!(destination, source);
    }
}
