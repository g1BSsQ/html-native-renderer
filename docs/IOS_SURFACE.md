# iOS document-to-CGImage surface

Status: **NOT_RUN** by the author. The first CI run is the build, smoke-test
and staticlib size evidence; no successful run or number is claimed here.

## Architecture

`isr-core::render` remains the one offscreen HTML/CSS engine for all hosts.
`crates/isr-ffi` is a platform-neutral Rust `staticlib` exposing a small C ABI.
It copies the resulting RGBA8 bytes into a library-owned output buffer, returns
the existing `Rendered::layout()` element-box JSON schema, and catches Rust
panics at both exported function boundaries. There is no document script
engine or network resource loading. A Swift bridge copies the image bytes,
creates a premultiplied RGBA8 `CGImage` and releases the native allocations.
Rendering runs in a detached task, not on the SwiftUI main thread.

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

`isr_render(html_utf8, len, width_css_px, scale, out)` accepts valid UTF-8
bytes, CSS viewport width 1..=8192, and finite scale 1..=3. `out` must
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
high-density rerendering. Maximum output is 128 MiB. `RenderOptions` uses
the default `system_fonts=false` and no host font bytes; only `data:`
subresources are accepted. External and filesystem resource requests are
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
  cargo +1.95.0 build --release -p isr-ffi --target aarch64-apple-ios-sim

SDKROOT="$(xcrun --sdk iphoneos --show-sdk-path)" \
CARGO_TARGET_AARCH64_APPLE_IOS_LINKER="$(xcrun --sdk iphoneos --find clang)" \
  cargo +1.95.0 build --release -p isr-ffi --target aarch64-apple-ios

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
Default system fonts are disabled, so text in these fixtures may not paint
without explicit in-memory fonts. Image colours and geometry can still be
smoke-tested.

| Artifact | Compile/test | Measured size |
| --- | --- | --- |
| aarch64-apple-ios-sim `libisr_ffi.a` | NOT_RUN | NOT_RUN |
| aarch64-apple-ios `libisr_ffi.a` | NOT_RUN | NOT_RUN |
| simulator XCTest + screenshot | NOT_RUN | n/a |

After the first CI run, use artifact `ios-demo-smoke/size.txt` for **measured
unstripped staticlib bytes** on both targets, and inspect `smoke.xcresult`
and exported screenshots. Do not infer device behavior from a simulator pass.

The new workspace package changes dependency resolution metadata.
`Cargo.lock` was deliberately not hand-edited. The maintainer must
regenerate and commit it before requiring `--locked` checks to pass; the
new workflow's Cargo commands intentionally resolve locally without
`--locked` until then.
