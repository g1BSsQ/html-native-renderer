// Copyright 2026 g1BSsQ. Licensed under Apache-2.0.
import XCTest
import UIKit
@testable import IsrDemo

final class SmokeTests: XCTestCase {
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

        let screenshot = XCTAttachment(image: UIImage(cgImage: rendered.image))
        screenshot.name = "01-card-render"
        screenshot.lifetime = .keepAlways
        add(screenshot)
    }
}
