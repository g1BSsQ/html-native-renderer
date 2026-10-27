// Copyright 2026 g1BSsQ. Licensed under Apache-2.0.
package org.htmlnativerenderer.demo

import android.graphics.Bitmap
import android.graphics.Color
import android.graphics.Rect
import android.os.Build
import android.os.SystemClock
import android.view.WindowInsets
import android.widget.ImageView
import androidx.test.core.app.ActivityScenario
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import java.io.File
import kotlin.math.abs
import kotlin.math.ceil
import kotlin.math.floor
import kotlin.math.roundToInt

@RunWith(AndroidJUnit4::class)
class RenderSmokeTest {
    private fun savePng(bitmap: Bitmap, destination: File) {
        destination.outputStream().use { out ->
            assertTrue("PNG encode failed: " + destination.name,
                bitmap.compress(Bitmap.CompressFormat.PNG, 100, out))
        }
        assertTrue("PNG evidence is empty: " + destination.name, destination.length() > 0)
    }

    // The layout JSON coordinates are CSS pixels; RGBA and bitmap dimensions
    // are device pixels. A genuine text rendering must put dark ink in h1.
    private fun assertHeadingInk(result: RenderResult, scale: Float) {
        val elements = JSONObject(result.elementBoxesJson).getJSONArray("elements")
        var box: JSONObject? = null
        for (index in 0 until elements.length()) {
            val element = elements.getJSONObject(index)
            if (element.getString("tag") == "h1") {
                box = element
                break
            }
        }
        val heading = requireNotNull(box) { "element-box JSON has no h1" }
        val left = floor(heading.getDouble("x") * scale).toInt().coerceIn(0, result.width)
        val top = floor(heading.getDouble("y") * scale).toInt().coerceIn(0, result.height)
        val right = ceil(
            (heading.getDouble("x") + heading.getDouble("width")) * scale
        ).toInt().coerceIn(0, result.width)
        val bottom = ceil(
            (heading.getDouble("y") + heading.getDouble("height")) * scale
        ).toInt().coerceIn(0, result.height)
        assertTrue("h1 must have non-empty pixel bounds", right > left && bottom > top)
        var inkPixels = 0
        for (y in top until bottom) {
            for (x in left until right) {
                val offset = (y * result.width + x) * 4
                val rgba = result.rgba
                val r = rgba[offset].toInt() and 0xff
                val g = rgba[offset + 1].toInt() and 0xff
                val b = rgba[offset + 2].toInt() and 0xff
                val a = rgba[offset + 3].toInt() and 0xff
                if (r < 120 && g < 120 && b < 120 && a > 0) inkPixels++
            }
        }
        assertTrue(
            "h1 must contain dark text ink from the bundled font (dark pixels=" +
                inkPixels + ")", inkPixels >= 12,
        )
    }

    // Compare actual screenshot pixels with the JNI RGBA buffer at the same
    // ImageView coordinates. Crop both to the on-screen visible part of the
    // ImageView AND the window's system-bar-free content rectangle. System
    // icons, navigation gestures or a blank-white ImageView cannot satisfy
    // this check merely by contributing multiple colours.
    private fun matchesRawRgba(
        capture: Bitmap, result: RenderResult, origin: IntArray,
        visibleBounds: Rect, systemBars: Rect,
    ): Boolean {
        val inside = Rect(visibleBounds)
        if (!inside.intersect(
                systemBars.left, systemBars.top,
                capture.width - systemBars.right, capture.height - systemBars.bottom
            )) return false
        var samples = 0
        var matched = 0
        var painted = 0
        var matchedPaint = 0
        for (y in inside.top until inside.bottom step 2) {
            val srcY = y - origin[1]
            if (srcY !in 0 until result.height) continue
            for (x in inside.left until inside.right step 2) {
                val srcX = x - origin[0]
                if (srcX !in 0 until result.width) continue
                val offset = (srcY * result.width + srcX) * 4
                val red = result.rgba[offset].toInt() and 0xff
                val green = result.rgba[offset + 1].toInt() and 0xff
                val blue = result.rgba[offset + 2].toInt() and 0xff
                val actual = capture.getPixel(x, y)
                val same = abs(red - Color.red(actual)) <= 12 &&
                    abs(green - Color.green(actual)) <= 12 &&
                    abs(blue - Color.blue(actual)) <= 12
                samples++
                if (same) matched++
                // White pixels alone do not prove that the document painted.
                if (red < 250 || green < 250 || blue < 250) {
                    painted++
                    if (same) matchedPaint++
                }
            }
        }
        return samples > 0 && painted >= 100 &&
            matched * 100 >= samples * 90 &&
            matchedPaint * 100 >= painted * 85
    }

