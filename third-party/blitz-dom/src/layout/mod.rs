// Modified from Blitz 92b11f1b: static controls, auto-table sizing and rendered fieldset legends.
//! Enable the dom to lay itself out using taffy
//!
//! In servo, style and layout happen together during traversal
//! However, in Blitz, we do a style pass then a layout pass.
//! This is slower, yes, but happens fast enough that it's not a huge issue.

use crate::node::{ComputedStyleRef, ImageData, NodeData, SpecialElementData};
use crate::{document::BaseDocument, dom_node_id, node::Node, taffy_node_id};
use markup5ever::{LocalName, local_name};
use std::cell::Ref;
use std::ops::{Deref, DerefMut};
use std::sync::Arc;
use style::Atom;
use style::values::computed::CSSPixelLength;
use style::values::computed::length_percentage::CalcLengthPercentage;
use stylo_taffy::TaffyStyloStyle;
use taffy::{
    AxisStaticEdge, AxisStaticPosition, BlockContext, BoxSizing, CoreStyle as _, DetailedLayoutInfo,
    FlexDirection, LayoutContainingBlock, LayoutPartialTree, MaybeMath as _, MaybeResolve,
    NodeId, OofCandidate, ResolveOrZero, RoundTree, RunMode, TraversePartialTree, TraverseTree,
    compute_block_layout,
    compute_cached_layout, compute_flexbox_layout, compute_grid_layout, compute_leaf_layout,
    compute_oof_layout, prelude::*,
};

pub(crate) mod construct;
pub(crate) mod damage;
pub(crate) mod inline;
pub(crate) mod list;
pub(crate) mod paint_tree;
pub(crate) mod replaced;
pub(crate) mod table;
pub(crate) mod text_transform;
#[cfg(feature = "writing-mode")]
pub(crate) mod writing_mode;

use self::replaced::{
    IntrinsicSizes, ReplacedContext, compute_replaced_layout, is_replaced_element,
};
use self::table::TableTreeWrapper;

/// The default object size for replaced elements
/// (https://drafts.csswg.org/css-images/#default-object-size).
const DEFAULT_OBJECT_SIZE: taffy::Size<f32> = taffy::Size {
    width: 300.0,
    height: 150.0,
};

/// The HTML count defaults apply to invalid and non-positive attributes.
fn form_count(value: Option<&str>, default: u32) -> f32 {
    value.and_then(|value| value.parse::<u32>().ok())
        .filter(|count| *count > 0)
        .map(|count| count.min(1000))
        .unwrap_or(default) as f32
}

#[cfg(test)]
mod form_size_tests {
    use super::form_count;
    #[test]
    fn count_attributes_have_html_defaults_and_finite_caps() {
        assert_eq!(form_count(None, 20), 20.0);
        assert_eq!(form_count(Some("0"), 20), 20.0);
        assert_eq!(form_count(Some("-1"), 20), 20.0);
        assert_eq!(form_count(Some("8"), 20), 8.0);
        assert_eq!(form_count(Some("1000000"), 20), 1000.0);
    }
}


/// The intrinsic dimensions and default object size for a replaced element
/// whose intrinsic dimensions are determined by its tag: an image with no
/// loaded resource has no intrinsic dimensions and a zero default object size;
/// a canvas has an intrinsic size and aspect ratio given by its width/height
/// attributes (defaulting to 300x150); other replaced elements (video, iframe,
/// embed) have no intrinsic dimensions and the 300x150 default object size.
fn tag_intrinsic_sizes(
    tag_name: &LocalName,
    attr_size: taffy::Size<Option<f32>>,
) -> (IntrinsicSizes, taffy::Size<f32>) {
    if *tag_name == local_name!("img") {
        // A not-yet-loaded image has no concrete object size.
        return (IntrinsicSizes::default(), taffy::Size::ZERO);
    }
    if *tag_name == local_name!("svg") {
        // Inline SVG participates in replaced layout even without width and
        // height attributes. It has the CSS default object size.
        return (IntrinsicSizes::default(), DEFAULT_OBJECT_SIZE);
    }
    if *tag_name == local_name!("canvas") {
        let width = attr_size.width.unwrap_or(300.0);
        let height = attr_size.height.unwrap_or(150.0);
        return (
            IntrinsicSizes {
                width: Some(width),
                height: Some(height),
                ratio: Some(width / height),
            },
            DEFAULT_OBJECT_SIZE,
        );
    }
    (IntrinsicSizes::default(), DEFAULT_OBJECT_SIZE)
}

pub(crate) fn resolve_calc_value(calc_ptr: *const (), parent_size: f32) -> f32 {
    let calc = unsafe { &*(calc_ptr as *const CalcLengthPercentage) };
    let result = calc.resolve(CSSPixelLength::new(parent_size));
    result.px()
}

/// Per-pass layout state: wraps the document for the duration of a layout pass and carries
/// the Taffy tree trait implementations. Derefs to [`BaseDocument`].
pub(crate) struct LayoutPassState<'doc> {
    doc: &'doc mut BaseDocument,
    /// The writing mode of the box whose layout algorithm is currently running (see
    /// `layout::writing_mode`). Child styles read during that algorithm are expressed in its axes.
    #[cfg(feature = "writing-mode")]
    pub(crate) layout_wm: stylo_taffy::WritingMode,
    /// The root element and the writing mode its algorithm runs in (see `BaseDocument::root_layout_wm`).
    #[cfg(feature = "writing-mode")]
    root_id: Option<crate::NodeId>,
    #[cfg(feature = "writing-mode")]
    root_wm: stylo_taffy::WritingMode,
    /// Whether the node whose layout algorithm is running aligns its children's `align-self` in its
    /// inline axis (a column flex container).
    #[cfg(feature = "writing-mode")]
    pub(crate) current_align_axis_is_inline: bool,
    /// The box currently being laid out in an orthogonal flow and its containing block's inline
    /// size, against which its percentage padding resolves (see `layout::writing_mode`).
    #[cfg(feature = "writing-mode")]
    pub(crate) orthogonal_percent_basis: Option<(crate::NodeId, f32)>,
}

impl<'doc> LayoutPassState<'doc> {
    /// The containing block's inline size when `dom_id` is the box currently laid out in an orthogonal
    /// flow: its percentage padding resolves against it (see `layout::writing_mode`).
    #[inline]
    fn orthogonal_percent_basis_of(&self, dom_id: crate::NodeId) -> Option<f32> {
        #[cfg(feature = "writing-mode")]
        {
            self.orthogonal_percent_basis
                .filter(|(node, _)| *node == dom_id)
                .map(|(_, basis)| basis)
        }
        #[cfg(not(feature = "writing-mode"))]
        {
            let _ = dom_id;
            None
        }
    }

