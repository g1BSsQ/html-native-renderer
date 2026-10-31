// Copyright 2026 g1BSsQ. Licensed under Apache-2.0.
package org.htmlnativerenderer.demo

/** Native rendering and retained input entry points. */
object NativeRenderer {
    init {
        System.loadLibrary("isr_android")
    }

    external fun render(
        html: String, widthCssPx: Int, scale: Float, fonts: Array<ByteArray>
    ): RenderResult

    /** Starts a retained document on the calling worker thread. */
    external fun inputOpen(
        html: String, widthCssPx: Int, scale: Float, fonts: Array<ByteArray>
    ): RenderResult

    /** Input samples and clock values must remain on the input worker thread. */
    external fun inputPointer(
        pointerId: Long, phase: Int, xCss: Double, yCss: Double, timeMs: Long
    ): InputResult

    external fun inputTick(timeMs: Long): InputResult

    /** Returns three newline-delimited JSON records, measured on the calling worker. */
    external fun bench(
        widthCssPx: Int, heightCssPx: Int, density: Float, fonts: Array<ByteArray>,
        warmup: Int, iterations: Int
    ): String
}
