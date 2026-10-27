// Copyright 2026 g1BSsQ. Licensed under Apache-2.0.
package org.htmlnativerenderer.demo

import android.graphics.Bitmap
import android.graphics.Rect
import android.os.SystemClock
import android.widget.ImageView
import androidx.test.core.app.ActivityScenario
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import java.io.File
import kotlin.math.roundToInt

@RunWith(AndroidJUnit4::class)
class RenderSmokeTest {
    private fun savePng(bitmap: Bitmap, destination: File) {
        destination.outputStream().use { out ->
            assertTrue(
                "PNG encode failed: " + destination.name,
                bitmap.compress(Bitmap.CompressFormat.PNG, 100, out),
            )
        }
        assertTrue("PNG evidence is empty: " + destination.name, destination.length() > 0)
    }

    // Inspect the actual display pixels, not just the native RGBA buffer.
    // The rectangle is in screen coordinates and clipped to the ScrollView's
    // visible portion of the ImageView, excluding status/navigation bars.
    private fun hasMultipleScreenColours(screenshot: Bitmap, imageBounds: Rect): Boolean {
        val clipped = Rect(imageBounds)
        if (!clipped.intersect(0, 0, screenshot.width, screenshot.height)) return false
        val width = clipped.width()
        val height = clipped.height()
        if (width <= 0 || height <= 0) return false
        val pixels = IntArray(width * height)
        screenshot.getPixels(pixels, 0, width, clipped.left, clipped.top, width, height)
        val first = pixels[0]
        return pixels.any { it != first }
    }

    @Test
    fun bundledCardRendersAndAppearsInActivity() {
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        val context = instrumentation.targetContext
        val evidenceDir = requireNotNull(context.getExternalFilesDir(null)) {
            "app-specific external files directory is unavailable"
        }
        val html = context.assets.open("01-card.html").bufferedReader().use { it.readText() }

        // Match MainActivity's document width and density scaling so the
        // independently rendered bitmap is an exact expectation for its drawable.
        val density = context.resources.displayMetrics.density
        val cssWidth = (context.resources.displayMetrics.widthPixels / density)
            .toInt().coerceAtLeast(1)
        val result = NativeRenderer.render(html, cssWidth, density)
        assertEquals("native bitmap width", (cssWidth * density).roundToInt(), result.width)
        assertTrue("native bitmap height", result.height > 0)
        assertEquals(
            "RGBA byte count",
            result.width.toLong() * result.height * 4,
            result.rgba.size.toLong(),
        )
        val first = result.rgba.copyOfRange(0, 4)
        assertTrue("native bitmap must contain more than one colour",
            (4 until result.rgba.size step 4).any { offset ->
                (0..3).any { channel -> result.rgba[offset + channel] != first[channel] }
            })
        assertTrue("element-box JSON", result.elementBoxesJson.contains("\"elements\""))

        // Save the raw bitmap even if the later Activity or display assertions fail.
        val rawBitmap = result.toBitmap()
        try {
            savePng(rawBitmap, File(evidenceDir, "android-render.png"))
        } finally {
            rawBitmap.recycle()
        }

        ActivityScenario.launch(MainActivity::class.java).use { scenario ->
            val deadline = SystemClock.elapsedRealtime() + 20_000L
            var ready = false
            var lastState = "Activity not inspected"
            var visibleBounds = Rect()

            // onActivity runs on the UI thread. Poll from the instrumentation
            // thread only; never sleep on the Activity's main thread.
            while (SystemClock.elapsedRealtime() < deadline) {
                scenario.onActivity { activity ->
                    val image = activity.findViewById<ImageView>(R.id.document_image)
                    val drawable = image.drawable
                    val local = Rect()
                    val visible = image.getLocalVisibleRect(local)
                    val location = IntArray(2)
                    image.getLocationOnScreen(location)
                    visibleBounds = Rect(
                        location[0] + local.left, location[1] + local.top,
                        location[0] + local.right, location[1] + local.bottom,
                    )
                    ready = drawable != null &&
                        drawable.intrinsicWidth == result.width &&
                        drawable.intrinsicHeight == result.height &&
                        image.width > 0 && image.height == result.height &&
                        image.isShown && visible && !local.isEmpty
                    lastState = "drawable=" +
                        (drawable?.let { it.intrinsicWidth.toString() + "x" + it.intrinsicHeight }
                            ?: "null") +
                        ", expected=" + result.width + "x" + result.height +
                        ", view=" + image.width + "x" + image.height +
                        ", shown=" + image.isShown +
                        ", visible=" + visible +
                        ", screenBounds=" + visibleBounds
                }
                if (ready) break
                SystemClock.sleep(100)
            }
            assertTrue(
                "Timed out after 20s waiting for a visible ImageView with the " +
                    "rendered bitmap's intrinsic dimensions and document-height layout: " +
                    lastState,
                ready,
            )

            instrumentation.waitForIdleSync()

            // A drawable is not enough: wait for a frame that actually paints
            // more than one colour in the ImageView's on-screen visible bounds.
            // Keep the final (even blank) screen image for failed-CI diagnosis.
            val paintDeadline = SystemClock.elapsedRealtime() + 10_000L
            var screenshot: Bitmap? = null
            var painted = false
            do {
                val capture = requireNotNull(instrumentation.uiAutomation.takeScreenshot()) {
                    "UiAutomation.takeScreenshot() returned null"
                }
                screenshot?.recycle()
                screenshot = capture
                painted = hasMultipleScreenColours(capture, visibleBounds)
                if (painted) break
                SystemClock.sleep(150)
            } while (SystemClock.elapsedRealtime() < paintDeadline)

            val finalScreenshot = requireNotNull(screenshot) { "no screen capture available" }
            try {
                savePng(finalScreenshot, File(evidenceDir, "android-smoke.png"))
            } finally {
                finalScreenshot.recycle()
            }
            assertTrue(
                "ImageView was ready, but the screen remained a single colour " +
                    "inside its on-screen bounds after 10s: " + visibleBounds,
                painted,
            )
        }
    }
}
