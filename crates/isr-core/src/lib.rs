// Copyright 2026 g1BSsQ. Licensed under Apache-2.0.
// Offscreen scene construction adapted from the pinned screenshot example; see NOTICE.
mod geometry;

use anyrender::{PaintScene as _, render_to_buffer};
use anyrender_vello_cpu::VelloCpuImageRenderer;
use blitz_dom::{BaseDocument, DocumentConfig, FontContext, Node, NodeData, util::Color};
use parley::fontique::{Blob, Collection, CollectionOptions, GenericFamily, SourceCache};
use blitz_html::HtmlDocument;
use blitz_traits::{
    net::{Bytes, NetHandler, NetProvider, Request},
    shell::{ColorScheme, Viewport},
};
use peniko::{Fill, kurbo::Rect};
use serde::Serialize;
use std::{
    sync::{Arc, Mutex, atomic::{AtomicUsize, Ordering}},
};

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// Host capabilities provided to the offscreen renderer.
#[derive(Clone, Debug)]
pub struct RenderOptions {
    pub width: u32,
    /// Opt in to scanning and opening installed system fonts (and platform font configuration).
    /// Defaults to false: no system font discovery or filesystem access.
    pub system_fonts: bool,
    /// Font files supplied by the host in memory (TTF/OTF or supported WOFF formats).
    /// Available even when system font discovery is disabled.
    pub fonts: Vec<Vec<u8>>,
}
impl Default for RenderOptions {
    fn default() -> Self {
        Self { width: 412, system_fonts: false, fonts: Vec::new() }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Element {
    pub index: usize,
    pub tag: String,
    pub id: String,
    pub class: String,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub computed: std::collections::BTreeMap<String, String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Layout {
    pub schema_version: u32,
    pub engine: &'static str,
    pub width: u32,
    pub height: u32,
    pub viewport_height: u32,
    pub device_scale_factor: u32,
    pub elements: Vec<Element>,
    pub blocked_resources: Vec<String>,
}

pub struct Rendered {
    pub rgba: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub elements: Vec<Element>,
    pub blocked_resources: Vec<String>,
}
impl Rendered {
    pub fn layout(&self) -> Layout {
        Layout {
            schema_version: 1, engine: "blitz-vello-cpu",
            width: self.width, height: self.height, viewport_height: 800,
            device_scale_factor: 1, elements: self.elements.clone(),
            blocked_resources: self.blocked_resources.clone(),
        }
    }
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
        handler.bytes(url.to_owned(), Bytes::new());
    }
}

// Construct a fresh per-render context. Never call FontContext::default:
// its default collection can discover system fonts when compiled with that feature.
// The compiled system-fonts feature permits opt-in, not automatic host access.
fn font_context(system_fonts: bool, fonts: Vec<Vec<u8>>) -> FontContext {
    let mut ctx = FontContext {
        source_cache: SourceCache::new_shared(),
        collection: Collection::new(CollectionOptions {
            shared: false,
            system_fonts,
        }),
    };
    // Match Blitz's default internal bullet font without accessing any files.
    ctx.collection.register_fonts(
        Blob::new(Arc::new(blitz_dom::BULLET_FONT) as _), None,
    );
    for bytes in fonts {
        // The same decoder used by Blitz supports host-provided WOFF/WOFF2.
        let decoded = blitz_dom::decode_font_bytes(&bytes).into_owned();
        let registered = ctx.collection.register_fonts(
            Blob::new(Arc::new(decoded) as _), None,
        );
        let ids: Vec<_> = registered.iter().map(|(id, _)| *id).collect();
        for generic in [
            GenericFamily::SansSerif, GenericFamily::Serif,
            GenericFamily::Monospace, GenericFamily::SystemUi,
        ] {
            ctx.collection.append_generic_families(generic, ids.iter().copied());
        }
    }
    ctx
}

// DOM order is independent of anonymous layout boxes and the paint tree.
fn collect(doc: &BaseDocument, node: &Node, elements: &mut Vec<Element>) {
    if let NodeData::Element(data) = &node.data {
        let bounds = geometry::element_box(doc, node);
        let (x, y, width, height) = bounds
            .map(|r| (r.x, r.y, r.width, r.height))
            .unwrap_or((0.0, 0.0, 0.0, 0.0));
        elements.push(Element {
            index: elements.len(),
            tag: data.name.local.to_string(),
            id: node.attr(blitz_dom::local_name!("id")).unwrap_or_default().to_string(),
            class: node.attr(blitz_dom::local_name!("class")).unwrap_or_default().to_string(),
            x, y, width, height,
            computed: if node.primary_styles().is_none() {
                Default::default()
            } else {
                [
                    "display", "position", "font-family", "font-size", "line-height",
                    "letter-spacing", "text-overflow", "overflow", "border-radius",
                    "box-shadow", "background-image", "grid-template-columns", "gap",
                    "margin-top", "padding-top",
                ].into_iter().map(|p| (p.to_owned(), doc.resolved_style_value(node.id, p))).collect()
            },
        });
    }
    for id in &node.children {
        if let Some(child) = doc.get_node(*id) { collect(doc, child, elements); }
    }
}

/// Render a self-contained document to RGBA8 and document-order CSS-pixel boxes.
/// With `system_fonts: false` (the default), performs no filesystem access:
/// only data: URI @font-face resources and `fonts` bytes supplied in memory
/// are available. With `system_fonts: true`, reads installed font files via
/// the platform font system and may read its configuration. The library never
/// performs network access; document resource URLs cannot trigger filesystem
/// access. Non-data: requests are refused and recorded in blocked_resources.
pub fn render(html: &str, opts: RenderOptions) -> Result<Rendered> {
    let RenderOptions { width, system_fonts, fonts } = opts;
    if !(1..=8192).contains(&width) { return Err("width must be in 1..=8192".into()); }
    let net = Arc::new(DataOnly::default());
    let mut doc = HtmlDocument::from_html(
        html,
        DocumentConfig {
            base_url: Some("https://invalid.invalid/".into()),
            net_provider: Some(net.clone()),
            font_ctx: Some(font_context(system_fonts, fonts)),
            viewport: Some(Viewport::new(width, 800, 1.0, ColorScheme::Light)),
            ..Default::default()
        },
    );
    let mut settled = false;
    for _ in 0..64 {
        let before = net.requests.load(Ordering::SeqCst);
        doc.resolve(0.0);
        if before == net.requests.load(Ordering::SeqCst) && !doc.has_pending_critical_resources() {
            doc.resolve(0.0);
            settled = true;
            break;
        }
    }
    if !settled { return Err("resources did not settle after 64 passes".into()); }
    let mut elements = Vec::new();
    collect(&doc, doc.root_node(), &mut elements);
    let height = elements.iter().map(|e| e.y + e.height)
        .fold(800.0_f64, f64::max).ceil() as u32;
    if height > 32768 { return Err("page exceeds 32768 CSS px height limit".into()); }

    // Separator painting is deferred pending CSS font weight/face parity.
    // Keep the geometry export independent of this future paint pass.
    let rgba = render_to_buffer::<VelloCpuImageRenderer, _>(
        |scene| {
            scene.fill(
                Fill::NonZero, Default::default(), Color::WHITE,
                Default::default(), &Rect::new(0.0, 0.0, width as f64, height as f64),
            );
            blitz_paint::paint_scene(scene, &mut doc, 1.0, width, height, 0, 0);
        }, width, height,
    );
    Ok(Rendered {
        rgba, width, height, elements, blocked_resources: net.blocked.lock().unwrap().clone(),
    })
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
    fn element<'a>(rendered: &'a Rendered, tag: &str) -> &'a Element {
        rendered.elements.iter().find(|e| e.tag == tag).unwrap()
    }
    #[test]
    fn table_row_and_group_boxes_have_area() {
        let r = render(r#"<table style="border-collapse:collapse"><thead><tr><th>Header</th></tr></thead><tbody><tr><td>Value</td></tr></tbody></table>"#, RenderOptions::default()).unwrap();
        for tag in ["thead", "tbody", "tr"] {
            let b = element(&r, tag);
            assert!(b.width > 0.0 && b.height > 0.0, "{tag}: {b:?}");
        }
    }
    #[test]
    fn svg_circle_uses_geometry_bounds() {
        let r = render(r#"<svg width="32" height="32" viewBox="0 0 32 32"><circle cx="16" cy="16" r="13"/></svg>"#, RenderOptions::default()).unwrap();
        let svg = element(&r, "svg");
        let c = element(&r, "circle");
        assert!((c.x - svg.x - 3.0).abs() <= 0.02);
        assert!((c.y - svg.y - 3.0).abs() <= 0.02);
        assert!((c.width - 26.0).abs() <= 0.02);
        assert!((c.height - 26.0).abs() <= 0.02);
    }
    #[test]
    fn wrapped_inline_bounds_union_fragments() {
        let r = render(r#"<style>body{margin:0;font:16px/20px sans-serif}p{width:90px}span{line-height:20px}</style><p>lead <span id="target">words words words words words</span> tail</p>"#, RenderOptions::default()).unwrap();
        let b = r.elements.iter().find(|e| e.id == "target").unwrap();
        assert!(b.width > 0.0 && b.height > 20.0, "{b:?}");
    }
    #[test]
    fn refused_external_resources_are_recorded() {
        let r = render(r#"<img src="http://example.org/a.png">"#, RenderOptions::default()).unwrap();
        assert!(r.blocked_resources.iter().any(|u| u == "http://example.org/a.png"));
    }
    #[test]
    fn provider_refuses_network_and_files_without_fetching() {
        for url in ["https://example.org/a", "file:///tmp/a"] {
            let provider = DataOnly::default();
            let bytes = Arc::new(Mutex::new(vec![42]));
            provider.fetch(0, Request::get(Url::parse(url).unwrap()), Box::new(Capture(bytes.clone())));
            assert!(bytes.lock().unwrap().is_empty());
            assert_eq!(provider.blocked.lock().unwrap().len(), 1);
        }
    }
    #[test]
    fn data_uri_font_renders_text_with_system_fonts_disabled() {
        // Includes only the original public fixture's data: URI @font-face.
        // include_str! embeds bytes at compile time; render never opens this file.
        let fixture = include_str!("../../../fixtures/15-embedded-font.html");
        let after_face = fixture.split_once("@font-face{").unwrap().1;
        let declaration = after_face.split_once('}').unwrap().0;
        let html = format!(
            "<!doctype html><style>@font-face{{{declaration}}}body{{margin:0;background:white}}p{{font:36px FixtureRoboto;color:black}}</style><p id='sample'>Font test</p>"
        );
        let rendered = render(&html, RenderOptions::default()).unwrap();
        let text = rendered.elements.iter().find(|e| e.id == "sample").unwrap();
        assert!(text.width > 0.0 && text.height > 0.0);
        let ink = rendered.rgba.chunks_exact(4).filter(|rgba|
            rgba[0] < 100 && rgba[1] < 100 && rgba[2] < 100 && rgba[3] > 0
        ).count();
        assert!(ink > 20, "embedded data: font did not paint nonblank text (ink={ink})");
        assert!(rendered.blocked_resources.is_empty());
    }
    #[test]
    fn host_injected_font_is_available_without_system_font_discovery() {
        let fixture = include_str!("../../../fixtures/15-embedded-font.html");
        let uri = fixture.split_once("src:url(").unwrap().1.split_once(')').unwrap().0;
        let (font_bytes, _) = data_url::DataUrl::process(uri).unwrap().decode_to_vec().unwrap();
        let opts = RenderOptions { fonts: vec![font_bytes], ..RenderOptions::default() };
        let rendered = render(
            "<style>body{margin:0;background:white}p{font:36px Roboto;color:black}</style><p id='sample'>Injected font</p>",
            opts,
        ).unwrap();
        let sample = rendered.elements.iter().find(|e| e.id == "sample").unwrap();
        assert!(sample.width > 0.0 && sample.height > 0.0);
        let ink = rendered.rgba.chunks_exact(4).filter(|rgba|
            rgba[0] < 100 && rgba[1] < 100 && rgba[2] < 100 && rgba[3] > 0
        ).count();
        assert!(ink > 20, "host-injected font did not paint text (ink={ink})");
        assert!(rendered.blocked_resources.is_empty());
    }
    #[test]
    fn data_uri_is_decoded() {
        let provider = DataOnly::default();
        let bytes = Arc::new(Mutex::new(Vec::new()));
        provider.fetch(0, Request::get(Url::parse("data:text/plain;base64,aGVsbG8=").unwrap()), Box::new(Capture(bytes.clone())));
        assert_eq!(*bytes.lock().unwrap(), b"hello");
    }
}
