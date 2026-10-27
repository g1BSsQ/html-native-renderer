# iOS document-to-CGImage surface

Status: prior CI **PASS** at run 40528405833, head
`3ddef24c351a1af4d72caee6ac60f818b1a887bd`; the simulator
smoke test executed 1 test with 0 failures. That screenshot shows the card,
shadow and badge **but no text**. The font-injection and other changes in this
revision are **NOT_RUN** by the author; do not read the prior result as their
validation.

## Architecture

`isr-core::render` remains the one offscreen HTML/CSS engine for all hosts.
`crates/isr-ffi` is a platform-neutral Rust `staticlib` exposing a small C ABI.
It copies the resulting premultiplied RGBA8 channels unchanged into a library-owned output buffer, returns
the existing `Rendered::layout()` element-box JSON schema, and catches Rust
panics at both exported function boundaries. There is no document script
engine or network resource loading. A Swift bridge copies the image bytes,
creates a premultiplied RGBA8 `CGImage` and releases the native allocations.
Rendering runs in a detached task, not on the SwiftUI main thread.

The demo bundles the byte-identical public Roboto Regular font documented in
NOTICE and supplies it as an in-memory byte slice through the extended C ABI.
The demo is one SwiftUI view with a selector for locally bundled copies of
`fixtures/01-card.html` and `fixtures/19-flex-alignment.html`. A SwiftUI
`ScrollView` displays the image at CSS-document size; there is no web view.
The C ABI does not depend on iOS types, so other native hosts can use it.

## C ABI and ownership

See `crates/isr-ffi/include/isr_ffi.h` and `cbindgen.toml`. The checked-in
header is the cbindgen output target; regenerate it with:

```sh
cbindgen --config crates/isr-ffi/cbindgen.toml \
  --crate isr-ffi --output crates/isr-ffi/include/isr_ffi.h
```

`isr_render(html_utf8, len, width_css_px, scale, out)` remains the zero-font
wrapper. `isr_render_with_fonts(html_utf8, len, width_css_px, scale, fonts,
font_count, out)` additionally accepts zero or more borrowed `IsrBytes`
(`data` pointer, `len` size); no file paths, network access, or ownership
transfer. It copies the font bytes into memory before rendering. At most 32
font entries and 64 MiB total font bytes are allowed. Both entry points accept
valid UTF-8 HTML bytes, CSS viewport width 1..=8192, and finite scale 1..=3. `out` must
point to writable memory and must be freed before it is reused. Return
statuses: 0 success, 1 invalid argument, 2 rendering error, 3 caught panic.
On success `pixels` holds row-major RGBA8 premultiplied bytes, `pixels_len`,
pixel `width`/`height`, and a NUL-terminated `element_boxes_json`.
On error `error_message` contains a NUL-terminated message (except when
`out` is NULL). Call `isr_free(out)` after every call with non-NULL output,
even for error; it frees owned buffers and zeroes the structure. Never free a
copy of the same output. The input buffer must remain valid for the call.

The engine currently paints at scale 1 and the FFI expands pixels with
nearest-neighbour sampling for other requested scales. This is not true
high-density rerendering. Maximum output is 128 MiB. **Allocation limit:** isr-core builds
its scale-1 source buffer before the FFI checks this 128 MiB cap. Extremely
large documents can therefore allocate substantially more than 128 MiB
(up to about 1 GiB at core's 8192×32768 maximum) before returning an error.
`RenderOptions` uses the default `system_fonts=false` with only explicitly
supplied in-memory font bytes; only `data:` subresources are accepted. External and filesystem resource requests are
refused. **Known exception in the pinned engine:** parsing SVG can access
the host font configuration/filesystem independently of `system_fonts`;
therefore this is not a full filesystem sandbox for arbitrary SVG HTML.
The two bundled demo fixtures do not contain SVG.

## Build locally (macOS + Xcode)

Requires Xcode with iOS simulator, Rust 1.95.0 with both Apple ARM64 targets,
XcodeGen and cbindgen for header regeneration. From the repository root:

```sh
rustup toolchain install 1.95.0 --profile minimal
rustup target add --toolchain 1.95.0 aarch64-apple-ios-sim aarch64-apple-ios

SDKROOT="$(xcrun --sdk iphonesimulator --show-sdk-path)" \
CARGO_TARGET_AARCH64_APPLE_IOS_SIM_LINKER="$(xcrun --sdk iphonesimulator --find clang)" \
  cargo +1.95.0 build --locked --release -p isr-ffi --target aarch64-apple-ios-sim

SDKROOT="$(xcrun --sdk iphoneos --show-sdk-path)" \
CARGO_TARGET_AARCH64_APPLE_IOS_LINKER="$(xcrun --sdk iphoneos --find clang)" \
  cargo +1.95.0 build --locked --release -p isr-ffi --target aarch64-apple-ios

cd ios/demo
xcodegen generate --spec project.yml
xcodebuild build -project IsrDemo.xcodeproj -scheme IsrDemo \
  -destination 'generic/platform=iOS Simulator' CODE_SIGNING_ALLOWED=NO
```

Choose an available iPhone simulator and run the `IsrDemo` test scheme in
Xcode, or run `xcodebuild test` with its device id. No code signing,
provisioning profiles, or network permissions are needed.

The CI workflow `.github/workflows/ios-demo.yml` runs on pull requests and
pushes to main. Its `simulator-smoke` job builds both staticlibs, builds the
simulator demo, checks image width/nonempty/non-uniform pixels for 01-card,
and exports the XCTest screenshot attachment in `ios-demo-smoke`.

## Limits and first CI measurement

This is a static image surface: no engine-owned touch hit testing, DOM scrolling,
native HTML controls, editable input, navigation, or incremental repaint.
The SwiftUI ScrollView scrolls the **image**, not the HTML layout tree. The
C ABI returns box geometry JSON but does not yet attach event handlers.
The prior no-font simulator capture at run 40528405833 shows **no text**.
The revised demo supplies one bundled font in memory; the new test inspects
dark pixels inside the h1 element's JSON box and saves both the direct bridge
image and a SwiftUI `ContentView` image. These changes are **NOT_RUN** by
the author.

| Artifact | Compile/test | Measured size |
| --- | --- | --- |
| aarch64-apple-ios-sim `libisr_ffi.a` | PASS (prior run 40528405833) | 71,283,536 B (unstripped `.a`) |
| aarch64-apple-ios `libisr_ffi.a` | PASS (prior run 40528405833) | 71,394,120 B (unstripped `.a`) |
| simulator XCTest + screenshot | PASS (prior run 40528405833, head `3ddef24c`; 1 test, 0 failures; no text) | n/a |
| revised FFI, fonts, view and updated CI | **NOT_RUN** by the author | **NOT_RUN** |

The byte counts above come from `ios-demo-smoke/size.txt` in run
40528405833 and describe the **previous** build only. Record fresh size
measurements for this new revision from its own CI artifact; values may
change. Inspect the XCTest result and both screenshot attachments, especially
heading ink and the complete view. Device runtime remains untested.

The maintainer already regenerated `Cargo.lock` at `d8165086`; both
staticlib build commands use `--locked`. The new CI also runs Rust FFI
failure-path tests and verifies the committed header with cbindgen 0.29.2.
XcodeGen 2.44.1 is downloaded with a pinned SHA-256.
