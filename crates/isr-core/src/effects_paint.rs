// Copyright 2026 g1BSsQ. Licensed under Apache-2.0.
// Filtered groups are rasterized only on pages with CSS filter effects.
// Unfiltered pages retain the direct paint path.
use std::sync::Arc;

use anyrender::{
    filters::{Filter, FilterEffect, component_transfer::TransferFunction},
    recording::RenderCommand,
    render_to_buffer, PaintScene, Scene,
};
use anyrender_vello_cpu::VelloCpuImageRenderer;
use peniko::{Blob, ImageAlphaType, ImageBrush, ImageData, ImageFormat, kurbo::Affine};

// A finite cap prevents malicious radii and very large pages from causing
// unbounded temporary allocations or quadratic convolution work.
const MAX_SIGMA: f32 = 8.0;
const MAX_FILTER_PIXELS: usize = 1_048_576;

fn kernel(sigma: f32) -> Vec<f32> {
    let sigma = sigma.clamp(0.0, MAX_SIGMA);
    if sigma < 0.01 {
        return vec![1.0];
    }
    let radius = (sigma * 3.0).ceil() as i32;
    let mut weights = Vec::with_capacity((radius * 2 + 1) as usize);
    let mut sum = 0.0;
    for offset in -radius..=radius {
        let weight = (-0.5 * (offset as f32 / sigma).powi(2)).exp();
        sum += weight;
        weights.push(weight);
    }
    for weight in &mut weights {
        *weight /= sum;
    }
    weights
}

// Two separable passes through premultiplied RGBA avoid the dark fringes
// produced by blurring unassociated channels. All scratch is allocated once
// per pass, never once per pixel.
fn blur_rgba(pixels: &mut Vec<u8>, width: usize, height: usize, sigma: f32) {
    let weights = kernel(sigma);
    if weights.len() == 1 {
        return;
    }
    let radius = (weights.len() / 2) as isize;
    let mut intermediate = vec![[0.0f32; 4]; width * height];
    for y in 0..height {
        for x in 0..width {
            let mut sum = [0.0f32; 4];
            for (i, &weight) in weights.iter().enumerate() {
                let sx = x as isize + i as isize - radius;
                if sx < 0 || sx >= width as isize {
                    continue;
                }
                let p = (y * width + sx as usize) * 4;
                let a = pixels[p + 3] as f32 / 255.0;
                for channel in 0..3 {
                    sum[channel] += weight * (pixels[p + channel] as f32 / 255.0) * a;
                }
                sum[3] += weight * a;
            }
            intermediate[y * width + x] = sum;
        }
    }
    for y in 0..height {
        for x in 0..width {
            let mut sum = [0.0f32; 4];
            for (i, &weight) in weights.iter().enumerate() {
                let sy = y as isize + i as isize - radius;
                if sy < 0 || sy >= height as isize {
                    continue;
                }
                let p = intermediate[sy as usize * width + x];
                for channel in 0..4 {
                    sum[channel] += weight * p[channel];
                }
            }
            let base = (y * width + x) * 4;
            let alpha = sum[3].clamp(0.0, 1.0);
            for channel in 0..3 {
                pixels[base + channel] =
                    (255.0 * (sum[channel] / alpha.max(1e-6)).clamp(0.0, 1.0)).round() as u8;
            }
            pixels[base + 3] = (255.0 * alpha).round() as u8;
        }
    }
}

fn blur_alpha(input: &[f32], width: usize, height: usize, sigma: f32) -> Vec<f32> {
    let weights = kernel(sigma);
    if weights.len() == 1 {
        return input.to_vec();
    }
    let radius = (weights.len() / 2) as isize;
    let mut horizontal = vec![0.0; input.len()];
    let mut output = vec![0.0; input.len()];
    for y in 0..height {
        for x in 0..width {
            let mut sum = 0.0;
            for (i, &weight) in weights.iter().enumerate() {
                let sx = x as isize + i as isize - radius;
                if sx >= 0 && sx < width as isize {
                    sum += weight * input[y * width + sx as usize];
                }
            }
            horizontal[y * width + x] = sum;
        }
    }
    for y in 0..height {
        for x in 0..width {
            let mut sum = 0.0;
            for (i, &weight) in weights.iter().enumerate() {
                let sy = y as isize + i as isize - radius;
                if sy >= 0 && sy < height as isize {
                    sum += weight * horizontal[sy as usize * width + x];
                }
            }
            output[y * width + x] = sum;
        }
    }
    output
}

fn drop_shadow(
    pixels: &mut Vec<u8>,
    width: usize,
    height: usize,
    dx: f32,
    dy: f32,
    sigma: f32,
    color: [f32; 4],
) {
    let alpha: Vec<f32> = pixels.chunks_exact(4).map(|p| p[3] as f32 / 255.0).collect();
    let mask = blur_alpha(&alpha, width, height, sigma);
    let shift_x = dx.clamp(-(width as f32), width as f32).round() as isize;
    let shift_y = dy.clamp(-(height as f32), height as f32).round() as isize;
    for y in 0..height {
        for x in 0..width {
            let sx = x as isize - shift_x;
            let sy = y as isize - shift_y;
            if sx < 0 || sy < 0 || sx >= width as isize || sy >= height as isize {
                continue;
            }
            let shadow_a = (mask[sy as usize * width + sx as usize] * color[3])
                .clamp(0.0, 1.0);
            let p = (y * width + x) * 4;
            let source_a = pixels[p + 3] as f32 / 255.0;
            let behind = shadow_a * (1.0 - source_a);
            let out_a = source_a + behind;
            for c in 0..3 {
                let source = pixels[p + c] as f32 / 255.0;
                let out = (source * source_a + color[c] * behind) / out_a.max(1e-6);
                pixels[p + c] = (out.clamp(0.0, 1.0) * 255.0).round() as u8;
            }
            pixels[p + 3] = (out_a * 255.0).round() as u8;
        }
    }
}

