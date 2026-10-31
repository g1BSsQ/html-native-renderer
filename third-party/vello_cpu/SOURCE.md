# CPU renderer source and signed crop

Based on `linebender/vello` tag `sparse-strips-v0.3.0`, commit
`f3000c8d9e7a9c7e6abb09b9587238bf8b4860ff`
(`vello_cpu` 0.3.0). The original project is licensed
MIT OR Apache-2.0. Vendored source files retain upstream headers.

The source changes (also reflected in `third-party/vello_cpu.patch`)
backport signed raster destination offsets while preserving the original
viewport strip/tile coordinates. The only modified upstream Rust modules
are `src/region.rs`, `src/render.rs`, `src/fine/mod.rs`,
`src/fine/lowp/mod.rs`, and `src/fine/highp/mod.rs`.
The manifest replaces workspace-inherited dependency versions with
explicit versions; features and registry dependency families are preserved.

This fork is intentionally narrow because `RasterizerSettings::offset`
in 0.3.0 is unsigned. A negative offset is needed to render original
viewport-space AA coverage into a compact RGBA pixmap without translating
the painting commands before strip generation.

Only the five files listed above have locally added modification headers;
all other Rust source modules match the pinned upstream release.