    pub(crate) fn new(doc: &'doc mut BaseDocument) -> Self {
        #[cfg(feature = "writing-mode")]
        let root_id = doc.try_root_element().map(|root| root.id);
        Self {
            #[cfg(feature = "writing-mode")]
            root_wm: root_id.map_or(stylo_taffy::WritingMode::empty(), |id| {
                doc.root_layout_wm(id)
            }),
            doc,
            #[cfg(feature = "writing-mode")]
            root_id,
            #[cfg(feature = "writing-mode")]
            layout_wm: stylo_taffy::WritingMode::empty(),
            #[cfg(feature = "writing-mode")]
            current_align_axis_is_inline: false,
            #[cfg(feature = "writing-mode")]
            orthogonal_percent_basis: None,
        }
    }
}

impl Deref for LayoutPassState<'_> {
    type Target = BaseDocument;
    fn deref(&self) -> &BaseDocument {
        self.doc
    }
}

impl DerefMut for LayoutPassState<'_> {
    fn deref_mut(&mut self) -> &mut BaseDocument {
        self.doc
    }
}

impl LayoutPassState<'_> {
    fn node_from_id(&self, node_id: taffy::prelude::NodeId) -> &Node {
        &self.nodes[dom_node_id(node_id)]
    }
    fn node_from_id_mut(&mut self, node_id: taffy::prelude::NodeId) -> &mut Node {
        &mut self.nodes[dom_node_id(node_id)]
    }
}

