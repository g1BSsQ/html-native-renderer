# Published CPU image backend provenance

Origin: https://github.com/DioxusLabs/anyrender
Published crate: `anyrender_vello_cpu` 0.18.0 (crates.io).
Published crate VCS commit: `cf0f14776102ec775af61bcdcaac2ab345f4fafb`.
Licence: MIT OR Apache-2.0; complete texts in `LICENSE-MIT`
and `LICENSE-APACHE`.

Copied paths: `third-party/anyrender_vello_cpu/src/` (all seven published
source modules, including `filters.rs`), and the two licence files.
All `src/` files are byte-identical to published 0.18.0 source at the
recorded VCS commit **except** `src/image_renderer.rs`, which adds only
`VelloCpuImageRenderer::render_window` using signed output cropping.
Exact upstream delta: `third-party/anyrender_vello_cpu.patch`,
generated against the published 0.18.0 source.

The local `Cargo.toml` replaces upstream workspace-inherited fields
with pinned dependencies and trims optional window-renderer features;
it is not the published Cargo manifest. The local signed raster API
comes from `third-party/vello_cpu.patch`. On an upstream bump,
re-vendor the published source, reapply only the minimal backend method,
regenerate both patches, and rerun pixel-parity tests.
