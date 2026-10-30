// Modified from Blitz 92b11f1b: form-control baselines, inline edges and atomic whitespace line boxes.
use blitz_traits::node_id::NodeId;
use parley::{AlignmentOptions, BreakReason, IndentOptions};
use style::values::specified::box_::{DisplayInside, DisplayOutside};
use style::values::{
    computed::{BaselineSource, CSSPixelLength, Contain},
    generics::text::GenericTextIndent,
};
use taffy::{
    AvailableSpace, AxisStaticPosition, BlockContainerStyle, BlockContext, BlockFormattingContext,
    BoxSizing, CollapsibleMarginSet, CompactLength, CoreStyle as _, Direction, LayoutInput,
    LayoutOutput, LayoutPartialTree as _, MaybeMath as _, MaybeResolve as _, OofCandidate,
    OofCandidates, OofItemStyle, OofPositioningArea, Overflow, Point, RequestedAxis,
    ResolveOrZero as _, RunMode, Size, SizingMode,
};

#[cfg(feature = "floats")]
use parley::YieldData;
#[cfg(feature = "floats")]
use taffy::{BlockItemStyle as _, Clear, Float, prelude::TaffyMaxContent};

use stylo_taffy::StyleFlags;

use super::{construct, resolve_calc_value};
use crate::layout::LayoutPassState;
use crate::stylo_to_parley;
use crate::node::TextLayout;

/// Subtract a child's margins from the definite axes of the available space it is laid out in.
// A bare ratio uses the box set by box-sizing; an "auto <ratio>"
// always transfers content-box dimensions (CSS Sizing 4 §4.1).
fn ratio_sized_box(
    specified: Size<Option<f32>>,
    min: Size<Option<f32>>,
    max: Size<Option<f32>>,
    ratio: Option<f32>,
    insets: Size<f32>,
    content_box: bool,
) -> Size<Option<f32>> {
    let max = max.maybe_max(min);
    let mut result = specified.maybe_clamp(min, max);
    let Some(ratio) = ratio.filter(|r| r.is_finite() && *r > 0.0) else {
        return result;
    };
    match (result.width, result.height) {
        (Some(width), None) => {
            let height = if content_box {
                (width - insets.width).max(0.0) / ratio + insets.height
            } else {
                width / ratio
            };
            result.height = Some(height.max(insets.height)
                .max(min.height.unwrap_or(0.0))
                .min(max.height.unwrap_or(f32::INFINITY)));
        }
        (None, Some(height)) => {
            let width = if content_box {
                (height - insets.height).max(0.0) * ratio + insets.width
            } else {
                height * ratio
            };
            result.width = Some(width.max(insets.width)
                .max(min.width.unwrap_or(0.0))
                .min(max.width.unwrap_or(f32::INFINITY)));
        }
        _ => {}
    }
    result
}

/// Taffy's convention is that the parent subtracts a child's margins from the available space
/// before passing it to the child, and that the child does not subtract them again.
fn subtract_margins(mut child_inputs: LayoutInput, margin: taffy::Rect<f32>) -> LayoutInput {
    child_inputs.available_space = child_inputs
        .available_space
        .maybe_sub(margin.sum_axes())
        .maybe_max(Size::ZERO);
    child_inputs
}

/// Layout inputs for an atomic inline box, with any sizing keyword on its `width` style
/// (`min-content`, `max-content`, `fit-content`, `fit-content(...)`, `stretch`) resolved
/// into the available space or known width the box is measured with.
fn inline_box_inputs(
    width_style: taffy::Dimension,
    margin: taffy::Rect<f32>,
    child_inputs: LayoutInput,
) -> LayoutInput {
    let mut inputs = subtract_margins(child_inputs, margin);
    let stretch_width = inputs.available_space.width.into_option();
    let percent_basis = child_inputs.parent_size.width;

    match width_style.tag() {
        CompactLength::MIN_CONTENT_TAG => inputs.available_space.width = AvailableSpace::MinContent,
        CompactLength::MAX_CONTENT_TAG => inputs.available_space.width = AvailableSpace::MaxContent,
        CompactLength::FIT_CONTENT_PX_TAG => {
            inputs.available_space.width = AvailableSpace::Definite(width_style.value())
        }
        CompactLength::FIT_CONTENT_PERCENT_TAG => {
            if let Some(basis) = percent_basis {
                inputs.available_space.width =
                    AvailableSpace::Definite(basis * width_style.value());
            }
        }
        CompactLength::FIT_CONTENT_KEYWORD_TAG => {
            if let Some(width) = stretch_width {
                inputs.available_space.width = AvailableSpace::Definite(width);
            }
        }
        _ if width_style.is_fit_content_calc() => {
            if let Some(basis) = percent_basis {
                inputs.available_space.width =
                    AvailableSpace::Definite(resolve_calc_value(width_style.calc_value(), basis));
            }
        }
        CompactLength::STRETCH_TAG => {
            if let Some(width) = stretch_width {
                inputs.known_dimensions.width = Some(width);
                inputs.available_space.width = AvailableSpace::Definite(width);
            }
        }
        _ => {}
    }
    inputs
}

