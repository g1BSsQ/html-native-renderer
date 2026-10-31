// Copyright 2026 g1BSsQ. Licensed under Apache-2.0.
package org.htmlnativerenderer.demo

import android.util.Log
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit

/** Emulator smoke: the 20/200 device run uses the Activity extra instead. */
@RunWith(AndroidJUnit4::class)
class InteractionBenchSmokeTest {
    @Test
    fun nativeMotionAndInteractionBenchRunsOnWorker() {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val metrics = context.resources.displayMetrics
        val density = metrics.density
        val cssWidth = (metrics.widthPixels / density).toInt().coerceAtLeast(1)
        val cssHeight = (metrics.heightPixels / density).toInt().coerceAtLeast(1)
        val font = context.assets.open("Roboto-Regular.ttf").use { it.readBytes() }
        val worker = Executors.newSingleThreadExecutor()
        try {
            val result = worker.submit<String> {
                NativeRenderer.bench(cssWidth, cssHeight, density, arrayOf(font), 1, 2)
            }.get(180, TimeUnit.SECONDS)
            val records = result.lineSequence().filter { it.isNotBlank() }.toList()
            assertEquals(4, records.size)
            records.forEach {
                Log.i("ISR_BENCH", "ISR_BENCH $it")
                val data = JSONObject(it)
                val motion = data.getString("page") == "motion_12_cards"
                assertEquals(if (motion) 120 else 2, data.getInt("iterations"))
                assertEquals(if (motion) 0 else 1, data.getInt("warmup"))
                assertTrue(data.getJSONObject("total_us").has("p95"))
                assertTrue(data.getJSONObject("damage_px").has("p95"))
                assertTrue(data.getJSONObject("paint_path").has("partial"))
                assertTrue(data.getLong("baseline_render_us") >= 0)
            }
        } finally {
            worker.shutdownNow()
        }
    }
}
