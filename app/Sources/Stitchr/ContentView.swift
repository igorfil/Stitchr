import AppKit
import SwiftUI
import UniformTypeIdentifiers

private struct InputImage: Identifiable {
    let id = UUID()
    let url: URL
    var position: AppPosition
}

private struct PreviewRequest: Equatable, Sendable {
    let paths: [String]
    let options: AppOptions
}

struct ContentView: View {
    @State private var images: [InputImage] = []
    @State private var layout: AppLayout = .grid
    @State private var alignment: AppAlignment = .start
    @State private var columns = 2
    @State private var spacing = 10
    @State private var format: AppFormat = .png
    @State private var preview: NSImage?
    @State private var previewError: String?
    @State private var status: String?
    @State private var isDropTarget = false
    @State private var isRendering = false
    @State private var isExporting = false
    @State private var renderTask: Task<Void, Never>?

    private var request: PreviewRequest {
        PreviewRequest(
            paths: images.map { $0.url.path },
            options: AppOptions(
                layout: layout,
                columns: UInt32(columns),
                spacing: UInt32(spacing),
                alignment: alignment,
                positions: images.map(\.position)
            )
        )
    }

    var body: some View {
        VStack(spacing: 0) {
            HStack {
                Text("Stitchr").font(.title2).bold()
                Spacer()
                Button(action: importImages) {
                    Label("Add Images", systemImage: "plus")
                }
                Button(action: saveImage) {
                    Label("Export…", systemImage: "square.and.arrow.up")
                }
                .disabled(images.isEmpty || isRendering || isExporting || previewError != nil)
            }
            .padding()

            Divider()
            HSplitView {
                sidebar.frame(minWidth: 210, idealWidth: 240, maxWidth: 320)
                canvas.frame(minWidth: 400, maxWidth: .infinity)
                inspector.frame(minWidth: 230, idealWidth: 270, maxWidth: 320)
            }

            Divider()
            HStack {
                if let previewError {
                    Text(previewError).foregroundStyle(.red)
                } else if let status {
                    Text(status).foregroundStyle(.secondary)
                } else {
                    Text("PNG and JPEG images · Drop files onto the canvas")
                        .foregroundStyle(.secondary)
                }
                Spacer()
                if isRendering { ProgressView("Updating preview…") }
                if isExporting { ProgressView("Exporting…") }
            }
            .font(.caption)
            .padding(.horizontal)
            .frame(height: 36)
        }
        .onAppear { refreshPreview(request) }
        .onChange(of: request) { _, newValue in refreshPreview(newValue) }
        .onDisappear { renderTask?.cancel() }
    }

