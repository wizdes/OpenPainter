// One open image as the main window sees it: the engine document, the cached
// `doc.info`, the selection bounds, which commands are enabled, and a thumbnail
// for the image strip. Everything goes through engine commands.
import AppKit

/// What the app's NSDocument does for an editor.
@MainActor
public protocol EditorHost: AnyObject {
    /// Closes the image, asking to save it first when it is modified.
    func closeEditor()
    /// The engine's dirty flag changed (drives the window's Edited state).
    func dirtyChanged(_ dirty: Bool)
}

public struct DocInfo: Equatable, Sendable {
    public struct Layer: Equatable, Sendable {
        public var name: String
        public var visible: Bool
        public var id = 0
        /// 0 to 255.
        public var opacity = 255
        /// The engine's blend mode name, such as "normal" or "color_burn".
        public var blend = "normal"
        /// Changes when the layer's pixels change (thumbnail cache key).
        public var revision = 0
    }
    public var width = 0
    public var height = 0
    public var layers: [Layer] = []
    public var activeLayer = 0
    public var historyLabels: [String] = []
    /// The command id of each history step (the History panel's icons).
    public var historyCommands: [String] = []
    public var historyIndex = 0
    public var dirty = false
    public var revision: Int = 0

    public init() {}

    init(_ json: [String: Any]) {
        width = (json["width"] as? NSNumber)?.intValue ?? 0
        height = (json["height"] as? NSNumber)?.intValue ?? 0
        layers = (json["layers"] as? [[String: Any]] ?? []).map {
            Layer(name: $0["name"] as? String ?? "", visible: $0["visible"] as? Bool ?? true,
                  id: ($0["id"] as? NSNumber)?.intValue ?? 0, opacity: ($0["opacity"] as? NSNumber)?.intValue ?? 255,
                  blend: $0["blend"] as? String ?? "normal", revision: ($0["revision"] as? NSNumber)?.intValue ?? 0)
        }
        activeLayer = (json["active_layer"] as? NSNumber)?.intValue ?? 0
        historyLabels = json["history_labels"] as? [String] ?? []
        historyCommands = json["history_commands"] as? [String] ?? []
        historyIndex = (json["history_index"] as? NSNumber)?.intValue ?? 0
        dirty = json["dirty"] as? Bool ?? false
        revision = (json["revision"] as? NSNumber)?.intValue ?? 0
    }
}

@MainActor
public final class Editor {
    public let document: Document
    public let pixelSize: CGSize
    public var name: String { didSet { if name != oldValue { changed() } } }
    public weak var host: EditorHost?
    public private(set) var info = DocInfo()
    /// Selection bounds in document pixels; nil when nothing is selected.
    public private(set) var selection: CGRect?
    /// Engine command id to enabled, from `commands.list`.
    public private(set) var enabled: [String: Bool] = [:]
    public private(set) var thumbnail: CGImage?
    /// The window controller that shows this editor (set when it is made).
    public internal(set) weak var windowController: EditorWindowController?
    /// The last message for the status bar (errors, "coming soon").
    public private(set) var message: String?

    private var observers: [(AnyObject?, () -> Void)] = []
    private var thumbnailRevision = -1
    private var thumbnailBusy = false
    private var refreshQueued = false

    public init(document: Document, pixelSize: CGSize, name: String) {
        self.document = document
        self.pixelSize = pixelSize
        self.name = name
        refresh()
    }

    public func observe(_ owner: AnyObject, _ body: @escaping () -> Void) {
        observers.append((owner, body))
    }

    private func changed() {
        observers.removeAll { $0.0 == nil }
        for (_, body) in observers { body() }
    }

    /// Re-reads the engine's published state. Cheap: it never waits for work.
    public func refresh() {
        let wasDirty = info.dirty
        do {
            info = DocInfo(try document.info())
            let sel = try document.query("doc.selection") as? [String: Any] ?? [:]
            if sel["active"] as? Bool == true, let b = sel["bounds"] as? [NSNumber], b.count == 4 {
                selection = CGRect(x: b[0].doubleValue, y: b[1].doubleValue, width: b[2].doubleValue, height: b[3].doubleValue)
            } else {
                selection = nil
            }
            enabled = try document.enabledCommands()
        } catch {
            report("could not read the document: \(error)")
        }
        if info.dirty != wasDirty { host?.dirtyChanged(info.dirty) }
        changed()
        updateThumbnail()
    }

