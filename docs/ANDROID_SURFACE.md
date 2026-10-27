# Android surface (K8)

Status of this follow-up change: **NOT_RUN** (no local build, instrumented
test, screenshot inspection, or new size measurement). CI run
[40527665443](https://github.com/g1BSsQ/html-native-renderer/actions/runs/40527665443)
measured the original debug build and reported a passing smoke test, but its
screenshot was blank. This commit strengthens the on-screen evidence checks;
their outcome is **NOT_RUN** until new CI. K8 remains a Bitmap surface,
not yet a native interactive document view.

## Architecture

```text
bundled HTML asset (01-card / 19-flex-alignment)
  → MainActivity worker executor (not the UI thread)
  → NativeRenderer.render(html: String, widthCssPx: Int, scale: Float)
  → JNI: isr-android cdylib → isr-core::render(RenderOptions::default())
  → RenderResult(width, height, rgba: ByteArray, elementBoxesJson: String)
  → Bitmap.Config.ARGB_8888 → ImageView in ScrollView
```

The Rust binding uses `jni 0.21.1` and exports only
`Java_org_htmlnativerenderer_demo_NativeRenderer_render`. The Java-visible
result constructor is `(Int, Int, ByteArray, String)`. RGBA8 bytes are
row-major, with four bytes per pixel; width and height are **bitmap pixels**.
`elementBoxesJson` uses the existing `isr-core` layout schema, with
element rectangles in **CSS pixels**. Only the bitmap is scaled, using
nearest-neighbour resampling (bounded to scale 0.25..=4.0 and 16 million
output pixels). The native entry rejects width outside 1..=8192 CSS px and
HTML inputs over 4 MiB. Rust render errors and caught Rust unwinds become
Java exceptions rather than crossing the JNI boundary. Out-of-memory aborts
cannot be caught by this mechanism.

The native call explicitly uses `RenderOptions::default()`: system font
discovery is off, and the renderer accepts `data:` resources only; network
and `file:` subresources are refused. The Android manifest requests **no
network permission**. HTML scripts have no execution engine. **Known
exception inherited from the pinned core**: parsing SVG may read the system
font database/filesystem despite `system_fonts=false`. Therefore this
build is not a filesystem sandbox for arbitrary SVG documents. The two demo
assets contain no SVG. Text requiring installed fonts may be missing until
fonts are supplied as in-memory assets.

## Build locally (debug only)

Requirements: Rust 1.95.0, Android SDK with API 35/build-tools 35.0.0,
NDK `27.3.13750724`, Android API 26 Clang target linkers, JDK 17, a
Gradle 8.11.1 installation, and a C/C++ host toolchain with libclang and
pkg-config. From the repository root on a Linux x86_64 host:

```sh
rustup toolchain install 1.95.0 --profile minimal
rustup target add --toolchain 1.95.0 aarch64-linux-android x86_64-linux-android
"$ANDROID_HOME/cmdline-tools/latest/bin/sdkmanager" --install \
  "ndk;27.3.13750724" "platforms;android-35" "build-tools;35.0.0"
export ANDROID_NDK_HOME="$ANDROID_HOME/ndk/27.3.13750724"
export NDK_HOME="$ANDROID_NDK_HOME"
bin="$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/bin"
export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$bin/aarch64-linux-android26-clang"
export CARGO_TARGET_X86_64_LINUX_ANDROID_LINKER="$bin/x86_64-linux-android26-clang"
export CC_aarch64_linux_android="$bin/aarch64-linux-android26-clang"
export CXX_aarch64_linux_android="$bin/aarch64-linux-android26-clang++"
export AR_aarch64_linux_android="$bin/llvm-ar"
export CC_x86_64_linux_android="$bin/x86_64-linux-android26-clang"
export CXX_x86_64_linux_android="$bin/x86_64-linux-android26-clang++"
export AR_x86_64_linux_android="$bin/llvm-ar"
cargo +1.95.0 build --locked --release -p isr-android --target aarch64-linux-android
cargo +1.95.0 build --locked --release -p isr-android --target x86_64-linux-android
mkdir -p android/demo/app/src/main/jniLibs/{arm64-v8a,x86_64}
cp target/aarch64-linux-android/release/libisr_android.so android/demo/app/src/main/jniLibs/arm64-v8a/
cp target/x86_64-linux-android/release/libisr_android.so android/demo/app/src/main/jniLibs/x86_64/
cd android/demo
gradle --no-daemon :app:assembleDebug :app:assembleDebugAndroidTest
```

Install `app/build/outputs/apk/debug/app-debug.apk` onto an Android
API 26+ device/emulator. The default asset is `01-card.html`; launch
`MainActivity` with string extra `fixture=19-flex-alignment.html` to
show the second bundled fixture. Only debug APKs are produced; no signing
keys are checked in.

## CI / first run evidence

`.github/workflows/android-demo.yml` runs on pull requests and main pushes.
`build-apk` cross-compiles the cdylib for **arm64-v8a** and **x86_64**
with NDK API 26, packages the two libraries into the debug APK, and builds
the instrumented-test APK. `emulator-smoke` runs separately on a hosted
x86_64 emulator with acceleration **required**; missing `/dev/kvm` fails
that job explicitly, not silently. The revised smoke test renders bundled
`01-card.html` off the UI thread, checks non-empty RGBA, a pixel width
matching the Activity's CSS viewport and density, more than one raw bitmap
colour, and layout JSON. It saves the raw `android-render.png`. It then
waits at most 20 seconds for the Activity's visible ImageView to have a
drawable with matching intrinsic bitmap dimensions and exact document-pixel
height. After UI idleness, it waits at most another 10 seconds for an actual
screen capture with more than one colour **inside the ImageView's visible
on-screen bounds**. It saves `android-smoke.png` even if that colour
assertion fails, so a blank screen is a failing test with screenshot evidence.

The build job uploads `android-demo-apks` (debug and test APKs plus
`sizes.txt`); the smoke job uploads `android-demo-smoke`
(`android-smoke.png` and `android-render.png` when produced, plus
instrumentation and emulator logs). The workflow attempts to retrieve both
PNGs even if the instrumented test fails; missing PNGs are reported as errors.

| Metric | CI run 40527665443 (previous commit) |
| --- | --- |
| Debug APK bytes | **45,372,140** |
| arm64-v8a `libisr_android.so`, unstripped / packaged bytes | **22,337,624 / 22,337,624** |
| x86_64 `libisr_android.so`, unstripped / packaged bytes | **22,185,232 / 22,185,232** |
| APK assembly | **PASS** |
| x86_64 emulator smoke | **PASS** under the old test; **blank light-grey screenshot** (evidence gap) |
| Revised drawable, layout, screen-pixel assertions and both PNG artifacts | **NOT_RUN** |

These are measured **debug** artifact sizes from `android-demo-apks/sizes.txt`
in run 40527665443, not predictions or measurements of this follow-up
commit. Release builds should strip the native library; **release stripping
and its resulting size are not measured here**.

The maintainer must inspect the next CI run's `android-smoke.png` beside
`android-render.png`. The new test fails if the Activity cannot display
its bitmap or its screen pixels remain monochrome, even if direct native
rendering succeeds. Capture or acceleration failures must remain visible.

## Current limits

K8 displays a static Bitmap. The containing Android `ScrollView`
scrolls the bitmap, but **engine-level touch, box-aware scrolling/gestures,
drag, focus, text input, and accessibility semantics are not implemented**.
That interactive surface is K9. No WebView is used. There is no live DOM
update or JavaScript execution. Resource restrictions are not a sandbox
for hostile input; the SVG caveat above remains.