impl LayoutPassState<'_> {
    /// Run the node's layout algorithm, then lay out the out-of-flow (absolute/fixed)
    /// boxes for which it is the containing block. Must be called inside the layout
    /// cache wrapper so that cache hits do not re-run the out-of-flow pass.
    fn compute_child_layout_internal(
        &mut self,
        node_id: NodeId,
        inputs: taffy::tree::LayoutInput,
        block_ctx: Option<&mut BlockContext<'_>>,
    ) -> taffy::tree::LayoutOutput {
        #[cfg(feature = "writing-mode")]
        {
            self.compute_child_layout_in_own_wm(node_id, inputs, block_ctx)
        }
        #[cfg(not(feature = "writing-mode"))]
        {
            self.compute_child_layout_in_current_wm(node_id, inputs, block_ctx)
        }
    }

    /// Lay out `node_id` in the writing mode of the currently running algorithm (its parent's)
    pub(crate) fn compute_child_layout_in_current_wm(
        &mut self,
        node_id: NodeId,
        inputs: taffy::tree::LayoutInput,
        block_ctx: Option<&mut BlockContext<'_>>,
    ) -> taffy::tree::LayoutOutput {
        let mut output = self.dispatch_child_layout(node_id, inputs, block_ctx);
        if inputs.run_mode == RunMode::PerformLayout {
            compute_oof_layout(self, node_id, &mut output);
        }
        output
    }

    /// The style of a node being laid out *by* the node whose algorithm is currently running,
    /// expressed in that node's writing mode (see `layout::writing_mode`).
    #[inline]
    pub(crate) fn child_layout_style<'a>(
        &self,
        node: &'a Node,
    ) -> stylo_taffy::TaffyStyloStyle<ComputedStyleRef<'a>> {
        #[cfg(feature = "writing-mode")]
        {
            let mut style = node.layout_style_in(self.layout_wm);
            style.align_axis_is_inline = self.current_align_axis_is_inline;
            style
        }
        #[cfg(not(feature = "writing-mode"))]
        {
            node.layout_style()
        }
    }

    fn dispatch_child_layout(
        &mut self,
        node_id: NodeId,
        inputs: taffy::tree::LayoutInput,
        block_ctx: Option<&mut BlockContext<'_>>,
    ) -> taffy::tree::LayoutOutput {
        let orthogonal_percent_basis = self.orthogonal_percent_basis_of(dom_node_id(node_id));
        let static_select = construct::use_static_select_editor(&*self.doc, dom_node_id(node_id));
        let listbox = self.doc.nodes[dom_node_id(node_id)].element_data()
            .is_some_and(|el| el.name.local == local_name!("select")
                && (el.attr(local_name!("multiple")).is_some()
                    || el.attr(local_name!("size"))
                        .and_then(|size| size.parse::<u32>().ok())
                        .is_some_and(|size| size > 1)));
        // Listboxes lay out their option children as rows. A multiple select
        // without a size attribute reserves four rows by default, in both
        // flex and normal flow, without overriding an author-set height.
        if listbox {
            let default_multiple = self.doc.nodes[dom_node_id(node_id)].element_data()
                .is_some_and(|el| el.attr(local_name!("multiple")).is_some()
                    && el.attr(local_name!("size")).is_none());
            let auto_height = self.nodes[dom_node_id(node_id)].layout_style()
                .size().height.is_auto();
            let mut result = compute_block_layout(self, node_id, inputs, None);
            if default_multiple && auto_height && inputs.known_dimensions.height.is_none() {
                // Default-size listbox calibration, not a general font metric.
                result.size.height = result.size.height.max(72.0);
                result.scrollable_overflow_rect.bottom =
                    result.scrollable_overflow_rect.bottom.max(result.size.height);
            }
            return result;
        }
        // Leave a legend-less all-inline fieldset on its normal inline-root
        // dispatch. Layout and painting share the exact rendered-legend
        // predicate, including layout-child membership.
        let rendered_legend = self.nodes[dom_node_id(node_id)].rendered_fieldset_legend();
        if let Some(legend_id) = rendered_legend {
            let (auto_height, border_top, inline_insets, min_height, max_height) = {
                let style = self.nodes[dom_node_id(node_id)].layout_style();
                let padding =
                    style.padding().resolve_or_zero(inputs.parent_size.width, resolve_calc_value);
                let border =
                    style.border().resolve_or_zero(inputs.parent_size.width, resolve_calc_value);
                let box_sizing_adjustment = if style.box_sizing() == BoxSizing::ContentBox {
                    (padding + border).sum_axes()
                } else {
                    Size::ZERO
                };
                let min_height = style
                    .min_size()
                    .maybe_resolve(inputs.parent_size, resolve_calc_value)
                    .maybe_add(box_sizing_adjustment)
                    .height;
                let max_height = style
                    .max_size()
                    .maybe_resolve(inputs.parent_size, resolve_calc_value)
                    .maybe_add(box_sizing_adjustment)
                    .height;
                (
                    style.size().height.is_auto(),
                    border.top,
                    padding.left + padding.right + border.left + border.right,
                    min_height,
                    max_height,
                )
            };
            let mut output = compute_block_layout(self, node_id, inputs, block_ctx);

            // Auto-height fieldsets need the same border/legend overlap in
            // intrinsic measurement and final layout. A parent may use the
            // measured size as a known height during PerformLayout (flex/grid);
            // never subtract it a second time from that definite dimension.
            let correct_auto_height = auto_height && inputs.known_dimensions.height.is_none();
            let should_measure_height = inputs.run_mode != RunMode::ComputeSize
                || inputs.axis != taffy::RequestedAxis::Horizontal;
            let legend_height = if inputs.run_mode == RunMode::PerformLayout {
                let legend = self.nodes[legend_id].unrounded_layout();
                legend.size.height + legend.margin.top + legend.margin.bottom
            } else if correct_auto_height && should_measure_height {
                // ComputeSize does not commit child layouts. Measure the
                // legend independently instead of reading a stale layout box.
                let content_width = (output.size.width - inline_insets).max(0.0);
                let margins = self.nodes[legend_id]
                    .layout_style()
                    .margin()
                    .resolve_or_zero(Some(content_width), resolve_calc_value);
                let measured = self.compute_child_layout(
                    taffy_node_id(legend_id),
                    taffy::LayoutInput {
                        run_mode: RunMode::ComputeSize,
                        axis: taffy::RequestedAxis::Vertical,
                        sizing_mode: taffy::SizingMode::InherentSize,
                        known_dimensions: Size::NONE,
                        parent_size: Size {
                            width: Some(content_width),
                            height: None,
                        },
                        available_space: Size {
                            width: taffy::AvailableSpace::Definite(content_width),
                            height: taffy::AvailableSpace::MaxContent,
                        },
                        vertical_margins_are_collapsible: taffy::Line::FALSE,
                        ..inputs
                    },
                );
                measured.size.height + margins.top + margins.bottom
            } else {
                0.0
            };

            // The fieldset's top region is max(legend margin height, border
            // width), not their sum. Keep this correction in both sizing
            // modes for content-sized heights, but not for declared or
            // parent-supplied heights.
            let overlap = legend_height.min(border_top).max(0.0);
            if inputs.run_mode == RunMode::PerformLayout {
                self.nodes[legend_id].unrounded_layout_mut().location.y = 0.0;
                // Taffy's direct layout children include anonymous blocks for
                // inline text and controls. DOM children include raw text nodes
                // with no layout box; moving those panics and misses the actual
                // anonymous block that must move with the following content.
                let layout_children: Vec<_> = self.nodes[dom_node_id(node_id)]
                    .layout_children
                    .borrow()
                    .as_ref()
                    .map(|children| children.iter().copied().collect())
                    .unwrap_or_default();
                for child_id in layout_children {
                    if child_id == legend_id {
                        continue;
                    }
                    let child = &self.nodes[child_id];
                    if child.is_out_of_flow()
                        || child
                            .primary_styles()
                            .is_some_and(|s| s.clone_float().is_floating())
                        || child.taffy_display() == Display::None
                    {
                        continue;
                    }
                    self.nodes[child_id].unrounded_layout_mut().location.y -= overlap;
                }
            }
            if correct_auto_height && should_measure_height {
                // Block layout has already applied min/max sizing before the
                // legend overlap is removed. Clamp the corrected outer height
                // again so auto fieldsets cannot fall below min-height or
                // exceed max-height in either sizing pass.
                output.size.height = (output.size.height - overlap)
                    .max(0.0)
                    .maybe_clamp(min_height, max_height);
                output.scrollable_overflow_rect.bottom =
                    (output.scrollable_overflow_rect.bottom - overlap).max(output.size.height);
            }
            return output;
        }
        let select_has_optgroup = self.doc.nodes[dom_node_id(node_id)].children.iter()
            .copied()
            .any(|child| self.doc.nodes[child].element_data()
                .is_some_and(|el| el.name.local == local_name!("optgroup")));
        let node = &mut self.nodes[dom_node_id(node_id)];

        let font_styles = node.primary_styles().map(|style| {
            use style::values::computed::font::LineHeight;

            let font_size = style.clone_font_size().used_size().px();
            let line_height = match style.clone_line_height() {
                LineHeight::Normal => font_size * 1.2,
                LineHeight::Number(num) => font_size * num.0,
                LineHeight::Length(value) => value.0.px(),
            };

            (font_size, line_height, matches!(style.clone_line_height(), LineHeight::Normal))
        });
        let font_size = font_styles.map(|s| s.0);
        let resolved_line_height = font_styles.map(|s| s.1);
        let normal_line_height = font_styles.is_some_and(|s| s.2);

        match &mut node.data {
            NodeData::Text(data) => {
                // With the new "inline context" architecture all text nodes should be wrapped in an "inline layout context"
                // and should therefore never be measured individually.
                #[cfg(feature = "tracing")]
                tracing::error!(
                    node_id = ?dom_node_id(node_id),
                    data = ?data,
                    "Tried to lay out text node individually",
                );

                #[cfg(not(feature = "tracing"))]
                let _ = data;

                taffy::LayoutOutput::HIDDEN
                // unreachable!();

                // compute_leaf_layout(inputs, &node.style, |known_dimensions, available_space| {
                //     let context = TextContext {
                //         text_content: &data.content.trim(),
                //         writing_mode: WritingMode::Horizontal,
                //     };
                //     let font_metrics = FontMetrics {
                //         char_width: 8.0,
                //         char_height: 16.0,
                //     };
                //     text_measure_function(
                //         known_dimensions,
                //         available_space,
                //         &context,
                //         &font_metrics,
                //     )
                // })
            }
            NodeData::Element(element_data) | NodeData::AnonymousBlock(element_data) => {
                // TODO: deduplicate with single-line text input
                if *element_data.name.local == *"textarea" {
                    let rows = form_count(element_data.attr(local_name!("rows")), 2);
                    let cols = form_count(element_data.attr(local_name!("cols")), 20);
                    let fs = font_size.unwrap_or(13.3333);
                    // The default small-control monospace cell is approximately 7px.
                    // 0.525 and the 13.333px branch are default-size calibration.
                    // Include the 15px editor/scrollbar allowance in HTML cols sizing.
                    // Custom font sizes retain the count-based font-relative width.
                    let intrinsic_width = if (fs - 13.3333).abs() < 0.1 {
                        cols * fs * 0.525 + 15.0
                    } else {
                        cols * fs * 0.60 + 2.0
                    };

                    let output = compute_leaf_layout(
                        inputs,
                        &node.layout_style(),
                        resolve_calc_value,
                        |_known_size, _available_space| taffy::Size {
                            width: intrinsic_width,
                            height: resolved_line_height.unwrap_or(16.0) * rows,
                        },
                    );
                    if inputs.run_mode == RunMode::PerformLayout {
                        let pb = {
                            let style = node.layout_style();
                            style
                                .padding()
                                .resolve_or_zero(inputs.parent_size.width, resolve_calc_value)
                                + style
                                    .border()
                                    .resolve_or_zero(inputs.parent_size.width, resolve_calc_value)
                        };
                        let content_width = (output.size.width - pb.horizontal_axis_sum()).max(0.0);
                        let doc = &mut *self.doc;
                        let scale = doc.viewport.scale();
                        let node = &mut doc.nodes[dom_node_id(node_id)];
                        if let Some(input) = node
                            .data
                            .downcast_element_mut()
                            .and_then(|el| el.text_input_data_mut())
                        {
                            input.editor.set_width(Some(content_width * scale));
                            input.editor.refresh_layout(
                                &mut doc.font_ctx.lock().unwrap(),
                                &mut doc.layout_ctx,
                            );
                        }
                    }
                    return output;
                }

                // Leaf intrinsic sizes are independent of the available line width.
                // Explicit CSS dimensions, min/max and padding/borders are still
                // resolved by the Taffy leaf algorithm.
                if *element_data.name.local == *"input" {
                    let kind = element_data.attr(local_name!("type")).unwrap_or("text");
                    if kind == "hidden" { return taffy::LayoutOutput::HIDDEN; }
                    // Inline form controls contribute intrinsic content size only;
                    // parent grid/flex algorithms and author dimensions remain
                    // authoritative when computing the used border box.
                    {
                        let text_width = element_data.text_input_data()
                            .and_then(|data| data.editor.try_layout())
                            .map(|layout| layout.full_width() / layout.scale())
                            .unwrap_or(0.0);
                        let fs = font_size.unwrap_or(13.3333);
                        let line = resolved_line_height.unwrap_or(fs * 1.2);
                        let small_line = if (fs - 13.3333).abs() < 0.1 {
                            fs * 1.125
                        } else { line };
                        // 129px range, 100.3px date, 0.67 text-cell factor and
                        // 13.333px branch calibrate default-size controls.
                        let intrinsic = match kind {
                            "checkbox" | "radio" => (13.0, 13.0),
                            "color" => (42.0, 21.0),
                            "range" => (129.0, 16.0),
                            "submit" | "reset" | "button" => (
                                text_width.max(16.0),
                                if normal_line_height { small_line } else { line },
                            ),
                            "date" => (100.3, 18.0),
                            _ => ((form_count(element_data.attr(local_name!("size")), 20)
                                * fs * 0.67 - 2.0).max(0.0).ceil(), small_line),
                        };
                        return compute_leaf_layout(
                            inputs, &node.layout_style(), resolve_calc_value,
                            |_known_size, _available_space| taffy::Size {
                                width: intrinsic.0, height: intrinsic.1,
                            },
                        );
                    }
                }
                if *element_data.name.local == *"select" && static_select {
                    let fs = font_size.unwrap_or(13.3333);
                    let line = if (fs - 13.3333).abs() < 0.1 {
                        fs * 1.125
                    } else {
                        resolved_line_height.unwrap_or(fs * 1.2)
                    };
                    let listbox = element_data.attr(local_name!("multiple")).is_some()
                        || element_data.attr(local_name!("size"))
                            .and_then(|s| s.parse::<u32>().ok()).is_some_and(|s| s > 1);
                    let default_rows = if listbox { 4 } else { 1 };
                    let rows = form_count(element_data.attr(local_name!("size")), default_rows);
                    let text_width = element_data.text_input_data()
                        .and_then(|data| data.editor.try_layout())
                        .map(|layout| layout.full_width() / layout.scale())
                        .unwrap_or(0.0);
                    // Intrinsic content dimensions are used only for auto
                    // sizing; explicit CSS sizes and flex stretch still win.
                    // 19px optgroup allowance is default-size calibration.
                    let intrinsic_content_width =
                        (text_width + if select_has_optgroup { 19.0 } else { 0.0 }).max(26.0);
                    let intrinsic_height = if listbox { 17.0 * rows } else { line };
                    let style = node.layout_style();
                    // A dropdown's auto intrinsic *border box* is pixel-snapped,
                    // not its text advance. Keep fractions for other inline boxes
                    // and for author-defined select widths. Resolve the control's
                    // own padding/borders before snapping, including in intrinsic
                    // measurement used by its parent.
                    let intrinsic_width = if style.size().width.is_auto() {
                        let padding = style.padding().resolve_or_zero(
                            inputs.parent_size.width, resolve_calc_value);
                        let border = style.border().resolve_or_zero(
                            inputs.parent_size.width, resolve_calc_value);
                        let insets = (padding + border).horizontal_axis_sum();
                        (intrinsic_content_width + insets).round() - insets
                    } else {
                        intrinsic_content_width
                    };
                    // With a preferred aspect ratio, an automatic minimum
                    // width still includes the selected option's text.
                    // min-width:0 disables this intrinsic minimum, and
                    // author-specified width remains authoritative.
                    let intrinsic_auto_min = style.aspect_ratio().is_some()
                        && style.size().width.is_auto()
                        && style.min_size().width.is_auto();
                    let content_insets = if intrinsic_auto_min {
                        let padding = style.padding().resolve_or_zero(
                            inputs.parent_size.width, resolve_calc_value);
                        let border = style.border().resolve_or_zero(
                            inputs.parent_size.width, resolve_calc_value);
                        (padding + border).horizontal_axis_sum()
                    } else { 0.0 };
                    let mut output = compute_leaf_layout(
                        inputs, &style, resolve_calc_value,
                        |_known_size, _available_space| taffy::Size {
                            width: intrinsic_width, height: intrinsic_height,
                        },
                    );
                    if intrinsic_auto_min {
                        let min_outer_width = intrinsic_width + content_insets;
                        if output.size.width < min_outer_width {
                            output.size.width = min_outer_width;
                            output.scrollable_overflow_rect.right =
                                output.scrollable_overflow_rect.right.max(min_outer_width);
                        }
                    }
                    return output;
                }

                if is_replaced_element(&element_data.name.local) {
                    // Width/height attributes are presentational hints mapped to the CSS
                    // width/height properties by
                    // `synthesize_presentational_hints_for_legacy_attributes`, so they
                    // are already part of the style. They are only read here for the
                    // elements whose attributes determine their *intrinsic* size
                    // (canvas, and custom widgets on canvas tags).
                    let attr_size = taffy::Size {
                        width: element_data
                            .attr(local_name!("width"))
                            .and_then(|val| val.parse::<f32>().ok()),
                        height: element_data
                            .attr(local_name!("height"))
                            .and_then(|val| val.parse::<f32>().ok()),
                    };

                    // Get the element's intrinsic dimensions and default object size
                    let (intrinsic_sizes, default_object_size) = match &element_data.special_data {
                        SpecialElementData::Image(image_data) => match &**image_data {
                            ImageData::Raster(image) => {
                                let (width, height) = (image.width as f32, image.height as f32);
                                (
                                    IntrinsicSizes {
                                        width: Some(width),
                                        height: Some(height),
                                        ratio: Some(width / height),
                                    },
                                    DEFAULT_OBJECT_SIZE,
                                )
                            }
                            #[cfg(feature = "svg")]
                            ImageData::Svg(svg) => {
                                let mut width = svg.intrinsic_width();
                                let mut height = svg.intrinsic_height();
                                // A natural ratio exists only with a viewBox or with both
                                // natural dimensions. A single SVG dimension does not
                                // establish a ratio from the resolved fallback viewport.
                                let ratio = svg.viewbox_aspect_ratio().or_else(|| {
                                    width.zip(height)
                                        .filter(|(_, height)| *height > 0.0)
                                        .map(|(width, height)| width / height)
                                });
                                // The parsed drawing viewport is not an intrinsic size.
                                // Missing dimensions fall back to the default object size.
                                (
                                    IntrinsicSizes {
                                        width,
                                        height,
                                        ratio,
                                    },
                                    DEFAULT_OBJECT_SIZE,
                                )
                            }
                            ImageData::None => (IntrinsicSizes::default(), taffy::Size::ZERO),
                        },
                        SpecialElementData::Canvas(_)
                        | SpecialElementData::SubDocument(_)
                        | SpecialElementData::None => {
                            let (mut sizes, default) =
                                tag_intrinsic_sizes(&element_data.name.local, attr_size);
                            if element_data.name.local == local_name!("svg") {
                                // Inline SVG has no image resource. Its viewBox supplies
                                // a ratio, but no intrinsic width or height.
                                sizes.ratio = element_data
                                    .attr(local_name!("viewBox"))
                                    .and_then(|value| {
                                        let mut parts = value.split(|c: char| {
                                            c == ',' || c.is_ascii_whitespace()
                                        }).filter(|part| !part.is_empty());
                                        let _x: f32 = parts.next()?.parse().ok()?;
                                        let _y: f32 = parts.next()?.parse().ok()?;
                                        let width: f32 = parts.next()?.parse().ok()?;
                                        let height: f32 = parts.next()?.parse().ok()?;
                                        (parts.next().is_none() && width > 0.0 && height > 0.0)
                                            .then_some(width / height)
                                    });
                            }
                            (sizes, default)
                        }
                        #[cfg(feature = "custom-widget")]
                        SpecialElementData::CustomWidget(widget_data) => {
                            let (fallback, default_object_size) =
                                tag_intrinsic_sizes(&element_data.name.local, attr_size);
                            // A canvas's content attributes determine its intrinsic size,
                            // overriding the widget-reported one; the widget-reported size
                            // in turn overrides the tag's fallback.
                            let attr_intrinsic =
                                if *element_data.name.local == local_name!("canvas") {
                                    attr_size
                                } else {
                                    taffy::Size::NONE
                                };
                            let attr_ratio = match (attr_intrinsic.width, attr_intrinsic.height) {
                                (Some(w), Some(h)) => Some(w / h),
                                _ => None,
                            };
                            let widget_sizes = widget_data.widget.intrinsic_sizes();
                            (
                                IntrinsicSizes {
                                    width: attr_intrinsic
                                        .width
                                        .or(widget_sizes.width)
                                        .or(fallback.width),
                                    height: attr_intrinsic
                                        .height
                                        .or(widget_sizes.height)
                                        .or(fallback.height),
                                    ratio: attr_ratio.or(widget_sizes.ratio).or(fallback.ratio),
                                },
                                default_object_size,
                            )
                        }
                        _ => unreachable!(),
                    };

                    #[cfg(feature = "writing-mode")]
                    let (intrinsic_sizes, default_object_size) =
                        if node.writing_mode().is_vertical() {
                            // Intrinsic sizes are physical; the element's algorithm runs in its own writing mode
                            (
                                crate::layout::replaced::IntrinsicSizes {
                                    width: intrinsic_sizes.height,
                                    height: intrinsic_sizes.width,
                                    ratio: intrinsic_sizes.ratio.map(|ratio| 1.0 / ratio),
                                },
                                default_object_size.transpose(),
                            )
                        } else {
                            (intrinsic_sizes, default_object_size)
                        };

                    let replaced_context = ReplacedContext {
                        intrinsic_sizes,
                        default_object_size,
                    };

                    let mut style = node.layout_style();
                    style.set_percent_basis(orthogonal_percent_basis);
                    return compute_replaced_layout(
                        inputs,
                        &style,
                        resolve_calc_value,
                        &replaced_context,
                    );
                }

                if node.flags.is_table_root() {
                    // The space available to an absolutely positioned table never exceeds
                    // the size of its containing block (less the table's margins), even
                    // when negative insets would otherwise add to it
                    // (https://drafts.csswg.org/css-tables-3/#abspos).
                    let mut inputs = inputs;
                    if self.nodes[dom_node_id(node_id)].is_out_of_flow() {
                        let margin = self.nodes[dom_node_id(node_id)]
                            .layout_style()
                            .margin()
                            .resolve_or_zero(inputs.parent_size.width, resolve_calc_value);
                        let max_available_space = inputs
                            .parent_size
                            .maybe_sub(margin.sum_axes())
                            .maybe_max(taffy::Size::ZERO);
                        inputs.available_space =
                            inputs.available_space.maybe_min(max_available_space);
                    }

                    let SpecialElementData::TableRoot(context) = &self.nodes[dom_node_id(node_id)]
                        .data
                        .downcast_element()
                        .unwrap()
                        .special_data
                    else {
                        panic!("Node marked as table root but doesn't have TableContext");
                    };
                    let context = Arc::clone(context);

                    let mut table_wrapper = TableTreeWrapper {
                        doc: self,
                        ctx: context,
                        proportional_style: None,
                    };
                    table_wrapper.prepare_proportional_auto_columns(inputs);
                    let mut output = compute_grid_layout(&mut table_wrapper, node_id, inputs);

                    // Absolutely positioned children take no part in the table grid. Their
                    // static position is the top-left of the table's content box.
                    let context = Arc::clone(&table_wrapper.ctx);
                    drop(table_wrapper);

                    // CSS 2.1 §17.4: captions participate in the table wrapper,
                    // not the cell grid. Lay them out as normal block boxes with
                    // their own margins, padding and borders.
                    if !context.captions.is_empty() {
                        use style::computed_values::caption_side::T as CaptionSide;

                        let grid_height = output.size.height;
                        let table_width = output.size.width;
                        let mut measured = Vec::new();
                        let mut top_height = 0.0_f32;
                        let mut bottom_height = 0.0_f32;
                        let mut wrapper_width = table_width;
                        for &caption_id in &context.captions {
                            let (margin, padding, border, bottom) = {
                                let node = &self.nodes[caption_id];
                                let style = node.layout_style();
                                (
                                    style.margin().resolve_or_zero(Some(table_width), resolve_calc_value),
                                    style.padding().resolve_or_zero(Some(table_width), resolve_calc_value),
                                    style.border().resolve_or_zero(Some(table_width), resolve_calc_value),
                                    node.primary_styles().is_some_and(|s| {
                                        s.clone_caption_side() == CaptionSide::Bottom
                                    }),
                                )
                            };
                            let available_width =
                                (table_width - margin.left - margin.right).max(0.0);
                            let size = self.compute_child_layout(
                                taffy_node_id(caption_id),
                                taffy::LayoutInput {
                                    known_dimensions: taffy::Size::NONE,
                                    known_dimensions_are_definite: taffy::Size {
                                        width: false, height: false,
                                    },
                                    parent_size: taffy::Size {
                                        width: Some(table_width), height: None,
                                    },
                                    available_space: taffy::Size {
                                        width: taffy::AvailableSpace::Definite(available_width),
                                        height: taffy::AvailableSpace::MaxContent,
                                    },
                                    vertical_margins_are_collapsible: taffy::Line::FALSE,
                                    ..inputs
                                },
                            ).size;
                            let outer_height = margin.top + size.height + margin.bottom;
                            if bottom { bottom_height += outer_height; }
                            else { top_height += outer_height; }
                            wrapper_width = wrapper_width.max(
                                size.width + margin.left + margin.right,
                            );
                            measured.push((caption_id, size, margin, padding, border, bottom));
                        }

                        if inputs.run_mode == RunMode::PerformLayout {
                            let mut above = 0.0_f32;
                            let mut below = 0.0_f32;
                            for (id, size, margin, padding, border, bottom) in measured {
                                let stack = if bottom { &mut below } else { &mut above };
                                let mut layout = taffy::Layout::new();
                                layout.size = size;
                                layout.margin = margin;
                                layout.padding = padding;
                                layout.border = border;
                                layout.location = taffy::Point {
                                    x: margin.left,
                                    y: if bottom {
                                        top_height + grid_height + *stack + margin.top
                                    } else {
                                        *stack + margin.top
                                    },
                                };
                                layout.scrollable_overflow_rect = taffy::Rect {
                                    left: 0.0, top: 0.0,
                                    right: size.width, bottom: size.height,
                                };
                                *self.nodes[id].unrounded_layout_mut() = layout;
                                *stack += margin.top + size.height + margin.bottom;
                            }
                            // The grid is translated below the top caption.
                            // Adjust the detailed tracks too: table backgrounds
                            // and collapsed-border paint use those positions.
                            for cell in &context.cells {
                                self.nodes[cell.node_id].unrounded_layout_mut().location.y += top_height;
                            }
                            if let Some(info) = &mut *context.computed_grid_info.borrow_mut() {
                                for line in &mut info.rows.positions {
                                    line.start += top_height;
                                    line.end += top_height;
                                }
                            }
                        }
                        output.size.height += top_height + bottom_height;
                        output.size.width = wrapper_width;
                    }

                    if inputs.run_mode == RunMode::PerformLayout && !context.oof_children.is_empty()
                    {
                        let style = &context.style;
                        let parent_width = inputs.parent_size.width;
                        let padding = style
                            .padding
                            .resolve_or_zero(parent_width, resolve_calc_value);
                        let border = style
                            .border
                            .resolve_or_zero(parent_width, resolve_calc_value);
                        let static_position = taffy::Point {
                            x: padding.left + border.left,
                            y: padding.top + border.top,
                        }
                        .map(|pos| AxisStaticPosition::from_edge(pos, AxisStaticEdge::Start));
                        let in_flow_count = context.cells.len();
                        for (idx, (child_id, position)) in context.oof_children.iter().enumerate() {
                            output.oof_candidates.push(OofCandidate {
                                node: taffy_node_id(*child_id),
                                order: (in_flow_count + idx) as u32,
                                position: *position,
                                static_position,
                            });
                        }
                    }

                    // HACK: Cap scrollable overflow at node size to prevent scrolling
                    output.scrollable_overflow_rect.left = 0.0;
                    output.scrollable_overflow_rect.top = 0.0;
                    output.scrollable_overflow_rect.right =
                        output.scrollable_overflow_rect.right.min(output.size.width);
                    output.scrollable_overflow_rect.bottom = output
                        .scrollable_overflow_rect
                        .bottom
                        .min(output.size.height);

                    return output;
                }

                if node.flags.is_inline_root() {
                    return self.compute_inline_layout(dom_node_id(node_id), inputs, block_ctx);
                }

                // The default CSS file will set
                match node.taffy_display() {
                    Display::Block => self.compute_clamped_block(node_id, inputs, block_ctx),
                    Display::FlowRoot => self.compute_clamped_block(node_id, inputs, None),
                    Display::Flex => {
                        #[cfg(feature = "writing-mode")]
                        {
                            self.current_align_axis_is_inline = node.is_column_flex_container();
                        }
                        compute_flexbox_layout(self, node_id, inputs)
                    }
                    Display::Grid => compute_grid_layout(self, node_id, inputs),
                    Display::None => taffy::LayoutOutput::HIDDEN,
                }
            }
            NodeData::Document(_) => compute_block_layout(self, node_id, inputs, None),

            _ => taffy::LayoutOutput::HIDDEN,
        }
    }
}

