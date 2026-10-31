# CPU raster backend provenance

Vendored from `DioxusLabs/anyrender` at commit `770343654739f60cc98117eb9f8e824545ea246d`
(`crates/anyrender_vello_cpu`, version 0.18.0), dual-licensed
MIT OR Apache-2.0.

The only rendering behavior change is `VelloCpuImageRenderer::render_window`,
which keeps the full-viewport rendering context and draws into a smaller
pixmap using `vello_cpu::RasterizerSettings::offset`. It does not
modify paint command transforms. See `third-party/anyrender_vello_cpu.patch`
for the upstream delta.

The local `Cargo.toml` uses explicit pinned dependency versions instead of
upstream monorepo workspace inheritance and omits unused window-renderer
optional dependencies. No new registry dependencies are required.
