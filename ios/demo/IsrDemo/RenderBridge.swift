// Copyright 2026 g1BSsQ. Licensed under Apache-2.0.
import CoreGraphics
import Foundation

struct RenderedDocument {
    let image: CGImage
    let rgba: Data
    let pixelWidth: Int
    let pixelHeight: Int
    let scale: Float
    let elementBoxesJSON: String
}

enum RenderBridgeError: LocalizedError {
    case invalidOutput
    case native(status: Int32, message: String)

    var errorDescription: String? {
        switch self {
        case .invalidOutput:
            return "Invalid RGBA8 buffer from renderer"
        case .native(let status, let message):
            return "Renderer status \(status): \(message)"
        }
    }
}

enum RenderBridge {
    // The detached task ensures layout and painting do not run on the UI thread.
    static func render(html: String, widthCSSPixels: UInt32, scale: Float) async throws -> RenderedDocument {
        try await Task.detached(priority: .userInitiated) {
            try renderSynchronously(html: html, widthCSSPixels: widthCSSPixels, scale: scale)
        }.value
    }

    private static func renderSynchronously(
        html: String, widthCSSPixels: UInt32, scale: Float
    ) throws -> RenderedDocument {
        var output = IsrOutput()
        let utf8 = Array(html.utf8)
        let status: Int32 = utf8.withUnsafeBufferPointer { buffer in
            isr_render(buffer.baseAddress, buffer.count, widthCSSPixels, scale, &output)
        }
        defer { isr_free(&output) }
        guard status == 0 else {
            let message = output.error_message.map { String(cString: $0) } ?? "No error message"
            throw RenderBridgeError.native(status: status, message: message)
        }

        let width = Int(output.width)
        let height = Int(output.height)
        guard width > 0, height > 0,
              width <= Int.max / 4,
              height <= Int.max / (width * 4),
              output.pixels_len == width * height * 4,
              let pixels = output.pixels,
              let provider = CGDataProvider(data: Data(bytes: pixels, count: output.pixels_len) as CFData)
        else {
            throw RenderBridgeError.invalidOutput
        }
        // isr_free runs on return; retain a Swift-owned copy before release.
        let rgba = Data(bytes: pixels, count: output.pixels_len)
        let info = CGBitmapInfo.byteOrder32Big.union(
            CGBitmapInfo(rawValue: CGImageAlphaInfo.premultipliedLast.rawValue)
        )
        guard let image = CGImage(
            width: width, height: height,
            bitsPerComponent: 8, bitsPerPixel: 32,
            bytesPerRow: width * 4,
            space: CGColorSpaceCreateDeviceRGB(),
            bitmapInfo: info,
            provider: provider,
            decode: nil,
            shouldInterpolate: false,
            intent: .defaultIntent
        ) else {
            throw RenderBridgeError.invalidOutput
        }
        return RenderedDocument(
            image: image, rgba: rgba, pixelWidth: width, pixelHeight: height,
            scale: scale,
            elementBoxesJSON: output.element_boxes_json.map { String(cString: $0) } ?? ""
        )
    }
}
