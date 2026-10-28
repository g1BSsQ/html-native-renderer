// Copyright 2026 g1BSsQ. Licensed under Apache-2.0.
// Offscreen scene construction adapted from the pinned screenshot example; see NOTICE.
mod geometry;
mod counters;
mod fonts;

use anyrender::{PaintScene as _, render_to_buffer};
use anyrender_vello_cpu::VelloCpuImageRenderer;
use blitz_dom::{BaseDocument, DocumentConfig, FontContext, Node, NodeData, UserAgentStylesheet, util::Color};
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
    /// By default, neither the text stack nor the vendored scoped SVG parser
    /// accesses installed fonts. Host font discovery requires explicit opt-in.
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
                    "letter-spacing", "text-overflow", "overflow", "font-weight",
                    "font-style", "font-stretch", "border-radius",
                    "box-shadow", "background-image", "grid-template-columns", "gap",
                    "margin-top", "padding-top",
                    // Additional paint and stacking properties for native diagnostics.
                    "opacity", "filter", "clip-path", "z-index", "mix-blend-mode",
                    "transform", "overflow-x", "overflow-y",
                ].into_iter().map(|p| (p.to_owned(), doc.resolved_style_value(node.id, p))).collect()
            },
        });
    }
    for id in &node.children {
        if let Some(child) = doc.get_node(*id) { collect(doc, child, elements); }
    }
}

// Match the element export traversal's document order, including descendants
// that have no rendered layout box in a closed details tree.
fn details_hidden_flags(doc: &BaseDocument, node: &Node, flags: &mut Vec<bool>) {
    if matches!(&node.data, NodeData::Element(_)) {
        flags.push(geometry::hidden_in_details(doc, node));
    }
    for &id in &node.children {
        if let Some(child) = doc.get_node(id) { details_hidden_flags(doc, child, flags); }
    }
}

fn closed_details_ids(doc: &BaseDocument, node: &Node, ids: &mut Vec<blitz_dom::NodeId>) {
    if let NodeData::Element(data) = &node.data {
        if &*data.name.local == "details"
            && node.attr(blitz_dom::local_name!("open")).is_none()
        {
            ids.push(node.id);
        }
    }
    for &id in &node.children {
        if let Some(child) = doc.get_node(id) { closed_details_ids(doc, child, ids); }
    }
}

fn open_attribute() -> blitz_dom::QualName {
    blitz_dom::QualName::new(None, blitz_dom::Namespace::from(""),
        blitz_dom::LocalName::from("open"))
}

// Upstream parse-order workaround: Blitz caches the default UA sheet while
// DocumentConfig::default is constructed, before BaseDocument::new enables
// this preference. It must be enabled before the first sheet parse. Remove
// after the pinned dependency is upgraded.
pub(crate) fn enable_counter_ua_parsing() {
    static COUNTER_PREF: std::sync::Once = std::sync::Once::new();
    COUNTER_PREF.call_once(|| {
        stylo_static_prefs::set_pref!("layout.unimplemented", true);
    });
}

/// Construct the same counter-capable document configuration for rendering and
/// DOM-level tests. Enable Stylo's pref before any default UA sheet is parsed.
pub(crate) fn document_config(
    width: u32,
    font_ctx: Option<FontContext>,
    net_provider: Option<Arc<dyn NetProvider>>,
) -> DocumentConfig {
    enable_counter_ua_parsing();
    DocumentConfig {
        base_url: Some("https://invalid.invalid/".into()),
        net_provider,
        // The normal UA stylesheet comes first; this supplemental HTML rule
        // provides reversed ordered-list counter initialization.
        ua_stylesheets: vec![
            UserAgentStylesheet::default_sheet(),
            UserAgentStylesheet::parse("ol[reversed] { counter-reset: reversed(list-item); }"),
        ],
        font_ctx,
        viewport: Some(Viewport::new(width, 800, 1.0, ColorScheme::Light)),
        ..Default::default()
    }
}

