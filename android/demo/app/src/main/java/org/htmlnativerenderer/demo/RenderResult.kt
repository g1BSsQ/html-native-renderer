// Copyright 2026 g1BSsQ. Licensed under Apache-2.0.
package org.htmlnativerenderer.demo

import android.graphics.Bitmap
import java.nio.ByteBuffer

/** Pixels are RGBA8; elementBoxesJson uses the existing CSS-pixel layout schema. */
data class RenderResult(
    val width: Int,
    val height: Int,
    val rgba: ByteArray,
    val elementBoxesJson: String,
) {
    fun toBitmap(): Bitmap {
        require(width > 0 && height > 0 && width.toLong() * height * 4 == rgba.size.toLong()) {
            "invalid RGBA buffer dimensions"
        }
        return Bitmap.createBitmap(width, height, Bitmap.Config.ARGB_8888).also {
            it.copyPixelsFromBuffer(ByteBuffer.wrap(rgba))
        }
    }
}
