// Copyright 2026 g1BSsQ. Licensed under Apache-2.0.
import XCTest

final class IsrDemoUITests: XCTestCase {
    func testRenderedDocumentIsVisibleInApplication() {
        let app = XCUIApplication()
        app.launch()

        // A real simulator screenshot is required: image flattening does not
        // capture the system-backed segmented picker used by this view.
        let image = app.images["rendered-document-image"]
        XCTAssertTrue(image.waitForExistence(timeout: 60),
                      "The document image must appear in the running app")
        XCTAssertGreaterThan(image.frame.width, 0)
        XCTAssertGreaterThan(image.frame.height, 0)

        let attachment = XCTAttachment(screenshot: app.screenshot())
        attachment.name = "01-card-app-screen"
        attachment.lifetime = .keepAlways
        add(attachment)
    }
}
