# Pinned upstream patches

The structure renderer requires no upstream source patch. All changes are local integration
code under `crates/isr-core/src/`. Dependency revisions remain pinned to the
full commits in the lockfile; no third-party source has been copied here.

Known upstream initialization limitation: if the host creates a Blitz
`DocumentConfig::default()` before the first renderer call, Blitz may already
have cached the default user-agent stylesheet without its counter rules.
The renderer configures the counter preference before creating documents but
cannot repair a stylesheet cached earlier in the host process. This will
need an upstream parse-order fix after the pinned dependency is updated.