fn apply_color_matrix(pixels: &mut [u8], matrix: &[f32; 20]) {
    for pixel in pixels.chunks_exact_mut(4) {
        let v = [
            pixel[0] as f32 / 255.0,
            pixel[1] as f32 / 255.0,
            pixel[2] as f32 / 255.0,
            pixel[3] as f32 / 255.0,
            1.0,
        ];
        for channel in 0..4 {
            let m = &matrix[channel * 5..channel * 5 + 5];
            let result = (0..5).map(|k| m[k] * v[k]).sum::<f32>();
            pixel[channel] = (255.0 * result.clamp(0.0, 1.0)).round() as u8;
        }
    }
}

fn transfer(value: u8, function: &TransferFunction) -> Option<u8> {
    let x = value as f32 / 255.0;
    let result = match function {
        TransferFunction::Identity => x,
        TransferFunction::Linear(f) => x * f.slope + f.intercept,
        _ => return None,
    };
    Some((255.0 * result.clamp(0.0, 1.0)).round() as u8)
}

fn supports(filter: &Filter) -> bool {
    !filter.nodes().is_empty() && filter.nodes().iter().all(|node| match &node.effect {
        FilterEffect::GaussianBlur(_)
        | FilterEffect::DropShadow(_)
        | FilterEffect::ColorMatrix(_) => true,
        FilterEffect::ComponentTransfer(f) => [
            &f.red_function, &f.green_function, &f.blue_function, &f.alpha_function
        ].iter().all(|func| matches!(func, TransferFunction::Identity | TransferFunction::Linear(_))),
        _ => false,
    })
}

fn apply_filters(pixels: &mut Vec<u8>, width: usize, height: usize, filter: &Filter) {
    // linear_list stores successive CSS functions in their specified order.
    for node in filter.nodes() {
        match &node.effect {
            FilterEffect::GaussianBlur(f) => {
                blur_rgba(pixels, width, height, f.std_deviation);
            }
            FilterEffect::DropShadow(f) => {
                drop_shadow(pixels, width, height, f.dx, f.dy, f.std_deviation, f.color.components);
            }
            FilterEffect::ColorMatrix(f) => apply_color_matrix(pixels, &f.0),
            FilterEffect::ComponentTransfer(f) => {
                for px in pixels.chunks_exact_mut(4) {
                    for (c, func) in [
                        &f.red_function, &f.green_function,
                        &f.blue_function, &f.alpha_function,
                    ].iter().enumerate() {
                        px[c] = transfer(px[c], func).unwrap_or(px[c]);
                    }
                }
            }
            _ => {} // Never reached: supports() rejects unsupported primitives.
        }
    }
}

fn matching_pop(commands: &[RenderCommand], start: usize) -> Option<usize> {
    let mut depth = 1usize;
    for (i, cmd) in commands.iter().enumerate().skip(start + 1) {
        match cmd {
            RenderCommand::PushLayer(_) | RenderCommand::PushClipLayer(_) => depth += 1,
            RenderCommand::PopLayer => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

fn process(commands: &[RenderCommand], width: u32, height: u32) -> Vec<RenderCommand> {
    let mut output = Vec::with_capacity(commands.len());
    let mut i = 0;
    while i < commands.len() {
        if let RenderCommand::PushLayer(layer) = &commands[i] {
            if let Some(end) = matching_pop(commands, i) {
                let body = process(&commands[i + 1..end], width, height);
                let filter = layer.filter.as_ref().filter(|f| supports(f));
                let pixel_count = width as usize * height as usize;
                if let Some(filter) = filter.filter(|_| pixel_count <= MAX_FILTER_PIXELS) {
                    let offscreen = Scene { tolerance: 0.1, commands: body };
                    let mut rgba = render_to_buffer::<VelloCpuImageRenderer, _>(
                        |scene| scene.append_scene(offscreen, Affine::IDENTITY),
                        width, height,
                    );
                    apply_filters(&mut rgba, width as usize, height as usize, filter);
                    let brush = ImageBrush::new(ImageData {
                        data: Blob::new(Arc::new(rgba)),
                        format: ImageFormat::Rgba8,
                        alpha_type: ImageAlphaType::Alpha,
                        width, height,
                    });
                    let mut painted = Scene::new();
                    let mut unfiltered_layer = layer.clone();
                    unfiltered_layer.filter = None;
                    painted.commands.push(RenderCommand::PushLayer(unfiltered_layer));
                    painted.draw_image(brush.as_ref(), Affine::IDENTITY);
                    painted.pop_layer();
                    output.extend(painted.commands);
                } else {
                    // Keep unsupported functions unchanged; the native backend
                    // paints the unfiltered source, not a fabricated approximation.
                    output.push(commands[i].clone());
                    output.extend(body);
                    output.push(RenderCommand::PopLayer);
                }
                i = end + 1;
                continue;
            }
        }
        output.push(commands[i].clone());
        i += 1;
    }
    output
}

/// Replace supported filtered subtrees with their offscreen raster results.
/// The caller invokes this only for documents with filter properties.
pub(crate) fn resolve(scene: Scene, width: u32, height: u32) -> Scene {
    Scene {
        tolerance: scene.tolerance,
        commands: process(&scene.commands, width, height),
    }
}
