// Copyright 2026 g1BSsQ. Licensed under Apache-2.0.
// Offscreen scene construction adapted from the pinned screenshot example; see NOTICE.
mod geometry;
mod counters;
mod effects_paint;
mod fonts;
mod interaction;
mod line_clamp_css;
#[cfg(test)]
mod effects_tests;
pub use interaction::{Document, NodeRef, Timings};

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
            // Reset each box independently; these custom declarations otherwise inherit.
            UserAgentStylesheet::parse("* { --native-lines: none; --native-max-lines: none; --native-legacy-lines: none; --native-legacy-orient: horizontal; --native-legacy-display: no; --native-ellipsis: auto; }"),
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
    let normalized_html = line_clamp_css::normalize(html);
    let mut doc = HtmlDocument::from_html(
        &normalized_html,
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
    // The common case stays on the original direct paint path. Only pages
    // using CSS filters record and resolve offscreen groups.
    let has_filters = elements.iter().any(|element| {
        element.computed.get("filter").is_some_and(|value| value != "none")
    });
    let rgba = if has_filters {
        let mut recording = anyrender::Scene::new();
        recording.fill(
            Fill::NonZero, Default::default(), Color::WHITE,
            Default::default(), &Rect::new(0.0, 0.0, width as f64, height as f64),
        );
        blitz_paint::paint_scene(&mut recording, &mut doc, 1.0, width, height, 0, 0);
        let scene = effects_paint::resolve(recording, width, height);
        render_to_buffer::<VelloCpuImageRenderer, _>(
            |output| output.append_scene(scene, Default::default()),
            width, height,
        )
    } else {
        render_to_buffer::<VelloCpuImageRenderer, _>(
            |scene| {
                scene.fill(
                    Fill::NonZero, Default::default(), Color::WHITE,
                    Default::default(), &Rect::new(0.0, 0.0, width as f64, height as f64),
                );
                blitz_paint::paint_scene(scene, &mut doc, 1.0, width, height, 0, 0);
            }, width, height,
        )
    };
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
    fn collapsed_table_rows_use_shared_vertical_borders() {
        let r = render(
            include_str!("../../../fixtures/07-table.html"),
            RenderOptions { system_fonts: true, ..RenderOptions::default() },
        ).unwrap();
        let first_cells: Vec<_> = r.elements.iter().filter(|e| e.tag == "td")
            .step_by(3).collect();
        assert_eq!(first_cells.len(), 3);
        for (cell, expected_y) in first_cells.iter().zip([108.7, 154.7, 200.7]) {
            assert!((cell.y - expected_y).abs() < 0.8,
                "collapsed row shifted at y={expected_y}: {cell:?}");
            assert!((cell.height - 46.0).abs() < 0.8, "{cell:?}");
        }
    }

    #[test]
    fn disclosure_marker_does_not_grow_explicit_summary_line_height() {
        let r = render(
            include_str!("../../../fixtures/probes/details-state.html"),
            RenderOptions::default(),
        ).unwrap();
        let summaries: Vec<_> = r.elements.iter()
            .filter(|e| e.tag == "summary").collect();
        assert_eq!(summaries.len(), 2);
        for summary in summaries {
            assert!((summary.height - 28.0).abs() < 0.2,
                "disclosure marker inflated summary: {summary:?}");
        }
        let first = r.elements.iter().find(|e| e.id == "closed-first").unwrap();
        assert!((first.y - 55.0).abs() < 0.8, "{first:?}");
        let second = r.elements.iter().find(|e| e.id == "closed-second").unwrap();
        assert!((second.y - 93.0).abs() < 0.8, "{second:?}");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn oversized_atomic_inlines_do_not_create_whitespace_only_lines() {
        let r = render(
            include_str!("../../../fixtures/probes/font-family-aliases.html"),
            RenderOptions { system_fonts: true, ..RenderOptions::default() },
        ).unwrap();
        for (id, expected_y) in [
            ("arial", 12.0), ("liberation-sans", 48.0),
            ("georgia", 84.0), ("liberation-serif", 119.0),
        ] {
            let b = r.elements.iter().find(|e| e.id == id).unwrap();
            assert!((b.x - 12.0).abs() < 0.8, "{id}: {b:?}");
            assert!((b.y - expected_y).abs() < 0.8, "{id}: {b:?}");
        }
    }

    #[test]
    fn nbsp_between_replaced_inlines_preserves_its_line() {
        // CSS Text: there is a soft wrap opportunity between each replaced
        // image and NBSP, but NBSP itself is not collapsible whitespace.
        // Compare the zero-width wrapping case with explicit line breaks.
        let pixel = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkaGD4DwACiQGBU29HsgAAAABJRU5ErkJggg==";
        let html = format!(
            r#"<style>{}body{{margin:0}}div{{font:16px/20px FixtureRoboto}}</style>
            <div style="width:0"><img id="narrow-first" src="{pixel}" width="50" height="50">&nbsp;<img id="narrow-last" src="{pixel}" width="50" height="50"></div>
            <div><img id="reference-first" src="{pixel}" width="50" height="50"><br>&nbsp;<br><img id="reference-last" src="{pixel}" width="50" height="50"></div>"#,
            embedded_face(),
        );
        let r = render(&html, RenderOptions::default()).unwrap();
        let narrow_first = r.elements.iter().find(|e| e.id == "narrow-first").unwrap();
        let narrow_last = r.elements.iter().find(|e| e.id == "narrow-last").unwrap();
        let reference_first = r.elements.iter().find(|e| e.id == "reference-first").unwrap();
        let reference_last = r.elements.iter().find(|e| e.id == "reference-last").unwrap();

        let narrow_gap = narrow_last.y - narrow_first.y;
        let reference_gap = reference_last.y - reference_first.y;
        assert!(reference_gap > 50.0, "explicit-break reference: {reference_gap}");
        assert!(
            (narrow_gap - reference_gap).abs() < 1.0,
            "NBSP line was collapsed: got {narrow_gap}, expected {reference_gap}"
        );
    }

    #[test]
    fn replaced_inline_followed_by_break_keeps_image_line_height() {
        // A final <br> must not remove a second editor-only empty line.
        let pixel = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkaGD4DwACiQGBU29HsgAAAABJRU5ErkJggg==";
        let html = format!(
            r#"<style>body{{margin:0}}</style>
            <div id="alone"><img src="{pixel}" width="50" height="50"></div>
            <div id="break"><img src="{pixel}" width="50" height="50"><br></div>"#,
        );
        let r = render(&html, RenderOptions::default()).unwrap();
        let alone = r.elements.iter().find(|e| e.id == "alone").unwrap();
        let with_break = r.elements.iter().find(|e| e.id == "break").unwrap();
        assert!((alone.height - with_break.height).abs() < 0.8,
            "img+br changed height: img={alone:?}, img+br={with_break:?}");
        assert!(with_break.height >= 50.0, "{with_break:?}");
    }

    #[test]
    fn decorated_inline_image_retains_start_edge() {
        let pixel = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkaGD4DwACiQGBU29HsgAAAABJRU5ErkJggg==";
        let html = format!(
            r#"<style>body{{margin:0}}</style>
            <div><a id="decorated" style="padding-left:10px;border-left:3px solid"><img id="nested" src="{pixel}" width="50" height="50"></a> <img src="{pixel}" width="50" height="50"></div>"#,
        );
        let r = render(&html, RenderOptions::default()).unwrap();
        let decorated = r.elements.iter().find(|e| e.id == "decorated").unwrap();
        let nested = r.elements.iter().find(|e| e.id == "nested").unwrap();
        assert!((nested.x - decorated.x).abs() < 0.8,
            "inline start edge lost: inline={decorated:?}, image={nested:?}");
        assert!((nested.x - 13.0).abs() < 0.8, "nested image: {nested:?}");
    }

    #[test]
    fn preceding_replaced_inline_is_not_shifted_by_later_inline_edges() {
        // Compare the start of the decorated inline with an undecorated
        // second image: its 13px start edge belongs after the first image.
        let pixel = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkaGD4DwACiQGBU29HsgAAAABJRU5ErkJggg==";
        let html = format!(
            r#"<style>{}body{{margin:0;font:16px/20px FixtureRoboto}}</style>
            <div><img id="first" src="{pixel}" width="50" height="50"> <a id="later-edge" style="padding-left:10px;border-left:3px solid"><img id="inside" src="{pixel}" width="50" height="50"></a></div>
            <div><img id="plain-first" src="{pixel}" width="50" height="50"> <img id="plain-second" src="{pixel}" width="50" height="50"></div>"#,
            embedded_face(),
        );
        let r = render(&html, RenderOptions::default()).unwrap();
        let first = r.elements.iter().find(|e| e.id == "first").unwrap();
        let inside = r.elements.iter().find(|e| e.id == "inside").unwrap();
        let plain_first = r.elements.iter().find(|e| e.id == "plain-first").unwrap();
        let plain_second = r.elements.iter().find(|e| e.id == "plain-second").unwrap();
        assert!(first.x.abs() < 0.8, "preceding image moved offscreen: {first:?}");
        let inline_start = first.x + (plain_second.x - plain_first.x);
        assert!((inside.x - (inline_start + 13.0)).abs() < 0.8,
            "nested image must follow the inline start plus its 13px edge: {inside:?}, start={inline_start}");
    }

    #[test]
    fn empty_summary_marker_keeps_a_line_box() {
        let r = render(
            r#"<style>body{margin:0}summary{line-height:24px}</style>
               <details open><summary id="empty"></summary></details>
               <details open><summary id="text">Label</summary></details>"#,
            RenderOptions::default(),
        ).unwrap();
        let empty = r.elements.iter().find(|e| e.id == "empty").unwrap();
        // Marker line rounding can differ by 1px from the specified 24px line-height.
        assert!((23.0..=25.0).contains(&empty.height),
            "empty marker must retain one 24px line box (23..=25): {empty:?}");
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
    fn padded_inline_span_includes_padding_and_border() {
        // Arial resolves through the system fonts installed by the CI runner.
        // Compare a decorated run with an undecorated run in the same line,
        // instead of assuming a font-independent ascent/descent to em ratio.
        let html = r#"<style>body{margin:0;font:16px/48px Arial,sans-serif}</style>
            <div><span id="bare">Text</span>
            <span id="padded" style="padding:7px 4px;border:2px solid">Text</span></div>"#;
        let r = render(html, RenderOptions {
            system_fonts: true, ..RenderOptions::default()
        }).unwrap();
        let bare = r.elements.iter().find(|e| e.id == "bare").unwrap();
        let padded = r.elements.iter().find(|e| e.id == "padded").unwrap();
        assert!(bare.height > 0.0 && bare.height < 0.6 * 48.0,
            "bare inline must use the content area, not the line: {bare:?}");
        assert!((padded.height - bare.height - 18.0).abs() < 0.05,
            "7px padding and 2px border on each vertical edge: {bare:?} {padded:?}");
        assert!(padded.width > 0.0, "horizontal text fragment: {padded:?}");
    }

    #[test]
    fn multiline_inline_span_unions_content_fragments() {
        let html = r#"<style>body{margin:0;font:16px/48px Arial,sans-serif}</style>
            <div><span id="single">Text</span>
            <span id="wrapped">First<br>Second</span></div>"#;
        let r = render(html, RenderOptions {
            system_fonts: true, ..RenderOptions::default()
        }).unwrap();
        let single = r.elements.iter().find(|e| e.id == "single").unwrap();
        let wrapped = r.elements.iter().find(|e| e.id == "wrapped").unwrap();
        assert!(single.height > 0.0 && single.height < 0.6 * 48.0,
            "single line must report its own font content area: {single:?}");
        assert!(wrapped.width > 0.0, "{wrapped:?}");
        assert!((wrapped.height - (48.0 + single.height)).abs() < 0.05,
            "union must span two content areas 48px apart: {single:?} {wrapped:?}");
    }

    #[test]
    fn nested_large_font_does_not_expand_outer_span_content_area() {
        // The reference owns a 16px run; outer owns no text and contains only
        // the 32px nested run. The outer strut must match the reference, not
        // the nested font or the 60px line box.
        let html = r#"<style>body{margin:0;font:16px/60px Arial,sans-serif}</style>
            <div><span id="reference">Small</span>
            <span id="outer"><span id="inner" style="font-size:32px">Large</span></span></div>"#;
        let r = render(html, RenderOptions {
            system_fonts: true, ..RenderOptions::default()
        }).unwrap();
        let reference = r.elements.iter().find(|e| e.id == "reference").unwrap();
        let outer = r.elements.iter().find(|e| e.id == "outer").unwrap();
        let inner = r.elements.iter().find(|e| e.id == "inner").unwrap();
        assert!(reference.width > 0.0 && inner.width > 0.0,
            "both font sizes must shape real text: {reference:?} {inner:?}");
        assert_eq!(outer.computed.get("font-size"), reference.computed.get("font-size"));
        assert_ne!(inner.computed.get("font-size"), reference.computed.get("font-size"));
        assert!(reference.height > 0.0 && reference.height < 0.6 * 60.0,
            "reference must have a shaped 16px font content area: {reference:?}");
        assert!((outer.height - reference.height).abs() < 0.05,
            "outer 16px strut must match its own font: {reference:?} {outer:?}");
        assert!(inner.height > 1.8 * reference.height
            && inner.height < 2.2 * reference.height,
            "nested 32px run must be distinct from the outer 16px strut: {reference:?} {inner:?}");
    }

    #[test]
    fn control_only_inline_label_uses_own_font_strut() {
        // An adjacent text-only span supplies a real run for the same
        // 16px font while the label has no glyph runs of its own.
        let html = r#"<style>body{margin:0;font:16px/56px Arial,sans-serif}</style>
            <div><span id="reference">Text</span>
            <label id="only-control"><input type="checkbox"></label></div>"#;
        let r = render(html, RenderOptions {
            system_fonts: true, ..RenderOptions::default()
        }).unwrap();
        let reference = r.elements.iter().find(|e| e.id == "reference").unwrap();
        let label = r.elements.iter().find(|e| e.id == "only-control").unwrap();
        assert!(label.width > 0.0,
            "control still contributes horizontal extent: {label:?}");
        assert!((label.height - reference.height).abs() < 0.05,
            "control-only label must use its own font strut: {reference:?} {label:?}");
        assert!(label.height < 0.6 * 56.0,
            "control must not define the label's line-height extent: {label:?}");
    }

    #[test]
    fn text_only_span_in_tall_line_uses_small_font_content_area() {
        // The parent has 40px text in a 48px line; the unit and reference
        // spans both own 16px text runs of the same resolved font.
        let html = r#"<style>body{margin:0}
            .number{font:40px/48px Arial,sans-serif}
            .units{font-size:16px}</style>
            <div class="number" id="number">42
            <span id="units" class="units">ms</span>
            <span id="reference" class="units">sec</span></div>"#;
        let r = render(html, RenderOptions {
            system_fonts: true, ..RenderOptions::default()
        }).unwrap();
        let number = r.elements.iter().find(|e| e.id == "number").unwrap();
        let units = r.elements.iter().find(|e| e.id == "units").unwrap();
        let reference = r.elements.iter().find(|e| e.id == "reference").unwrap();
        assert!(number.height >= 47.0, "parent must have a tall line: {number:?}");
        assert!(units.width > 0.0 && reference.width > 0.0,
            "both small spans must shape text: {units:?} {reference:?}");
        assert_ne!(number.computed.get("font-size"), units.computed.get("font-size"));
        assert_eq!(units.computed.get("font-size"), reference.computed.get("font-size"));
        assert!((units.height - reference.height).abs() < 0.05,
            "matching 16px glyph runs must have the same content height: {units:?} {reference:?}");
        assert!(units.height > 0.0 && units.height < 0.6 * number.height,
            "small-font span must be clearly shorter than its parent line: {units:?} {number:?}");
        assert!(units.y > number.y, "{units:?} {number:?}");
    }

    #[test]
    fn empty_inline_span_has_a_font_strut() {
        // The empty span explicitly requests 24px rather than the inherited
        // 16px size. Its height must equal a shaped 24px run, not the 16px
        // sibling's content area or the full 48px line.
        let html = r#"<style>body{margin:0;font:16px/48px Arial,sans-serif}</style>
            <div><span id="small">Small</span>
            <span id="empty" style="font-size:24px"></span>
            <span id="reference" style="font-size:24px">Reference</span></div>"#;
        let r = render(html, RenderOptions {
            system_fonts: true, ..RenderOptions::default()
        }).unwrap();
        let small = r.elements.iter().find(|e| e.id == "small").unwrap();
        let empty = r.elements.iter().find(|e| e.id == "empty").unwrap();
        let reference = r.elements.iter().find(|e| e.id == "reference").unwrap();
        assert!(small.width > 0.0 && reference.width > 0.0,
            "strut references must shape real text: {small:?} {reference:?}");
        assert_ne!(empty.computed.get("font-size"), small.computed.get("font-size"));
        assert_eq!(empty.computed.get("font-size"), reference.computed.get("font-size"));
        assert!(empty.width.abs() < 0.1,
            "empty inline has no text advance: {empty:?}");
        assert!((empty.height - reference.height).abs() < 0.05,
            "empty 24px span must match its own font strut: {empty:?} {reference:?}");
        assert!(empty.height > 1.35 * small.height && empty.height < 0.8 * 48.0,
            "empty span must use 24px, not inherited 16px or line extent: {empty:?} {small:?}");
    }


    #[test]
    fn default_button_boxes_use_rounded_content_and_intrinsic_advances() {
        let r = render(include_str!("../../../fixtures/probes/form-buttons.html"),
            RenderOptions { system_fonts: true, ..RenderOptions::default() }).unwrap();
        for (id, width) in [("plain-button", 47.1), ("nested-button", 134.5)] {
            let b = r.elements.iter().find(|e| e.id == id).unwrap();
            assert!((b.height - 21.0).abs() < 0.1, "{id} content plus 1px padding and 2px border: {b:?}");
            assert!((b.width - width).abs() < 0.35, "{id} intrinsic advance: {b:?}");
        }
        let strong = r.elements.iter().find(|e| e.tag == "strong").unwrap();
        assert!((strong.x - 24.0).abs() < 0.15, "button starts content at 2+6px: {strong:?}");
    }

    #[test]
    fn action_input_boxes_use_rounded_content_and_border_insets() {
        let r = render(include_str!("../../../fixtures/probes/form-action-inputs.html"),
            RenderOptions { system_fonts: true, ..RenderOptions::default() }).unwrap();
        for (id, y, width) in [("submit", 18.0, 76.8), ("button", 56.0, 53.0),
                              ("reset", 94.0, 50.8)] {
            let b = r.elements.iter().find(|e| e.id == id).unwrap();
            assert!((b.height - 21.0).abs() < 0.1, "{id} height: {b:?}");
            assert!((b.width - width).abs() < 0.35, "{id} intrinsic width: {b:?}");
            assert!((b.y - y).abs() < 0.35, "{id} baseline: {b:?}");
        }
    }

    #[test]
    fn action_inputs_keep_author_line_height() {
        let html = r#"<style>
            body { margin: 0; font: 16px/24px Arial,sans-serif }
        </style>
        <input id="submit" type="submit" value="Send" style="line-height:40px">
        <input id="reset" type="reset" value="Reset" style="line-height:40px">
        <input id="button" type="button" value="Action" style="line-height:40px">"#;
        let r = render(html,
            RenderOptions { system_fonts: true, ..RenderOptions::default() }).unwrap();
        for id in ["submit", "reset", "button"] {
            let b = r.elements.iter().find(|e| e.id == id).unwrap();
            assert!((b.height - 46.0).abs() < 0.1,
                "{id}: 40px author line height + 2px padding + 4px border: {b:?}");
        }
    }

    #[test]
    fn default_text_input_intrinsic_cells_round_up_and_align_to_baseline() {
        let r = render(include_str!("../../../fixtures/probes/form-text-inputs.html"),
            RenderOptions { system_fonts: true, ..RenderOptions::default() }).unwrap();
        for id in ["text", "email", "number", "password", "search"] {
            let b = r.elements.iter().find(|e| e.id == id).unwrap();
            assert!((b.width - 185.0).abs() < 0.1, "{id} size=20 intrinsic width: {b:?}");
            assert!((b.height - 21.0).abs() < 0.1, "{id} height: {b:?}");
        }
        let text = r.elements.iter().find(|e| e.id == "text").unwrap();
        assert!((text.y - 18.0).abs() < 0.35, "text field baseline: {text:?}");
    }

    #[test]
    fn empty_inline_font_strut_is_css_scaled_at_one_and_two_device_pixels() {
        // The inline element contributes no glyph run; the only line item is
        // an atomic checkbox, so the empty span must use its own 16px strut.
        let html = r#"<style>body{margin:0;font:16px/24px Arial,sans-serif}</style>
            <div><span id="empty"></span><input type="checkbox"></div>"#;
        for scale in [1.0, 2.0] {
            let mut config = document_config(
                412, Some(fonts::context(false, &[], html)), None);
            config.viewport = Some(Viewport::new(412, 800, scale, ColorScheme::Light));
            let mut doc = HtmlDocument::from_html(html, config);
            doc.resolve(0.0);
            doc.resolve(0.0);
            let node = doc.get_node(doc.get_element_by_id("empty").unwrap()).unwrap();
            let b = geometry::element_box(&doc, node).unwrap();
            assert!((b.width).abs() < 0.1, "empty inline width at {scale}: {b:?}");
            // No glyph metrics are available in this line. The em fallback
            // starts in CSS pixels and is rounded in device pixels.
            let root = node.inline_root_ancestor().unwrap();
            let inline = root.element_data().unwrap().inline_layout_data.as_ref().unwrap();
            let baseline = inline.layout.lines().next().unwrap().metrics().baseline as f64
                / inline.layout.scale() as f64;
            let device_scale = scale as f64;
            let expected_y = doc.get_client_bounding_rect(root.id).unwrap().y
                + baseline - (16.0_f64 * 0.8 * device_scale).round() / device_scale;
            assert!((b.y - expected_y).abs() < 0.01,
                "no-glyph strut y at scale {scale}: {b:?}, expected {expected_y}");
            assert!((b.height - 16.0).abs() < 0.01,
                "16 CSS-pixel content height at scale {scale}: {b:?}");
            assert!((b.height - b.height.round()).abs() < 0.00001,
                "integer CSS-pixel content height at scale {scale}: {b:?}");
        }
    }

    #[test]
    fn empty_inline_uses_css_pixel_glyph_strut_at_both_device_scales() {
        // The following "Hello" run supplies font metrics in device pixels.
        // The span itself owns no glyphs and falls back to that run's strut.
        let html = r#"<style>body{margin:0;font:16px/24px Arial,sans-serif}</style>
            <div><span id="empty"></span>Hello</div>"#;
        for (scale, expected_height) in [(1.0, 17.0), (2.0, 18.0)] {
            let mut config = document_config(
                412, Some(fonts::context(true, &[], html)), None);
            config.viewport = Some(Viewport::new(412, 800, scale, ColorScheme::Light));
            let mut doc = HtmlDocument::from_html(html, config);
            doc.resolve(0.0);
            doc.resolve(0.0);
            let node = doc.get_node(doc.get_element_by_id("empty").unwrap()).unwrap();
            let b = geometry::element_box(&doc, node).unwrap();
            assert!(b.width.abs() < 0.01, "empty span advance at scale {scale}: {b:?}");
            assert_eq!(b.y, 3.0, "glyph-present strut CSS y at scale {scale}: {b:?}");
            assert_eq!(b.height, expected_height,
                "glyph-present strut CSS height at scale {scale}: {b:?}");
            assert!((b.height - b.height.round()).abs() < 0.00001,
                "integer content height at scale {scale}: {b:?}");
        }
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
    fn collapsed_spaces_adjacent_to_inline_edges_match_compact_markup() {
        let face = embedded_face();
        let render_row = |inner: &str| render(&format!(
            r#"<style>{face}
                body{{margin:0;font:16px/24px FixtureRoboto}}
                #outer{{border-left:5px solid blue}}
                #inner{{margin-left:50px;border-left:5px solid orange}}
            </style><div><span id="outer">{inner}</span></div>"#
        ), RenderOptions::default()).unwrap();
        let compact = render_row(r#"<span id="inner">Filler Text</span>"#);
        let spaced = render_row("\n  <span id=\"inner\">Filler Text</span>\n");
        let width = |r: &Rendered| r.elements.iter()
            .find(|e| e.id == "outer").unwrap().width;
        assert!((width(&compact) - width(&spaced)).abs() < 0.8,
            "collapsed boundary spaces expanded inline: compact={} spaced={}",
            width(&compact), width(&spaced));
    }

    #[test]
    fn inline_root_leading_space_after_border_collapses_before_nested_inline() {
        let face = embedded_face();
        let paint = |edge: &str, whitespace: &str| render(&format!(
            r#"<style>{face}
                body{{margin:0;font:16px/24px FixtureRoboto}}
                div{{display:inline}}
                #div1{{border-left:5px solid blue;{edge}}}
                #inner{{border-left:5px solid orange;{edge}}}
            </style><p>Explanatory block above the inline line.</p>
            <div id="div1">{whitespace}<div id="inner">Filler Text</div></div>"#
        ), RenderOptions::default()).unwrap();
        fn get<'a>(r: &'a Rendered, id: &str) -> &'a Element {
            r.elements.iter().find(|e| e.id == id).unwrap()
        }
        for edge in ["margin-left:50px", "padding-left:50px"] {
            let compact = paint(edge, "");
            let spaced = paint(edge, "\n  ");
            let a = get(&compact, "div1");
            let b = get(&spaced, "div1");
            assert!((a.width - b.width).abs() < 0.8,
                "line-start collapsed space after inline border expanded {edge}: {a:?} vs {b:?}");
            let x0 = get(&compact, "inner").x;
            let x1 = get(&spaced, "inner").x;
            assert!((x0 - x1).abs() < 0.8,
                "line-start space shifted nested inline for {edge}: {x0} vs {x1}");
        }
    }

    #[test]
    fn collapsed_nested_inline_border_whitespace_matches_compact_text() {
        let face = embedded_face();
        let paint = |html: &str| render(&format!(
            r#"<style>{face}
                body{{margin:0;font:16px/24px FixtureRoboto}}
                .inner{{border:medium solid green}}
            </style><div>{html}</div>"#
        ), RenderOptions::default()).unwrap();
        let compact = paint(r#"<span id="outer" class="inner"><span>Text</span></span>"#);
        let spaced = paint("<span id=\"outer\" class=\"inner\">\n <span>\n Text \n</span>\n</span>");
        let width = |r: &Rendered| r.elements.iter()
            .find(|e| e.id == "outer").unwrap().width;
        assert!((width(&compact) - width(&spaced)).abs() < 0.8,
            "nested collapsed whitespace expanded inline border: {} vs {}",
            width(&compact), width(&spaced));
    }

    #[test]
    fn inline_edge_does_not_eat_interior_collapsible_spaces() {
        let face = embedded_face();
        let paint = |spaced: bool| {
            let (first, second) = if spaced { ("a ", " c") } else { ("a", "c") };
            render(&format!(
                r#"<style>{face}
                    body{{margin:0;font:16px/24px FixtureRoboto}}
                    #middle{{border-left:5px solid}}
                </style><div>{first}<span id="middle">b</span><span id="tail">{second}</span></div>"#
            ), RenderOptions::default()).unwrap()
        };
        let compact = paint(false);
        let spaced = paint(true);
        let x = |r: &Rendered, id: &str| r.elements.iter()
            .find(|e| e.id == id).unwrap().x;
        let mid_space = x(&spaced, "middle") - x(&compact, "middle");
        let tail_space = x(&spaced, "tail") - x(&compact, "tail");
        assert!(mid_space > 2.0,
            "a space before bordered inline must survive: {mid_space}");
        assert!(tail_space > 2.0 && (tail_space - mid_space).abs() < 0.05,
            "a space after bordered inline must survive within layout rounding: {tail_space} / {mid_space}");
    }

    #[test]
    fn generated_inline_boxes_keep_intervening_collapsible_space() {
        let face = embedded_face();
        let paint = |space: &str| render(&format!(
            r#"<style>{face}
                body{{margin:0;font:16px/24px FixtureRoboto}}
                #one,#two{{border-left:1px solid transparent}}
                #one::before{{content:"A"}}
                #two::before{{content:"B"}}
            </style><div><span id="one"></span>{space}<span id="two"></span></div>"#
        ), RenderOptions::default()).unwrap();
        let compact = paint("");
        let spaced = paint(" ");
        let x = |r: &Rendered| r.elements.iter().find(|e| e.id == "two").unwrap().x;
        assert!(x(&spaced) - x(&compact) > 2.0,
            "collapsed whitespace between generated inline boxes was removed");
    }

    #[test]
    fn preserved_whitespace_survives_inline_borders() {
        let face = embedded_face();
        let paint = |mode: &str, spaces: &str| render(&format!(
            r#"<style>{face}
                body{{margin:0;font:16px/24px FixtureRoboto}}
                #line{{white-space:{mode}}}
                #first{{border-left:5px solid}}
            </style><div id="line"><span id="first">{spaces}A{spaces}</span><span id="next">B</span></div>"#
        ), RenderOptions::default()).unwrap();
        for mode in ["pre", "pre-wrap", "break-spaces"] {
            let compact = paint(mode, "");
            let spaced = paint(mode, "  ");
            let x = |r: &Rendered| r.elements.iter().find(|e| e.id == "next").unwrap().x;
            assert!(x(&spaced) - x(&compact) > 8.0,
                "{mode}: preserved spaces beside border were discarded");
        }
    }

    #[test]
    fn inline_boundary_whitespace_does_not_change_max_content() {
        let face = embedded_face();
        let paint = |words: &str| render(&format!(
            r#"<style>{face}
                body{{margin:0;font:20px/24px FixtureRoboto}}
                #line{{width:max-content}}
                #first{{border-left:5px solid}}
            </style><div id="line"><span id="first">{words}</span></div>"#
        ), RenderOptions::default()).unwrap();
        let grouped = paint("XXXp pXXp pXXp pXXp");
        let one = paint("XXXp pXXppXXppXXp");
        let compact = paint("XXXppXXppXXppXXp");
        let width = |r: &Rendered| r.elements.iter().find(|e| e.id == "line").unwrap().width;
        let space = width(&one) - width(&compact);
        let three = width(&grouped) - width(&compact);
        assert!(space > 2.0 && (three - 3.0*space).abs() < 1.8,
            "three spaces must contribute roughly three advances: {space} vs {three}");
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
    fn range_without_value_uses_midpoint_of_custom_limits() {
        let render_range = |value: &str| render(&format!(
            r#"<style>body{{margin:0;background:white}}
                input{{margin:8px;width:130px;height:24px}}</style>
                <input type="range" min="0" max="10" {value}>"#
        ), RenderOptions::default()).unwrap();
        let implicit = render_range("");
        let explicit = render_range("value=\"5\"");
        assert_eq!(implicit.rgba, explicit.rgba,
            "range default must use the midpoint of its specified limits");
    }

    #[test]
    fn bare_range_and_color_paint_spec_defaults() {
        let render_controls = |explicit: bool| {
            let controls = if explicit {
                r##"<input type="range" value="50"><input type="color" value="#000000">"##
            } else {
                r#"<input type="range"><input type="color">"#
            };
            render(&format!(
                "<style>body{{margin:0;background:white}}input{{display:block;margin:8px;width:130px;height:24px}}</style>{controls}"
            ), RenderOptions::default()).unwrap()
        };
        assert_eq!(render_controls(false).rgba, render_controls(true).rgba,
            "bare range/color controls must paint their default values");
    }

    #[test]
    fn synthetic_inline_edge_preserves_unbroken_word_min_content() {
        let face = embedded_face();
        let paint = |width: &str| render(&format!(
            r#"<style>{face}body{{margin:0;font:16px/24px FixtureRoboto}}
                #line{{width:{width}}}</style>
                <div id="line">un<b style="border-left:1px solid">believable</b></div>"#
        ), RenderOptions::default()).unwrap();
        let min = paint("min-content");
        let max = paint("max-content");
        let w = |r: &Rendered| r.elements.iter().find(|e| e.id == "line").unwrap().width;
        assert!((w(&min) - w(&max)).abs() < 0.8,
            "inline edge must not break unbroken word: min={} max={}",w(&min),w(&max));
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
                    p.iter().take(3).all(|&channel| channel.abs_diff(117) <= 10)
                }).count()
        };
        assert_eq!(gray(&blank), 0, "blank field should have no inner ink");
        assert!(gray(&hint) > 4, "placeholder must have pixels near rgb(117,117,117)");
        let dark_count = {
            let left = (box_for.x+6.0).floor().max(0.0) as usize;
            let right = (box_for.x+box_for.width-6.0).ceil().min(hint.width as f64) as usize;
            let top = (box_for.y+4.0).floor().max(0.0) as usize;
            let bottom = (box_for.y+box_for.height-4.0).ceil().min(hint.height as f64) as usize;
            (top..bottom).flat_map(|y| (left..right).map(move |x| (x,y)))
                .filter(|&(x,y)| {
                    let offset = (y*hint.width as usize+x)*4;
                    hint.rgba[offset..offset+3].iter().all(|&channel| channel < 90)
                }).count()
        };
        assert!(dark_count <= 2, "placeholder should have almost no dark text pixels");
    }

    #[test]
    fn textarea_labels_follow_text_height_not_editor_rows() {
        let r = render(include_str!("../../../fixtures/probes/form-textarea.html"),
            RenderOptions { system_fonts: true, ..RenderOptions::default() }).unwrap();
        let labels: Vec<_> = r.elements.iter().filter(|e| e.tag == "label").collect();
        let areas: Vec<_> = r.elements.iter().filter(|e| e.tag == "textarea").collect();
        assert_eq!(labels.len(), 2);
        assert_eq!(areas.len(), 2);
        for (label, area) in labels.iter().zip(areas) {
            assert!(label.height > 0.0 && label.height < 21.0,
                "textarea label is not a text content box: {label:?}");
            assert!(label.height < area.height / 2.0, "label includes editor: {label:?} {area:?}");
            assert!(label.y > area.y + area.height / 2.0,
                "label starts at editor top: {label:?} {area:?}");
        }
    }

    #[test]
    fn choice_and_text_input_labels_use_glyph_bounds() {
        for html in [
            include_str!("../../../fixtures/probes/form-choice-inputs.html"),
            include_str!("../../../fixtures/probes/form-text-inputs.html"),
        ] {
            let r = render(html, RenderOptions { system_fonts: true, ..RenderOptions::default() }).unwrap();
            let row = r.elements.iter().find(|e| e.class == "row").unwrap();
            let label = r.elements.iter().find(|e| e.tag == "label").unwrap();
            assert!(label.height > 0.0 && label.height < 21.0,
                "input label is not a text content box: {label:?}");
            assert!(label.height < row.height, "inline label fills its row: {label:?} {row:?}");
            assert!(label.y > row.y, "inline label begins at row top: {label:?} {row:?}");
        }
    }

    #[test]
    fn listbox_label_baseline_is_below_the_control_midline() {
        let r = render(include_str!("../../../fixtures/probes/form-select.html"),
            RenderOptions { system_fonts: true, ..RenderOptions::default() }).unwrap();
        let label = r.elements.iter().filter(|e| e.tag == "label").nth(1).unwrap();
        let control = r.elements.iter().find(|e| e.id == "multiple-rows").unwrap();
        assert!(label.height > 0.0 && label.height < 21.0,
            "listbox label is not a text content box: {label:?}");
        assert!(label.y > control.y + control.height / 2.0,
            "listbox label expanded to control top: {label:?} {control:?}");
        assert!(label.height < control.height / 2.0,
            "listbox label expanded to control height: {label:?} {control:?}");
    }

    #[test]
    fn inline_without_nested_control_keeps_previous_geometry() {
        let r = render(r#"<style>body{margin:0;font:16px/24px sans-serif}</style>
            <div><span id="plain">Text</span></div>
            <div><label id="with-control">Text <input value="A"></label></div>"#,
            RenderOptions { system_fonts: true, ..RenderOptions::default() }).unwrap();
        let plain = r.elements.iter().find(|e| e.id == "plain").unwrap();
        let label = r.elements.iter().find(|e| e.id == "with-control").unwrap();
        assert!(plain.height > 0.75 * 16.0 && plain.height < 1.25 * 16.0,
            "ordinary span must use its font content area: {plain:?}");
        assert!((plain.height - label.height).abs() < 1.5,
            "inline text and label text use the same font bounds: {plain:?} {label:?}");
    }

    #[test]
    fn block_form_label_keeps_padding_border_and_explicit_height() {
        let r = render(
            r#"<style>
                body { margin: 0; font: 16px/24px Arial, sans-serif; }
                label {
                    display: block; box-sizing: content-box;
                    width: 120px; height: 36px; padding: 8px; border: 2px solid black;
                }
            </style>
            <label id="block-label">Text <input type="checkbox"></label>"#,
            RenderOptions { system_fonts: true, ..RenderOptions::default() },
        ).unwrap();
        let label = r.elements.iter().find(|e| e.id == "block-label").unwrap();
        assert!((label.width - 140.0).abs() < 0.05, "block border-box width: {label:?}");
        assert!((label.height - 56.0).abs() < 0.05, "block border-box height: {label:?}");
    }

    #[test]
    fn inline_block_form_label_keeps_padding_border_and_explicit_height() {
        let r = render(
            r#"<style>
                body { margin: 0; font: 16px/24px Arial, sans-serif; }
                label {
                    display: inline-block; box-sizing: content-box;
                    width: 100px; height: 32px; padding: 10px; border: 3px solid black;
                }
            </style>
            <label id="inline-block-label">Text <input type="checkbox"></label>"#,
            RenderOptions { system_fonts: true, ..RenderOptions::default() },
        ).unwrap();
        let label = r.elements.iter().find(|e| e.id == "inline-block-label").unwrap();
        assert!((label.width - 126.0).abs() < 0.05, "inline-block border-box width: {label:?}");
        assert!((label.height - 58.0).abs() < 0.05, "inline-block border-box height: {label:?}");
    }

    #[test]
    fn native_control_baselines_preserve_form_line_struts() {
        let html = r#"<style>body{margin:0;font:16px/24px Arial,sans-serif}
               .row{white-space:nowrap}</style>
               <div class="row" id="choice">Check <input type="checkbox"></div>
               <div class="row" id="range">Range <input type="range"></div>
               <div class="row" id="color">Color <input id="color-input" type="color"></div>
               <div class="row" id="asymmetric">Padding <input id="asym-input" type="checkbox"
                 style="box-sizing:border-box;width:22px;height:22px;
                 padding:1px 3px 5px 7px;border-style:solid;
                 border-width:2px 4px 3px 1px"></div>"#;
        let rendered = render(
            html,
            RenderOptions {
                system_fonts: true,
                ..RenderOptions::default()
            },
        )
        .unwrap();
        let row = |id| rendered.elements.iter().find(|e| e.id == id).unwrap();
        let strut = row("choice").computed["line-height"]
            .strip_suffix("px")
            .unwrap()
            .parse::<f64>()
            .unwrap();
        assert!((row("choice").height - strut).abs() < 1.0, "{:?}", row("choice"));
        assert!((row("range").height - strut).abs() < 1.5, "{:?}", row("range"));

        let mut doc = HtmlDocument::from_html(
            html,
            document_config(412, Some(fonts::context(true, &[], html)), None),
        );
        doc.resolve(0.0);
        doc.resolve(0.0);

        let color_row = doc.get_node(doc.get_element_by_id("color").unwrap()).unwrap();
        let color_inline = color_row
            .element_data()
            .unwrap()
            .inline_layout_data
            .as_ref()
            .unwrap();
        let color_line_height = color_inline.layout.lines().next().unwrap().metrics().line_height
            / color_inline.layout.scale();
        let color = rendered
            .elements
            .iter()
            .find(|e| e.id == "color-input")
            .unwrap();
        let expected_color_height = (color_line_height as f64).max(color.height);
        assert!(
            (row("color").height - expected_color_height).abs() <= 1.0,
            "color row: {:?}, control: {color:?}, expected={expected_color_height}",
            row("color")
        );
        // The native color control is reference-calibrated from every
        // vertical inset, unlike the bottom-inset baseline used for the
        // checkbox below. This is not a general CSS baseline identity.
        let color_node = doc
            .get_node(doc.get_element_by_id("color-input").unwrap())
            .unwrap();
        let color_used = color_node.unrounded_layout();
        let color_baseline = (color_used.size.height
            - color_used.padding.top
            - color_used.padding.bottom
            - color_used.border.top
            - color_used.border.bottom)
            .max(0.0);
        let line_baseline = color_inline.layout.lines().next().unwrap().metrics().baseline
            / color_inline.layout.scale();
        let expected_color_y = row("color").y + line_baseline as f64
            - color_baseline as f64;
        assert!(
            (color.y - expected_color_y).abs() < 0.05,
            "color baseline must use all resolved vertical insets: control={color:?}, expected y={expected_color_y}"
        );

        let asym_node = doc.get_node(doc.get_element_by_id("asym-input").unwrap()).unwrap();
        let asym_row_node = doc.get_node(doc.get_element_by_id("asymmetric").unwrap()).unwrap();
        let asym_inline = asym_row_node
            .element_data()
            .unwrap()
            .inline_layout_data
            .as_ref()
            .unwrap();
        let parent_baseline = asym_inline.layout.lines().next().unwrap().metrics().baseline
            / asym_inline.layout.scale();
        let used = asym_node.unrounded_layout();
        let input = rendered.elements.iter().find(|e| e.id == "asym-input").unwrap();
        let expected_y = row("asymmetric").y + parent_baseline as f64
            - (used.size.height - used.padding.bottom - used.border.bottom) as f64;
        assert!(
            (input.y - expected_y).abs() < 0.05,
            "asymmetric padding/border: input={input:?}, expected y={expected_y}"
        );
    }

    #[test]
    fn radio_ua_margins_do_not_depend_on_appearance() {
        let html = r#"<style>body{margin:0}</style>
            <input id="radio-auto" type="radio">
            <input id="radio-none" type="radio" style="appearance:none">
            <input id="checkbox-auto" type="checkbox">
            <input id="checkbox-none" type="checkbox" style="appearance:none">"#;
        let mut doc = HtmlDocument::from_html(
            html, document_config(412, None, None),
        );
        doc.resolve(0.0);
        doc.resolve(0.0);

        for id in ["radio-auto", "radio-none"] {
            let node = doc.get_element_by_id(id).unwrap();
            for (side, value) in [
                ("margin-top", "3px"), ("margin-right", "3px"),
                ("margin-bottom", "0px"), ("margin-left", "5px"),
            ] {
                assert_eq!(doc.resolved_style_value(node, side), value, "{id} {side}");
            }
        }
        for id in ["checkbox-auto", "checkbox-none"] {
            let node = doc.get_element_by_id(id).unwrap();
            for (side, value) in [
                ("margin-top", "3px"), ("margin-right", "3px"),
                ("margin-bottom", "3px"), ("margin-left", "4px"),
            ] {
                assert_eq!(doc.resolved_style_value(node, side), value, "{id} {side}");
            }
        }
    }

    #[test]
    fn appearance_none_radio_uses_reference_margin_box() {
        let rendered = render(
            include_str!("../../../fixtures/probes/form-appearance-none.html"),
            RenderOptions { system_fonts: true, ..RenderOptions::default() },
        ).unwrap();
        let radio = rendered.elements.iter().find(|e| e.id == "none-radio").unwrap();
        let rows: Vec<_> = rendered.elements.iter().filter(|e| e.class == "row").collect();
        assert_eq!(rows.len(), 3);
        let row = rows[1];
        // The 5px left UA margin yields 16px body inset + 5px.
        assert!((radio.x - 21.0).abs() < 0.05, "{radio:?}");
        // The 18px appearance:none border box has a top margin but no
        // bottom margin. Its containing line is 28px rather than 31px.
        assert!((row.height - 28.0).abs() < 0.05, "{row:?} {radio:?}");
    }

    #[test]
    fn form_labels_use_rounded_system_font_content_metrics() {
        for (name, html, label_count) in [
            ("form-text-inputs", include_str!("../../../fixtures/probes/form-text-inputs.html"), 6),
            ("form-choice-inputs", include_str!("../../../fixtures/probes/form-choice-inputs.html"), 4),
        ] {
            let rendered = render(
                html, RenderOptions { system_fonts: true, ..RenderOptions::default() },
            ).unwrap();
            let labels: Vec<_> = rendered.elements.iter().filter(|e| e.tag == "label").collect();
            assert_eq!(labels.len(), label_count, "{name}");
            for label in labels {
                assert_eq!(label.height, 17.0, "{name}: {label:?}");
            }
        }
    }

    #[test]
    fn choice_radio_positions_follow_default_margin_width() {
        let rendered = render(
            include_str!("../../../fixtures/probes/form-choice-inputs.html"),
            RenderOptions { system_fonts: true, ..RenderOptions::default() },
        ).unwrap();
        let labels: Vec<_> = rendered.elements.iter()
            .filter(|e| e.tag == "label").collect();
        assert_eq!(labels.len(), 4);
        let radio = rendered.elements.iter()
            .find(|e| e.id == "radio-off").unwrap();
        let label = labels[2];
        // The first radio is at the beginning of its label, so its start
        // margin is measurable without the preceding label's fractional text
        // advance or inter-element whitespace affecting its position.
        let inset = radio.x - label.x;
        assert!((inset - 5.0).abs() < 0.05,
            "label={label:?}, radio={radio:?}, inset={inset}");
    }

    #[test]
    fn normal_inline_font_metrics_round_individually() {
        let system_html = r#"<style>body{margin:0;font:16px Arial,sans-serif}</style>
            <div id="system-row"><label id="system-label">System text</label></div>"#;
        let system = render(
            system_html, RenderOptions { system_fonts: true, ..RenderOptions::default() },
        ).unwrap();
        fn get<'a>(r: &'a Rendered, id: &str) -> &'a Element {
            r.elements.iter().find(|e| e.id == id).unwrap()
        }
        // The system face's ascent and descent at 16px are 14.48px
        // and 3.39px: 14 + 3 = 17, not the fractional 17.87.
        assert_eq!(get(&system, "system-label").height, 17.0);
        // Existing normal line spacing already includes the rounded line gap.
        // Only the inline label content area changes in this patch.
        assert_eq!(get(&system, "system-row").height, 18.0);

        let loaded_html = format!(
            r#"<style>{}body{{margin:0;font:18px FixtureRoboto}}</style>
                <div id="loaded-row"><label id="loaded-label">Embedded text</label></div>"#,
            embedded_face(),
        );
        let loaded = render(&loaded_html, RenderOptions::default()).unwrap();
        // The loaded face stores 2146 ascent and 555 descent in 2048 UPEM.
        // At 18px, each metric rounds separately to 19 and 5px.
        assert_eq!(get(&loaded, "loaded-label").height, 24.0);
        assert_eq!(get(&loaded, "loaded-row").height, 24.0);
    }

    #[test]
    fn appearance_none_choices_keep_bottom_margin_baseline() {
        let html = include_str!("../../../fixtures/probes/form-appearance-none.html");
        let rendered = render(
            html, RenderOptions { system_fonts: true, ..RenderOptions::default() },
        ).unwrap();
        let rows: Vec<_> = rendered.elements.iter().filter(|e| e.class == "row").collect();
        assert_eq!(rows.len(), 3);

        let mut doc = HtmlDocument::from_html(
            html,
            document_config(412, Some(fonts::context(true, &[], html)), None),
        );
        doc.resolve(0.0);
        doc.resolve(0.0);

        for (i, id) in ["none-checkbox", "none-radio", "none-range"].iter().enumerate() {
            let input_id = doc.get_element_by_id(id).unwrap();
            assert_eq!(doc.resolved_style_value(input_id, "appearance"), "none");
            let input_node = doc.get_node(input_id).unwrap();
            let row_node = doc.get_node(input_node.parent.unwrap()).unwrap();
            let inline = row_node.element_data().unwrap().inline_layout_data.as_ref().unwrap();
            let line = inline.layout.lines().next().unwrap();
            let scale = inline.layout.scale();
            let baseline = line.metrics().baseline / scale;
            let expected_row_height = line.metrics().block_max_coord / scale;

            // With appearance:none, the atomic control has no native baseline.
            // Its bottom margin edge, not its content-box bottom, meets the
            // parent's resolved line baseline.
            let used = input_node.unrounded_layout();
            let input = rendered.elements.iter().find(|e| e.id == *id).unwrap();
            let row = rows[i];
            let expected_y = row.y + baseline as f64
                - (used.size.height + used.margin.bottom) as f64;
            assert!((input.y - expected_y).abs() < 0.05,
                "appearance:none control must sit on bottom margin edge: {id}: row={row:?}, input={input:?}, expected y={expected_y}");
            assert!((row.height - expected_row_height as f64).abs() < 0.05,
                "row must retain its resolved line-box height: {id}: row={row:?}, line={:?}",
                line.metrics());
        }

        // Appearance:none choice rows retain their margin-box descent below
        // the parent's text baseline.
        let strut = rows[0].computed["line-height"]
            .strip_suffix("px").unwrap().parse::<f64>().unwrap();
        assert!(rows[0].height > strut + 2.0, "checkbox row: {:?}", rows[0]);
        assert!(rows[1].height > strut + 2.0, "radio row: {:?}", rows[1]);
    }

    #[test]
    fn fieldset_without_legend_preserves_inline_text_and_input() {
        let html = r#"<style>
            body{margin:0;font:16px/24px Arial,sans-serif}
            fieldset{display:block;border:2px solid;padding:6px;width:340px;white-space:nowrap}
            </style>
            <fieldset id="plain">Name <input id="field" value="A"></fieldset>"#;
        let rendered = render(
            html,
            RenderOptions {
                system_fonts: true,
                ..RenderOptions::default()
            },
        )
        .unwrap();
        let mut doc = HtmlDocument::from_html(
            html,
            document_config(412, Some(fonts::context(true, &[], html)), None),
        );
        doc.resolve(0.0);
        doc.resolve(0.0);
        let fieldset = doc
            .get_node(doc.get_element_by_id("plain").unwrap())
            .unwrap();
        let inline = fieldset
            .element_data()
            .unwrap()
            .inline_layout_data
            .as_ref()
            .expect("legend-less all-inline fieldset must retain inline layout");
        assert_eq!(inline.layout.len(), 1);
        let line = inline.layout.lines().next().unwrap();
        assert!(line.runs().next().is_some(), "fieldset text run missing");
        assert!(
            line.items()
                .any(|item| matches!(item, parley::layout::PositionedLayoutItem::InlineBox(_))),
            "input must share the text line"
        );
        let used = fieldset.unrounded_layout();
        let expected_height = line.metrics().line_height / inline.layout.scale()
            + used.padding.top
            + used.padding.bottom
            + used.border.top
            + used.border.bottom;
        let box_rect = rendered.elements.iter().find(|e| e.id == "plain").unwrap();
        let input = rendered.elements.iter().find(|e| e.id == "field").unwrap();
        assert!((box_rect.height - expected_height as f64).abs() < 0.05);
        assert!(input.y >= box_rect.y && input.y + input.height <= box_rect.y + box_rect.height);
    }

    #[test]
    fn fieldset_with_legend_preserves_following_inline_text() {
        let html = r#"<style>
            body{margin:0;font:16px/24px Arial,sans-serif}
            fieldset{display:block;border:2px solid;padding:6px;width:340px}
            </style>
            <fieldset id="group"><legend id="caption">Details</legend>
              <span id="inline-text">Name</span> <input id="inline-input" value="A">
            </fieldset>"#;
        let rendered = render(
            html,
            RenderOptions {
                system_fonts: true,
                ..RenderOptions::default()
            },
        )
        .unwrap();
        let label = rendered
            .elements
            .iter()
            .find(|e| e.id == "inline-text")
            .unwrap();
        let input = rendered
            .elements
            .iter()
            .find(|e| e.id == "inline-input")
            .unwrap();
        let legend = rendered
            .elements
            .iter()
            .find(|e| e.id == "caption")
            .unwrap();
        assert!(label.width > 0.0 && label.height > 0.0);
        assert!(label.y >= legend.y + legend.height);
        assert!(
            input.y < label.y + label.height && label.y < input.y + input.height,
            "inline input must overlap the label's line box: label={label:?}, input={input:?}"
        );

        let mut doc = HtmlDocument::from_html(
            html,
            document_config(412, Some(fonts::context(true, &[], html)), None),
        );
        doc.resolve(0.0);
        doc.resolve(0.0);
        let root_id = doc.get_element_by_id("group").unwrap();
        let legend_id = doc.get_element_by_id("caption").unwrap();
        let mut pending = vec![root_id];
        let mut combined_line = false;
        while let Some(id) = pending.pop() {
            let node = doc.get_node(id).unwrap();
            if let Some(inline) = node
                .element_data()
                .and_then(|el| el.inline_layout_data.as_ref())
            {
                combined_line |= inline.layout.lines().any(|line| {
                    line.runs().next().is_some()
                        && line.items().any(|item| {
                            matches!(item, parley::layout::PositionedLayoutItem::InlineBox(_))
                        })
                });
            }
            // Inline text and controls following the legend are wrapped in
            // anonymous layout blocks, which are absent from DOM children.
            // Walk layout children first; fall back for nodes without them.
            let children = {
                let layout_children = node.layout_children.borrow();
                layout_children
                    .as_ref()
                    .filter(|children| !children.is_empty())
                    .map(|children| children.to_vec())
                    .unwrap_or_else(|| node.children.to_vec())
            };
            pending.extend(children.into_iter().filter(|id| *id != legend_id));
        }
        assert!(
            combined_line,
            "text and input must share an inline line after the legend"
        );
    }

    #[test]
    fn auto_height_fieldsets_in_flex_and_grid_match_block_layout() {
        let html = r#"<style>
            body { margin: 0; font: 16px/24px Arial, sans-serif; }
            fieldset {
                box-sizing: border-box;
                width: 280px;
                min-width: 0;
                margin: 0;
                border: 2px solid black;
                padding: 8px;
            }
            .flex { display: flex; flex-direction: column; align-items: flex-start; }
            .grid { display: grid; grid-template-columns: 280px; align-items: start; }
            .content { margin: 0; }
        </style>
        <div><fieldset id="block">
            <legend>Details</legend><div class="content">Text</div>
        </fieldset></div>
        <div class="flex"><fieldset id="flex">
            <legend>Details</legend><div class="content">Text</div>
        </fieldset></div>
        <div class="grid"><fieldset id="grid">
            <legend>Details</legend><div class="content">Text</div>
        </fieldset></div>"#;
        let rendered = render(
            html,
            RenderOptions {
                system_fonts: true,
                ..RenderOptions::default()
            },
        )
        .unwrap();
        let field = |id| {
            rendered
                .elements
                .iter()
                .find(|element| element.id == id)
                .unwrap()
        };
        let reference = field("block").height;
        for id in ["flex", "grid"] {
            assert!(
                (field(id).height - reference).abs() < 0.05,
                "auto-height fieldset inside {id} must match its block counterpart: block={:?}, child={:?}",
                field("block"),
                field(id)
            );
        }
    }

    #[test]
    fn fieldset_legend_min_height_survives_overlap_in_block_and_flex() {
        let html = r#"<style>
            body { margin: 0; font: 16px/24px Arial, sans-serif; }
            fieldset { border: 2px solid black; padding: 8px; min-width: 0; }
            .flex { display: flex; flex-direction: column; }
        </style>
        <fieldset id="min-block" style="min-height:100px;box-sizing:border-box">
            <legend>Details</legend>Text
        </fieldset>
        <div class="flex">
            <fieldset id="min-flex" style="min-height:100px;box-sizing:border-box">
                <legend>Details</legend>Text
            </fieldset>
        </div>"#;
        let rendered = render(
            html,
            RenderOptions {
                system_fonts: true,
                ..RenderOptions::default()
            },
        )
        .unwrap();
        for id in ["min-block", "min-flex"] {
            let field = rendered.elements.iter().find(|e| e.id == id).unwrap();
            assert!(
                (field.height - 100.0).abs() < 0.05,
                "legend overlap must not reduce min-height in {id}: {field:?}"
            );
        }
    }

    #[test]
    fn fieldset_legend_preserves_definite_and_flex_stretched_height() {
        let html = r#"<style>
            body { margin: 0; font: 16px/24px Arial, sans-serif; }
            fieldset { border: 2px solid black; padding: 8px; min-width: 0; }
            .flex { display: flex; align-items: stretch; height: 100px; }
        </style>
        <fieldset id="sized" style="height:100px;box-sizing:border-box">
            <legend>Details</legend>Text
        </fieldset>
        <div class="flex">
            <fieldset id="flexed" style="height:auto;box-sizing:border-box">
                <legend>Details</legend>Text
            </fieldset>
        </div>"#;
        let rendered = render(
            html,
            RenderOptions {
                system_fonts: true,
                ..RenderOptions::default()
            },
        )
        .unwrap();
        for id in ["sized", "flexed"] {
            let field = rendered.elements.iter().find(|e| e.id == id).unwrap();
            assert!(
                (field.height - 100.0).abs() < 0.05,
                "declared or flex-stretched fieldset height must remain definite: {field:?}"
            );
        }
    }

    #[test]
    fn floated_legend_does_not_shift_or_notch_fieldset_border() {
        let rendered = render(
            r#"<style>
                body { margin: 0; background: white; font: 16px/24px Arial, sans-serif; }
                fieldset { border: 4px solid black; padding: 12px; width: 280px; }
                legend { float: left; padding: 0 10px; }
            </style>
            <fieldset id="field"><legend id="floating">Floating</legend>
                <div>Body</div>
            </fieldset>"#,
            RenderOptions {
                system_fonts: true,
                ..RenderOptions::default()
            },
        )
        .unwrap();
        let field = rendered.elements.iter().find(|e| e.id == "field").unwrap();
        let legend = rendered
            .elements
            .iter()
            .find(|e| e.id == "floating")
            .unwrap();
        assert!(
            legend.y > field.y + 2.0,
            "floated legend must not be moved onto the fieldset border: {field:?} {legend:?}"
        );
        let x = (legend.x + 5.0).round() as usize;
        let y = (field.y + 1.0).round() as usize;
        let pixel = rendered.rgba[(y * rendered.width as usize + x) * 4];
        assert!(
            pixel < 80,
            "floated legend must not cut a notch in the top border: {field:?} {legend:?}"
        );
    }

    #[test]
    fn legend_straddles_fieldset_border_and_opens_top_notch() {
        let html = r#"<style>
                body{margin:0;background:white;font:16px/24px Arial,sans-serif}
                fieldset{border:4px solid black;padding:12px;width:280px}
                legend{padding:0 12px}
                p{margin:0}
               </style>
               <fieldset id="field"><legend id="legend">Details</legend>
                   <p id="content">Content</p></fieldset>"#;
        let rendered = render(
            html,
            RenderOptions {
                system_fonts: true,
                ..RenderOptions::default()
            },
        )
        .unwrap();
        let field = rendered.elements.iter().find(|e| e.id == "field").unwrap();
        let legend = rendered.elements.iter().find(|e| e.id == "legend").unwrap();
        let content = rendered
            .elements
            .iter()
            .find(|e| e.id == "content")
            .unwrap();
        assert!(
            (legend.y - field.y).abs() < 0.1,
            "legend must start at fieldset border-box top: {field:?} {legend:?}"
        );

        let mut doc = HtmlDocument::from_html(
            html,
            document_config(412, Some(fonts::context(true, &[], html)), None),
        );
        doc.resolve(0.0);
        doc.resolve(0.0);
        let field_node = doc
            .get_node(doc.get_element_by_id("field").unwrap())
            .unwrap();
        let legend_node = doc
            .get_node(doc.get_element_by_id("legend").unwrap())
            .unwrap();
        let field_box = field_node.unrounded_layout();
        let legend_box = legend_node.unrounded_layout();
        let legend_margin_height =
            legend_box.margin.top + legend_box.size.height + legend_box.margin.bottom;
        let reserved_top = field_box.border.top.max(legend_margin_height);
        let expected_content_top = field.y + (reserved_top + field_box.padding.top) as f64;
        assert!(
            (content.y - expected_content_top).abs() < 0.05,
            "fieldset content must follow max(legend height, top border) + padding: content={content:?}, expected={expected_content_top}"
        );
        let expected_field_height = reserved_top
            + field_box.padding.top
            + content.height as f32
            + field_box.padding.bottom
            + field_box.border.bottom;
        assert!(
            (field.height - expected_field_height as f64).abs() < 0.05,
            "fieldset must not double-count the block-start border: field={field:?}, expected={expected_field_height}"
        );

        let y = (legend.y + legend.height / 2.0).round() as usize;
        let pixel = |x: usize| rendered.rgba[(y * rendered.width as usize + x) * 4];
        let border_x = (field.x + 1.0).round() as usize;
        let notch_x = (legend.x + 5.0).round() as usize;
        assert!(pixel(border_x) < 80, "border must cross legend midpoint");
        assert!(
            pixel(notch_x) > 200,
            "border must leave transparent legend notch"
        );

        let outside_notch = (field.x + field.width - 10.0).round() as usize;
        let border_center = (legend.y + legend.height / 2.0).round() as usize;
        let above_border =
            (legend.y + legend.height / 2.0 - field_box.border.top as f64 / 2.0 - 1.0).round()
                as usize;
        let sample = |x: usize, y: usize| rendered.rgba[(y * rendered.width as usize + x) * 4];
        assert!(
            sample(outside_notch, border_center) < 80,
            "top border must be dark at the legend's block-axis centre"
        );
        assert!(
            sample(outside_notch, above_border) > 200,
            "top border must be light above half the border width"
        );

        // For b=4 and legend h=24, the correct stroke covers rows
        // [10, 14), not [12, 16). The midpoint sample alone misses
        // a border shifted down by b/2.
        let inside_top_edge = (legend.y + legend.height / 2.0 - field_box.border.top as f64 / 2.0
            + 1.0)
            .round() as usize;
        let below_border =
            (legend.y + legend.height / 2.0 + field_box.border.top as f64 / 2.0 + 1.0).round()
                as usize;
        assert!(
            sample(outside_notch, inside_top_edge) < 80,
            "stroke must already be dark inside the expected top edge"
        );
        assert!(
            sample(outside_notch, below_border) > 200,
            "stroke must be light immediately below the expected band"
        );
    }

    fn probe_font_positions(word: &str, size: f32) -> (f32, Vec<f32>, Vec<f32>) {
        // Register the unchanged fixture font as an in-memory face. A document
        // created without a resource provider cannot fetch @font-face data URLs.
        let html = format!(
            r#"<style>body{{margin:0;background:white}}
                #sample{{font:{size}px/26px Roboto;white-space:nowrap;
                width:max-content;color:black}}</style><div id="sample">{word}</div>"#
        );
        let font_bytes = vec![embedded_bytes()];
        let mut doc = HtmlDocument::from_html(
            &html, document_config(412, Some(fonts::context(false, &font_bytes, &html)), None),
        );
        doc.resolve(0.0);
        doc.resolve(0.0);

        let root = doc.get_element_by_id("sample").unwrap();
        // CSS box sizing ceils max-content widths, so read the text layout's
        // intrinsic width instead of the containing element's rounded width.
        let mut width = None;
        let mut positions = Vec::new();
        let mut advances = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let mut pending = vec![root];
        // Paint visits glyph runs from every laid-out inline context,
        // including anonymous descendants; a first line alone can be empty.
        while let Some(id) = pending.pop() {
            if !seen.insert(id) { continue; }
            let node = doc.get_node(id).unwrap();
            if let Some(inline) = node.element_data()
                .and_then(|data| data.inline_layout_data.as_ref())
            {
                let intrinsic = inline.layout.calculate_content_widths().max
                    / inline.layout.scale();
                for line in inline.layout.lines() {
                    for item in line.items() {
                        if let parley::layout::PositionedLayoutItem::GlyphRun(run) = item {
                            assert!((run.run().font_size() / inline.layout.scale() - size).abs() < 0.01,
                                "shaping must receive the fractional CSS size: {word}, {size}");
                            width = Some(intrinsic);
                            for glyph in run.positioned_glyphs() {
                                positions.push(glyph.x);
                                advances.push(glyph.advance);
                            }
                        }
                    }
                }
            }
            pending.extend(node.children.iter().copied());
            if let Some(layout_children) = node.layout_children.borrow().as_ref() {
                pending.extend(layout_children.iter().copied());
            }
        }
        assert_eq!(positions.len(), word.len(),
            "expected one painted glyph per test letter: {word}, size={size}");
        let origin = positions[0];
        for x in &mut positions { *x -= origin; }
        (width.expect("expected intrinsic width from a painted glyph run"),
            positions, advances)
    }

    #[test]
    fn hinted_probe_font_widths_and_painted_positions_share_advances() {
        for (word, expected, glyph_advances) in [
            ("North", 39.0, &[11., 9., 5., 5., 9.][..]),
            ("East", 31.0, &[9., 9., 8., 5.][..]),
            ("West", 35.0, &[14., 8., 8., 5.][..]),
        ] {
            let (width, x, advances) = probe_font_positions(word, 16.0);
            assert_eq!(width, expected, "{word}: intrinsic width");
            assert_eq!(advances.as_slice(), glyph_advances, "{word}: shaped advances");
            let pen = glyph_advances.iter().scan(0.0_f32, |pen, advance| {
                let here = *pen;
                *pen += advance;
                Some(here)
            }).collect::<Vec<_>>();
            assert_eq!(x, pen, "{word}: painted glyph x positions");
        }
        let html = format!(
            "<style>{}body{{margin:0;background:white}}p{{font:16px FixtureRoboto;color:black}}</style><p>North East West</p>",
            embedded_face()
        );
        assert!(dark_ink(&render(&html, RenderOptions::default()).unwrap()) > 30);
    }


    #[cfg(target_os = "linux")]
    #[test]
    fn integral_system_font_run_retains_fractional_advances() {
        // This face is resolved through the platform collection, not through
        // the in-memory @font-face / caller-provided registration paths.
        let html = r#"<style>body{margin:0}
            #sample{font:14px/20px 'Liberation Sans';white-space:nowrap;
            width:max-content}</style><div id="sample">Lake circuit</div>"#;
        let mut font_ctx = fonts::context(true, &[], html);
        let system_face_resolved = font_ctx.collection.family_id("Liberation Sans")
            .is_some_and(|family| {
                let mut query = font_ctx.collection.query(&mut font_ctx.source_cache);
                query.set_families(["Liberation Sans"]);
                query.first_available_font()
                    .is_some_and(|selected| selected.family.0 == family)
            });
        if !system_face_resolved {
            eprintln!("SKIP: Liberation Sans is not resolved by the host font collection");
            return;
        }
        let mut doc = HtmlDocument::from_html(
            html, document_config(412, Some(font_ctx), None),
        );
        doc.resolve(0.0);
        doc.resolve(0.0);
        let root = doc.get_element_by_id("sample").unwrap();
        let mut pending = vec![root];
        let mut seen = std::collections::HashSet::new();
        let mut widths = Vec::new();
        let mut advances = Vec::new();
        while let Some(id) = pending.pop() {
            if !seen.insert(id) { continue; }
            let node = doc.get_node(id).unwrap();
            if let Some(inline) = node.element_data()
                .and_then(|data| data.inline_layout_data.as_ref())
            {
                for line in inline.layout.lines() {
                    for item in line.items() {
                        if let parley::layout::PositionedLayoutItem::GlyphRun(run) = item {
                            widths.push(inline.layout.calculate_content_widths().max
                                / inline.layout.scale());
                            for glyph in run.positioned_glyphs() {
                                advances.push(glyph.advance);
                            }
                        }
                    }
                }
            }
            pending.extend(node.children.iter().copied());
            if let Some(children) = node.layout_children.borrow().as_ref() {
                pending.extend(children.iter().copied());
            }
        }
        assert_eq!(advances.len(), "Lake circuit".len(),
            "expected a real system-font glyph for each character");
        assert_eq!(widths.len(), 1, "expected a single unwrapped system-font run");
        assert!((widths[0] - 70.78125).abs() < 0.1,
            "system font must preserve the reference's fractional advance: {:?}", widths);
        assert!(advances.iter().any(|a| (a - a.round()).abs() > 0.05),
            "integral-size system run must keep fractional glyph advances: {advances:?}");
    }

    #[test]
    fn explicit_font_super_variant_retains_unquantized_advances() {
        fn advances_for(variant: &str) -> Vec<f32> {
            let html = format!(
                r#"<style>body{{margin:0}}
                #sample{{font:16px/24px Roboto;white-space:nowrap;
                font-variant-position:{variant}}}</style><div id="sample">East</div>"#
            );
            let bytes = vec![embedded_bytes()];
            let mut doc = HtmlDocument::from_html(
                &html, document_config(412, Some(fonts::context(false, &bytes, &html)), None),
            );
            doc.resolve(0.0);
            doc.resolve(0.0);
            let root = doc.get_element_by_id("sample").unwrap();
            let mut pending = vec![root];
            let mut seen = std::collections::HashSet::new();
            let mut advances = Vec::new();
            while let Some(id) = pending.pop() {
                if !seen.insert(id) { continue; }
                let node = doc.get_node(id).unwrap();
                if let Some(inline) = node.element_data()
                    .and_then(|data| data.inline_layout_data.as_ref())
                {
                    for line in inline.layout.lines() {
                        for item in line.items() {
                            if let parley::layout::PositionedLayoutItem::GlyphRun(run) = item {
                                advances.extend(run.positioned_glyphs().map(|g| g.advance));
                            }
                        }
                    }
                }
                pending.extend(node.children.iter().copied());
                if let Some(children) = node.layout_children.borrow().as_ref() {
                    pending.extend(children.iter().copied());
                }
            }
            advances
        }

        let normal = advances_for("normal");
        let super_run = advances_for("super");
        let nominal_hmtx = [1164.0_f32, 1114.0, 1056.0, 669.0];
        assert_eq!(normal, [9.0, 9.0, 8.0, 5.0],
            "ordinary explicit-font glyphs must remain grid-fitted");
        assert_eq!(super_run.len(), nominal_hmtx.len(),
            "superscript must shape every test character");
        for (i, (advance, nominal)) in super_run.iter().zip(nominal_hmtx).enumerate() {
            let unquantized = nominal * 16.0 / 2048.0;
            assert!((advance - unquantized).abs() < 0.02,
                "superscript glyph {i}: {advance} vs unquantized {unquantized}");
        }
        assert!(super_run.iter().any(|a| (a - a.round()).abs() > 0.1),
            "superscript must retain fractional advances: {super_run:?}");
    }

    #[test]
    fn integral_probe_font_size_17_rounds_glyph_advances() {
        let (width, positions, advances) = probe_font_positions("East", 17.0);
        assert_eq!(width, 34.0);
        assert_eq!(advances, [10.0, 9.0, 9.0, 6.0]);
        assert_eq!(positions, [0.0, 10.0, 19.0, 28.0]);
    }

    #[test]
    fn fractional_probe_font_size_keeps_unrounded_advances() {
        let (width, positions, advances) = probe_font_positions("East", 16.5);
        let unrounded = 4003.0_f32 * 16.5 / 2048.0;
        assert!((width - unrounded).abs() < 0.02,
            "fractional glyph metrics must be preserved: {width} vs {unrounded}");
        assert!((advances.iter().sum::<f32>() - unrounded).abs() < 0.02);
        assert!((positions[1] - 1164.0 * 16.5 / 2048.0).abs() < 0.02);
    }
}

#[cfg(test)]
mod line_clamp_line_count_tests {
    use super::*;

    #[test]
    fn real_inline_line_count_drives_the_visible_budget() {
        let html = r#"<style>body{margin:0;font:20px/24px monospace}
            #clamp{line-clamp:2}</style>
            <div id="clamp">A<br>B<br>C<br>D<br>E</div>"#;
        let normalized = line_clamp_css::normalize(html);
        let mut doc = HtmlDocument::from_html(
            &normalized,
            document_config(320, Some(fonts::context(true, &[], html)), None),
        );
        doc.resolve(0.0);
        doc.resolve(0.0);
        let node = doc.get_node(doc.get_element_by_id("clamp").unwrap()).unwrap();
        let inline = node.element_data().unwrap().inline_layout_data.as_ref().unwrap();
        assert_eq!(inline.layout.len(), 5, "the full five-line text must be shaped");
        assert_eq!(inline.clamp_visible_lines, Some(2), "only the first two lines paint");
        assert!(inline.clamp_marker.is_some(), "the end marker is independently shaped");
    }
}
