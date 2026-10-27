// Copyright 2026 g1BSsQ. Licensed under Apache-2.0.
// Offscreen painting adapted from Blitz examples/screenshot.rs; see NOTICE.
use anyrender::{PaintScene as _, render_to_buffer};
use anyrender_vello_cpu::VelloCpuImageRenderer;
use blitz_dom::{BaseDocument, DocumentConfig, Node, NodeData, util::Color};
use blitz_html::HtmlDocument;
use blitz_traits::{
    net::{Bytes, NetHandler, NetProvider, Request},
    shell::{ColorScheme, Viewport},
};
use clap::{Parser, Subcommand};
use peniko::{Fill, kurbo::Rect};
use serde::Serialize;
use std::{
    fs,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

#[derive(Parser)]
#[command(about = "Offscreen script-free HTML/CSS renderer")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    Render {
        input: PathBuf,
        #[arg(long, default_value_t = 412, value_parser = clap::value_parser!(u32).range(1..=8192))]
        width: u32,
        #[arg(long, required = true)]
        height_auto: bool,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        layout: PathBuf,
    },
}

#[derive(Default)]
struct DataOnly {
    requests: AtomicUsize,
    blocked: Mutex<Vec<String>>,
}
impl NetProvider for DataOnly {
    fn fetch(&self, _: usize, request: Request, handler: Box<dyn NetHandler>) {
        self.requests.fetch_add(1, Ordering::SeqCst);
        let url = request.url.as_str();
        if request.url.scheme() == "data" {
            if let Ok(data) = data_url::DataUrl::process(url) {
                if let Ok((bytes, _)) = data.decode_to_vec() {
                    handler.bytes(url.to_owned(), Bytes::from(bytes));
                    return;
                }
            }
        }
        self.blocked.lock().unwrap().push(url.to_owned());
        // Complete failed requests so a refused stylesheet cannot stall layout.
        handler.bytes(url.to_owned(), Bytes::new());
    }
}

#[derive(Serialize)]
struct Element {
    index: usize,
    tag: String,
    id: String,
    class: String,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    computed: std::collections::BTreeMap<String, String>,
}
#[derive(Serialize)]
struct Layout {
    schema_version: u32,
    engine: &'static str,
    width: u32,
    height: u32,
    viewport_height: u32,
    device_scale_factor: u32,
    elements: Vec<Element>,
    blocked_resources: Vec<String>,
}

// Original DOM children preserve document order; anonymous layout boxes are skipped.
fn collect(doc: &BaseDocument, node: &Node, elements: &mut Vec<Element>) {
    if let NodeData::Element(data) = &node.data {
        let (x, y, width, height) = doc
            .get_client_bounding_rect(node.id)
            .map(|r| (r.x, r.y, r.width, r.height))
            .unwrap_or((0.0, 0.0, 0.0, 0.0));
        elements.push(Element {
            index: elements.len(),
            tag: data.name.local.to_string(),
            id: node
                .attr(blitz_dom::local_name!("id"))
                .unwrap_or_default()
                .to_string(),
            class: node
                .attr(blitz_dom::local_name!("class"))
                .unwrap_or_default()
                .to_string(),
            x,
            y,
            width,
            height,
            // Undisplayed subtrees have no stored style. Do not trigger a new
            // Stylo traversal from the diagnostic exporter outside layout.
            computed: if node.primary_styles().is_none() {
                Default::default()
            } else {
                [
                    "display",
                    "position",
                    "font-family",
                    "font-size",
                    "line-height",
                    "letter-spacing",
                    "text-overflow",
                    "overflow",
                    "border-radius",
                    "box-shadow",
                    "background-image",
                    "grid-template-columns",
                    "gap",
                    "margin-top",
                    "padding-top",
                ]
                .into_iter()
                .map(|p| (p.to_owned(), doc.resolved_style_value(node.id, p)))
                .collect()
            },
        });
    }
    for id in &node.children {
        if let Some(child) = doc.get_node(*id) {
            collect(doc, child, elements);
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let Command::Render {
        input,
        width,
        out,
        layout,
        ..
    } = Cli::parse().command;
    let html = fs::read_to_string(input)?;
    let net = Arc::new(DataOnly::default());
    let mut doc = HtmlDocument::from_html(
        &html,
        DocumentConfig {
            base_url: Some("https://invalid.invalid/".into()),
            net_provider: Some(net.clone()),
            viewport: Some(Viewport::new(width, 800, 1.0, ColorScheme::Light)),
            ..Default::default()
        },
    );
    let mut settled = false;
    for _ in 0..64 {
        let before = net.requests.load(Ordering::SeqCst);
        doc.resolve(0.0);
        if before == net.requests.load(Ordering::SeqCst) && !doc.has_pending_critical_resources() {
            // Flush resources synchronously queued by the preceding resolve.
            doc.resolve(0.0);
            settled = true;
            break;
        }
    }
    if !settled {
        return Err("resources did not settle after 64 passes".into());
    }
    let mut elements = Vec::new();
    collect(&doc, doc.root_node(), &mut elements);
    let height = elements
        .iter()
        .map(|e| e.y + e.height)
        .fold(800.0_f64, f64::max)
        .ceil() as u32;
    if height > 32768 {
        return Err("page exceeds 32768 CSS px height limit".into());
    }
    let buffer = render_to_buffer::<VelloCpuImageRenderer, _>(
        |scene| {
            scene.fill(
                Fill::NonZero,
                Default::default(),
                Color::WHITE,
                Default::default(),
                &Rect::new(0.0, 0.0, width as f64, height as f64),
            );
            blitz_paint::paint_scene(scene, &mut doc, 1.0, width, height, 0, 0);
        },
        width,
        height,
    );
    for path in [&out, &layout] {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            fs::create_dir_all(parent)?;
        }
    }
    let mut encoder = png::Encoder::new(fs::File::create(out)?, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header()?;
    writer.write_image_data(&buffer)?;
    writer.finish()?;
    fs::write(
        layout,
        serde_json::to_vec_pretty(&Layout {
            schema_version: 1,
            engine: "blitz-vello-cpu",
            width,
            height,
            viewport_height: 800,
            device_scale_factor: 1,
            elements,
            blocked_resources: net.blocked.lock().unwrap().clone(),
        })?,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use blitz_traits::net::Url;
    struct Capture(Arc<Mutex<Vec<u8>>>);
    impl NetHandler for Capture {
        fn bytes(self: Box<Self>, _: String, bytes: Bytes) {
            *self.0.lock().unwrap() = bytes.to_vec();
        }
    }
    #[test]
    fn data_only_provider_blocks_network_and_files() {
        for url in ["https://example.org/a", "file:///tmp/a"] {
            let provider = DataOnly::default();
            let bytes = Arc::new(Mutex::new(vec![42]));
            provider.fetch(
                0,
                Request::get(Url::parse(url).unwrap()),
                Box::new(Capture(bytes.clone())),
            );
            assert!(bytes.lock().unwrap().is_empty());
            assert_eq!(provider.blocked.lock().unwrap().len(), 1);
        }
    }
    #[test]
    fn data_uri_is_decoded() {
        let provider = DataOnly::default();
        let bytes = Arc::new(Mutex::new(Vec::new()));
        provider.fetch(
            0,
            Request::get(Url::parse("data:text/plain;base64,aGVsbG8=").unwrap()),
            Box::new(Capture(bytes.clone())),
        );
        assert_eq!(*bytes.lock().unwrap(), b"hello");
    }
}
