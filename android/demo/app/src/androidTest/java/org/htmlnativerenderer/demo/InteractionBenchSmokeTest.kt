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
    fun inputTapAndDragOnRetainedDocument() {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val density = context.resources.displayMetrics.density
        val cssWidth = (context.resources.displayMetrics.widthPixels / density)
            .toInt().coerceAtLeast(1)
        val html = context.assets.open("20-input-gestures.html")
            .bufferedReader(Charsets.UTF_8).use { it.readText() }
        val font = context.assets.open("Roboto-Regular.ttf").use { it.readBytes() }
        val worker = Executors.newSingleThreadExecutor()
        try {
            val result = worker.submit<Boolean> {
                val rendered = NativeRenderer.inputOpen(
                    html, cssWidth, density, arrayOf(font)
                )
                assertTrue(rendered.width > 0 && rendered.height > 0)
                val down = NativeRenderer.inputPointer(1, 0, 30.0, 40.0, 100)
                assertTrue(down.rgba != null)
                val tap = NativeRenderer.inputPointer(1, 2, 30.0, 40.0, 140)
                val taps = org.json.JSONArray(tap.eventsJson)
                assertTrue((0 until taps.length()).any { i ->
                    taps.getJSONObject(i).getString("type") == "Tap"
                        && taps.getJSONObject(i).getString("target") == "tap"
                })
                NativeRenderer.inputPointer(2, 0, 30.0, 320.0, 200)
                val move = NativeRenderer.inputPointer(2, 1, 55.0, 340.0, 220)
                val drags = org.json.JSONArray(move.eventsJson)
                assertTrue((0 until drags.length()).any { i ->
                    drags.getJSONObject(i).getString("type") == "DragStart"
                        && drags.getJSONObject(i).getString("target") == "drag"
                })
                assertTrue((0 until drags.length()).any { i ->
                    drags.getJSONObject(i).getString("type") == "DragMove"
                })
                val up = NativeRenderer.inputPointer(2, 2, 60.0, 350.0, 240)
                val ended = org.json.JSONArray(up.eventsJson)
                assertTrue((0 until ended.length()).any { i ->
                    ended.getJSONObject(i).getString("type") == "DragEnd"
                        && ended.getJSONObject(i).getString("target") == "drag"
                })
                assertTrue(up.rgba != null)
                true
            }.get(180, TimeUnit.SECONDS)
            assertTrue(result)
        } finally {
            worker.shutdownNow()
        }
    }

    @Test
    fun nativeThreePageBenchRunsOnWorker() {
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
            assertEquals(3, records.size)
            records.forEach {
                Log.i("ISR_BENCH", "ISR_BENCH $it")
                val data = JSONObject(it)
                assertEquals(2, data.getInt("iterations"))
                assertEquals(1, data.getInt("warmup"))
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