    private var sidebar: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("Images").font(.headline).padding([.top, .horizontal])
            List {
                ForEach(images.indices, id: \.self) { index in
                    HStack(spacing: 6) {
                        Image(systemName: "photo").foregroundStyle(.secondary)
                        Text(images[index].url.lastPathComponent)
                            .lineLimit(1)
                            .truncationMode(.middle)
                        Spacer(minLength: 0)
                        Button { images.swapAt(index - 1, index) } label: {
                            Image(systemName: "arrow.up")
                        }
                        .disabled(index == 0)
                        .help("Move up")
                        Button { images.swapAt(index, index + 1) } label: {
                            Image(systemName: "arrow.down")
                        }
                        .disabled(index == images.count - 1)
                        .help("Move down")
                        Button { images.remove(at: index) } label: {
                            Image(systemName: "xmark")
                        }
                        .help("Remove image")
                    }
                    .buttonStyle(.borderless)
                    .accessibilityElement(children: .contain)
                }
            }
        }
    }

    private var canvas: some View {
        ZStack {
            Color(nsColor: .windowBackgroundColor)
            if let preview {
                Image(nsImage: preview)
                    .resizable()
                    .scaledToFit()
                    .padding(24)
                    .accessibilityLabel("Stitched image preview")
            } else {
                VStack(spacing: 12) {
                    Image(systemName: "square.stack.3d.up")
                        .font(.system(size: 44))
                    Text("Drop PNG/JPEG images here or Add Images…")
                }
                .foregroundStyle(.secondary)
            }
        }
        .overlay {
            RoundedRectangle(cornerRadius: 12)
                .stroke(isDropTarget ? Color.accentColor : .clear, lineWidth: 3)
                .padding(4)
        }
        .dropDestination(for: URL.self) { urls, _ in
            addImages(urls)
            return !urls.isEmpty
        } isTargeted: { targeted in
            isDropTarget = targeted
        }
    }

    private var inspector: some View {
        Form {
            Picker("Layout", selection: $layout) {
                Text("Grid").tag(AppLayout.grid)
                Text("Horizontal").tag(AppLayout.horizontal)
                Text("Vertical").tag(AppLayout.vertical)
                Text("Freeform").tag(AppLayout.freeform)
            }

            if layout == .grid {
                Stepper("Columns: \(columns)", value: $columns, in: 1...12)
            }
            if layout != .freeform {
                Stepper("Spacing: \(spacing) px", value: $spacing, in: 0...200)
                Picker("Alignment", selection: $alignment) {
                    Text("Start").tag(AppAlignment.start)
                    Text("Center").tag(AppAlignment.center)
                    Text("End").tag(AppAlignment.end)
                }
            } else {
                Text("Position each image (pixels)").font(.headline)
                ForEach(images.indices, id: \.self) { index in
                    HStack {
                        Text("\(index + 1)")
                        TextField("X", value: $images[index].position.x, format: .number)
                        TextField("Y", value: $images[index].position.y, format: .number)
                    }
                }
            }

            Picker("Export format", selection: $format) {
                Text("PNG").tag(AppFormat.png)
                Text("JPEG").tag(AppFormat.jpeg)
            }
            .pickerStyle(.segmented)
        }
        .formStyle(.grouped)
        .disabled(images.isEmpty)
    }

    private func addImages(_ urls: [URL]) {
        let accepted = urls.filter { ["png", "jpg", "jpeg"].contains($0.pathExtension.lowercased()) }
        for url in accepted {
            let index = UInt32(images.count)
            images.append(InputImage(url: url, position: AppPosition(x: index * 160, y: index * 80)))
        }
        if accepted.count != urls.count {
            status = "Only PNG and JPEG images can be added."
        }
    }

    private func importImages() {
        let panel = NSOpenPanel()
        panel.allowedContentTypes = [.png, .jpeg]
        panel.allowsMultipleSelection = true
        if panel.runModal() == .OK { addImages(panel.urls) }
    }

    private func refreshPreview(_ request: PreviewRequest) {
        renderTask?.cancel()
        guard !request.paths.isEmpty else {
            preview = nil
            previewError = nil
            isRendering = false
            return
        }
        isRendering = true
        renderTask = Task {
            try? await Task.sleep(nanoseconds: 250_000_000)
            guard !Task.isCancelled else { return }
            do {
                let data = try await Task.detached(priority: .userInitiated) {
                    try previewPng(paths: request.paths, options: request.options)
                }.value
                guard !Task.isCancelled else { return }
                preview = NSImage(data: data)
                previewError = preview == nil ? "Unable to display preview" : nil
            } catch {
                guard !Task.isCancelled else { return }
                previewError = error.localizedDescription
            }
            isRendering = false
        }
    }

    private func saveImage() {
        let panel = NSSavePanel()
        panel.allowedContentTypes = format == .png ? [.png] : [.jpeg]
        panel.nameFieldStringValue = format == .png ? "stitched.png" : "stitched.jpg"
        guard panel.runModal() == .OK, let destination = panel.url else { return }

        let request = request
        let selectedFormat = format
        isExporting = true
        Task {
            do {
                try await Task.detached(priority: .userInitiated) {
                    try exportFile(
                        paths: request.paths,
                        options: request.options,
                        destination: destination.path,
                        format: selectedFormat
                    )
                }.value
                status = "Saved \(destination.lastPathComponent)"
            } catch {
                status = error.localizedDescription
            }
            isExporting = false
        }
    }
}