    @Test
    fun bundledCardRendersAndAppearsInActivity() {
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        val context = instrumentation.targetContext
        val evidenceDir = requireNotNull(context.getExternalFilesDir(null)) {
            "app-specific external files directory is unavailable"
        }
        val html = context.assets.open("01-card.html").bufferedReader().use { it.readText() }
        val font = context.assets.open("Roboto-Regular.ttf").use { it.readBytes() }

        // Same fixture, font bytes, CSS width and density as MainActivity.
        val density = context.resources.displayMetrics.density
        val cssWidth = (context.resources.displayMetrics.widthPixels / density)
            .toInt().coerceAtLeast(1)
        val result = NativeRenderer.render(html, cssWidth, density, arrayOf(font))
        assertEquals("native bitmap width", (cssWidth * density).roundToInt(), result.width)
        assertTrue("native bitmap height", result.height > 0)
        assertEquals("RGBA byte count", result.width.toLong() * result.height * 4,
            result.rgba.size.toLong())
        assertTrue("element-box JSON", result.elementBoxesJson.contains("\"elements\""))
        val first = result.rgba.copyOfRange(0, 4)
        assertTrue("native render must contain more than one colour",
            (4 until result.rgba.size step 4).any { offset ->
                (0..3).any { channel -> result.rgba[offset + channel] != first[channel] }
            })

        // Preserve raw render evidence if the heading/Activity assertion fails.
        val rawBitmap = result.toBitmap()
        try {
            savePng(rawBitmap, File(evidenceDir, "android-render.png"))
        } finally {
            rawBitmap.recycle()
        }
        assertHeadingInk(result, density)

        ActivityScenario.launch(MainActivity::class.java).use { scenario ->
            val deadline = SystemClock.elapsedRealtime() + 20_000L
            var ready = false
            var lastState = "Activity not inspected"
            var visibleBounds = Rect()
            var imageOrigin = intArrayOf(0, 0)
            var systemBars = Rect()

            while (SystemClock.elapsedRealtime() < deadline) {
                // This callback runs on the UI thread; polling sleeps below run
                // only on the instrumentation thread.
                scenario.onActivity { activity ->
                    val image = activity.findViewById<ImageView>(R.id.document_image)
                    val drawable = image.drawable
                    val local = Rect()
                    val visible = image.getLocalVisibleRect(local)
                    val location = IntArray(2)
                    image.getLocationOnScreen(location)
                    imageOrigin = location
                    visibleBounds = Rect(
                        location[0] + local.left, location[1] + local.top,
                        location[0] + local.right, location[1] + local.bottom,
                    )
                    val insets = image.rootWindowInsets
                    systemBars = when {
                        insets == null -> Rect()
                        Build.VERSION.SDK_INT >= Build.VERSION_CODES.R -> {
                            val bars = insets.getInsets(WindowInsets.Type.systemBars())
                            Rect(bars.left, bars.top, bars.right, bars.bottom)
                        }
                        else -> {
                            @Suppress("DEPRECATION")
                            Rect(
                                insets.systemWindowInsetLeft, insets.systemWindowInsetTop,
                                insets.systemWindowInsetRight, insets.systemWindowInsetBottom,
                            )
                        }
                    }
                    ready = drawable != null &&
                        drawable.intrinsicWidth == result.width &&
                        drawable.intrinsicHeight == result.height &&
                        image.width == result.width && image.height == result.height &&
                        image.isShown && visible && !local.isEmpty
                    lastState = "drawable=" +
                        (drawable?.let { it.intrinsicWidth.toString() + "x" + it.intrinsicHeight }
                            ?: "null") +
                        ", expected=" + result.width + "x" + result.height +
                        ", view=" + image.width + "x" + image.height +
                        ", shown=" + image.isShown + ", visible=" + visible +
                        ", screenBounds=" + visibleBounds + ", systemInsets=" + systemBars
                }
                if (ready) break
                SystemClock.sleep(100)
            }
            assertTrue(
                "Timed out after 20s waiting for a visible ImageView with the " +
                    "expected drawable and full document-height layout: " + lastState,
                ready,
            )
            assertTrue("document must start below the status bar: " + lastState,
                imageOrigin[1] >= systemBars.top)

            instrumentation.waitForIdleSync()

            // Bounded capture retry; store even a mismatched/blank final image
            // for the maintainer to inspect if CI fails.
            val paintDeadline = SystemClock.elapsedRealtime() + 10_000L
            var screenshot: Bitmap? = null
            var matched = false
            do {
                val capture = requireNotNull(instrumentation.uiAutomation.takeScreenshot()) {
                    "UiAutomation.takeScreenshot() returned null"
                }
                screenshot?.recycle()
                screenshot = capture
                matched = matchesRawRgba(
                    capture, result, imageOrigin, visibleBounds, systemBars,
                )
                if (matched) break
                SystemClock.sleep(150)
            } while (SystemClock.elapsedRealtime() < paintDeadline)

            val finalScreenshot = requireNotNull(screenshot) { "no screenshot available" }
            try {
                savePng(finalScreenshot, File(evidenceDir, "android-smoke.png"))
                // Negative control: a full-screen blank white frame must NOT
                // pass the same comparison, even when the bitmap size matches.
                val blank = Bitmap.createBitmap(
                    finalScreenshot.width, finalScreenshot.height, Bitmap.Config.ARGB_8888)
                try {
                    blank.eraseColor(Color.WHITE)
                    assertFalse("blank white ImageView must fail the paint comparison",
                        matchesRawRgba(blank, result, imageOrigin, visibleBounds, systemBars))
                } finally {
                    blank.recycle()
                }
            } finally {
                finalScreenshot.recycle()
            }
            assertTrue(
                "Screen pixels do not match the JNI RGBA document inside inset-free " +
                    "ImageView bounds after 10s: " + visibleBounds +
                    ", systemInsets=" + systemBars,
                matched,
            )
        }
    }
}
