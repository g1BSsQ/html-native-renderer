// Copyright 2026 g1BSsQ. Licensed under Apache-2.0.
package org.htmlnativerenderer.demo

import android.app.Activity
import android.os.Build
import android.os.Bundle
import android.widget.ImageView
import android.widget.FrameLayout
import android.view.ViewGroup
import java.util.concurrent.Executors

/** An explicit card-motion page; the existing activity retains touch ownership. */
class CompositedCardActivity : Activity() {
    private val worker = Executors.newSingleThreadExecutor()

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val host = FrameLayout(this)
        setContentView(host)
        val density = resources.displayMetrics.density
        val width = (resources.displayMetrics.widthPixels / density).toInt().coerceAtLeast(1)
        val html = buildString {
            append("<!doctype html><meta charset='utf-8'><style>")
            append("html,body{margin:0;background:#f7f8fa}")
            append("main{padding:16px}h2{font:700 22px sans-serif;margin:8px 0 24px}")
            append(".card{position:relative;z-index:1;isolation:isolate;")
            append("background:white;border-radius:12px;padding:18px 14px;")
            append("margin:8px 0;box-shadow:0 2px 5px #0002;")
            append("font:16px sans-serif;color:#253046}")
            append("</style><main><h2>Move a card</h2>")
            for (index in 0..6) {
                append("<div class='card' id='card-$index'>Card ${index + 1}</div>")
            }
            append("</main>")
        }
        worker.execute {
            try {
                val font = assets.open("Roboto-Regular.ttf").use { it.readBytes() }
                val fonts = arrayOf(font)
                val layers = if (Build.VERSION.SDK_INT >= 29) {
                    NativeRenderer.renderLayers(html, width, density, fonts,
                        Array(7) { "card-$it" })
                } else emptyArray()
                val useLayers = Build.VERSION.SDK_INT >= 29 && layers.isNotEmpty()
                val fallbackBitmap = if (useLayers) null
                    else NativeRenderer.render(html, width, density, fonts).toBitmap()
                runOnUiThread {
                    if (!isFinishing && !isDestroyed) {
                        val view = if (useLayers) {
                            CompositedCardView(this, layers)
                        } else {
                            ImageView(this).apply {
                                scaleType = ImageView.ScaleType.FIT_XY
                                setImageBitmap(fallbackBitmap)
                            }
                        }
                        host.addView(view, FrameLayout.LayoutParams(
                            ViewGroup.LayoutParams.MATCH_PARENT,
                            ViewGroup.LayoutParams.MATCH_PARENT
                        ))
                    } else {
                        fallbackBitmap?.recycle()
                    }
                }
            } catch (_: Throwable) {
                // A failed layer capture does not change the original page path.
                val fallback = try {
                    val font = assets.open("Roboto-Regular.ttf").use { it.readBytes() }
                    NativeRenderer.render(html, width, density, arrayOf(font)).toBitmap()
                } catch (_: Throwable) { null }
                runOnUiThread {
                    if (!isFinishing && !isDestroyed && fallback != null) {
                        host.addView(ImageView(this).apply { setImageBitmap(fallback) })
                    }
                }
            }
        }
    }

    override fun onDestroy() {
        worker.shutdownNow()
        super.onDestroy()
    }
}
