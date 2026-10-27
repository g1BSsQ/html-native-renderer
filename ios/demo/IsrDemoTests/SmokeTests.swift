// Copyright 2026 g1BSsQ. Licensed under Apache-2.0.
import XCTest
import UIKit
import SwiftUI
@testable import IsrDemo

final class SmokeTests: XCTestCase {
    @MainActor
    func testCardProducesNonUniformImage() async throws {
        let url = try XCTUnwrap(Bundle.main.url(forResource: "01-card", withExtension: "html"))
        let html = try String(contentsOf: url, encoding: .utf8)
        let rendered = try await RenderBridge.render(html: html, widthCSSPixels: 412, scale: 1)

        XCTAssertEqual(rendered.image.width, 412)
        XCTAssertEqual(rendered.pixelWidth, 412)
        XCTAssertGreaterThan(rendered.image.height, 0)
        XCTAssertEqual(rendered.rgba.count, rendered.pixelWidth * rendered.pixelHeight * 4)
        XCTAssertFalse(rendered.elementBoxesJSON.isEmpty)

        let bytes = [UInt8](rendered.rgba)
        let first = Array(bytes.prefix(4))
        let nonUniform = stride(from: 4, to: bytes.count, by: 4)
            .contains { Array(bytes[$0..<($0 + 4)]) != first }
        XCTAssertTrue(nonUniform, "01-card should not be a single solid colour")

        // Layout's boxes use CSS pixels and the requested test scale is 1.
        let json = try XCTUnwrap(
            JSONSerialization.jsonObject(with: Data(rendered.elementBoxesJSON.utf8))
                as? [String: Any]
        )
        let elements = try XCTUnwrap(json["elements"] as? [[String: Any]])
        let heading = try XCTUnwrap(elements.first { $0["tag"] as? String == "h1" })
        let x = try XCTUnwrap((heading["x"] as? NSNumber)?.doubleValue)
        let y = try XCTUnwrap((heading["y"] as? NSNumber)?.doubleValue)
        let boxWidth = try XCTUnwrap((heading["width"] as? NSNumber)?.doubleValue)
        let boxHeight = try XCTUnwrap((heading["height"] as? NSNumber)?.doubleValue)
        let left = max(0, Int(floor(x)))
        let top = max(0, Int(floor(y)))
        let right = min(rendered.pixelWidth, Int(ceil(x + boxWidth)))
        let bottom = min(rendered.pixelHeight, Int(ceil(y + boxHeight)))
        XCTAssertGreaterThan(right, left)
        XCTAssertGreaterThan(bottom, top)
        var darkInk = 0
        for row in top..<bottom {
            for column in left..<right {
                let offset = (row * rendered.pixelWidth + column) * 4
                if bytes[offset] < 100 && bytes[offset + 1] < 100 &&
                    bytes[offset + 2] < 100 && bytes[offset + 3] > 0 {
                    darkInk += 1
                }
            }
        }
        XCTAssertGreaterThan(darkInk, 20, "Missing dark text pixels inside h1 box")

        let screenshot = XCTAttachment(image: UIImage(cgImage: rendered.image))
        screenshot.name = "01-card-render"
        screenshot.lifetime = .keepAlways
        add(screenshot)

        // This attachment is the rendered ContentView (not merely the bridge
        // CGImage), capturing the picker and document inside its ScrollView.
        let viewRenderer = ImageRenderer(
            content: ContentView(initialRendered: rendered)
                .frame(width: 430, height: 932)
        )
        viewRenderer.scale = 1
        let viewImage = try XCTUnwrap(viewRenderer.uiImage)
        let viewAttachment = XCTAttachment(image: viewImage)
        viewAttachment.name = "01-card-content-view"
        viewAttachment.lifetime = .keepAlways
        add(viewAttachment)
    }
}
