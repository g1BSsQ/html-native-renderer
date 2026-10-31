// Copyright 2026 g1BSsQ. Licensed under Apache-2.0.
package org.htmlnativerenderer.demo

import android.graphics.Bitmap
import java.nio.ByteBuffer

/** One retained paint-order segment or independently composited element. */
data class LayerBitmap(
    val id: Long,
    val kind: Int,
    val width: Int,
    val height: Int,
    val left: Int,
    val top: Int,
    val rgba: ByteArray,
    val clipsJson: String,
    val borderX: Float,
    val borderY: Float,
) {
    fun toBitmap(): Bitmap {
        require(width > 0 && height > 0 && width.toLong() * height * 4 == rgba.size.toLong())
        return Bitmap.createBitmap(width, height, Bitmap.Config.ARGB_8888).also {
            it.copyPixelsFromBuffer(ByteBuffer.wrap(rgba))
        }
    }
}
