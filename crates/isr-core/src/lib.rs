// Copyright 2026 g1BSsQ. Licensed under Apache-2.0.
// Offscreen scene construction adapted from the pinned screenshot example; see NOTICE.
mod geometry;
mod counters;

use anyrender::{PaintScene as _, render_to_buffer};
use anyrender_vello_cpu::VelloCpuImageRenderer;
use blitz_dom::{BaseDocument, DocumentConfig, FontContext, Node, NodeData, UserAgentStylesheet, util::Color};
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
    /// By default, font discovery through the text stack is off.
    /// KNOWN LIMITATION: SVG parsing in the pinned Blitz revision loads the
    /// system font database (filesystem + fontconfig env vars) on first SVG
    /// use, regardless of options; removal is tracked for the fonts capability.
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
// The compiled system-fonts feature permits text-stack opt-in; SVG parsing
// independently accesses a global font database (documented on render).
fn font_context(system_fonts: bool, fonts: Vec<Vec<u8>>) -> FontContext {
    let mut ctx = FontContext {
        source_cache: SourceCache::new_shared(),
        collection: Collection::new(CollectionOptions {
            shared: false,
            system_fonts,
        }),
    };
    // Always register Blitz's compiled-in bullet font for list markers.
    // These bytes are in memory and do not trigger filesystem access.
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
/// KNOWN LIMITATION: SVG parsing in the pinned Blitz revision loads the system
/// font database (filesystem + fontconfig env vars) on first SVG use,
/// regardless of options; removal is tracked for the fonts capability.
/// Inline SVG and data: URI SVG images can trigger this independent access even
/// when `system_fonts: false`. The library never performs network access;
/// non-data: subresource requests are refused and recorded in blocked_resources.
/// KNOWN LIMITATION: If the host creates a Blitz `DocumentConfig::default()`
/// before the first call to `render()`, Blitz may cache its UA stylesheet
/// without the counter rules. This shared initializer cannot repair an already
/// cached sheet; a pinned upstream parse-order fix is required.
pub fn render(html: &str, opts: RenderOptions) -> Result<Rendered> {
    let RenderOptions { width, system_fonts, fonts } = opts;
    if !(1..=8192).contains(&width) { return Err("width must be in 1..=8192".into()); }
    let net = Arc::new(DataOnly::default());
    let mut doc = HtmlDocument::from_html(
        html,
        document_config(width, Some(font_context(system_fonts, fonts)), Some(net.clone())),
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
}
