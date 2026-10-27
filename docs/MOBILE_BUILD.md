# Mobile cross-compile probe (Week 2)

Status: **NOT_RUN** by the author. The CI jobs collect build evidence; neither a
successful compile nor a binary-size result is claimed here. These are compile
checks, not mobile device tests or app packaging.

## Toolchains and runner assumptions

| Item | Android | iOS |
|---|---|---|
| Hosted runner | `ubuntu-latest` (rolling image) | `macos-latest` (rolling image) |
| Rust | 1.95.0 (installed explicitly) | 1.95.0 (installed explicitly) |
| Rust target | `aarch64-linux-android` | `aarch64-apple-ios` |
| Native toolchain | Android NDK `27.3.13750724`; API 26 Clang linker | Runner's selected Xcode, iPhoneOS SDK and Clang; exact versions recorded by CI |
| Toolchain versions at run time | `out/android/toolchain.txt` | `out/ios/toolchain.txt` |

The Ubuntu hosted image is assumed to provide `ANDROID_HOME`, the Android
command-line SDK manager at `$ANDROID_HOME/cmdline-tools/latest/bin/sdkmanager`,
a Java runtime and accepted Android SDK licences. The workflow installs the exact
NDK revision through that SDK manager rather than assuming an NDK revision is
preinstalled. Host `clang`, `libclang-dev`, `pkg-config` and
`libfontconfig1-dev` are installed for native build scripts. No target
fontconfig libraries are fabricated or linked from the host.

The macOS hosted image is assumed to provide a selected Xcode installation and
the iPhoneOS SDK accessible via `xcrun`. The SDK path and selected Clang are
set for the cross-compiler; the exact Xcode and SDK versions are logged, not
assumed to be stable across rolling runner images. Both jobs use only standard
hosted runners and pinned checkout/artifact actions.

## Commands used by CI

Run from the repository root. After installing the host build packages, Android
runs the following (the exported values also apply to Cargo's build scripts):

```sh
rustup toolchain install 1.95.0 --profile minimal
rustup target add --toolchain 1.95.0 aarch64-linux-android
"$ANDROID_HOME/cmdline-tools/latest/bin/sdkmanager" --install "ndk;27.3.13750724"
export ANDROID_NDK_HOME="$ANDROID_HOME/ndk/27.3.13750724"
export NDK_HOME="$ANDROID_NDK_HOME"
bin="$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/bin"
export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$bin/aarch64-linux-android26-clang"
export CC_aarch64_linux_android="$bin/aarch64-linux-android26-clang"
export CXX_aarch64_linux_android="$bin/aarch64-linux-android26-clang++"
export AR_aarch64_linux_android="$bin/llvm-ar"
cargo +1.95.0 tree --locked -e normal --target aarch64-linux-android -p isr
cargo +1.95.0 build --locked --release -p isr --target aarch64-linux-android
"$bin/llvm-size" target/aarch64-linux-android/release/isr
wc -c < target/aarch64-linux-android/release/isr
```

iOS runs the following on the macOS runner:

```sh
rustup toolchain install 1.95.0 --profile minimal
rustup target add --toolchain 1.95.0 aarch64-apple-ios
export SDKROOT="$(xcrun --sdk iphoneos --show-sdk-path)"
export CARGO_TARGET_AARCH64_APPLE_IOS_LINKER="$(xcrun --sdk iphoneos --find clang)"
# If Xcode provides libclang.dylib, the workflow sets LIBCLANG_PATH to its directory.
cargo +1.95.0 tree --locked -e normal --target aarch64-apple-ios -p isr
cargo +1.95.0 build --locked --release -p isr --target aarch64-apple-ios
xcrun --sdk iphoneos size target/aarch64-apple-ios/release/isr
wc -c < target/aarch64-apple-ios/release/isr
```

The workflow captures the dependency tree in `cargo-tree.txt` and the build
output in `build.log`. Each successful build also uploads the unstripped
release binary (`isr`), `size.txt`, and `binary-bytes.txt`, alongside
`toolchain.txt`. Failed builds fail their job normally; the always-running
artifact step uploads any evidence collected before the failure. `size.txt`
and the binary do not exist when compilation or linking fails.

## Crate features and iOS binary choice

No crates, target-specific features, or dependencies are changed, added,
disabled, or conditionally excluded. Both targets compile the unchanged locked
`isr` dependency graph, including the existing `blitz-dom` features `svg`,
`woff` and `system-fonts`. These features may reveal platform-specific
build-script or native-library failures; such failures are the purpose of the
probe and must not be hidden.

`isr` is currently a binary-only package. No library target or reusable
render-entry library exists. The iOS job therefore attempts to **link the
existing binary** instead of building an empty probe or treating dependency
resolution as a successful compile. Whether this link works is **NOT_RUN**.
If iOS rejects the executable or one of its dependencies, the job must fail
with the link or compilation error. A future change can introduce a staticlib
that calls the same renderer entry point after the failure is understood;
that change is not part of this compile-only probe.

## CI results (maintainer fills after the first run)

| Target | Compile/link | Release binary bytes | Size tool output / observation |
|---|---|---:|---|
| Android aarch64 (API 26) | NOT_RUN | NOT_RUN | NOT_RUN |
| iOS aarch64 | NOT_RUN | NOT_RUN | NOT_RUN |

Artifacts: `mobile-android` and `mobile-ios`, respectively. Record any
failed dependency or linker diagnostic with its job run when filling the table.
These measurements do not establish device compatibility, runtime performance,
font availability or app integration.
