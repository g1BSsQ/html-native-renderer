// Copyright 2026 g1BSsQ. Licensed under Apache-2.0.
// Shared native benchmark workload for host and Android JNI entry points.
use isr_core::{Document, RenderOptions, Result, Timings, render};
use serde_json::json;
use std::hint::black_box;
use std::time::Instant;

pub const WARMUP: usize = 20;
pub const ITERATIONS: usize = 200;

#[derive(Clone, Copy)]
enum Change { Class(&'static str), Text }

struct Page {
    name: &'static str,
    html: String,
    changed_html: String,
    id: &'static str,
    change: Change,
}

fn pages() -> Vec<Page> {
    // All workload transformations are in memory. The checked-in source
    // fixtures are not modified, and the generated list retains their styles.
    let form = include_str!("../../../fixtures/08-form-layout.html")
        .replace("</style>", ".value.invalid{border:3px solid #c22;background:#ffe4e4}</style>")
        .replace("<div class=\"value\">River bend</div>",
                 "<input id=\"probe\" class=\"value\" type=\"text\" value=\"River bend\">");
    let form_changed = form.replace("id=\"probe\" class=\"value\"",
                                    "id=\"probe\" class=\"value invalid\"");

    let list_source = include_str!("../../../fixtures/02-list-rows.html");
    let mut rows = String::new();
    for index in 1..=200 {
        let id = if index == 100 { " id=\"probe\"" } else { "" };
        rows.push_str(&format!(
            "<div class=\"row item\"><div class=\"dot\"></div><div class=\"copy\">Row {index}</div><div class=\"muted\"{id}>08:00</div></div>"
        ));
    }
    let open = list_source.find("<main id=\"list\">").expect("list main");
    let close = list_source.find("</main>").expect("list end") + "</main>".len();
    let list = format!(
        "{}<main id=\"list\"><h1>Today</h1>{rows}</main>{}",
        &list_source[..open], &list_source[close..]
    );
    let list_changed = list.replace("id=\"probe\">08:00", "id=\"probe\">13:45");

    let gallery = include_str!("../../../fixtures/09-grid-gallery.html")
        .replace("</style>", ".tile.selected{border:4px solid #176ab3;opacity:.82}</style>")
        .replace("<div class=\"tile\">River</div>",
                 "<div id=\"probe\" class=\"tile\">River</div>");
    let gallery_changed = gallery.replace("id=\"probe\" class=\"tile\"",
                                          "id=\"probe\" class=\"tile selected\"");

    vec![
        Page { name: "form_invalid", html: form, changed_html: form_changed,
            id: "probe", change: Change::Class("invalid") },
        Page { name: "list_200_text", html: list, changed_html: list_changed,
            id: "probe", change: Change::Text },
        Page { name: "gallery_selected", html: gallery, changed_html: gallery_changed,
            id: "probe", change: Change::Class("selected") },
    ]
}

fn options(width: u32, fonts: &[Vec<u8>]) -> RenderOptions {
    RenderOptions { width, fonts: fonts.to_vec(), ..RenderOptions::default() }
}

fn summary(mut samples: Vec<u64>) -> serde_json::Value {
    samples.sort_unstable();
    let n = samples.len();
    // Nearest-rank percentiles, not interpolated. Always have >=1 sample.
    let percentile = |p: f64| samples[((n as f64 * p).ceil() as usize).saturating_sub(1)];
    json!({
        "p50": percentile(0.50),
        "p95": percentile(0.95),
        "max": samples[n - 1]
    })
}

fn motion_benchmark(
    width_css: u32, height_css: u32, scale: f32, fonts: &[Vec<u8>],
) -> Result<String> {
    const FRAMES: usize = 120;
    let html = include_str!("../../../fixtures/motion/staggered-cards.html");
    let mut retained = Document::new_scaled(html, options(width_css, fonts), scale)?;
    retained.set_viewport(0.0, height_css)?;
    let mut samples = Vec::with_capacity(FRAMES);
    for frame in 0..FRAMES {
        let (timings, _) = retained.tick(frame as f64 * (1000.0 / 60.0));
        samples.push(timings);
    }
    let baseline_start = Instant::now();
    let one_shot = render(html, options(width_css, fonts))?;
    black_box(one_shot.rgba.len());
    let baseline_us = baseline_start.elapsed().as_micros() as u64;
    let (bitmap_w, bitmap_h) = retained.bitmap_dimensions();
    Ok(json!({
        "page": "motion_12_cards",
        "width_css_px": width_css,
        "scale": scale,
        "bitmap_w": bitmap_w,
        "bitmap_h": bitmap_h,
        "viewport_height_css_px": height_css,
        "viewport_scroll_y_css": retained.viewport_scroll_y(),
        "baseline_render_scale": 1.0,
        "warmup": 0,
        "iterations": FRAMES,
        "phase_accounting": "style_only_per_tick_no_layout",
        "style_us": summary(samples.iter().map(|t| t.style_us).collect()),
        "layout_us": summary(samples.iter().map(|t| t.layout_us).collect()),
        "paint_us": summary(samples.iter().map(|t| t.paint_us).collect()),
        "damage_px": summary(samples.iter().map(|t| t.damage_px).collect()),
        "raster_threads": samples.iter().map(|t| t.raster_threads).max().unwrap_or(1),
        "strips": samples.iter().map(|t| t.strips).max().unwrap_or(1),
        "paint_path": {
            "partial": samples.iter().filter(|t| t.paint_path == "partial").count(),
            "full_viewport": samples.iter().filter(|t| t.paint_path == "full_viewport").count()
        },
        "total_us": summary(samples.iter().map(Timings::total_us).collect()),
        "baseline_render_us": baseline_us
    }).to_string())
}

/// Run the same three retained-document workloads on a host or via JNI.
/// Exactly `iterations` samples follow `warmup` discarded updates.
/// One additional fresh render of the final DOM is recorded as the baseline.
pub fn run_all_viewport(
    width_css: u32, height_css: u32, scale: f32, fonts: &[Vec<u8>],
    warmup: usize, iterations: usize,
) -> Result<Vec<String>> {
    if iterations == 0 { return Err("iterations must be positive".into()); }
    if height_css == 0 { return Err("viewport height must be positive".into()); }
    if !scale.is_finite() || !(0.25..=4.0).contains(&scale) {
        return Err("scale must be finite and in 0.25..=4.0".into());
    }
    let mut lines = Vec::new();
    for page in pages() {
        let mut retained = Document::new_scaled(&page.html, options(width_css, fonts), scale)?;
        let target = retained.element_by_id(page.id)
            .ok_or("benchmark target was not parsed")?;
        let (_, top, _, target_h) = retained.element_bounds(target)
            .ok_or("benchmark target has no layout box")?;
        let scroll_y = (top + target_h / 2.0 - height_css as f64 / 2.0).max(0.0);
        retained.set_viewport(scroll_y, height_css)?;
        let mut updates: Vec<Timings> = Vec::with_capacity(iterations);
        for i in 0..(warmup + iterations) {
            match page.change {
                Change::Class(class) => retained.toggle_class(target, class)?,
                Change::Text => retained.set_text(
                    target, if i % 2 == 0 { "13:45" } else { "08:00" }
                )?,
            }
            let timing = retained.update();
            if i >= warmup {
                updates.push(timing);
            }
        }
        let is_changed = (warmup + iterations - 1) % 2 == 0;
        let final_html = if is_changed { &page.changed_html } else { &page.html };
        let baseline_start = Instant::now();
        let baseline = render(final_html, options(width_css, fonts))?;
        black_box(baseline.rgba.len());
        let baseline_us = baseline_start.elapsed().as_micros() as u64;
        let (bitmap_w, bitmap_h) = retained.bitmap_dimensions();
        let record = json!({
            "page": page.name,
            "width_css_px": width_css,
            "scale": scale,
            "bitmap_w": bitmap_w,
            "bitmap_h": bitmap_h,
            "viewport_height_css_px": height_css,
            "viewport_scroll_y_css": retained.viewport_scroll_y(),
            "baseline_render_scale": 1.0,
            "warmup": warmup,
            "iterations": iterations,
            "phase_accounting": "style_layout_combined_in_layout_us",
            "style_us": summary(updates.iter().map(|t| t.style_us).collect()),
            "layout_us": summary(updates.iter().map(|t| t.layout_us).collect()),
            "paint_us": summary(updates.iter().map(|t| t.paint_us).collect()),
            "damage_px": summary(updates.iter().map(|t| t.damage_px).collect()),
            "raster_threads": updates.iter().map(|t| t.raster_threads).max().unwrap_or(1),
            "strips": updates.iter().map(|t| t.strips).max().unwrap_or(1),
            "paint_path": {
                "partial": updates.iter().filter(|t| t.paint_path == "partial").count(),
                "full_viewport": updates.iter().filter(|t| t.paint_path == "full_viewport").count()
            },
            "total_us": summary(updates.iter().map(Timings::total_us).collect()),
            "baseline_render_us": baseline_us
        });
        lines.push(record.to_string());
    }
    lines.push(motion_benchmark(width_css, height_css, scale, fonts)?);
    Ok(lines)
}

/// Compatibility wrapper using an 800-CSS-pixel viewport.
pub fn run_all(
    width_css: u32, scale: f32, fonts: &[Vec<u8>],
    warmup: usize, iterations: usize,
) -> Result<Vec<String>> {
    run_all_viewport(width_css, 800, scale, fonts, warmup, iterations)
}

pub fn run_standard_viewport(
    width_css: u32, height_css: u32, scale: f32, fonts: &[Vec<u8>],
) -> Result<Vec<String>> {
    run_all_viewport(width_css, height_css, scale, fonts, WARMUP, ITERATIONS)
}

pub fn run_standard(width_css: u32, scale: f32, fonts: &[Vec<u8>]) -> Result<Vec<String>> {
    run_all(width_css, scale, fonts, WARMUP, ITERATIONS)
}
