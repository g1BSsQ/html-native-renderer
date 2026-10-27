// Copyright 2026 g1BSsQ. Licensed under Apache-2.0.
import SwiftUI

struct ContentView: View {
    @State private var document = "01-card"
    @State private var rendered: RenderedDocument?
    @State private var errorMessage: String?

    // Allows XCTest's ImageRenderer to capture the actual SwiftUI view after
    // the bridge has completed, without starting a second background render.
    private let initialRendered: RenderedDocument?

    init(initialRendered: RenderedDocument? = nil) {
        self.initialRendered = initialRendered
        _rendered = State(initialValue: initialRendered)
    }

    var body: some View {
        VStack(spacing: 12) {
            Picker("Document", selection: $document) {
                Text("Card").tag("01-card")
                Text("Flex alignment").tag("19-flex-alignment")
            }
            .pickerStyle(.segmented)
            .padding(.horizontal)

            ScrollView([.horizontal, .vertical]) {
                if let rendered {
                    Image(decorative: rendered.image, scale: CGFloat(rendered.scale), orientation: .up)
                        .resizable()
                        .interpolation(.none)
                        .frame(
                            width: CGFloat(rendered.pixelWidth) / CGFloat(rendered.scale),
                            height: CGFloat(rendered.pixelHeight) / CGFloat(rendered.scale)
                        )
                } else if let errorMessage {
                    Text(errorMessage).foregroundStyle(.red).padding()
                } else {
                    ProgressView("Rendering").padding()
                }
            }
        }
        .task(id: document) {
            if initialRendered != nil { return }
            let requestedDocument = document
            rendered = nil
            errorMessage = nil
            do {
                try Task.checkCancellation()
                guard let url = Bundle.main.url(forResource: requestedDocument, withExtension: "html") else {
                    throw CocoaError(.fileNoSuchFile)
                }
                let html = try String(contentsOf: url, encoding: .utf8)
                let image = try await RenderBridge.render(html: html, widthCSSPixels: 412, scale: 2)
                try Task.checkCancellation()
                guard requestedDocument == document else { return }
                rendered = image
            } catch is CancellationError {
                // The replacement .task will publish its own image.
            } catch {
                guard !Task.isCancelled && requestedDocument == document else { return }
                errorMessage = error.localizedDescription
            }
        }
    }
}
