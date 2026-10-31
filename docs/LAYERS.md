# Explicit retained compositing layers

## Purpose

Continuous motion should not re-enter style resolution, layout or raster
painting. A document can explicitly promote a stacking context, retain its
raster, and change only the layer transform or alpha during a motion frame.
Promotion is never triggered automatically by a style animation.

## API and units

`Document::promote(NodeRef) -> Result<LayerId>` isolates one subtree.
A node without a stacking context is given `isolation:isolate` once during
promotion. `set_layer_transform(id, LayerTransform)` takes a six-coefficient
2D affine in CSS pixels about the border-box top-left origin. The supported
operations include translation, scale, rotation and their compositions.
`set_layer_opacity(id, f32)` takes a finite value in [0, 1].
`demote(id)` returns the subtree to normal in-place raster painting.
`layers()` exposes paint-order entries with cropped premultiplied RGBA8,
device-pixel origin and extent, CSS border-box bounds, transform, opacity,
clips and stable identifiers. `composite()` is the CPU compositor.

A promoted bitmap is rasterized from its subtree's recorded paint commands.
It remains unchanged while its transform or opacity changes. The host can
instead retain the layer bitmap in a rendering node and update its transform
and alpha without another native raster call.

## Paint order and base segments

The painter emits nonpainting boundaries immediately before and after each
promoted stacking context. The recorded command stream is partitioned at
those boundaries. Each intervening base segment remains in its original
paint position, including later siblings and hoisted positive stacking
content. Open parent clip commands are copied around base segments.
Promoted subtrees are extracted without their parent's clip commands;
fixed ancestor clips are reapplied after each motion transform.

The ordered stream is thus base segment, layer, base segment, layer, and so
on. Base segments and promoted layers have independent bitmaps. The
compositor blends these in order; there is no unconditional "layers on
top" pass. The recording and initial raster work happen on promotion.
Movement does not rebuild the recording, style, layout or raster.

For a subtree-local mutation whose border box is unchanged, the next
`update()` resolves the DOM and captures a new paint recording but
rasterizes only its damaged promoted layer; the other layer bitmaps and
all base segments are retained. Structural or geometry changes cause a
conservative full segment rebuild. The counters
`style_layout_resolves`, `layer_rasters` and `segment_rasters`
distinguish motion-only compositing from both update paths.

## Supported and fallback cases

Supported: explicitly promoted element stacking contexts; 2D affine
transforms; group opacity; ordinary overlapping painted siblings; fixed
rectangular ancestor overflow clips; equal-radius rounded overflow clips
with pixel lengths. These clips remain fixed while the layer moves.
The promotion path rejects nested promotions.

Unsupported promotions return `LayerFallback` with a
`LayerFallbackReason` instead of silently altering paint order.
`UnsupportedAncestorEffect` covers ancestor filters, backdrop filters,
blend groups, masks, opacity groups and transforms.
`UnsupportedOwnEffect` covers those effects on the promoted root except
ordinary opacity. `UnsupportedClip` includes ancestor clip-path,
mixed-axis overflow clips, percentage/asymmetric rounded clips and other
unsupported clip forms. `MissingBox` covers invisible or nonfinite boxes,
and `NonStackingContext` is returned if creating the isolated context
does not yield a paintable stacking context.
In fallback cases the document remains on its original in-place painter
path; no independent raster layer is published for the unsupported node.
Descendant effects inside a supported promoted subtree are included in
that subtree's recorded raster.

The current CPU oracle samples transformed rasters at device-pixel centers
using nearest-neighbor reconstruction, so interpolated rotations and
fractional translations are not claimed pixel-identical to re-rasterized
vector paths. At identity, it can reuse the exact retained original page.
Ancestor rounded-clip reconstruction is conservative and not a general
replacement for arbitrary vector clip paths.

## Host path

On API 29 and later the demo's compositing page records each ordered
bitmap in a separate `RenderNode`, draws the nodes in painter order,
and updates translation, scale, rotation and alpha of the selected card
during a drag. Ancestor clips are installed on the host drawing canvas
outside the moved rendering node. The existing bitmap path is selected
below API 29 or when no supported promotion is returned. The example
activity is separate from the existing touch owner.

## Benchmark and verification

The benchmark includes `layers_200_7_motion`: a 200-row list with a
selected card and six adjacent promoted rows moved over 120 frames.
Run it separately from the three existing interaction workloads with:\n\n\`cargo run --release -p isr-bench -- 392 --height 800 --scale 2.75 --layers\`\n\nIt separately reports per-frame renderer-side microseconds for setters
alone and setters plus the CPU compositor, each with p50, p95 and max.
It also reports the number of style/layout resolves and both raster
counts occurring during those frames. The setter-only measurement is the
host compositor path; it excludes host drawing time.

New integration tests cover identity, translated pixels, opacity,
overlap order, overflow and rounded clips, a fallback reason,
zero-work motion counters and promoted-only raster damage.

No host, device or benchmark measurements were run in the authoring
environment. No performance result or test pass is claimed here.