impl LayoutPassState<'_> {
    // A block clamp has one line budget shared by its in-flow descendants.
    // Collect line endpoints in layout order after children have been placed.
    fn collect_block_lines(
        &self,
        node_id: crate::NodeId,
        y: f32,
        lines: &mut Vec<(crate::NodeId, usize, f32)>,
    ) {
        let children: Vec<_> = self.nodes[node_id].layout_children.borrow()
            .as_ref().map(|c| c.iter().copied().collect()).unwrap_or_default();
        for child_id in children {
            let child = &self.nodes[child_id];
            if child.is_out_of_flow() || child.primary_styles()
                .is_some_and(|s| s.clone_float().is_floating())
            {
                continue;
            }
            // An independent formatting context owns its lines. Scrollable
            // overflow (including hidden) establishes a BFC; flow-root,
            // flex and grid descendants also delimit the outer line budget.
            if matches!(child.taffy_display(), Display::FlowRoot | Display::Flex | Display::Grid)
                || child.primary_styles().is_some_and(|style| {
                    style.get_box().overflow_x.is_scrollable()
                        || style.get_box().overflow_y.is_scrollable()
                })
            {
                continue;
            }
            let layout = child.unrounded_layout();
            let offset = y + layout.location.y;
            if child.flags.is_inline_root() {
                if let Some(text) = child.element_data()
                    .and_then(|el| el.inline_layout_data.as_ref())
                {
                    let content_top = offset + layout.border.top + layout.padding.top
                        + text.block_offset;
                    let scale = self.viewport.scale();
                    let count = text.layout.len();
                    for (i, line) in text.layout.lines().enumerate() {
                        if i + 1 == count && i > 0 && line.text_range().is_empty()
                            && line.items().next().is_none()
                            && text.layout.get(i - 1).is_some_and(|prev|
                                prev.break_reason() == parley::BreakReason::Explicit)
                        {
                            break;
                        }
                        lines.push((child_id, i, content_top + line.metrics().block_max_coord / scale));
                    }
                }
            } else {
                self.collect_block_lines(child_id, offset, lines);
            }
        }
    }

    fn compute_clamped_block(
        &mut self,
        node_id: NodeId,
        inputs: taffy::tree::LayoutInput,
        block_ctx: Option<&mut BlockContext<'_>>,
    ) -> taffy::LayoutOutput {
        let root = dom_node_id(node_id);
        let spec = crate::line_clamp::for_node(&self.nodes[root]);
        // Resolve insets from the current containing block, not from the
        // root's stored layout (which can still describe a previous pass).
        let (auto_height, bottom_inset, min_height) = spec.map(|_| {
            let style = self.nodes[root].layout_style();
            let padding = style.padding()
                .resolve_or_zero(inputs.parent_size.width, resolve_calc_value);
            let border = style.border()
                .resolve_or_zero(inputs.parent_size.width, resolve_calc_value);
            let box_sizing_adjustment = if style.box_sizing() == BoxSizing::ContentBox {
                (padding + border).sum_axes()
            } else {
                Size::ZERO
            };
            let min_height = style.min_size()
                .maybe_resolve(inputs.parent_size, resolve_calc_value)
                .maybe_add(box_sizing_adjustment)
                .height
                .unwrap_or(0.0);
            (
                style.size().height.is_auto() && inputs.known_dimensions.height.is_none(),
                padding.bottom + border.bottom,
                min_height,
            )
        }).unwrap_or((false, 0.0, 0.0));
        let mut output = compute_block_layout(self, node_id, inputs, block_ctx);
        let Some(spec) = spec else { return output };
        // Intrinsic passes do not guarantee positioned child line boxes.
        if inputs.run_mode != RunMode::PerformLayout || !auto_height { return output; }
        let mut lines = Vec::new();
        self.collect_block_lines(root, 0.0, &mut lines);
        if lines.len() <= spec.lines { return output; }
        let (last_root, last_index, bottom) = lines[spec.lines - 1];
        // The child line endpoints include the root's top padding/border.
        // Only the lower inset remains to be added to the border-box height.
        let bottom = bottom + bottom_inset;
        output.size.height = output.size.height.min(bottom.max(0.0)).max(min_height);
        if spec.ellipsis {
            if let Some(mut inline) = self.nodes[last_root].element_data_mut()
                .and_then(|el| el.take_inline_layout())
            {
                self.install_clamp_marker(last_root, &mut inline, last_index + 1);
                self.nodes[last_root].element_data_mut().unwrap().inline_layout_data = Some(inline);
            }
        }
        output
    }
}