    /// Coalesces engine events into one refresh on the next main-queue turn.
    public func scheduleRefresh() {
        guard !refreshQueued else { return }
        refreshQueued = true
        DispatchQueue.main.async { [weak self] in
            guard let self else { return }
            self.refreshQueued = false
            self.refresh()
        }
    }

    public func isEnabled(_ command: String) -> Bool { enabled[command] ?? false }

    /// Dispatches an engine command. Failures show in the status bar.
    @discardableResult
    public func run(_ command: String, _ params: [String: any Sendable] = [:]) -> UInt64? {
        do {
            let ticket = try document.dispatch(command, params)
            Task { @MainActor [weak self] in
                guard let self else { return }
                let event = await self.document.engine.result(of: ticket, timeout: .seconds(120))
                if let status = event?.status, status != "ok" {
                    self.report("\(command) failed: \(status)")
                }
                self.refresh()
            }
            return ticket
        } catch {
            report("\(command) failed: \(error)")
            return nil
        }
    }

    /// Dispatches and waits for the result (tests and scripted flows).
    public func runAndWait(_ command: String, _ params: [String: any Sendable] = [:]) async -> String? {
        do {
            let event = try await document.run(command, params, timeout: .seconds(120))
            refresh()
            return event.status
        } catch {
            report("\(command) failed: \(error)")
            return nil
        }
    }

    public func report(_ text: String) {
        message = text
        changed()
    }

    public func clearMessage() {
        guard message != nil else { return }
        message = nil
        changed()
    }

    // MARK: Thumbnail

    /// Renders a 112 x 80 pixel (56 x 40 pt at 2x) thumbnail off the main thread
    /// when the revision changed.
    private func updateThumbnail() {
        guard info.revision != thumbnailRevision, !thumbnailBusy, info.width > 0 else { return }
        thumbnailBusy = true
        let revision = info.revision
        let doc = document
        let (w, h) = (info.width, info.height)
        Task.detached(priority: .utility) {
            let image = try? Editor.renderThumbnail(doc, width: w, height: h)
            await MainActor.run { [weak self] in
                guard let self else { return }
                self.thumbnailBusy = false
                self.thumbnailRevision = revision
                if let image { self.thumbnail = image }
                self.changed()
                self.updateThumbnail()
            }
        }
    }

    nonisolated static func renderThumbnail(_ doc: Document, width: Int, height: Int) throws -> CGImage? {
        let pixels = try doc.pixels()
        guard pixels.count == width * height * 4 else { return nil }
        let space = CGColorSpace(name: CGColorSpace.sRGB)!
        let info = CGImageAlphaInfo.premultipliedLast.rawValue
        // The engine gives straight alpha; premultiply for Core Graphics.
        var premultiplied = pixels
        for i in stride(from: 0, to: premultiplied.count, by: 4) {
            let a = UInt16(premultiplied[i + 3])
            if a == 255 { continue }
            for c in 0..<3 { premultiplied[i + c] = UInt8((UInt16(premultiplied[i + c]) * a + 127) / 255) }
        }
        guard
            let provider = CGDataProvider(data: Data(premultiplied) as CFData),
            let full = CGImage(
                width: width, height: height, bitsPerComponent: 8, bitsPerPixel: 32, bytesPerRow: width * 4,
                space: space, bitmapInfo: CGBitmapInfo(rawValue: info), provider: provider, decode: nil,
                shouldInterpolate: true, intent: .defaultIntent)
        else { return nil }
        let box = CGSize(width: 112, height: 80)
        let scale = min(box.width / CGFloat(width), box.height / CGFloat(height), 1)
        let tw = max(1, Int((CGFloat(width) * scale).rounded()))
        let th = max(1, Int((CGFloat(height) * scale).rounded()))
        guard
            let ctx = CGContext(
                data: nil, width: tw, height: th, bitsPerComponent: 8, bytesPerRow: 0, space: space,
                bitmapInfo: info)
        else { return nil }
        ctx.interpolationQuality = .high
        ctx.draw(full, in: CGRect(x: 0, y: 0, width: tw, height: th))
        return ctx.makeImage()
    }
}