/// Render a self-contained document to RGBA8 and document-order CSS-pixel boxes.
/// By default, font discovery through the text stack is off; it uses data:
/// URI @font-face fonts, host-provided `fonts` bytes, and the in-memory bullet
/// font. With `system_fonts: true`, the text stack reads installed font files
/// via the platform font system and may read its configuration.
/// Vendored Blitz's scoped SVG font database is lazy and opens installed
/// fonts only when `system_fonts` is enabled. The library never performs
/// network access; non-data: subresource requests are refused and recorded
/// in blocked_resources.
/// KNOWN LIMITATION: If the host creates a Blitz `DocumentConfig::default()`
/// before the first call to `render()`, Blitz may cache its UA stylesheet
/// without the counter rules. This shared initializer cannot repair an already
/// cached sheet; a pinned upstream parse-order fix is required.
pub fn render(html: &str, opts: RenderOptions) -> Result<Rendered> {
    let RenderOptions { width, system_fonts, fonts } = opts;
    if !(1..=8192).contains(&width) { return Err("width must be in 1..=8192".into()); }
    // The vendored scoped SVG font policy is enforced in every build.
    let _svg_font_scope = blitz_dom::util::svg_font_scope(system_fonts, &fonts);
    let net = Arc::new(DataOnly::default());
    let mut doc = HtmlDocument::from_html(
        html,
        document_config(width, Some(fonts::context(system_fonts, &fonts, html)), Some(net.clone())),
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
    // Counter-based pseudo text must be resolved before the final paint and export.
    // This is a second static style/layout pass; no script engine is involved.
    if counters::materialize(&mut doc) { doc.resolve(0.0); }
    let mut elements = Vec::new();
    collect(&doc, doc.root_node(), &mut elements);
    // Capture height is determined only by the real closed-state layout.
    // The CSSOM export of hidden details descendants is measured separately.
    let height = elements.iter().map(|e| e.y + e.height)
        .fold(800.0_f64, f64::max).ceil() as u32;
    if height > 32768 { return Err("page exceeds 32768 CSS px height limit".into()); }

    let mut details = Vec::new();
    closed_details_ids(&doc, doc.root_node(), &mut details);
    if !details.is_empty() {
        let mut hidden = Vec::new();
        details_hidden_flags(&doc, doc.root_node(), &mut hidden);
        if hidden.len() != elements.len() {
            return Err("details mask changed element order".into());
        }
        // A display:none descendant has no containing-block linkage in the
        // closed layout. Force box-tree reconstruction while temporarily open,
        // so the ordinary CSSOM absolute-offset walk sees its new layout parent
        // rather than reporting a node-local (0, 0) rectangle.
        let incremental = doc.incremental_layout();
        doc.set_incremental_layout(false);
        // Measure one closed details subtree at a time, with its closed
        // ancestors open. Opening all siblings together would move later
        // subtrees and report incorrect coordinates.
        for &id in &details {
            let mut to_open = Vec::new();
            let mut current = Some(id);
            while let Some(ancestor) = current {
                if details.contains(&ancestor) { to_open.push(ancestor); }
                current = doc.get_node(ancestor).and_then(|node| node.parent);
            }
            {
                let mut mutator = doc.mutate();
                for &candidate in to_open.iter().rev() {
                    mutator.set_attribute(candidate, open_attribute(), "");
                }
            }
            doc.resolve(0.0);
            // collect() calls geometry::element_box(), whose native box uses
            // the containing-block absolute-position walk, not local offsets.
            let mut measured = Vec::new();
            collect(&doc, doc.root_node(), &mut measured);
            {
                let mut mutator = doc.mutate();
                for &candidate in &to_open {
                    mutator.clear_attribute(candidate, open_attribute());
                }
            }
            // Restore the exact closed-state layout before painting.
            doc.resolve(0.0);
            if measured.len() != elements.len() {
                return Err("details measurement changed element order".into());
            }
            // Descendants of this details node only. A nested closed details
            // is measured in its own iteration with all its ancestors opened.
            let mut belongs = Vec::new();
            fn mark_subtree(doc: &BaseDocument, node: &Node, target: blitz_dom::NodeId,
                inside: bool, flags: &mut Vec<bool>) {
                let inside = inside || node.id == target;
                if matches!(&node.data, NodeData::Element(_)) {
                    flags.push(inside && node.id != target);
                }
                for &child_id in &node.children {
                    if let Some(child) = doc.get_node(child_id) {
                        mark_subtree(doc, child, target, inside, flags);
                    }
                }
            }
            mark_subtree(&doc, doc.root_node(), id, false, &mut belongs);
            for (index, element) in elements.iter_mut().enumerate() {
                if hidden[index] && belongs[index] {
                    let expanded = &measured[index];
                    // A zero-area box still has a meaningful absolute origin.
                    element.x = expanded.x;
                    element.y = expanded.y;
                    element.width = expanded.width;
                    element.height = expanded.height;
                }
            }
        }
        doc.set_incremental_layout(incremental);
    }

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
    fn embedded_face() -> String {
        // Embed the checked-in public font at compile time; no runtime file I/O.
        let html = include_str!("../../../fixtures/15-embedded-font.html");
        let declaration = html.split_once("@font-face{").unwrap().1
            .split_once('}').unwrap().0;
        format!("@font-face{{{declaration}}}")
    }
    #[test]
    fn table_row_and_group_boxes_have_area() {
        let html = format!(r#"<style>{}table{{font-family:FixtureRoboto}}</style><table style="border-collapse:collapse"><thead><tr><th>Header</th></tr></thead><tbody><tr><td>Value</td></tr></tbody></table>"#, embedded_face());
        let r = render(&html, RenderOptions::default()).unwrap();
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
        let html = format!(r#"<style>{}body{{margin:0;font:16px/20px FixtureRoboto}}p{{width:90px}}span{{line-height:20px}}</style><p>lead <span id="target">words words words words words</span> tail</p>"#, embedded_face());
        let r = render(&html, RenderOptions::default()).unwrap();
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
    fn dark_ink(rendered: &Rendered) -> usize {
        rendered.rgba.chunks_exact(4).filter(|rgba|
            rgba[0] < 100 && rgba[1] < 100 && rgba[2] < 100 && rgba[3] > 0
        ).count()
    }
    #[test]
    fn default_has_no_system_font_text_fallback() {
        // No SVG and no host- or data-supplied font: the text stack must not
        // silently discover fonts via the platform font system.
        let opts = RenderOptions::default();
        assert!(!opts.system_fonts);
        assert!(opts.fonts.is_empty());
        let rendered = render(
            r#"<!doctype html><html><body style="margin:0;background:white"><p style="margin:0;font:32px sans-serif;color:black">ABCDEFGHIJKLMNOPQRSTUVWXYZ abcdefghijklmnopqrstuvwxyz 12345</p></body></html>"#,
            opts,
        ).unwrap();
        assert_eq!(dark_ink(&rendered), 0,
            "plain sans-serif text unexpectedly painted with no supplied font");
    }
    #[test]
    fn compiled_bullet_font_paints_list_marker_without_system_fonts() {
        // Blitz's default FontContext only registers the in-memory BULLET_FONT:
        // list.rs selects "Bullet, monospace, sans-serif" for disc markers.
        // A nonempty text run with a host-provided font ensures the list item
        // receives line layout; the old empty <li> did not guarantee that.
        let fixture = include_str!("../../../fixtures/15-embedded-font.html");
        let uri = fixture.split_once("src:url(").unwrap().1.split_once(')').unwrap().0;
        let (font_bytes, _) = data_url::DataUrl::process(uri).unwrap().decode_to_vec().unwrap();
        let opts = RenderOptions { fonts: vec![font_bytes], ..RenderOptions::default() };
        assert!(!opts.system_fonts);
        let rendered = render(
            r#"<!doctype html><html><head><style>
                body { margin:0; background:white }
                ul { margin:0; padding-left:96px; list-style-position:outside }
                li { font:32px/48px Roboto; color:black; list-style-type:disc }
                span { display:inline-block }
            </style></head><body><ul><li><span id="label">List text</span></li></ul></body></html>"#,
            opts,
        ).unwrap();
        let label = rendered.elements.iter().find(|e| e.id == "label").unwrap();
        assert!(label.x > 40.0 && label.width > 0.0 && label.height > 0.0,
            "expected a laid-out text box: {label:?}");
        // No text or other decoration lies to the left of the label. A dark
        // pixel there must come from the outside-positioned disc marker.
        let right = (label.x.floor() as usize).saturating_sub(1)
            .min(rendered.width as usize);
        let top = (label.y.floor() as i64 - 16).max(0) as usize;
        let bottom = ((label.y + label.height).ceil() as usize + 16)
            .min(rendered.height as usize);
        let mut marker_ink = 0usize;
        for y in top..bottom {
            for x in 0..right {
                let offset = (y * rendered.width as usize + x) * 4;
                let rgba = &rendered.rgba[offset..offset + 4];
                if rgba[0] < 100 && rgba[1] < 100 && rgba[2] < 100 && rgba[3] > 0 {
                    marker_ink += 1;
                }
            }
        }
        assert!(marker_ink > 4,
            "in-memory disc marker did not paint left of text (ink={marker_ink}, label={label:?})");
        assert!(rendered.blocked_resources.is_empty());
    }
    #[test]
    fn data_uri_font_renders_text_with_system_fonts_disabled() {
        // Includes only the original public fixture's data: URI @font-face.
        // include_str! embeds bytes at compile time; render never opens this file.
        let html = format!(
            "<!doctype html><style>{}body{{margin:0;background:white}}p{{font:36px FixtureRoboto;color:black}}</style><p id='sample'>Font test</p>",
            embedded_face()
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
    fn br_exports_font_content_area_height() {
        let html = format!(r#"<style>{}body{{margin:0;font:18px/30px FixtureRoboto}}p{{margin:0}}</style><p>Before<br id="break">After</p>"#, embedded_face());
        let rendered = render(&html, RenderOptions::default()).unwrap();
        let br = rendered.elements.iter().find(|e| e.id == "break").unwrap();
        // FixtureRoboto's font content area at 18px is 24 CSS px,
        // independent of its 30px line box.
        assert!((br.height - 24.0).abs() < 0.5 && br.x > 0.0,
            "line break must export the font content area, not line-height: {br:?}");
    }
    #[test]
    fn closed_details_exports_measured_content_without_extending_capture() {
        let html = r#"<!doctype html><style>
            body{margin:0}summary{height:20px}details p{margin:0;height:880px}
        </style><details><summary>Heading</summary>
            <p id="hidden-a">Hidden A</p><p id="hidden-b">Hidden B</p>
        </details>"#;
        let closed = render(html, RenderOptions::default()).unwrap();
        let first = closed.elements.iter().find(|e| e.id == "hidden-a").unwrap();
        let second = closed.elements.iter().find(|e| e.id == "hidden-b").unwrap();
        // Zero rectangles from the old exporter must fail this test.
        assert!(first.x >= 0.0 && first.y > 0.0 && first.height > 0.0,
            "closed details first descendant was not measured: {first:?}");
        assert!(second.y > first.y && second.height > 0.0,
            "closed details second descendant was not measured: {second:?}");
        assert_eq!(closed.height, 800, "measurement must not extend the screenshot");
        let opened = html.replace("<details>", "<details open>");
        let open = render(&opened, RenderOptions::default()).unwrap();
        assert!(open.height > closed.height, "open details did not extend capture");
        let open_first = open.elements.iter().find(|e| e.id == "hidden-a").unwrap();
        assert!((first.x - open_first.x).abs() < 1.0
            && (first.y - open_first.y).abs() < 1.0
            && (first.height - open_first.height).abs() < 1.0,
            "closed descendant geometry differs from temporary open state");
    }
    #[test]
    fn transform_rotation_exports_axis_aligned_bounding_box() {
        let html = r#"<style>body{margin:0}#rot{width:100px;height:100px;
            transform:rotate(45deg);transform-origin:50% 50%}</style>
            <div id="rot"></div>"#;
        let rendered = render(html, RenderOptions::default()).unwrap();
        let rect = rendered.elements.iter().find(|e| e.id == "rot").unwrap();
        let expected = 100.0 * std::f64::consts::SQRT_2;
        assert!((rect.width - expected).abs() < 0.1, "{rect:?}");
        assert!((rect.height - expected).abs() < 0.1, "{rect:?}");
    }
    #[test]
    fn data_uri_is_decoded() {
        let provider = DataOnly::default();
        let bytes = Arc::new(Mutex::new(Vec::new()));
        provider.fetch(0, Request::get(Url::parse("data:text/plain;base64,aGVsbG8=").unwrap()), Box::new(Capture(bytes.clone())));
        assert_eq!(*bytes.lock().unwrap(), b"hello");
    }

    #[test]
    fn exports_effect_and_stacking_computed_style_keys() {
        // The previous exporter lacked these keys. The assertion checks
        // diagnostic availability and selected values, not pixel parity.
        let html = r#"<style>body{margin:0}#probe{
            position:relative;opacity:.65;z-index:2;
            filter:grayscale(1);clip-path:inset(3px);
            mix-blend-mode:multiply;transform:translateX(0);
            overflow:clip
        }</style><div id="probe">Effects</div>"#;
        let rendered = render(html, RenderOptions::default()).unwrap();
        let computed = &rendered.elements.iter()
            .find(|e| e.id == "probe").unwrap().computed;
        for property in [
            "opacity", "filter", "clip-path", "z-index", "mix-blend-mode",
            "transform", "overflow-x", "overflow-y",
        ] {
            assert!(computed.contains_key(property), "missing {property}");
        }
        assert_eq!(computed.get("opacity").map(String::as_str), Some("0.65"));
        assert_eq!(computed.get("z-index").map(String::as_str), Some("2"));
        assert_eq!(computed.get("mix-blend-mode").map(String::as_str), Some("multiply"));
        assert!(rendered.blocked_resources.is_empty());
    }
    #[test]
    fn ua_heading_table_and_emphasis_styles_are_resolved() {
        let html = format!(r#"<style>{}body{{font-family:FixtureRoboto}}</style>
            <h1>one</h1><h2>two</h2><h3>three</h3><h4>four</h4>
            <h5>five</h5><h6>six</h6>
            <table><tr><th>header</th></tr></table>
            <b>bold</b><strong>strong</strong><em>emphasis</em><i>italic</i>"#, embedded_face());
        let rendered = render(&html, RenderOptions::default()).unwrap();
        for tag in ["h1", "h2", "h3", "h4", "h5", "h6", "th", "b", "strong"] {
            let value = element(&rendered, tag).computed.get("font-weight").unwrap();
            assert!(value == "700" || value == "bold", "{tag} weight: {value}");
        }
        for tag in ["em", "i"] {
            let value = element(&rendered, tag).computed.get("font-style").unwrap();
            assert_eq!(value, "italic", "{tag} style");
        }
    }

    #[test]
    fn no_system_fonts_svg_fixture_does_not_require_host_fonts() {
        let opts = RenderOptions::default();
        let rendered = render(r#"<svg width="32" height="32" viewBox="0 0 32 32"><circle cx="16" cy="16" r="9"/></svg>"#, opts).unwrap();
        assert!(element(&rendered, "svg").width > 0.0);
        assert!(rendered.blocked_resources.is_empty());
    }

    #[test]
    fn no_system_fonts_data_svg_fixture_does_not_require_host_fonts() {
        let markup = r#"<img src="data:image/svg+xml,%3Csvg%20xmlns='http://www.w3.org/2000/svg'%20width='24'%20height='24'%3E%3Crect%20width='24'%20height='24'/%3E%3C/svg%3E">"#;
        let rendered = render(markup, RenderOptions::default()).unwrap();
        assert!(rendered.blocked_resources.is_empty());
    }

    #[test]
    fn synthetic_bold_changes_painted_pixels() {
        let face = embedded_face();
        let paint = |weight: u16| {
            let html = format!(
                r#"<style>{face}body{{margin:0;background:white}}p{{margin:0;font: {weight} 40px FixtureRoboto;color:black}}</style><p>Hamburgefontsiv 1234567890</p>"#
            );
            render(&html, RenderOptions::default()).unwrap()
        };
        let regular = paint(400);
        let bold = paint(700);
        assert_ne!(regular.rgba, bold.rgba,
            "Fontique synthetic embolden must affect actual pixels");
        let regular_ink = dark_ink(&regular);
        let bold_ink = dark_ink(&bold);
        assert!(bold_ink > regular_ink,
            "synthetic bold should cover more dark pixels (regular dark_ink={regular_ink}, bold dark_ink={bold_ink})");
    }

    // The installed family has an actual bold face; the output must also
    // differ from regular, but must not require synthetic emboldening.

    #[cfg(target_os = "linux")]
    #[test]
    fn real_bold_face_changes_painted_pixels() {
        let paint = |weight: u16| {
            let html = format!(
                r#"<style>body{{margin:0;background:white}}p{{margin:0;font:{weight} 40px 'Liberation Sans';color:black}}</style><p>Hamburgefontsiv 1234567890</p>"#
            );
            render(&html, RenderOptions {
                system_fonts: true, ..RenderOptions::default()
            }).unwrap()
        };
        let regular = paint(400);
        let bold = paint(700);
        assert_ne!(regular.rgba, bold.rgba,
            "real Liberation Sans bold face must paint different pixels");
        assert_ne!(dark_ink(&regular), dark_ink(&bold));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn auto_table_columns_distribute_excess_by_max_content() {
        // The same three-column, 372px definite-width case as the public
        // 07-table fixture. At 14px, regular/bold Liberation Sans text plus
        // 16px cell padding gives max-content 86.78125, 65.765625, 62.625.
        // 372 * max_i / 215.171875 = 150.03, 113.70, 108.27 CSS px.
        let html = include_str!("../../../fixtures/07-table.html");
        let rendered = render(html, RenderOptions {
            system_fonts: true, ..RenderOptions::default()
        }).unwrap();
        let header = rendered.elements.iter()
            .filter(|e| e.tag == "th").collect::<Vec<_>>();
        assert_eq!(header.len(), 3);
        let max_content = [86.78125_f64, 65.765625, 62.625];
        let total: f64 = max_content.iter().sum();
        for (cell, intrinsic) in header.iter().zip(max_content) {
            let expected = 372.0 * intrinsic / total;
            assert!((cell.width - expected).abs() < 0.8,
                "auto table column {}: width={} expected={} (proportional max-content)",
                cell.index, cell.width, expected);
        }
        assert!((header[1].x - (header[0].x + header[0].width)).abs() < 0.1);
        assert!((header[2].x - (header[1].x + header[1].width)).abs() < 0.1);
    }



    // Mutate only the OS/2 weight class of the compiled-in font. This leaves
    // glyph outlines identical while simulating a face with intrinsic weight
    // 700 that is *declared* as weight 400 by its @font-face rule.
    fn weight_class_700(mut bytes: Vec<u8>) -> Vec<u8> {
        let num_tables = u16::from_be_bytes([bytes[4], bytes[5]]) as usize;
        let os2 = (0..num_tables).find_map(|i| {
            let at = 12 + i * 16;
            if &bytes[at..at + 4] != b"OS/2" { return None; }
            let offset = u32::from_be_bytes(bytes[at + 8..at + 12].try_into().unwrap()) as usize;
            Some(offset)
        }).expect("the fixture must contain an OS/2 table");
        bytes[os2 + 4..os2 + 6].copy_from_slice(&700_u16.to_be_bytes());
        bytes
    }

    fn embedded_bytes() -> Vec<u8> {
        let html = include_str!("../../../fixtures/15-embedded-font.html");
        let uri = html.split_once("src:url(").unwrap().1.split_once(')').unwrap().0;
        data_url::DataUrl::process(uri).unwrap().decode_to_vec().unwrap().0
    }

    fn base64_data_uri(bytes: &[u8]) -> String {
        const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut encoded = String::with_capacity((bytes.len() + 2) / 3 * 4);
        for group in bytes.chunks(3) {
            let b0 = group[0];
            let b1 = *group.get(1).unwrap_or(&0);
            let b2 = *group.get(2).unwrap_or(&0);
            encoded.push(ALPHABET[(b0 >> 2) as usize] as char);
            encoded.push(ALPHABET[(((b0 & 3) << 4) | (b1 >> 4)) as usize] as char);
            encoded.push(if group.len() > 1 { ALPHABET[(((b1 & 15) << 2) | (b2 >> 6)) as usize] as char } else { '=' });
            encoded.push(if group.len() > 2 { ALPHABET[(b2 & 63) as usize] as char } else { '=' });
        }
        format!("data:font/ttf;base64,{encoded}")
    }

    #[test]
    fn no_weight_synthesis_paints_700_like_400() {
        let face = embedded_face();
        let paint = |weight: u16| render(&format!(
            r#"<style>{face}body{{margin:0;background:white}}p{{margin:0;font:{weight} 48px FixtureRoboto;font-synthesis-weight:none;color:black}}</style><p>Hamburgefontsiv</p>"#
        ), RenderOptions::default()).unwrap();
        let normal = paint(400);
        let no_synthesis = paint(700);
        assert!(dark_ink(&normal) > 40);
        assert_eq!(normal.rgba, no_synthesis.rgba, "font-synthesis-weight:none must suppress synthetic ink");
    }

    #[test]
    fn embedded_intrinsically_bold_face_is_never_doubly_emboldened() {
        let uri = base64_data_uri(&weight_class_700(embedded_bytes()));
        // The descriptor is omitted: the loader defaults it to weight 400,
        // while the embedded face's OS/2 reports an intrinsic 700.
        let face = format!("@font-face{{font-family:DeclaredRegular;src:url({uri})}}");
        let paint = |weight: u16| render(&format!(
            r#"<style>{face}body{{margin:0;background:white}}p{{margin:0;font:{weight} 48px DeclaredRegular;color:black}}</style><p>Hamburgefontsiv</p>"#
        ), RenderOptions::default()).unwrap();
        let normal = paint(400);
        let bold = paint(700);
        assert!(dark_ink(&normal) > 40, "embedded face must be loaded and visible");
        assert_eq!(normal.rgba, bold.rgba, "intrinsically bold outlines must not receive another synthetic stroke");
    }

    #[test]
    fn synthetic_bold_preserves_attached_combining_mark_ink() {
        let face = embedded_face();
        let paint = |content: &str| render(&format!(
            r#"<style>{face}body{{margin:0;background:white}}p{{margin:0;font:700 60px/90px FixtureRoboto;color:black}}</style><p>{content}</p>"#
        ), RenderOptions::default()).unwrap();
        // X has no canonical composition with U+0301: the mark remains a
        // zero-advance glyph attached by shaping to the same base.
        let base = paint("X");
        let combined = paint("X\u{0301}");
        assert!(dark_ink(&base) > 20);
        let added_ink = combined.rgba.chunks_exact(4).zip(base.rgba.chunks_exact(4))
            .filter(|(combined, base)| combined[0] < 100 && combined[1] < 100
                && combined[2] < 100 && base[0] >= 100).count();
        assert!(added_ink > 4, "combining acute accent must paint additional attached ink: added={added_ink}");
        let mut beyond_base = 0;
        for y in 0..combined.height as usize {
            for x in 100..combined.width as usize {
                let i = (y * combined.width as usize + x) * 4;
                if combined.rgba[i] < 100 && combined.rgba[i+1] < 100 && combined.rgba[i+2] < 100 {
                    beyond_base += 1;
                }
            }
        }
        assert_eq!(beyond_base, 0, "zero-advance combining mark must not drift away from base");
    }

    #[test]
    fn c0_and_c1_control_glyphs_paint_as_replacement_character() {
        let face = embedded_face();
        let paint = |escaped: &str| render(&format!(
            r#"<style>{face}body{{margin:0;background:white;font:64px FixtureRoboto}}p{{margin:0}}p::after{{content:"{escaped}"}}</style><p></p>"#
        ), RenderOptions::default()).unwrap();
        let replacement = paint(r"\FFFD ");
        assert!(dark_ink(&replacement) > 10, "embedded replacement glyph must be visible");
        for escaped in [r"\0001 ", r"\007F ", r"\0080 ", r"\009F "] {
            let actual = paint(escaped);
            assert_eq!(actual.rgba, replacement.rgba,
                "control {escaped} must render exactly like U+FFFD");
        }
    }


    #[test]
    fn listbox_appearance_aliases_keep_identical_option_rows() {
        // The style adjuster may set a menulist select's line-height to
        // normal. Listbox option rows must not inherit that adjusted value.
        // Exercise both multiple and size-based listboxes through real layout.
        let option_boxes = |appearance: &str, attributes: &str, option_css: &str| {
            // Rendering defaults to no system fonts. Without a supplied
            // face the option text has zero advance and zero line height,
            // leaving only its 3px padding in the exported box.
            let face = embedded_face();
            let html = format!(
                r#"<!doctype html><style>{face}
                body{{margin:0}}
                select, option{{font-family:FixtureRoboto}}
                select{{{appearance}}}{option_css}</style>
                <select {attributes}>
                    <option id="first">Alpha</option>
                    <option id="second">Bravo</option>
                </select>"#
            );
            let rendered = render(&html, RenderOptions::default()).unwrap();
            ["first", "second"].map(|id| {
                let row = rendered.elements.iter().find(|e| e.id == id).unwrap();
                assert!(row.width > 3.0 && row.height > 0.0,
                    "{attributes}: option text must shape into a real row: {row:?}");
                (row.x, row.y, row.width, row.height)
            })
        };
        for attributes in ["multiple", "size='3'"] {
            let auto = option_boxes("appearance:auto", attributes, "");
            let menulist = option_boxes("appearance:menulist", attributes, "");
            let prefixed = option_boxes("-webkit-appearance:menulist", attributes, "");
            assert_eq!(menulist, auto, "{attributes}: menulist changed option boxes");
            assert_eq!(prefixed, auto, "{attributes}: prefixed menulist changed option boxes");
            assert!(auto.iter().all(|row| (row.3 - 17.0).abs() < 0.2),
                "{attributes}: default option rows must be 17px: {auto:?}");

            // An author option font or line-height must override the UA rule.
            let custom_css = "option{font-size:20px;line-height:26px}";
            let authored_auto = option_boxes("appearance:auto", attributes, custom_css);
            let authored_alias = option_boxes("appearance:menulist", attributes, custom_css);
            assert_eq!(authored_alias, authored_auto,
                "{attributes}: authored option style differs by appearance");
            assert!(authored_auto.iter().all(|row| (row.3 - 26.0).abs() < 0.2),
                "{attributes}: author option line-height must win: {authored_auto:?}");
        }
    }


    #[test]
    fn non_atomic_inline_edges_affect_flow_and_intrinsic_table_width() {
        let html = format!(r#"<style>{}
            body{{margin:0;font:16px/24px FixtureRoboto}}
            span.gap{{margin-right:14px}}
            table{{border-spacing:0}}
            td{{padding:1px}}
        </style>
        <div><span id="first" class="gap">Alpha</span><span id="next">Beta</span></div>
        <table><tr><td id="cell"><span id="inside" class="gap">Account</span></td></tr></table>"#,
            embedded_face());
        let rendered = render(&html, RenderOptions::default()).unwrap();
        let get = |id: &str| rendered.elements.iter()
            .find(|item| item.id == id).unwrap();
        let first = get("first");
        let next = get("next");
        let cell = get("cell");
        let inside = get("inside");
        let gap = next.x - (first.x + first.width);
        assert!((gap - 14.0).abs() < 0.6,
            "inline margin must advance the next sibling by 14px: {first:?}, {next:?}");
        let extra = cell.width - inside.width;
        // Whole-pixel table-column rounding can add less than 1px.
        assert!(extra >= 16.0 - 0.01 && extra < 17.0,
            "auto-width cell must include 14px inline margin and 2px padding: {cell:?}, {inside:?}");
    }

    #[test]
    fn vertical_inline_block_axis_borders_do_not_advance_siblings() {
        // Without vertical layout support, style getters expose physical
        // edges. A border on the block axis must not advance inline text.
        let face = embedded_face();
        let render_span = |edge: &str| render(&format!(r#"<style>{face}
            body{{margin:0;font:16px/24px FixtureRoboto}}
            #first{{writing-mode:vertical-lr;{edge}}}
            </style><div><span id="first">Alpha</span><span id="next">Beta</span></div>"#),
            RenderOptions::default()).unwrap();
        let plain = render_span("");
        let bordered = render_span("border-left:24px solid transparent");
        let gap = |r: &Rendered| {
            let a = r.elements.iter().find(|e| e.id == "first").unwrap();
            let b = r.elements.iter().find(|e| e.id == "next").unwrap();
            b.x - a.x - a.width
        };
        // Border on the block axis must not create an inline-axis gap.
        assert!((gap(&plain) - gap(&bordered)).abs() < 0.8,
            "vertical block-axis border changed inline gap: plain={} bordered={}",
            gap(&plain), gap(&bordered));
    }

    #[test]
    fn inline_calc_start_edges_resolve_in_intrinsic_width() {
        let face = embedded_face();
        let paint = |css: &str| render(&format!(
            r#"<style>{face}
                body{{margin:0;font:16px/24px FixtureRoboto}}
                #outer{{width:max-content}}
                #span{{{css}}}
            </style><div id="outer"><span id="span">ABCD</span></div>"#
        ), RenderOptions::default()).unwrap();
        let plain = paint("");
        let calc = paint("margin-left:calc(0% + 30px);padding-left:calc(0% + 50px)");
        fn get<'a>(r: &'a Rendered, id: &str) -> &'a Element {
            r.elements.iter().find(|e| e.id == id).unwrap()
        }
        let width = get(&calc, "outer").width - get(&plain, "outer").width;
        let start = get(&calc, "span").x - get(&plain, "span").x;
        assert!((width - 80.0).abs() < 0.8, "calc() intrinsic edges: {width}");
        assert!((start - 80.0).abs() < 0.8, "calc() used edges: {start}");
    }

    #[test]
    fn nowrap_inline_slice_and_clone_preserve_both_edges() {
        let face = embedded_face();
        let paint = |css: &str| render(&format!(
            r#"<style>{face}
                body{{margin:0;font:16px/24px FixtureRoboto}}
                #outer{{width:max-content;white-space:nowrap}}
                #span{{{css}}}
            </style><div id="outer"><span id="span">ABC</span></div>"#
        ), RenderOptions::default()).unwrap();
        let plain = paint("");
        fn get<'a>(r: &'a Rendered, id: &str) -> &'a Element {
            r.elements.iter().find(|e| e.id == id).unwrap()
        }
        for mode in ["slice", "clone"] {
            let styled = paint(&format!(
                "box-decoration-break:{mode};border:solid gray;\
                 border-width:0 8px 0 5px;padding:0 10px 0 6px;margin:0 4px 0 3px"
            ));
            let width = get(&styled, "outer").width - get(&plain, "outer").width;
            let start = get(&styled, "span").x - get(&plain, "span").x;
            assert!((width - 36.0).abs() < 0.8,
                "{mode}: 14px start and 22px end must count, got {width}");
            assert!((start - 14.0).abs() < 0.8,
                "{mode}: leading edge must precede text, got {start}");
        }
    }

    #[test]
    fn vertical_inline_edges_leave_exported_sibling_position_unchanged() {
        let face = embedded_face();
        let paint = |css: &str| render(&format!(
            r#"<style>{face}
                body{{margin:0;font:16px/24px FixtureRoboto}}
                #outer{{writing-mode:vertical-lr;white-space:nowrap}}
                #first{{{css}}}
            </style><div id="outer"><span id="first">ABC</span><span id="next">DEF</span></div>"#
        ), RenderOptions::default()).unwrap();
        let plain = paint("");
        let next_position = |r: &Rendered| {
            let next = r.elements.iter().find(|e| e.id == "next").unwrap();
            (next.x, next.y)
        };
        let (plain_x, plain_y) = next_position(&plain);
        for side in ["left", "right", "top", "bottom"] {
            let bordered = paint(&format!("border-{side}:20px solid transparent"));
            let (x, y) = next_position(&bordered);
            assert!((x - plain_x).abs() < 0.8 && (y - plain_y).abs() < 0.8,
                "vertical {side} border changed next span position: ({plain_x},{plain_y}) -> ({x},{y})");
        }
    }

    #[test]
    fn form_count_attributes_change_intrinsic_boxes() {
        let face = embedded_face();
        let html = format!(r#"<style>{face}
            body{{margin:0;background:white}}
            input,textarea{{font:16px/19.2px FixtureRoboto}}
            </style>
            <input id="a" size="8"><input id="b" size="16">
            <textarea id="c" rows="2" cols="10"></textarea>
            <textarea id="d" rows="4" cols="20"></textarea>"#);
        let result = render(&html, RenderOptions::default()).unwrap();
        let box_for = |id: &str| result.elements.iter().find(|e| e.id == id).unwrap();
        let a = box_for("a");
        let b = box_for("b");
        let c = box_for("c");
        let d = box_for("d");
        // 8 extra columns at 16px with the native control advance.
        assert!((b.width - a.width - 8.0*16.0*0.67).abs() <= 1.0,
            "size attr should change border-box width: {a:?} {b:?}");
        assert!((d.width - c.width - 10.0*16.0*0.60).abs() <= 1.0,
            "cols attr should change border-box width: {c:?} {d:?}");
        assert!((d.height - c.height - 2.0*19.2).abs() <= 1.0,
            "rows attr should change border-box height: {c:?} {d:?}");
    }

    #[test]
    fn static_checked_mark_paints_blue_pixels_and_appearance_none_suppresses_it() {
        let html = r#"<style>body{margin:0;background:white}</style>
            <input id="checked" type="checkbox" checked>
            <input id="plain" type="checkbox">
            <input id="none" type="checkbox" checked style="appearance:none">"#;
        let result = render(html, RenderOptions::default()).unwrap();
        let blue_pixels = |id: &str| {
            let b = result.elements.iter().find(|e| e.id == id).unwrap();
            let left = b.x.max(0.0).floor() as usize;
            let right = (b.x+b.width).ceil().min(result.width as f64) as usize;
            let top = b.y.max(0.0).floor() as usize;
            let bottom = (b.y+b.height).ceil().min(result.height as f64) as usize;
            (top..bottom).flat_map(|y| (left..right).map(move |x| (x,y)))
                .filter(|(x,y)| {
                    let i = (y*result.width as usize+x)*4;
                    let p = &result.rgba[i..i+4];
                    p[2] > p[0].saturating_add(60) && p[2] > p[1].saturating_add(25)
                }).count()
        };
        assert!(blue_pixels("checked") >= 8, "checked mark should paint blue");
        assert_eq!(blue_pixels("plain"), 0, "unchecked box must have no blue fill");
        assert_eq!(blue_pixels("none"), 0, "appearance:none must suppress UA fill");
    }

    #[test]
    fn placeholder_glyphs_paint_grey_inside_the_field() {
        let face = embedded_face();
        let paint = |placeholder: &str| render(&format!(
            r#"<style>{face}body{{margin:0;background:white}}
                input{{font:20px/24px FixtureRoboto}}</style>
                <input id="target" size="20" placeholder="{placeholder}">"#
        ), RenderOptions::default()).unwrap();
        let blank = paint("");
        let hint = paint("Example");
        let box_for = hint.elements.iter().find(|e| e.id == "target").unwrap();
        let gray = |r: &Rendered| {
            let left = (box_for.x+6.0).floor() as usize;
            let right = (box_for.x+box_for.width-6.0).ceil() as usize;
            let top = (box_for.y+4.0).floor() as usize;
            let bottom = (box_for.y+box_for.height-4.0).ceil() as usize;
            (top..bottom).flat_map(|y| (left..right).map(move |x| (x,y)))
                .filter(|(x,y)| {
                    let i = (y*r.width as usize+x)*4;
                    let p = &r.rgba[i..i+4];
                    p[0] >= 105 && p[0] <= 190 && p[1].abs_diff(p[0]) <= 3
                        && p[2].abs_diff(p[0]) <= 3
                }).count()
        };
        assert_eq!(gray(&blank), 0, "blank field should have no inner ink");
        assert!(gray(&hint) > 4, "placeholder must paint grey glyph pixels");
    }

}