impl TraversePartialTree for LayoutPassState<'_> {
    type ChildIter<'a>
        = RefCellChildIter<'a>
    where
        Self: 'a;

    fn child_ids(&self, node_id: NodeId) -> Self::ChildIter<'_> {
        let layout_children = self.node_from_id(node_id).layout_children.borrow(); //.unwrap().as_ref();
        RefCellChildIter::new(Ref::map(layout_children, |children| {
            children.as_ref().map(|c| c.as_slice()).unwrap_or(&[])
        }))
    }

    fn child_count(&self, node_id: NodeId) -> usize {
        self.node_from_id(node_id)
            .layout_children
            .borrow()
            .as_ref()
            .map(|c| c.len())
            .unwrap_or(0)
    }

    fn get_child_id(&self, node_id: NodeId, index: usize) -> NodeId {
        taffy_node_id(
            self.node_from_id(node_id)
                .layout_children
                .borrow()
                .as_ref()
                .unwrap()[index],
        )
    }
}
impl TraverseTree for LayoutPassState<'_> {}

impl LayoutPartialTree for LayoutPassState<'_> {
    type CoreContainerStyle<'a>
        = TaffyStyloStyle<ComputedStyleRef<'a>>
    where
        Self: 'a;

    type CustomIdent = Atom;

    fn get_core_container_style(&self, node_id: NodeId) -> Self::CoreContainerStyle<'_> {
        #[cfg(feature = "writing-mode")]
        {
            // Container styles are read by the node's own algorithm (or, for the containing block
            // of an out-of-flow box, by that box's algorithm) and are expressed in the node's writing mode.
            let dom_id = dom_node_id(node_id);
            let mut style = self.nodes[dom_id].layout_style_in(self.layout_wm_of(dom_id));
            style.set_percent_basis(self.orthogonal_percent_basis_of(dom_id));
            style
        }
        #[cfg(not(feature = "writing-mode"))]
        {
            self.node_from_id(node_id).layout_style()
        }
    }

    fn set_unrounded_layout(&mut self, node_id: NodeId, layout: &Layout) {
        *self.node_from_id_mut(node_id).unrounded_layout_mut() = *layout;
    }

    fn resolve_calc_value(&self, calc_ptr: *const (), parent_size: f32) -> f32 {
        resolve_calc_value(calc_ptr, parent_size)
    }

    #[inline(always)]
    fn compute_child_layout(
        &mut self,
        node_id: NodeId,
        inputs: taffy::LayoutInput,
    ) -> taffy::LayoutOutput {
        compute_cached_layout(self, node_id, inputs, |tree, node_id, inputs| {
            tree.compute_child_layout_internal(node_id, inputs, None)
        })
    }
}

