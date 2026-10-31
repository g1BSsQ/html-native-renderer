# CPU raster backend provenance

Vendored from `DioxusLabs/anyrender` at commit `770343654739f60cc98117eb9f8e824545ea246d`
(`crates/anyrender_vello_cpu`, version 0.18.0), dual-licensed
MIT OR Apache-2.0.

The only rendering behavior change is `VelloCpuImageRenderer::render_window`,
which retains the full-viewport rendering context and writes to a
smaller pixmap with the standard zero-offset `RenderContext::render`.
The caller applies an integer device-pixel translation to its recorded
paint commands. `RasterizerSettings::offset` has type `(u16, u16)`
and is not used for negative cropping. See `third-party/anyrender_vello_cpu.patch`
for the upstream delta.

The local `Cargo.toml` uses explicit pinned dependency versions instead of
upstream monorepo workspace inheritance and omits unused window-renderer
optional dependencies. No new registry dependencies are required.
