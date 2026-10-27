// Copyright 2026 g1BSsQ. Licensed under Apache-2.0.
package org.htmlnativerenderer.demo

import android.os.SystemClock
import android.widget.ImageView
import androidx.test.core.app.ActivityScenario
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import java.io.File

@RunWith(AndroidJUnit4::class)
class RenderSmokeTest {
    @Test
    fun bundledCardRendersAndAppearsInActivity() {
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        val context = instrumentation.targetContext
        val html = context.assets.open("01-card.html").bufferedReader().use { it.readText() }

        // Instrumentation tests execute off the Activity UI thread.
        val result = NativeRenderer.render(html, 412, 1.0f)
        assertEquals(412, result.width)
        assertTrue("bitmap height", result.height > 0)
        assertEquals(result.width.toLong() * result.height * 4, result.rgba.size.toLong())
        val first = result.rgba.copyOfRange(0, 4)
        val containsSecondColour = (4 until result.rgba.size step 4).any { offset ->
            (0..3).any { channel -> result.rgba[offset + channel] != first[channel] }
        }
        assertTrue("bitmap must contain more than one colour", containsSecondColour)
        assertTrue("element-box JSON", result.elementBoxesJson.contains("\"elements\""))

        ActivityScenario.launch(MainActivity::class.java).use { scenario ->
            var displayed = false
            for (attempt in 0 until 120) {
                scenario.onActivity { activity ->
                    val view = activity.findViewById<ImageView>(R.id.document_image)
                    displayed = view.drawable != null && view.height > 0
                }
                if (displayed) break
                SystemClock.sleep(250)
            }
            assertTrue("native bitmap must appear in the demo ImageView", displayed)

            val screenshot = requireNotNull(instrumentation.uiAutomation.takeScreenshot()) {
                "screenshot capture failed"
            }
            val destination = File(context.getExternalFilesDir(null), "android-smoke.png")
            destination.outputStream().use { out ->
                assertTrue("encode screenshot", screenshot.compress(
                    android.graphics.Bitmap.CompressFormat.PNG, 100, out
                ))
            }
            screenshot.recycle()
            assertTrue("screenshot artifact exists", destination.length() > 0)
        }
    }
}
