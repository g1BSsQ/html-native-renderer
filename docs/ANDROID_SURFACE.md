# Android surface (K8)

CI evidence: [run 40546362713](https://github.com/g1BSsQ/html-native-renderer/actions/runs/40546362713)
at commit `59a58a0a` passed APK assembly and the instrumented
x86_64 emulator smoke test (`OK (1 test)`). The system-bar insets,
screen-to-RGBA comparison, white-screen negative control, and raw-render
`h1` dark-ink assertions **PASS** in that run. Both
`android-smoke.png` and `android-render.png` were uploaded; the
screenshot shows the full card with text below the status bar.
The new host-only scaling tests in this follow-up commit are
**NOT_RUN locally by the author**. K8 remains a Bitmap surface,
not yet a native interactive document view.

## Architecture

```text
bundled HTML asset (01-card / 19-flex-alignment)
  + bundled 515100-byte Roboto font asset (memory-only bytes)
  → MainActivity worker executor (not the UI thread)
  → NativeRenderer.render(html: String, widthCssPx: Int, scale: Float,
                          fonts: Array<ByteArray>)
  → JNI: isr-android cdylib → isr-core::render(RenderOptions { fonts, ... })
  → RenderResult(width, height, rgba: ByteArray, elementBoxesJson: String)
  → Bitmap.Config.ARGB_8888 → ImageView inside inset-padded ScrollView
```

The Rust binding uses `jni 0.21.1` and exports only
`Java_org_htmlnativerenderer_demo_NativeRenderer_render`. The Java-visible
result constructor is `(Int, Int, ByteArray, String)`. RGBA8 bytes are
row-major, with four bytes per pixel; width and height are **bitmap pixels**.
`elementBoxesJson` uses the existing `isr-core` layout schema, with
element rectangles in **CSS pixels**. Only the bitmap is scaled, using
nearest-neighbour resampling (bounded to scale 0.25..=4.0 and 16 million
output pixels). The native entry rejects width outside 1..=8192 CSS px and
HTML inputs over 4 MiB. The JNI font argument accepts up to eight nonempty
font byte arrays, each at most 8 MiB and at most 16 MiB combined; it accepts
no paths. Rust render errors and caught Rust unwinds become Java exceptions
rather than crossing the JNI boundary. **Known allocation limit:** the
16-million-output-pixel check happens **after** `isr-core` has allocated its
full RGBA buffer; large documents can still exhaust memory before this check.
The no-rescale path now moves this buffer rather than cloning it. Out-of-memory
aborts cannot be caught by this mechanism.

The native call uses `RenderOptions { fonts, ..RenderOptions::default() }`
with `system_fonts=false`: no system fonts are discovered by the text stack.
In these fixtures (which declare Arial/sans-serif but no `@font-face`),
**text paints only from host-supplied font bytes**. The demo supplies the
same original Apache-2.0 Roboto Regular bytes already embedded as a data
URI in `fixtures/15-embedded-font.html`; its licence and provenance are
recorded in `NOTICE` and `third-party/Roboto-LICENSE`. The asset is
loaded into memory and sent to JNI; no font paths are passed.

The renderer accepts `data:` subresources only; network and `file:`
subresources are refused. The Android manifest requests **no network
permission**. HTML scripts have no execution engine. **Known exception
inherited from the pinned core**: parsing SVG may read the system font
database/filesystem despite `system_fonts=false`. Therefore this build
is not a filesystem sandbox for arbitrary SVG documents. The two bundled
HTML fixtures contain no SVG.

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
that job explicitly, not silently. The smoke test renders bundled
`01-card.html` with font bytes off the UI thread and saves the native
RGBA to `android-render.png`. It asserts dark text-ink pixels inside the
exported `h1` CSS box, then waits up to 20 seconds for a visible ImageView
whose intrinsic dimensions and document height match the native result.
The Activity pads its ScrollView by system-bar insets, keeping the top of
the document below the status bar. The test excludes both system bars and
checks that on-screen ImageView pixels match the raw rendered RGBA
(with a 12-unit per-channel tolerance). A pixel is counted as painted
only when a channel differs from white by **more than 12**; the
painted-pixel match gate remains 85%. The separate raw-render `h1`
dark-ink check proves text generation; the screen check proves visible
paint but does not independently prove the glyphs are present on screen.
A blank white ImageView fails a negative control. Screen capture retry
is bounded to 10 seconds and saves `android-smoke.png`, even when
paint validation fails. Host-only Rust unit tests cover the nearest-neighbour
scaler at 2.0 and 2.625 without JNI, including rounded dimensions and
known pixel colours (local execution **NOT_RUN**).

The build job uploads `android-demo-apks` (debug and test APKs plus
`sizes.txt`); the smoke job uploads `android-demo-smoke`
(`android-smoke.png` and `android-render.png` when produced, plus
instrumentation and emulator logs). The workflow attempts to retrieve both
PNGs even if the instrumented test fails; missing PNGs are reported as errors.

| Metric | CI run [40546362713](https://github.com/g1BSsQ/html-native-renderer/actions/runs/40546362713), commit `59a58a0a` |
| --- | --- |
| Debug APK bytes | **45,665,575** |
| arm64-v8a `libisr_android.so`, unstripped / packaged bytes | **22,347,384 / 22,347,384** |
| x86_64 `libisr_android.so`, unstripped / packaged bytes | **22,195,904 / 22,195,904** |
| APK assembly | **PASS** |
| x86_64 emulator smoke test | **PASS** (`OK (1 test)`) |
| System-bar insets and correct ImageView dimensions | **PASS** |
| Screen-to-RGBA match, blank-white negative control and `h1` ink | **PASS** |
| `android-smoke.png` and `android-render.png` artifacts | **PASS**; both uploaded and inspected, card and text visible |
| New host-only scaling tests (2.0, 2.625) | **NOT_RUN locally**; CI evidence pending for this follow-up |

These debug APK and native-library sizes come from `sizes.txt` in run
40546362713 at `59a58a0a`. Release builds should strip the native
library; **release stripping and its resulting size have not been
measured**.

Historical context only: [run 40533012267](https://github.com/g1BSsQ/html-native-renderer/actions/runs/40533012267)
painted the card and badge but had status-bar overlap and no text.
[Run 40527665443](https://github.com/g1BSsQ/html-native-renderer/actions/runs/40527665443)
passed a weaker smoke assertion despite a blank screen capture.
Run 40546362713 supersedes those results as the reference evidence for K8.

## Current limits

K8 displays a static Bitmap. The containing Android `ScrollView`
scrolls the bitmap, but **engine-level touch, box-aware scrolling/gestures,
drag, focus, text input, and accessibility semantics are not implemented**.
That interactive surface is K9. No WebView is used. There is no live DOM
update or JavaScript execution. Resource restrictions are not a sandbox
for hostile input; the SVG caveat above remains.
