// Copyright 2026 g1BSsQ. Licensed under Apache-2.0.
package org.htmlnativerenderer.demo

import android.app.Activity
import android.os.Bundle
import android.widget.FrameLayout
import android.widget.ImageView
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.TextView
import android.view.View
import android.view.ViewGroup
import java.util.concurrent.Executors

/** Stage K8: one offscreen document painted into a vertically scrollable ImageView. */
class MainActivity : Activity() {
    private val worker = Executors.newSingleThreadExecutor()
    private lateinit var image: ImageView
    private lateinit var status: TextView

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
        setContentView(ScrollView(this).apply {
            addView(container, FrameLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT
            ))
        })

        val fixture = when (intent.getStringExtra(EXTRA_FIXTURE)) {
            "19-flex-alignment.html" -> "19-flex-alignment.html"
            else -> "01-card.html"
        }
        val density = resources.displayMetrics.density
        val cssWidth = (resources.displayMetrics.widthPixels / density).toInt().coerceAtLeast(1)
        worker.execute {
            try {
                val html = assets.open(fixture).bufferedReader(Charsets.UTF_8).use { it.readText() }
                val result = NativeRenderer.render(html, cssWidth, density)
                val bitmap = result.toBitmap()
                runOnUiThread {
                    if (!isFinishing && !isDestroyed) {
                        // Match the BitmapDrawable's intrinsic pixels to the
                        // native result, independent of Android density defaults.
                        bitmap.density = resources.displayMetrics.densityDpi
                        image.setImageBitmap(bitmap)
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

    override fun onDestroy() {
        worker.shutdownNow()
        super.onDestroy()
    }

    companion object {
        const val EXTRA_FIXTURE = "fixture"
    }
}