impl LayoutContainingBlock for LayoutPassState<'_> {
    type OofItemStyle<'a>
        = TaffyStyloStyle<ComputedStyleRef<'a>>
    where
        Self: 'a;

    fn get_oof_item_style(&self, node_id: NodeId) -> Self::OofItemStyle<'_> {
        self.child_layout_style(self.node_from_id(node_id))
    }

    fn clear_hoisted_children(&mut self, node_id: NodeId) {
        let containing_block = dom_node_id(node_id);
        let mut hoisted = self.node_from_id(node_id).hoisted_children.borrow_mut();
        for &hoisted_id in hoisted.iter() {
            // A box which has since been claimed by another containing block
            // must keep pointing at that one.
            if let Some(node) = self.nodes.get(hoisted_id) {
                if node.oof_containing_block.get() == Some(containing_block) {
                    node.oof_containing_block.set(None);
                }
            }
        }
        hoisted.clear();
    }

    fn add_hoisted_children(&mut self, node_id: NodeId, hoisted: &[NodeId]) {
        let containing_block = dom_node_id(node_id);
        let node = self.node_from_id(node_id);
        node.hoisted_children
            .borrow_mut()
            .extend(hoisted.iter().copied().map(dom_node_id));
        for &hoisted_id in hoisted {
            self.node_from_id(hoisted_id)
                .oof_containing_block
                .set(Some(containing_block));
        }
    }

    fn get_detailed_layout_info(&self, node_id: NodeId) -> &DetailedLayoutInfo<Atom> {
        self.node_from_id(node_id)
            .element_data()
            .map(|element| &element.detailed_layout_info)
            .unwrap_or(&DetailedLayoutInfo::None)
    }
}

