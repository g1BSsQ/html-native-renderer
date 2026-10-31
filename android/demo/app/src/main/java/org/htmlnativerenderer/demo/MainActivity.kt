// Copyright 2026 g1BSsQ. Licensed under Apache-2.0.
package org.htmlnativerenderer.demo

import android.app.Activity
import android.os.Bundle
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.util.Log
import android.widget.FrameLayout
import android.widget.ImageView
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.TextView
import android.view.MotionEvent
import android.view.View
import android.view.ViewGroup
import android.view.WindowInsets
import java.util.concurrent.Executors
import org.json.JSONArray
import kotlin.math.roundToInt

/** A retained input page alongside the existing image fixtures. */
class MainActivity : Activity() {
    private val worker = Executors.newSingleThreadExecutor()
    private lateinit var image: ImageView
    private lateinit var status: TextView
    private var inputBitmapWidth = 0
    private var inputBitmapHeight = 0

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val container = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL }
        image = ImageView(this).apply {
            id = R.id.document_image
            scaleType = ImageView.ScaleType.FIT_XY
        }
        status = TextView(this).apply { text = "Rendering document…" }
        container.addView(image, LinearLayout.LayoutParams(
            ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT
        ))
        container.addView(status)
        val scroll = ScrollView(this).apply {
            clipToPadding = true
            addView(container, FrameLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT
            ))
            // On targetSdk 35, content otherwise sits beneath transparent system
            // bars. Insets are padding so the document begins below the status
            // bar while keeping the full Bitmap inside the scrollable content.
            setOnApplyWindowInsetsListener { view, insets ->
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
                    val bars = insets.getInsets(WindowInsets.Type.systemBars())
                    view.setPadding(bars.left, bars.top, bars.right, bars.bottom)
                } else {
                    @Suppress("DEPRECATION")
                    view.setPadding(
                        insets.systemWindowInsetLeft, insets.systemWindowInsetTop,
                        insets.systemWindowInsetRight, insets.systemWindowInsetBottom
                    )
                }
                insets
            }
        }
        setContentView(scroll)
        scroll.requestApplyInsets()

        val fixture = when (intent.getStringExtra(EXTRA_FIXTURE)) {
            "19-flex-alignment.html" -> "19-flex-alignment.html"
            "20-input-gestures.html" -> "20-input-gestures.html"
            else -> "01-card.html"
        }
        val density = resources.displayMetrics.density
        val cssWidth = (resources.displayMetrics.widthPixels / density).toInt().coerceAtLeast(1)
        val cssHeight = (resources.displayMetrics.heightPixels / density).toInt().coerceAtLeast(1)
        if (intent.getBooleanExtra(EXTRA_INTERACTION_BENCH, false)) {
            status.text = "Running interaction measurement…"
            worker.execute {
                try {
                    val font = assets.open("Roboto-Regular.ttf").use { it.readBytes() }
                    NativeRenderer.bench(
                        cssWidth, cssHeight, density, arrayOf(font), 20, 200
                    ).lineSequence().filter { it.isNotBlank() }.forEach { line ->
                        Log.i("ISR_BENCH", "ISR_BENCH $line")
                    }
                    runOnUiThread {
                        if (!isFinishing && !isDestroyed) status.text = "Measurement logged"
                    }
                } catch (error: Throwable) {
                    Log.e("ISR_BENCH", "Interaction benchmark failed", error)
                    runOnUiThread {
                        if (!isFinishing && !isDestroyed) status.text =
                            "Measurement error: ${error.message}"
                    }
                }
            }
            return
        }
        val inputPage = fixture == "20-input-gestures.html"
        if (inputPage) {
            image.setOnTouchListener { _, event ->
                val phase = when (event.actionMasked) {
                    MotionEvent.ACTION_DOWN, MotionEvent.ACTION_POINTER_DOWN -> 0
                    MotionEvent.ACTION_MOVE -> 1
                    MotionEvent.ACTION_UP -> 2
                    MotionEvent.ACTION_CANCEL -> 3
                    else -> -1
                }
                if (phase < 0 || inputBitmapWidth == 0) {
                    false
                } else {
                    // The hit-test consumes document CSS pixels, not viewport
                    // pixels or bitmap pixels. Include the viewport scroll.
                    val index = if (event.actionMasked == MotionEvent.ACTION_POINTER_DOWN) {
                        event.actionIndex
                    } else { 0 }
                    val imageOrigin = IntArray(2)
                    val viewportOrigin = IntArray(2)
                    image.getLocationOnScreen(imageOrigin)
                    scroll.getLocationOnScreen(viewportOrigin)
                    val x = (imageOrigin[0] + event.getX(index)
                        - viewportOrigin[0] - scroll.paddingLeft) / density
                    val yViewport = (imageOrigin[1] + event.getY(index)
                        - viewportOrigin[1] - scroll.paddingTop) / density
                    val y = yViewport + scroll.scrollY / density
                    val id = event.getPointerId(index).toLong()
                    val timestamp = event.eventTime
                    scroll.requestDisallowInterceptTouchEvent(true)
                    worker.execute {
                        try {
                            publishInput(NativeRenderer.inputPointer(
                                id, phase, x.toDouble(), y.toDouble(), timestamp
                            ), scroll, density)
                        } catch (error: Throwable) {
                            Log.e("ISR_INPUT", "Pointer dispatch failed", error)
                        }
                    }
                    if (phase == 0 && event.actionMasked == MotionEvent.ACTION_DOWN) {
                        Handler(Looper.getMainLooper()).postDelayed({
                            if (!isFinishing && !isDestroyed) {
                                worker.execute {
                                    try {
                                        publishInput(
                                            NativeRenderer.inputTick(timestamp + 500),
                                            scroll, density
                                        )
                                    } catch (error: Throwable) {
                                        Log.e("ISR_INPUT", "Input tick failed", error)
                                    }
                                }
                            }
                        }, 500)
                    }
                    true
                }
            }
        }
        worker.execute {
            try {
                val html = assets.open(fixture).bufferedReader(Charsets.UTF_8).use { it.readText() }
                // Exact public fixture font, decoded and bundled as an asset.
                // Supplied entirely as bytes: no platform font discovery.
                val font = assets.open("Roboto-Regular.ttf").use { it.readBytes() }
                val result = if (inputPage) {
                    NativeRenderer.inputOpen(html, cssWidth, density, arrayOf(font))
                } else {
                    NativeRenderer.render(html, cssWidth, density, arrayOf(font))
                }
                val bitmap = result.toBitmap()
                runOnUiThread {
                    if (!isFinishing && !isDestroyed) {
                        // Match the BitmapDrawable's intrinsic pixels to the
                        // native result, independent of Android density defaults.
                        bitmap.density = resources.displayMetrics.densityDpi
                        image.setImageBitmap(bitmap)
                        if (inputPage) {
                            inputBitmapWidth = result.width
                            inputBitmapHeight = result.height
                        }
                        // The ScrollView scrolls this exact document-height image.
                        image.layoutParams = LinearLayout.LayoutParams(
                            result.width, result.height
                        )
                        status.visibility = View.GONE
                    } else {
                        bitmap.recycle()
                    }
                }
            } catch (error: Throwable) {
                runOnUiThread {
                    if (!isFinishing && !isDestroyed) {
                        status.text = "Render error: ${error.message}"
                    }
                }
            }
        }
    }

    private fun publishInput(result: InputResult, scroll: ScrollView, scale: Float) {
        Log.i("ISR_INPUT", result.eventsJson)
        val bitmap = result.rgba?.let { bytes ->
            RenderResult(inputBitmapWidth, inputBitmapHeight, bytes, "{}").toBitmap()
        }
        runOnUiThread {
            if (!isFinishing && !isDestroyed) {
                if (bitmap != null) {
                    bitmap.density = resources.displayMetrics.densityDpi
                    image.setImageBitmap(bitmap)
                }
                val events = JSONArray(result.eventsJson)
                for (i in 0 until events.length()) {
                    val item = events.getJSONObject(i)
                    if (item.getString("type") == "Pan") {
                        val pixels = (-item.getDouble("dy_css") * scale).roundToInt()
                        scroll.scrollBy(0, pixels)
                    }
                }
            } else {
                bitmap?.recycle()
            }
        }
    }

    override fun onDestroy() {
        worker.shutdownNow()
        super.onDestroy()
    }

    companion object {
        const val EXTRA_FIXTURE = "fixture"
        const val EXTRA_INTERACTION_BENCH = "interaction_bench"
    }
}
