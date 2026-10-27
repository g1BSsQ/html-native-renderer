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
        // Load bundled font bytes inside the detached render task; never read
        // a path or discover system fonts in the engine.
        guard let fontURL = Bundle.main.url(forResource: "Roboto-Regular", withExtension: "ttf") else {
            throw CocoaError(.fileNoSuchFile)
        }
        let fontData = try Data(contentsOf: fontURL)
        let utf8 = Array(html.utf8)
        let status: Int32 = utf8.withUnsafeBufferPointer { htmlBuffer in
            fontData.withUnsafeBytes { fontBuffer in
                var font = IsrBytes(
                    data: fontBuffer.bindMemory(to: UInt8.self).baseAddress,
                    len: fontBuffer.count
                )
                return withUnsafePointer(to: &font) { fontPointer in
                    isr_render_with_fonts(
                        htmlBuffer.baseAddress, htmlBuffer.count, widthCSSPixels, scale,
                        fontPointer, 1, &output
                    )
                }
            }
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
              let pixels = output.pixels
        else {
            throw RenderBridgeError.invalidOutput
        }
        // One native-to-Data copy; the provider retains this same Swift data.
        // The native buffer is released at function exit by isr_free.
        let rgba = Data(bytes: pixels, count: output.pixels_len)
        guard let provider = CGDataProvider(data: rgba as CFData) else {
            throw RenderBridgeError.invalidOutput
        }
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