impl taffy::CacheTree for LayoutPassState<'_> {
    #[inline]
    fn cache_get(
        &mut self,
        node_id: NodeId,
        inputs: &taffy::LayoutInput,
    ) -> Option<taffy::LayoutOutput> {
        self.node_from_id_mut(node_id)
            .try_layout_data_mut()?
            .cache
            .get(inputs)
    }

    #[inline]
    fn cache_store(
        &mut self,
        node_id: NodeId,
        inputs: &taffy::LayoutInput,
        layout_output: taffy::LayoutOutput,
    ) {
        self.node_from_id_mut(node_id)
            .cache_mut()
            .store(inputs, layout_output);
    }

    #[inline]
    fn cache_clear(&mut self, node_id: NodeId) {
        self.node_from_id_mut(node_id).clear_layout_cache();
    }
}

impl taffy::LayoutBlockContainer for LayoutPassState<'_> {
    type BlockContainerStyle<'a>
        = TaffyStyloStyle<ComputedStyleRef<'a>>
    where
        Self: 'a;

    type BlockItemStyle<'a>
        = TaffyStyloStyle<ComputedStyleRef<'a>>
    where
        Self: 'a;

    fn get_block_container_style(&self, node_id: NodeId) -> Self::BlockContainerStyle<'_> {
        self.get_core_container_style(node_id)
    }

    fn get_block_child_style(&self, child_node_id: NodeId) -> Self::BlockItemStyle<'_> {
        self.child_layout_style(self.node_from_id(child_node_id))
    }

    #[inline(always)]
    fn compute_block_child_layout(
        &mut self,
        node_id: NodeId,
        inputs: taffy::LayoutInput,
        block_ctx: Option<&mut BlockContext<'_>>,
    ) -> taffy::LayoutOutput {
        compute_cached_layout(self, node_id, inputs, |tree, node_id, inputs| {
            tree.compute_child_layout_internal(node_id, inputs, block_ctx)
        })
    }
}