impl LayoutPassState<'_> {
    pub(crate) fn compute_inline_layout(
        &mut self,
        node_id: NodeId,
        inputs: taffy::tree::LayoutInput,
        block_ctx: Option<&mut BlockContext<'_>>,
    ) -> taffy::LayoutOutput {
        let LayoutInput {
            known_dimensions,
            parent_size,
            run_mode,
            ..
        } = inputs;
        let style = self.nodes[node_id].layout_style();

        // Pull these out earlier to avoid borrowing issues
        let is_scroll_container =
            style.overflow().x.is_scroll_container() || style.overflow().y.is_scroll_container();
        let padding = style
            .padding()
            .resolve_or_zero(parent_size.width, resolve_calc_value);
        let border = style
            .border()
            .resolve_or_zero(parent_size.width, resolve_calc_value);
        let padding_border_size = (padding + border).sum_axes();
        let box_sizing_adjustment = if style.box_sizing() == BoxSizing::ContentBox {
            padding_border_size
        } else {
            Size::ZERO
        };

        // Resolve node's preferred/min/max sizes (width/heights) against the available space (percentages resolve to pixel values)
        // For ContentSize mode, we pretend that the node has no size styles as these should be ignored.
        let content_box_ratio = style.box_sizing() == BoxSizing::ContentBox
            || self.nodes[node_id].primary_styles().is_some_and(|s| s.get_position().aspect_ratio.auto);
        let (clamped_style_size, min_size, max_size, aspect_ratio) = match inputs.sizing_mode {
            SizingMode::ContentSize => {
                let node_size = known_dimensions;
                let node_min_size = Size::NONE;
                let node_max_size = Size::NONE;
                (node_size, node_min_size, node_max_size, None)
            }
            SizingMode::InherentSize => {
                let aspect_ratio = style.aspect_ratio();
                let style_size = style
                    .size()
                    .maybe_resolve(parent_size, resolve_calc_value)
                    .maybe_add(box_sizing_adjustment);
                let style_min_size = style
                    .min_size()
                    .maybe_resolve(parent_size, resolve_calc_value)
                    .maybe_add(box_sizing_adjustment);
                let style_max_size = style
                    .max_size()
                    .maybe_resolve(parent_size, resolve_calc_value)
                    .maybe_add(box_sizing_adjustment);

                let node_size = ratio_sized_box(
                    known_dimensions.or(style_size), style_min_size, style_max_size,
                    aspect_ratio, padding_border_size, content_box_ratio,
                );
                (node_size, style_min_size, style_max_size, aspect_ratio)
            }
        };

        // If both min and max in a given axis are set and max <= min then this determines the size in that axis
        let min_max_definite_size = min_size.zip_map(max_size, |min, max| match (min, max) {
            (Some(min), Some(max)) if max <= min => Some(min),
            _ => None,
        });

        let styled_based_known_dimensions = ratio_sized_box(
            known_dimensions.or(min_max_definite_size).or(clamped_style_size),
            min_size, max_size, aspect_ratio, padding_border_size, content_box_ratio,
        ).maybe_max(padding_border_size);

        // Short-circuit layout if the container's size is fully determined by the container's size and the run mode
        // is ComputeSize (and thus the container's size is all that we're interested in)
        if run_mode == RunMode::ComputeSize {
            if let Size {
                width: Some(width),
                height: Some(height),
            } = styled_based_known_dimensions
            {
                return LayoutOutput::from_outer_size(Size { width, height });
            }
        }

        drop(style);

        // Unwrap the block formatting context if one was passed, or else create a new one
        match block_ctx {
            Some(inherited_bfc) if !is_scroll_container => self.compute_inline_layout_inner(
                node_id,
                LayoutInput {
                    known_dimensions: styled_based_known_dimensions,
                    ..inputs
                },
                inherited_bfc,
            ),
            _ => {
                let mut root_bfc = BlockFormattingContext::new();
                let mut root_ctx = root_bfc.root_block_context();
                self.compute_inline_layout_inner(
                    node_id,
                    LayoutInput {
                        known_dimensions: styled_based_known_dimensions,
                        ..inputs
                    },
                    &mut root_ctx,
                )
            }
        }
    }

    /// The out-of-flow positions for which an inline span between the inline box `box_id` and its
    /// inline root `root_id` is a containing block (as `SPAN_*_CB` flags)
    fn inline_span_cb_flags(&self, root_id: NodeId, box_id: NodeId) -> StyleFlags {
        let root = &self.nodes[root_id];
        // The spans of an anonymous inline root are descendants of its parent
        let root_parent = root.is_anonymous().then_some(root.parent).flatten();

        let mut flags = StyleFlags::empty();
        let mut current = self.nodes[box_id].parent;
        while let Some(id) = current {
            if id == root_id || Some(id) == root_parent {
                return flags;
            }
            let ancestor = &self.nodes[id];
            if let Some(style) = ancestor.primary_styles() {
                let display = style.clone_display();
                if display.outside() == DisplayOutside::Inline
                    && display.inside() == DisplayInside::Flow
                {
                    let claims = stylo_taffy::convert::inline_containing_block_claims(&style);
                    if claims.absolute {
                        flags |= StyleFlags::SPAN_ABSOLUTE_CB;
                    }
                    if claims.fixed {
                        flags |= StyleFlags::SPAN_FIXED_CB;
                    }
                }
            }
            current = ancestor.parent;
        }
        StyleFlags::empty()
    }

    fn compute_inline_layout_inner(
        &mut self,
        node_id: NodeId,
        inputs: taffy::tree::LayoutInput,
        block_ctx: &mut BlockContext<'_>,
    ) -> taffy::LayoutOutput {
        let scale = self.viewport.scale();
        let LayoutInput {
            known_dimensions,
            parent_size,
            available_space,
            sizing_mode,
            ..
        } = inputs;

        // Take inline layout to satisfy borrow checker
        let mut inline_layout = self.nodes[node_id]
            .data
            .downcast_element_mut()
            .unwrap()
            .take_inline_layout()
            .unwrap();
        // Layout objects can be reused across passes; clamp paint state cannot.
        inline_layout.clamp_visible_lines = None;
        inline_layout.clamp_marker = None;
        inline_layout.clamp_marker_at = None;
        inline_layout.clamp_marker_cut = None;

        let style = self.nodes[node_id].layout_style();

        // Note: both horizontal and vertical percentage padding/borders are resolved against the container's inline size (i.e. width).
        // This is not a bug, but is how CSS is specified (see: https://developer.mozilla.org/en-US/docs/Web/CSS/padding#values)
        let padding = style
            .padding()
            .resolve_or_zero(parent_size.width, resolve_calc_value);
        let border = style
            .border()
            .resolve_or_zero(parent_size.width, resolve_calc_value);
        let container_pb = padding + border;
        let pb_sum = container_pb.sum_axes();
        let box_sizing_adjustment = if style.box_sizing() == BoxSizing::ContentBox {
            pb_sum
        } else {
            Size::ZERO
        };

        // Scrollbar gutters are reserved when the `overflow` property is set to `Overflow::Scroll`.
        // However, the axis are switched (transposed) because a node that scrolls vertically needs
        // *horizontal* space to be reserved for a scrollbar
        let scrollbar_gutter = style.overflow().transpose().map(|overflow| match overflow {
            Overflow::Scroll => style.scrollbar_width(),
            _ => 0.0,
        });
        // TODO: make side configurable based on the `direction` property
        let mut content_box_inset = container_pb;
        content_box_inset.right += scrollbar_gutter.x;
        content_box_inset.bottom += scrollbar_gutter.y;

        let has_styles_preventing_being_collapsed_through = style.aspect_ratio().is_some()
            || !style.is_block()
            || style.overflow().x.is_scroll_container()
            || style.overflow().y.is_scroll_container()
            || style.position().is_out_of_flow()
            || padding.top > 0.0
            || padding.bottom > 0.0
            || border.top > 0.0
            || border.bottom > 0.0;
        // || matches!(node_size.height, Some(h) if h > 0.0)
        // || matches!(node_min_size.height, Some(h) if h > 0.0)
        // || !inline_layout.text.is_empty();
        // || inline_layout.layout.inline_boxes().len() > 0;

        // Resolve node's preferred/min/max sizes (width/heights) against the available space (percentages resolve to pixel values)
        // For ContentSize mode, we pretend that the node has no size styles as these should be ignored.
        let content_box_ratio = style.box_sizing() == BoxSizing::ContentBox
            || self.nodes[node_id].primary_styles().is_some_and(|s| s.get_position().aspect_ratio.auto);
        let ratio_auto_height = style.size().height.is_auto()
            && style.min_size().height.is_auto()
            && style.overflow().y == Overflow::Visible;
        let (node_size, node_min_size, node_max_size, aspect_ratio) = match sizing_mode {
            SizingMode::ContentSize => {
                let node_size = known_dimensions;
                let node_min_size = Size::NONE;
                let node_max_size = Size::NONE;
                (node_size, node_min_size, node_max_size, None)
            }
            SizingMode::InherentSize => {
                let aspect_ratio = style.aspect_ratio();
                let style_size = style
                    .size()
                    .maybe_resolve(parent_size, resolve_calc_value)
                    .maybe_add(box_sizing_adjustment);
                let style_min_size = style
                    .min_size()
                    .maybe_resolve(parent_size, resolve_calc_value)
                    .maybe_add(box_sizing_adjustment);
                let style_max_size = style
                    .max_size()
                    .maybe_resolve(parent_size, resolve_calc_value)
                    .maybe_add(box_sizing_adjustment);

                let node_size = ratio_sized_box(
                    known_dimensions.or(style_size), style_min_size, style_max_size,
                    aspect_ratio, pb_sum, content_box_ratio,
                );
                (node_size, style_min_size, style_max_size, aspect_ratio)
            }
        };

        let suppresses_baseline = style.contain().suppresses_baseline();
        drop(style);

        // Short circuit if inline context contains no text or inline boxes
        if !has_styles_preventing_being_collapsed_through
            && inline_layout.text.is_empty()
            && inline_layout.layout.inline_boxes().len() == 0
        {
            // Put layout back
            self.nodes[node_id]
                .data
                .downcast_element_mut()
                .unwrap()
                .inline_layout_data = Some(inline_layout);
            return LayoutOutput::from_outer_size(
                Size::ZERO.maybe_max(container_pb.sum_axes().map(Some)),
            );
        }

        // Compute available space
        let available_space = Size {
            width: known_dimensions
                .width
                .map(AvailableSpace::from)
                .unwrap_or(available_space.width)
                .maybe_set(known_dimensions.width)
                .maybe_set(node_size.width)
                .map_definite_value(|size| {
                    (size.maybe_clamp(node_min_size.width, node_max_size.width)
                        - content_box_inset.horizontal_axis_sum())
                    .max(0.0)
                }),
            height: known_dimensions
                .height
                .map(AvailableSpace::from)
                .unwrap_or(available_space.height)
                .maybe_set(known_dimensions.height)
                .maybe_set(node_size.height)
                .map_definite_value(|size| {
                    (size.maybe_clamp(node_min_size.height, node_max_size.height)
                        - content_box_inset.vertical_axis_sum())
                    .max(0.0)
                }),
        };

        // Compute size of inline boxes
        let child_inputs = taffy::tree::LayoutInput {
            known_dimensions: Size::NONE,
            available_space,
            sizing_mode: SizingMode::InherentSize,
            parent_size: Size {
                width: available_space.width.into_option(),
                // Anonymous blocks do not establish the containing block for percentages.
                height: if self.nodes[node_id].is_anonymous() {
                    parent_size.height
                } else {
                    available_space.height.into_option()
                },
            },
            // Atomic inlines (e.g. inline-block) establish independent formatting
            // contexts: their margins never collapse with their children's margins.
            vertical_margins_are_collapsible: taffy::Line::FALSE,
            ..inputs
        };
        #[cfg(feature = "floats")]
        let float_child_inputs = taffy::tree::LayoutInput {
            available_space: Size::MAX_CONTENT,
            ..child_inputs
        };

        // Update inline boxes
        for ibox in inline_layout.layout.inline_boxes_mut() {
            if let Some((span_id, trailing)) = construct::inline_edge_owner(ibox.id) {
                let style = self.child_layout_style(&self.nodes[span_id]);
                // A definite zero basis preserves the fixed length of calc()
                // while resolving cyclic percentage terms to zero in
                // min/max-content sizing. None would discard both terms.
                let edge_basis = taffy::Size {
                    width: if matches!(
                        available_space.width, AvailableSpace::MinContent | AvailableSpace::MaxContent
                    ) {
                        Some(0.0)
                    } else {
                        parent_size.width.or(child_inputs.parent_size.width)
                    },
                    height: parent_size.height,
                };
                let margin = style.margin().resolve_or_zero(edge_basis, resolve_calc_value);
                let padding = style.padding().resolve_or_zero(edge_basis, resolve_calc_value);
                let border = style.border().resolve_or_zero(edge_basis, resolve_calc_value);
                let leading = margin.left + padding.left + border.left;
                let ending = margin.right + padding.right + border.right;
                let rtl = style.direction() == taffy::Direction::Rtl;
                let advance = if trailing == rtl { leading } else { ending };
                // These are spacing-only items, not atomic element boxes.
                // Their width participates in min/max-content measurement.
                ibox.width = advance * scale;
                ibox.height = 0.0;
                ibox.baseline = Some(0.0);
                continue;
            }
            let style = self.child_layout_style(&self.nodes[NodeId::from_u64(ibox.id)]);
            let margin = style
                .margin()
                .resolve_or_zero(inputs.parent_size, resolve_calc_value);

            #[cfg(feature = "floats")]
            let is_floated = style.float().is_floated();
            #[cfg(not(feature = "floats"))]
            let is_floated = false;

            let is_out_of_flow = style.position().is_out_of_flow();
            // `baseline-source: auto` selects the last baseline for inline-blocks and the first
            // for other atomic inlines. A block-axis scroll-container inline-block (except buttons)
            // instead uses its bottom margin edge, but only with `auto` (css-align-3 §9.1).
            // `overflow: clip` is not a scroll container. Exported scroll-container baselines
            // are clamped to the border box.
            // A layout-contained box is treated as having no baseline (css-contain-1 §3.3).
            let overflow = style.overflow();
            let box_style = style.style.get_box();
            let is_flow = matches!(
                style.effective_display().inside(),
                DisplayInside::Flow | DisplayInside::FlowRoot
            );
            let baseline_source = box_style.clone_baseline_source();
            let uses_last_baseline = match baseline_source {
                BaselineSource::Auto => is_flow,
                BaselineSource::First => false,
                BaselineSource::Last => true,
            };
            let is_scroll_container = !matches!(overflow.y, Overflow::Visible | Overflow::Clip);
            let is_block_axis_scroll_container = is_flow
                && is_scroll_container
                && !style
                    .flags
                    .intersects(StyleFlags::IS_BUTTON | StyleFlags::IS_BUTTON_INPUT);
            let contain_layout = box_style.clone_contain().contains(Contain::LAYOUT);
            let exports_baseline = !(baseline_source == BaselineSource::Auto
                && is_block_axis_scroll_container)
                && !contain_layout;
            let box_inputs = inline_box_inputs(style.size().width, margin, child_inputs);
            drop(style);

            if is_out_of_flow || is_floated {
                ibox.width = 0.0;
                ibox.height = 0.0;
                ibox.baseline = None;
            } else {
                let output = self.compute_child_layout(taffy::NodeId::from(ibox.id), box_inputs);
                ibox.width = (margin.left + margin.right + output.size.width) * scale;
                // Native checkbox/radio/range baselines use their bottom
                // content edge. The color baseline uses all vertical padding
                // and border insets, calibrated against the measured reference
                // row (within 0.5px). This control-specific rule is not a
                // general CSS baseline derivation. Appearance:none choices
                // have no native baseline and align at the bottom margin edge.
                let native_baseline = {
                    let child = &self.nodes[NodeId::from_u64(ibox.id)];
                    let small_control_font = child.primary_styles().is_some_and(|s| {
                        (s.clone_font_size().used_size().px() - 13.3333).abs() < 0.1
                    });
                    child.element_data().and_then(|el| match el.name.local.as_ref() {
                        "input"
                            if matches!(
                                el.attr(crate::local_name!("type")),
                                Some("checkbox" | "radio" | "range" | "color")
                            ) =>
                        {
                            if self.doc.resolved_style_value(child.id, "appearance") == "none" {
                                None
                            } else {
                                let control_style = self.child_layout_style(child);
                                let padding = control_style
                                    .padding()
                                    .resolve_or_zero(inputs.parent_size.width, resolve_calc_value);
                                let border = control_style
                                    .border()
                                    .resolve_or_zero(inputs.parent_size.width, resolve_calc_value);
                                let baseline = if el.attr(crate::local_name!("type"))
                                    == Some("color")
                                {
                                    // Reference-calibrated color baseline: use
                                    // the full top/bottom border and padding.
                                    output.size.height
                                        - padding.top
                                        - padding.bottom
                                        - border.top
                                        - border.bottom
                                } else {
                                    output.size.height - padding.bottom - border.bottom
                                };
                                Some(baseline.max(0.0))
                            }
                        }
                        "textarea" if small_control_font =>
                            Some(output.size.height.max(0.0)),
                        "select" if small_control_font
                            && el.attr(crate::local_name!("size"))
                                .and_then(|s| s.parse::<u32>().ok()).is_some_and(|s| s > 1) =>
                            Some((output.size.height - 3.0).max(0.0)),
                        "select" if small_control_font =>
                            Some((output.size.height - 5.0).max(0.0)),
                        "input" if small_control_font => {
                            // A text control exports its font-content baseline:
                            // border + padding + rounded descent are below it.
                            let descent = el.text_input_data()
                                .and_then(|data| data.editor.try_layout())
                                .and_then(|layout| layout.lines().next().map(|line| {
                                    let metrics = line.metrics();
                                    (metrics.content_block_max_coord - metrics.baseline).round()
                                        / layout.scale()
                                }))
                                .unwrap_or_else(|| child.primary_styles().map_or(0.0, |style| {
                                    (style.clone_font_size().used_size().px() * 0.2).round()
                                }));
                            let control_style = self.child_layout_style(child);
                            let padding = control_style.padding()
                                .resolve_or_zero(inputs.parent_size.width, resolve_calc_value);
                            let border = control_style.border()
                                .resolve_or_zero(inputs.parent_size.width, resolve_calc_value);
                            Some((output.size.height - padding.bottom - border.bottom - descent)
                                .max(0.0))
                        },
                        _ => None,
                    })
                };
                ibox.baseline = if exports_baseline || native_baseline.is_some() {
                    let baseline = if uses_last_baseline {
                        output.baselines.last.or(output.baselines.first)
                    } else {
                        output.baselines.first.or(output.baselines.last)
                    };
                    native_baseline.or(baseline).map(|baseline| {
                        let baseline = if is_scroll_container {
                            baseline.clamp(0.0, output.size.height)
                        } else {
                            baseline
                        };
                        (margin.top + baseline) * scale
                    })
                } else {
                    None
                };
                // Vertical margins adjust the space the box reserves in the line. A box with a
                // baseline splits that space into ascent (`margin.top + baseline`) and descent
                // (`margin.bottom + height - baseline`), either of which may be negative. A box
                // without a baseline sits on the baseline and cannot reserve negative space.
                // Kept finite: huge author lengths can sum to infinity, which is taller than
                // the line breaker's `f32::MAX` height limit, and it then yields
                // `MaxHeightExceeded` for this box forever without advancing.
                let margin_box_height = margin.top + margin.bottom + output.size.height;
                ibox.height = if ibox.baseline.is_some() {
                    (margin_box_height * scale).min(f32::MAX)
                } else {
                    (margin_box_height.max(0.0) * scale).min(f32::MAX)
                };
            }
        }

        let text_indent = self.nodes[node_id]
            .primary_styles()
            .map(|s| s.clone_text_indent())
            .unwrap_or_else(GenericTextIndent::zero);
        let indent_options = IndentOptions {
            each_line: text_indent.each_line,
            hanging: text_indent.hanging,
        };
        // Percentage indents do not contribute to intrinsic widths.
        inline_layout.layout.set_text_indent(
            text_indent.length.resolve(CSSPixelLength::new(0.0)).px() * scale,
            indent_options,
        );

        let pbw = container_pb.horizontal_components().sum() * scale;
        let width = known_dimensions
            .width
            .map(|w| (w * scale) - pbw)
            .unwrap_or_else(|| {
                // TODO: Cache content widths.
                //
                // This is a little tricky as the size of the inline boxes may depend on whether we are sizing under
                // and a min-content or max-content constraint. So if we want to compute both widths in one pass then
                // we need to store both a min-content and max-content size on each box.
                let content_sizes = inline_layout.layout.calculate_content_widths();
                let max_content_width = content_sizes.max;
                // Synthetic inline edges can introduce artificial min-content
                // breaks. The nowrap override is root-scoped; descendants
                // with a different wrap mode require fragment-aware sizing.
                // A single ASCII word cannot wrap at a decorative edge.
                let no_wrap = self.nodes[node_id].primary_styles()
                    .or_else(|| self.nodes[node_id].parent
                        .and_then(|id| self.nodes[id].primary_styles()))
                    .is_some_and(|s| matches!(
                        stylo_to_parley::text_wrap_mode(s.clone_text_wrap_mode()),
                        parley::TextWrapMode::NoWrap
                    ));
                let mut any_edge = false;
                let mut only_edges = true;
                for ibox in inline_layout.layout.inline_boxes_mut() {
                    let edge = construct::inline_edge_owner(ibox.id).is_some();
                    any_edge |= edge;
                    only_edges &= edge;
                }
                let unbroken_ascii_word = any_edge && only_edges
                    && !inline_layout.text.is_empty()
                    && inline_layout.text.chars().all(|c| c.is_ascii_alphabetic());
                let min_content_width = if no_wrap || unbroken_ascii_word {
                    max_content_width
                } else {
                    content_sizes.min
                };

                #[cfg(feature = "floats")]
                let float_width = match available_space.width {
                    AvailableSpace::Definite(_) => 0.0,
                    AvailableSpace::MinContent => {
                        let mut width: f32 = 0.0;
                        for ibox in inline_layout.layout.inline_boxes_mut() {
                            if construct::inline_edge_owner(ibox.id).is_some() { continue; }
                            let (is_floated, margin) = {
                                let style =
                                    self.child_layout_style(&self.nodes[NodeId::from_u64(ibox.id)]);
                                (
                                    style.float().is_floated(),
                                    style
                                        .margin()
                                        .resolve_or_zero(inputs.parent_size, resolve_calc_value),
                                )
                            };

                            if is_floated {
                                let output = self.compute_child_layout(
                                    taffy::NodeId::from(ibox.id),
                                    subtract_margins(child_inputs, margin),
                                );
                                width = width.max(output.size.width + margin.left + margin.right);
                            }
                        }

                        width * scale
                    }
                    AvailableSpace::MaxContent => {
                        // When computing a max-content size the available width is effectively
                        // infinite, so floats never wrap onto a new "band" due to a lack of
                        // horizontal space. They only move below preceding floats when the `clear`
                        // property forces them to.
                        //
                        // Floats that share a band sit side-by-side and so their widths sum, whereas
                        // floats pushed onto a new band (via `clear`) stack vertically and so we
                        // take the maximum extent across bands rather than summing.
                        let mut left_band: f32 = 0.0;
                        let mut right_band: f32 = 0.0;
                        let mut width: f32 = 0.0;
                        for ibox in inline_layout.layout.inline_boxes_mut() {
                            if construct::inline_edge_owner(ibox.id).is_some() { continue; }
                            let (float, clear, margin) = {
                                let style =
                                    self.child_layout_style(&self.nodes[NodeId::from_u64(ibox.id)]);
                                (
                                    style.float(),
                                    style.clear(),
                                    style
                                        .margin()
                                        .resolve_or_zero(inputs.parent_size, resolve_calc_value),
                                )
                            };

                            if float.is_floated() {
                                if matches!(clear, Clear::Left | Clear::Both) {
                                    left_band = 0.0;
                                }
                                if matches!(clear, Clear::Right | Clear::Both) {
                                    right_band = 0.0;
                                }

                                let output = self.compute_child_layout(
                                    taffy::NodeId::from(ibox.id),
                                    subtract_margins(child_inputs, margin),
                                );
                                let box_width = output.size.width + margin.left + margin.right;

                                match float {
                                    Float::Left => left_band += box_width,
                                    Float::Right => right_band += box_width,
                                    Float::None => {}
                                }
                                width = width.max(left_band + right_band);
                            }
                        }

                        width * scale
                    }
                };

                #[cfg(not(feature = "floats"))]
                let float_width = 0.0;

                let computed_width = match available_space.width {
                    AvailableSpace::MinContent => min_content_width.max(float_width),
                    AvailableSpace::MaxContent => max_content_width + float_width,
                    AvailableSpace::Definite(limit) => (limit * scale)
                        .min(max_content_width + float_width)
                        .max(min_content_width),
                };
                let style_width = node_size.width.map(|w| w * scale);
                let min_width = node_min_size.width.map(|w| w * scale);
                let max_width = node_max_size.width.map(|w| w * scale);

                (style_width)
                    .unwrap_or(computed_width + pbw)
                    .max(computed_width)
                    .maybe_clamp(min_width, max_width)
                    - pbw
            });

        #[cfg(not(feature = "floats"))]
        let _ = block_ctx; // Suppress unused variable warning

        // Set block context width if this is a block context root
        #[cfg(feature = "floats")]
        let is_bfc_root = block_ctx.is_bfc_root();
        #[cfg(feature = "floats")]
        if is_bfc_root {
            block_ctx.set_width((width + pbw) / scale);
        }

        // Create sub-context to account for the inline layout's padding/border
        #[cfg(feature = "floats")]
        let outer_block_ctx = block_ctx;
        #[cfg(feature = "floats")]
        let mut block_ctx =
            outer_block_ctx.sub_context(container_pb.top, [container_pb.left, container_pb.right]);
        // block_ctx.apply_content_box_inset([container_pb.left, container_pb.right]);

        if inputs.run_mode == taffy::RunMode::ComputeSize
            && inputs.axis == RequestedAxis::Horizontal
        {
            // Put layout back
            self.nodes[node_id]
                .data
                .downcast_element_mut()
                .unwrap()
                .inline_layout_data = Some(inline_layout);

            let measured_size = inputs.known_dimensions.unwrap_or(taffy::Size {
                width: width / scale,
                // Height is ignored if RequestedAxis if Horizontal
                height: 0.0,
            });

            let clamped_size = inputs
                .known_dimensions
                .or(node_size)
                .unwrap_or(measured_size + content_box_inset.sum_axes())
                .maybe_clamp(node_min_size, node_max_size)
                .maybe_max(container_pb.sum_axes().map(Some));

            return LayoutOutput::from_outer_size(clamped_size);
        }

        let resolved_text_indent = text_indent
            .length
            .resolve(CSSPixelLength::new((width / scale).max(0.0)))
            .px();
        inline_layout
            .layout
            .set_text_indent(resolved_text_indent * scale, indent_options);

        #[cfg(not(feature = "floats"))]
        {
            inline_layout.layout.break_all_lines(Some(width));
        }

        // Out-of-flow candidates bubbled up from this container and its in-flow subtree.
        // These are laid out by the out-of-flow positioning pass (`compute_oof_layout`).
        let mut oof_candidates = OofCandidates::new();

        // Perform inline layout
        #[cfg(feature = "floats")]
        {
            let mut breaker = inline_layout.layout.break_lines();
            let initial_slot = block_ctx.find_content_slot(0.0, Clear::None, None);
            let mut has_active_floats = initial_slot.segment_id.is_some();
            let state = breaker.state_mut();
            state.set_layout_max_advance(width);
            state.set_line_max_advance(initial_slot.width * scale);
            state.set_line_x(initial_slot.x * scale);
            state.set_line_y((initial_slot.y * scale) as f64);

            // TODO: revert state and retry layout if a line doesn't fit
            //
            // Save initial state. Saved state is used to revert the layout to a previous state if needed
            // (e.g. to revert a line that doesn't fit in the space it was laid out into)
            //
            // let mut saved_state = breaker.state().clone();

            while let Some(yield_data) = breaker.break_next() {
                match yield_data {
                    YieldData::LineBreak(_line_break_data) => {
                        let state = breaker.state_mut();

                        if has_active_floats {
                            // TODO: revert state and retry layout if a line doesn't fit
                            // saved_state = state.clone();

                            let min_y = state.line_y() / scale as f64;
                            let next_slot =
                                block_ctx.find_content_slot(min_y as f32, Clear::None, None);
                            has_active_floats = next_slot.segment_id.is_some();

                            state.set_line_max_advance(next_slot.width * scale);
                            state.set_line_x(next_slot.x * scale);
                            state.set_line_y((next_slot.y * scale) as f64);
                        } else {
                            state.set_line_x(0.0);
                            state.set_line_max_advance(width);
                        }

                        continue;
                    }
                    YieldData::MaxHeightExceeded(_data) => {
                        // TODO
                        continue;
                    }
                    YieldData::InlineBoxBreak(box_break_data) => {
                        let state = breaker.state_mut();
                        let node_id = NodeId::from_u64(box_break_data.inline_box_id);

                        let (direction, clear, margin) = {
                            let style = self.child_layout_style(&self.nodes[node_id]);
                            // We can assume that the box is a float because we only set `break_on_box: true` for floats
                            let direction = match style.float() {
                                Float::Left => taffy::FloatDirection::Left,
                                Float::Right => taffy::FloatDirection::Right,
                                Float::None => unreachable!(),
                            };
                            (
                                direction,
                                style.clear(),
                                style
                                    .margin()
                                    .resolve_or_zero(inputs.parent_size, resolve_calc_value),
                            )
                        };

                        let margin_sum = margin.sum_axes();

                        let mut output = self.compute_child_layout(
                            crate::taffy_node_id(node_id),
                            float_child_inputs,
                        );
                        let min_y = state.line_y() as f32 / scale;

                        // Note: `pos` is content-box relative
                        let pos = block_ctx.place_floated_box(
                            output.size + margin_sum,
                            min_y,
                            direction,
                            clear,
                            false,
                        );

                        let min_y = state.line_y() / scale as f64; //.max(pos.y as f64);
                        let next_slot =
                            block_ctx.find_content_slot(min_y as f32, Clear::None, None);
                        has_active_floats = next_slot.segment_id.is_some();

                        state.set_line_max_advance(next_slot.width * scale);
                        state.set_line_x(next_slot.x * scale);
                        state.set_line_y((next_slot.y * scale) as f64);

                        let location = taffy::Point {
                            x: pos.x + margin.left + container_pb.left,
                            y: pos.y + margin.top + container_pb.top,
                        };
                        let layout = self.nodes[node_id].unrounded_layout_mut();
                        layout.size = output.size;
                        layout.location = location;
                        layout.margin = margin;

                        // Translate anchors from item-relative to container-relative
                        // coordinates and collect candidates bubbled from the float's subtree
                        if !output.oof_candidates.is_empty() {
                            output.oof_candidates.translate(location);
                            oof_candidates.append(&mut output.oof_candidates);
                        }

                        // dbg!(&layout.size);
                        // dbg!(&layout.location);

                        // Floats are out-of-flow and must not contribute to the line's height.
                        state.append_inline_box_to_line(
                            box_break_data.advance,
                            f32::NEG_INFINITY,
                            f32::NEG_INFINITY,
                        );

                        // if float.is_floated() {
                        //     println!("INLINE FLOATED BOX ({}) {:?}", ibox.id, float);
                        //     println!(
                        //         "w:{} h:{} x:{}, y:{}",
                        //         layout.size.width, layout.size.height, 0, 0
                        //     );
                        // }
                    }
                }
            }
            breaker.finish();
        }

        // Propagate the height consumed by floats placed within this container to the
        // enclosing block context (so that the BFC root can contain them)
        #[cfg(feature = "floats")]
        let float_height_contribution = block_ctx.floated_content_height_contribution();
        #[cfg(feature = "floats")]
        outer_block_ctx.add_child_floated_content_height_contribution(
            container_pb.top + float_height_contribution,
        );

        let (alignment, last_line_alignment) = self.nodes[node_id]
            .primary_styles()
            .map(|s| {
                (
                    stylo_to_parley::text_align(s.clone_text_align()),
                    stylo_to_parley::text_align_last(s.clone_text_align_last()),
                )
            })
            .unwrap_or((parley::layout::Alignment::Start, None));

        inline_layout.layout.align(
            alignment,
            AlignmentOptions {
                align_when_overflowing: false,
                last_line_alignment,
            },
        );

        // Parley lays out empty text as a single strut-height line (text-editor semantics),
        // but a line box containing no text, inline boxes or other in-flow content is a
        // zero-height line box in CSS (CSS2 §9.4.2).
        let has_inline_content =
            !inline_layout.text.is_empty() || inline_layout.layout.inline_boxes().len() > 0;

        let mut height = if has_inline_content {
            inline_layout.layout.height()
        } else {
            0.0
        };
        // A default button's own line uses the rounded font content area,
        // not a fractional synthetic line-height. Explicit line-height
        // declarations continue through the normal inline sizing path.
        let default_button = self.nodes[node_id].element_data()
            .is_some_and(|el| &*el.name.local == "button")
            && self.nodes[node_id].primary_styles().is_some_and(|style| {
                matches!(style.clone_line_height(),
                    style::values::computed::font::LineHeight::Normal)
            });
        if default_button && has_inline_content {
            height = inline_layout.layout.lines().map(|line| {
                let metrics = line.metrics();
                (metrics.baseline - metrics.content_block_min_coord).round()
                    + (metrics.content_block_max_coord - metrics.baseline).round()
            }).sum();
        }

        // A context containing only atomic inline boxes and collapsible
        // separators may wrap a separator onto a line by itself after an
        // overflowing box. Such an empty CSS line box has no height, and
        // whitespace at the beginning of the next line has no advance.
        // Leave NBSP, other preserved whitespace, real text and explicit breaks alone.
        let atomic_separators = !inline_layout.text.is_empty()
            && inline_layout.text.chars().all(|ch| ch.is_ascii_whitespace())
            && inline_layout.layout.inline_boxes().any(|b| {
                construct::inline_edge_owner(b.id).is_none()
                    && b.kind == parley::InlineBoxKind::InFlow
            })
            && self.nodes[node_id].primary_styles().is_some_and(|s| {
                stylo_to_parley::white_space_collapse(s.clone_white_space_collapse())
                    == parley::WhiteSpaceCollapse::Collapse
            });
        let mut removed_empty_height = 0.0_f32;
        let mut atomic_line_offsets = Vec::new();
        if atomic_separators {
            for line in inline_layout.layout.lines() {
                let first_box_x = line.items().find_map(|item| match item {
                    parley::layout::PositionedLayoutItem::InlineBox(b)
                        if b.kind == parley::InlineBoxKind::InFlow
                            && construct::inline_edge_owner(b.id).is_none() => Some(b.x),
                    _ => None,
                });
                // An editor-only trailing line has an empty text range. It
                // must not be removed here as well as by the explicit-break
                // trailing-line rule below.
                if first_box_x.is_none()
                    && !line.text_range().is_empty()
                    && line.break_reason() != BreakReason::Explicit
                {
                    removed_empty_height += line.metrics().line_height;
                }
                // The first in-flow item defines the line's leading advance.
                // An inline edge appearing after an image must not trim the
                // image's x coordinate; an edge before it must be preserved.
                let trim_x = if resolved_text_indent.abs() < 0.01
                    && matches!(alignment, parley::layout::Alignment::Start)
                    && !inline_layout.layout.is_rtl() {
                    line.items().find_map(|item| match item {
                        parley::layout::PositionedLayoutItem::InlineBox(b)
                            if b.kind == parley::InlineBoxKind::InFlow => Some(b.x),
                        _ => None,
                    }).unwrap_or(0.0)
                } else {
                    0.0
                };
                atomic_line_offsets.push((removed_empty_height, trim_x));
            }
            height = (height - removed_empty_height).max(0.0);
        }

        // A forced line break (e.g. `<br>` or a preserved newline) at the end of the inline
        // content ends the final line box without starting a new one. Parley still emits an
        // empty line after it (so that editors have a line to place the cursor on), so that
        // line is excluded from the measured height (and from the last baseline).
        let line_count = inline_layout.layout.len();
        let mut trailing_empty_line = None;
        if line_count >= 2
            && let (Some(prev_line), Some(last_line)) = (
                inline_layout.layout.get(line_count - 2),
                inline_layout.layout.get(line_count - 1),
            )
            && prev_line.break_reason() == BreakReason::Explicit
            && last_line.text_range().is_empty()
            && last_line.items().next().is_none()
        {
            height -= last_line.metrics().line_height;
            trailing_empty_line = Some(line_count - 1);
        }

        // Clamp only after final line breaking, so forced breaks, wrapping and
        // float-reduced line widths contribute the same lines as ordinary layout.
        let effective_line_count = trailing_empty_line.unwrap_or(line_count);
        if let Some(spec) = crate::line_clamp::for_node(&self.nodes[node_id])
            && effective_line_count > spec.lines
        {
            if let Some(last) = inline_layout.layout.get(spec.lines - 1) {
                height = height.min(last.metrics().block_max_coord.max(0.0));
            }
            inline_layout.clamp_visible_lines = Some(spec.lines);
            if spec.ellipsis && inputs.run_mode == RunMode::PerformLayout {
                self.install_clamp_marker(node_id, &mut inline_layout, spec.lines);
            }
        }

        #[cfg(feature = "floats")]
        {
            if is_bfc_root {
                height = height.max(float_height_contribution * scale)
            };
        }

        // Note: `width` and `height` are content-box measurements of the inline content.
        // `known_dimensions` must not be substituted in here: those are border-box sizes, and
        // using them for the scrollable overflow rect (which adds padding below) would
        // double-count padding, incorrectly making the container's content overflow it.
        let measured_size = taffy::Size {
            width: width / scale,
            height: height / scale,
        };

        // Unbreakable content (e.g. `white-space: pre` text) may overflow the max advance the
        // lines were broken at. `width` is the advance the lines were broken at, so the actual
        // extent of the line boxes must be taken from the laid-out lines for `content_size`.
        let content_width = f32_max(width, inline_layout.layout.width()) / scale;

        let measured_outer = measured_size + content_box_inset.sum_axes();
        let resolved = ratio_sized_box(
            inputs.known_dimensions.or(node_size), node_min_size, node_max_size,
            aspect_ratio, pb_sum, content_box_ratio,
        );
        let clamped_size = resolved.unwrap_or(measured_outer)
            .maybe_clamp(node_min_size, node_max_size);
        // The automatic minimum in the ratio-dependent axis protects visible
        // content; overflow that clips or scrolls retains the transferred size.
        let height = if aspect_ratio.is_some() && ratio_auto_height {
            clamped_size.height.max(measured_outer.height)
                .min(node_max_size.height.unwrap_or(f32::INFINITY)
                    .max(node_min_size.height.unwrap_or(0.0)))
        } else {
            clamped_size.height
        };
        let final_size = Size { width: clamped_size.width, height }
            .maybe_max(container_pb.sum_axes().map(Some));

        let container_direction = self.nodes[node_id].layout_style().direction();

        // `align-content` aligns the line boxes (as a single unit) within the content box in
        // the block axis. Floats are positioned relative to the formatting context and are not
        // moved. The offset is stored on the text layout for painting and hit-testing.
        let align_content = BlockContainerStyle::align_content(&self.nodes[node_id].layout_style());
        let block_offset = if align_content.keyword() == taffy::AlignContentKeyword::Normal {
            0.0
        } else {
            let free_space =
                final_size.height - content_box_inset.vertical_axis_sum() - measured_size.height;
            taffy::compute_block_align_content_offset(align_content, free_space)
        };
        inline_layout.block_offset = block_offset;
        let line_box_top = container_pb.top + block_offset;

        // Store sizes and positions of inline boxes
        let mut ibox_order: u32 = 0;
        let mut span_cb_flags = StyleFlags::empty();
        for (line_index, line) in inline_layout.layout.lines().enumerate() {
            let (skip_y, trim_x) = atomic_line_offsets
                .get(line_index).copied().unwrap_or((0.0, 0.0));
            for item in line.items() {
                if let parley::layout::PositionedLayoutItem::InlineBox(ibox) = item {
                    // Inline-span edge advances have already contributed
                    // their width; they are not layout or paint children.
                    if construct::inline_edge_owner(ibox.id).is_some() { continue; }
                    let order = ibox_order;
                    ibox_order += 1;
                    if inputs.run_mode == RunMode::PerformLayout {
                        span_cb_flags |=
                            self.inline_span_cb_flags(node_id, NodeId::from_u64(ibox.id));
                    }
                    let node = &self.nodes[NodeId::from_u64(ibox.id)];
                    let style = self.child_layout_style(node);
                    let padding = style
                        .padding()
                        .resolve_or_zero(child_inputs.parent_size, resolve_calc_value);
                    let border = style
                        .border()
                        .resolve_or_zero(child_inputs.parent_size, resolve_calc_value);
                    let margin = style
                        .margin()
                        .resolve_or_zero(child_inputs.parent_size, resolve_calc_value);

                    #[cfg(feature = "floats")]
                    let is_floated = style.float() != Float::None;
                    #[cfg(not(feature = "floats"))]
                    let is_floated = false;

                    let position = style.position();
                    let is_absolute = position.is_out_of_flow();
                    let item_direction = style.direction();
                    // Inline formatting contexts have no `justify-items`/`align-items` for an
                    // `auto` self-alignment to defer to, so it behaves as `normal`.
                    let justify_self =
                        OofItemStyle::justify_self(&style).unwrap_or(taffy::AlignItems::NORMAL);
                    let align_self =
                        OofItemStyle::align_self(&style).unwrap_or(taffy::AlignItems::NORMAL);

                    // The static position of an absolutely positioned box depends on the
                    // display its hypothetical box would have had (the display specified
                    // before position:absolute blockified it): inline-level boxes sit at
                    // their position within the line, while block-level boxes start at the
                    // content-box left edge of their containing block.
                    let is_inline_level =
                        style.style.get_box().original_display.outside() == DisplayOutside::Inline;

                    // Resolve relative inset offsets against the containing block
                    // (the content box of the inline container).
                    let container_content_size = final_size - content_box_inset.sum_axes();
                    let inset_style = style.inset();
                    let inset = taffy::Rect {
                        left: inset_style
                            .left
                            .maybe_resolve(container_content_size.width, resolve_calc_value),
                        right: inset_style
                            .right
                            .maybe_resolve(container_content_size.width, resolve_calc_value),
                        top: inset_style
                            .top
                            .maybe_resolve(container_content_size.height, resolve_calc_value),
                        bottom: inset_style
                            .bottom
                            .maybe_resolve(container_content_size.height, resolve_calc_value),
                    };
                    let box_inputs = inline_box_inputs(style.size().width, margin, child_inputs);
                    drop(style);

                    if is_absolute {
                        // The static-position rectangle
                        // (https://www.w3.org/TR/css-position-3/#staticpos-rect):
                        // - An inline-level box's rectangle is zero-width at its position
                        //   within the line (`ibox.y` is the baseline as out-of-flow boxes are
                        //   zero-sized) and spans the line box in the block axis.
                        // - A block-level box's rectangle spans the containing block's content
                        //   box in the inline axis and is zero-height below the line box.
                        let line_metrics = line.metrics();
                        let line_top = (line_metrics.block_min_coord / scale) + line_box_top;
                        let line_bottom = (line_metrics.block_max_coord / scale) + line_box_top;
                        let (inline_area, block_area) = if is_inline_level {
                            let x = (ibox.x / scale) + container_pb.left;
                            (
                                taffy::Line { start: x, end: x },
                                taffy::Line {
                                    start: line_top,
                                    end: line_bottom,
                                },
                            )
                        } else {
                            (
                                taffy::Line {
                                    start: container_pb.left,
                                    end: final_size.width - container_pb.right,
                                },
                                taffy::Line {
                                    start: line_bottom,
                                    end: line_bottom,
                                },
                            )
                        };

                        oof_candidates.push(OofCandidate {
                            node: taffy::NodeId::from(ibox.id),
                            order,
                            position,
                            static_position: taffy::Point {
                                x: AxisStaticPosition::from_alignment(
                                    justify_self.resolve_self_relative(
                                        item_direction,
                                        container_direction,
                                        true,
                                    ),
                                    inline_area,
                                    container_direction.is_rtl(),
                                ),
                                y: AxisStaticPosition::from_alignment(
                                    align_self.resolve_self_relative(
                                        item_direction,
                                        container_direction,
                                        false,
                                    ),
                                    block_area,
                                    false,
                                ),
                            },
                        });
                    } else if is_floated {
                        let layout = self.nodes[NodeId::from_u64(ibox.id)].unrounded_layout_mut();
                        layout.padding = padding; //.map(|p| p / scale);
                        layout.border = border; //.map(|p| p / scale);
                        layout.margin = margin;
                    } else {
                        // Re-measure the box to get its border-box size (this hits the layout
                        // cache). The size cannot be recovered from `ibox` dimensions as the
                        // space reserved in the line is clamped to be non-negative.
                        let mut output =
                            self.compute_child_layout(taffy::NodeId::from(ibox.id), box_inputs);
                        let size = output.size;
                        let node = &mut self.nodes[NodeId::from_u64(ibox.id)];

                        let is_relative = position == taffy::Position::Relative;
                        let inset_offset = if is_relative {
                            taffy::Point {
                                x: if container_direction == Direction::Rtl {
                                    inset.right.map(|x| -x).or(inset.left).unwrap_or(0.0)
                                } else {
                                    inset.left.or(inset.right.map(|x| -x)).unwrap_or(0.0)
                                },
                                y: inset.top.or(inset.bottom.map(|x| -x)).unwrap_or(0.0),
                            }
                        } else {
                            taffy::Point::ZERO
                        };

                        let layout = node.unrounded_layout_mut();
                        layout.size = size;
                        layout.scrollable_overflow_rect = output.scrollable_overflow_rect;
                        layout.location.x =
                            ((ibox.x - trim_x) / scale) + margin.left
                            + container_pb.left + inset_offset.x;
                        // Parley positions the margin box; offset to the border box even
                        // when a negative top margin makes it extend above the margin box.
                        layout.location.y =
                            ((ibox.y - skip_y) / scale) + margin.top
                            + line_box_top + inset_offset.y;
                        layout.padding = padding; //.map(|p| p / scale);
                        layout.border = border; //.map(|p| p / scale);
                        layout.margin = margin;

                        // Translate anchors from item-relative to container-relative
                        // coordinates and collect candidates bubbled from the box's subtree
                        if !output.oof_candidates.is_empty() {
                            let location = layout.location;
                            output.oof_candidates.translate(location);
                            oof_candidates.append(&mut output.oof_candidates);
                        }
                    }
                }
            }
        }

        // println!("INLINE LAYOUT FOR {:?}. max_advance: {:?}", node_id, max_advance);
        // dbg!(&inline_layout.text);
        // println!("Computed: w: {} h: {}", inline_layout.layout.width(), inline_layout.layout.height());
        // println!("known_dimensions: w: {:?} h: {:?}", inputs.known_dimensions.width, inputs.known_dimensions.height);
        // println!("\n");

        let line_baseline = |index: usize, line: parley::Line<'_, _>| {
            let skipped_height = atomic_line_offsets
                .get(index).map_or(0.0, |(height, _)| *height);
            ((line.metrics().baseline - skipped_height) / scale) + line_box_top
        };
        let exports_baseline = has_inline_content && !suppresses_baseline;
        let first_baseline = exports_baseline
            .then(|| inline_layout.layout.lines().next().map(|line| line_baseline(0, line)))
            .flatten();
        let last_line_index = trailing_empty_line.unwrap_or(line_count).checked_sub(1);
        let last_baseline = exports_baseline
            .then(|| last_line_index.and_then(|i| inline_layout.layout.get(i)
                .map(|line| line_baseline(i, line))))
            .flatten();

        if inputs.run_mode == RunMode::PerformLayout {
            inline_layout.span_cb_flags = span_cb_flags;
        }

        // Put layout back
        self.nodes[node_id]
            .data
            .downcast_element_mut()
            .unwrap()
            .inline_layout_data = Some(inline_layout);

        let oof_position_inset = taffy::Rect {
            left: border.left,
            right: border.right + scrollbar_gutter.x,
            top: border.top,
            bottom: border.bottom + scrollbar_gutter.y,
        };

        LayoutOutput {
            size: final_size,
            scrollable_overflow_rect: {
                let content_extent = taffy::Size {
                    width: content_width,
                    height: measured_size.height,
                } + padding.sum_axes();
                taffy::Rect {
                    left: 0.0,
                    right: content_extent.width,
                    top: 0.0,
                    bottom: content_extent.height + block_offset.max(0.0),
                }
            },
            baselines: taffy::Baselines {
                first: first_baseline,
                last: last_baseline,
            },
            top_margin: CollapsibleMarginSet::ZERO,
            bottom_margin: CollapsibleMarginSet::ZERO,
            margins_can_collapse_through: !has_styles_preventing_being_collapsed_through
                && final_size.height == 0.0
                && measured_size.height == 0.0,
            oof_candidates,
            oof_positioning_area: Some(OofPositioningArea {
                size: final_size - oof_position_inset.sum_axes(),
                offset: Point {
                    x: oof_position_inset.left,
                    y: oof_position_inset.top,
                },
            }),
        }
    }

    /// Shape an independent end marker in the style of the inline formatting
    /// context, then position it at the last fitting advance of the clamp line.
    pub(crate) fn install_clamp_marker(
        &mut self,
        node_id: NodeId,
        inline: &mut TextLayout,
        limit: usize,
    ) {
        let Some(last_line) = inline.layout.get(limit - 1) else { return };
        let scale = self.viewport.scale();
        // The owned text style must outlive the computed-style guard; drop the
        // guard before borrowing the layout context mutably to shape the marker.
        let parley_style = {
            let style = self.nodes[node_id].primary_styles().or_else(|| {
                self.nodes[node_id].parent.and_then(|id| self.nodes[id].primary_styles())
            });
            style.as_ref()
                .map(|style| stylo_to_parley::style(node_id, style))
                .unwrap_or_default()
        };

        let mut marker = parley::Layout::default();
        {
            // Borrow the document once so the font and layout contexts are
            // recognized as independent fields during marker shaping.
            let doc: &mut crate::document::BaseDocument = &mut *self;
            let mut font_ctx = doc.font_ctx.lock().unwrap();
            let mut builder = doc.layout_ctx.tree_builder(
                &mut font_ctx, scale, true, &parley_style
            );
            builder.push_text("…");
            builder.build_into(&mut marker);
        }
        marker.break_all_lines(None);
        let Some(marker_line) = marker.lines().next() else { return };
        let marker_width = marker.width();
        let metrics = last_line.metrics();
        let end = last_line.items().fold(metrics.inline_min_coord, |end, item| match item {
            parley::layout::PositionedLayoutItem::GlyphRun(run) =>
                end.max(run.offset() + run.advance()),
            parley::layout::PositionedLayoutItem::InlineBox(b) =>
                end.max(b.x + b.width),
        });
        let marker_x = end.min(metrics.inline_max_coord - marker_width)
            .max(metrics.inline_min_coord);
        let marker_y = metrics.baseline - marker_line.metrics().baseline;
        inline.clamp_visible_lines = Some(limit);
        inline.clamp_marker_at = Some((marker_x, marker_y));
        inline.clamp_marker_cut = Some(marker_x);
        inline.clamp_marker = Some(marker);
    }
}

#[inline(always)]
fn f32_max(a: f32, b: f32) -> f32 {
    a.max(b)
}
