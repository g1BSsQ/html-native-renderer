# Counter dependency patch

The computed counter-set property requires one source-compatible change to the
0.22.0 release workspace: make it available under the existing
`layout.unimplemented` preference. The renderer enables this preference
before constructing its document configuration.

The root `[patch.crates-io]` redirects all eleven release-workspace crates to
https://github.com/g1BSsQ/stylo, commit `0bd2117ad8598098290936c3c04abf074b2ce279` (parent
`34d25f741765a0bdb894f8271852e633299352d8`). The fork differs from
the published release only in the counter-set property gate. Patching all eleven
crates together preserves the shared selector and counter types.

After checkout, a build or `cargo update -p stylo` must regenerate
`Cargo.lock`. Then regenerate the dependency inventory with
`python tools/dependency-notices.py`. No local build or Cargo update was run;
the present lockfile and inventory still list the pre-patch registry sources.

A host-created default document configuration can still cache counter rules
before the renderer enables the preference. This initialization limitation
requires a separate upstream fix.