impl taffy::LayoutFlexboxContainer for LayoutPassState<'_> {
    type FlexboxContainerStyle<'a>
        = TaffyStyloStyle<ComputedStyleRef<'a>>
    where
        Self: 'a;

    type FlexboxItemStyle<'a>
        = TaffyStyloStyle<ComputedStyleRef<'a>>
    where
        Self: 'a;

    fn get_flexbox_container_style(&self, node_id: NodeId) -> Self::FlexboxContainerStyle<'_> {
        self.get_core_container_style(node_id)
    }

    fn get_flexbox_child_style(&self, child_node_id: NodeId) -> Self::FlexboxItemStyle<'_> {
        self.child_layout_style(self.node_from_id(child_node_id))
    }
}

impl taffy::LayoutGridContainer for LayoutPassState<'_> {
    type GridContainerStyle<'a>
        = TaffyStyloStyle<ComputedStyleRef<'a>>
    where
        Self: 'a;

    type GridItemStyle<'a>
        = TaffyStyloStyle<ComputedStyleRef<'a>>
    where
        Self: 'a;

    fn get_grid_container_style(&self, node_id: NodeId) -> Self::GridContainerStyle<'_> {
        self.get_core_container_style(node_id)
    }

    fn get_grid_child_style(&self, child_node_id: NodeId) -> Self::GridItemStyle<'_> {
        self.child_layout_style(self.node_from_id(child_node_id))
    }

    fn set_detailed_grid_info(
        &mut self,
        node_id: NodeId,
        detailed_grid_info: taffy::DetailedGridInfo<Atom>,
    ) {
        let node = self.node_from_id_mut(node_id);
        if let Some(element) = node.element_data_mut() {
            element.detailed_layout_info = DetailedLayoutInfo::Grid(Box::new(detailed_grid_info));
        }
    }
}

impl RoundTree for LayoutPassState<'_> {
    fn get_unrounded_layout(&self, node_id: NodeId) -> Layout {
        *self.node_from_id(node_id).unrounded_layout()
    }

    fn set_final_layout(&mut self, node_id: NodeId, layout: &Layout) {
        *self.node_from_id_mut(node_id).final_layout_mut() = *layout;
    }

    fn is_out_of_flow(&self, node_id: NodeId) -> bool {
        self.node_from_id(node_id).is_out_of_flow()
    }

    fn hoisted_child_count(&self, node_id: NodeId) -> usize {
        self.node_from_id(node_id).hoisted_children.borrow().len()
    }

    fn get_hoisted_child_id(&self, node_id: NodeId, index: usize) -> NodeId {
        taffy_node_id(self.node_from_id(node_id).hoisted_children.borrow()[index])
    }
}

pub(crate) struct TaffyDebugTree<'doc>(pub(crate) &'doc BaseDocument);

impl TaffyDebugTree<'_> {
    fn node_from_id(&self, node_id: NodeId) -> &Node {
        &self.0.nodes[dom_node_id(node_id)]
    }
}

impl TraversePartialTree for TaffyDebugTree<'_> {
    type ChildIter<'a>
        = RefCellChildIter<'a>
    where
        Self: 'a;

    fn child_ids(&self, node_id: NodeId) -> Self::ChildIter<'_> {
        let layout_children = self.node_from_id(node_id).layout_children.borrow();
        RefCellChildIter::new(Ref::map(layout_children, |children| {
            children.as_ref().map(|c| c.as_slice()).unwrap_or(&[])
        }))
    }

    fn child_count(&self, node_id: NodeId) -> usize {
        self.node_from_id(node_id)
            .layout_children
            .borrow()
            .as_ref()
            .map(|c| c.len())
            .unwrap_or(0)
    }

    fn get_child_id(&self, node_id: NodeId, index: usize) -> NodeId {
        taffy_node_id(
            self.node_from_id(node_id)
                .layout_children
                .borrow()
                .as_ref()
                .unwrap()[index],
        )
    }
}

impl TraverseTree for TaffyDebugTree<'_> {}

impl PrintTree for TaffyDebugTree<'_> {
    fn get_debug_label(&self, node_id: NodeId) -> &'static str {
        let node = &self.node_from_id(node_id);

        match node.data {
            NodeData::Document(_) => "DOCUMENT",
            // NodeData::Doctype { .. } => return "DOCTYPE",
            NodeData::Text { .. } => node.node_debug_str().leak(),
            NodeData::Comment { .. } => "COMMENT",
            NodeData::AnonymousBlock(_) => "ANONYMOUS BLOCK",
            NodeData::Element(_) => {
                let style = node.layout_style();
                let display = match node.taffy_display() {
                    Display::Flex => match taffy::FlexboxContainerStyle::flex_direction(&style) {
                        FlexDirection::Row | FlexDirection::RowReverse => "FLEX ROW",
                        FlexDirection::Column | FlexDirection::ColumnReverse => "FLEX COL",
                    },
                    Display::Grid => "GRID",
                    Display::Block => "BLOCK",
                    Display::FlowRoot => "FLOW ROOT",
                    Display::None => "NONE",
                };
                format!("{} ({})", node.node_debug_str(), display).leak()
            } // NodeData::ProcessingInstruction { .. } => return "PROCESSING INSTRUCTION",
        }
    }

    fn get_final_layout(&self, node_id: NodeId) -> Layout {
        *self.node_from_id(node_id).final_layout()
    }
}

// pub struct ChildIter<'a>(std::slice::Iter<'a, usize>);
// impl<'a> Iterator for ChildIter<'a> {
//     type Item = NodeId;
//     fn next(&mut self) -> Option<Self::Item> {
//         self.0.next().copied().map(NodeId::from)
//     }
// }

pub struct RefCellChildIter<'a> {
    items: Ref<'a, [crate::NodeId]>,
    idx: usize,
}
impl<'a> RefCellChildIter<'a> {
    fn new(items: Ref<'a, [crate::NodeId]>) -> RefCellChildIter<'a> {
        RefCellChildIter { items, idx: 0 }
    }
}

impl Iterator for RefCellChildIter<'_> {
    type Item = NodeId;
    fn next(&mut self) -> Option<Self::Item> {
        self.items.get(self.idx).map(|id| {
            self.idx += 1;
            taffy_node_id(*id)
        })
    }
}
