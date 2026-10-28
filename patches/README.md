# Pinned upstream patches

The structure renderer requires no upstream source patch. Structure and geometry changes remain local integration code under
`crates/isr-core/src/`; they do not patch upstream Blitz. The separate fonts
capability vendors the pinned `blitz-dom` and `blitz-paint` crates under
`third-party/`, with Cargo path overrides. It does not apply a checkout patch
script. The source revision and deviations are recorded in `NOTICE`.

Known upstream initialization limitation: if the host creates a Blitz
`DocumentConfig::default()` before the first renderer call, Blitz may already
have cached the default user-agent stylesheet without its counter rules.
The renderer configures the counter preference before creating documents but
cannot repair a stylesheet cached earlier in the host process. This will
need an upstream parse-order fix after the pinned dependency is updated.
