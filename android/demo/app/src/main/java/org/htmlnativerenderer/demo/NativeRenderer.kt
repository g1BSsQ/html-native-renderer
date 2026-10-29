// Copyright 2026 g1BSsQ. Licensed under Apache-2.0.
package org.htmlnativerenderer.demo

/** A single native entry point, with no WebView or platform HTTP client. */
object NativeRenderer {
    init {
        System.loadLibrary("isr_android")
    }

    external fun render(
        html: String, widthCssPx: Int, scale: Float, fonts: Array<ByteArray>
    ): RenderResult

    /** Returns three newline-delimited JSON records, measured on the calling worker. */
    external fun bench(
        widthCssPx: Int, density: Float, fonts: Array<ByteArray>,
        warmup: Int, iterations: Int
    ): String
}
