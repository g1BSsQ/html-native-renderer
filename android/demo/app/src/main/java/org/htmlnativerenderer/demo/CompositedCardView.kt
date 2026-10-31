// Copyright 2026 g1BSsQ. Licensed under Apache-2.0.
package org.htmlnativerenderer.demo

import android.content.Context
import android.graphics.Canvas
import android.graphics.Path
import android.graphics.RenderNode
import android.view.MotionEvent
import android.view.View
import org.json.JSONArray
import kotlin.math.abs
import kotlin.math.min

/** Draw retained segments and independently transformed elements in paint order. */
class CompositedCardView(
    context: Context,
    layers: Array<LayerBitmap>,
) : View(context) {
    private class Entry(val source: LayerBitmap, val node: RenderNode?, val clips: JSONArray)

    private val entries = layers.map { layer ->
        val node = if (layer.width > 0 && layer.height > 0) {
            RenderNode("layer").apply {
                setPosition(0, 0, layer.width, layer.height)
                val recording = beginRecording()
                recording.drawBitmap(layer.toBitmap(), 0f, 0f, null)
                endRecording()
                translationX = layer.left.toFloat()
                translationY = layer.top.toFloat()
                pivotX = layer.borderX - layer.left
                pivotY = layer.borderY - layer.top
            }
        } else null
        Entry(layer, node, JSONArray(layer.clipsJson))
    }
    private val promoted = entries.filter { it.source.kind == 1 }
    private var startX = 0f
    private var startY = 0f
    private var dragging = false

    override fun onDraw(canvas: Canvas) {
        super.onDraw(canvas)
        if (!canvas.isHardwareAccelerated) return
        for (entry in entries) {
            val node = entry.node ?: continue
            val save = canvas.save()
            for (index in 0 until entry.clips.length()) {
                val clip = entry.clips.getJSONObject(index)
                val left = clip.getDouble("x").toFloat()
                val top = clip.getDouble("y").toFloat()
                val right = left + clip.getDouble("w").toFloat()
                val bottom = top + clip.getDouble("h").toFloat()
                val radius = clip.getDouble("r").toFloat().coerceAtLeast(0f)
                if (radius == 0f) {
                    canvas.clipRect(left, top, right, bottom)
                } else {
                    val clipPath = Path()
                    clipPath.addRoundRect(left, top, right, bottom, radius, radius,
                        Path.Direction.CW)
                    canvas.clipPath(clipPath)
                }
            }
            canvas.drawRenderNode(node)
            canvas.restoreToCount(save)
        }
    }

    override fun onTouchEvent(event: MotionEvent): Boolean {
        if (promoted.isEmpty()) return false
        when (event.actionMasked) {
            MotionEvent.ACTION_DOWN -> {
                startX = event.x
                startY = event.y
                dragging = true
                return true
            }
            MotionEvent.ACTION_MOVE -> {
                if (!dragging) return false
                move(event.x - startX, event.y - startY)
                return true
            }
            MotionEvent.ACTION_UP, MotionEvent.ACTION_CANCEL -> {
                dragging = false
                move(0f, 0f)
                return true
            }
        }
        return true
    }

    private fun move(dx: Float, dy: Float) {
        val middle = promoted.size / 2
        promoted.forEachIndexed { index, entry ->
            val node = entry.node ?: return@forEachIndexed
            val selected = index == middle
            val shift = if (selected) dy else {
                (if (index < middle) -1f else 1f) * min(18f, abs(dy) * 0.4f)
            }
            node.translationX = entry.source.left + if (selected) dx else 0f
            node.translationY = entry.source.top + shift
            node.scaleX = if (selected && dragging) 1.035f else 1f
            node.scaleY = if (selected && dragging) 1.035f else 1f
            node.rotationZ = if (selected && dragging) dx / 24f else 0f
            node.alpha = if (selected && dragging) 0.90f else 1f
        }
        invalidate()
    }
}
