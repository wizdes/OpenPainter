// The Layers panel (M20): one 49 pt row per layer, top layer first, with a
// cached thumbnail, the name, a 24 x 24 pt eye target and the active row's
// 2 pt edge and diamond. The action row has Add, Delete, Duplicate, Merge
// Down, Move Up, Move Down and Properties. Rows reorder by drag. Every change
// is an engine command; choosing a layer adds no history step.
import AppKit

// MARK: Thumbnails

/// Layer thumbnails keyed by document, layer id and the layer's content key
/// (`revision` in `doc.info`). A key comes back when history returns to the
/// same pixels, so a History jump reuses the image it made before.
@MainActor
public final class LayerThumbnailCache {
    public struct Key: Hashable, Sendable {
        public let doc: UInt64
        public let layer: Int
        public let revision: Int
    }
    /// 40 pt boxes at 2x.
    public static let pixels = 80
    static let limit = 256
    private var images: [Key: CGImage] = [:]
    private var order: [Key] = []
    private var pending: Set<Key> = []
    /// Thumbnails rendered so far (tests check that cache hits render nothing).
    public private(set) var renders = 0
    /// Called with the key of each finished thumbnail.
    var onReady: ((Key) -> Void)?

    public init() {}

    public func cached(_ key: Key) -> CGImage? { images[key] }

    /// The cached image, or nil after starting a render that calls `onReady`.
    func image(for layer: DocInfo.Layer, in document: Document) -> CGImage? {
        let key = Key(doc: document.id, layer: layer.id, revision: layer.revision)
        if let image = images[key] { return image }
        guard !pending.contains(key) else { return nil }
        pending.insert(key)
        let id = layer.id
        Task.detached(priority: .utility) {
            let result: (CGImage, Int)?
            do {
                result = try LayerThumbnailCache.render(document, layer: id)
            } catch {
                FileHandle.standardError.write(Data("OpenPainter: layer thumbnail failed: \(error)\n".utf8))
                result = nil
            }
            await MainActor.run { [weak self] in
                guard let self else { return }
                self.pending.remove(key)
                guard let (image, revision) = result else { return }
                // The engine names the pixels it read; store under that key.
                let got = Key(doc: key.doc, layer: key.layer, revision: revision)
                self.store(image, got)
                self.renders += 1
                self.onReady?(got)
            }
        }
        return nil
    }

    private func store(_ image: CGImage, _ key: Key) {
        if images.updateValue(image, forKey: key) == nil { order.append(key) }
        while order.count > Self.limit { images.removeValue(forKey: order.removeFirst()) }
    }

    /// `layer.thumbnail`: width, height and content key, then straight RGBA.
    nonisolated static func render(_ doc: Document, layer: Int) throws -> (CGImage, Int)? {
        let data = try doc.queryData("layer.thumbnail", ["id": layer, "max": pixels])
        guard data.count >= 16 else { return nil }
        let bytes = [UInt8](data)
        func u32(_ at: Int) -> Int { (0..<4).reduce(0) { $0 | Int(bytes[at + $1]) << (8 * $1) } }
        let w = u32(0), h = u32(4)
        let key = (0..<8).reduce(UInt64(0)) { $0 | UInt64(bytes[8 + $1]) << (8 * UInt64($1)) }
        guard w > 0, h > 0, bytes.count == 16 + w * h * 4 else { return nil }
        var rgba = Array(bytes[16...])
        // Straight alpha to premultiplied for Core Graphics.
        for i in stride(from: 0, to: rgba.count, by: 4) {
            let a = UInt16(rgba[i + 3])
            if a == 255 { continue }
            for c in 0..<3 { rgba[i + c] = UInt8((UInt16(rgba[i + c]) * a + 127) / 255) }
        }
        guard
            let provider = CGDataProvider(data: Data(rgba) as CFData),
            let image = CGImage(
                width: w, height: h, bitsPerComponent: 8, bitsPerPixel: 32, bytesPerRow: w * 4,
                space: CGColorSpace(name: CGColorSpace.sRGB)!,
                bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.premultipliedLast.rawValue),
                provider: provider, decode: nil, shouldInterpolate: true, intent: .defaultIntent)
        else { return nil }
        return (image, Int(truncatingIfNeeded: key))
    }
}

/// A thumbnail on a checkerboard, aspect-fit in its bounds, with a 1 pt edge.
public final class LayerThumbView: NSView {
    public var image: CGImage? { didSet { needsDisplay = true } }
    var pixelSize = CGSize(width: 1, height: 1)

    public override var isFlipped: Bool { true }

    /// The image rectangle inside the bounds.
    var imageRect: NSRect {
        let s = min(bounds.width / pixelSize.width, bounds.height / pixelSize.height)
        let w = (pixelSize.width * s).rounded(), h = (pixelSize.height * s).rounded()
        return NSRect(x: ((bounds.width - w) / 2).rounded(), y: ((bounds.height - h) / 2).rounded(), width: max(w, 1),
                      height: max(h, 1))
    }

    public override func draw(_ dirtyRect: NSRect) {
        let r = imageRect
        // Transparency checkerboard: content, not themed (like the canvas).
        NSColor.white.setFill()
        r.fill()
        NSColor(white: 0.8, alpha: 1).setFill()
        let sq: CGFloat = 4
        var y = r.minY
        var row = 0
        while y < r.maxY {
            var x = r.minX + (row % 2 == 0 ? 0 : sq)
            while x < r.maxX {
                NSRect(x: x, y: y, width: sq, height: sq).intersection(r).fill()
                x += 2 * sq
            }
            y += sq
            row += 1
        }
        if let image, let ctx = NSGraphicsContext.current?.cgContext {
            ctx.saveGState()
            // Flipped view: draw the image upright.
            ctx.translateBy(x: 0, y: r.minY + r.maxY)
            ctx.scaleBy(x: 1, y: -1)
            ctx.interpolationQuality = .high
            ctx.draw(image, in: r)
            ctx.restoreGState()
        }
        Theme.panelLine.setStroke()
        NSBezierPath(rect: r.insetBy(dx: -0.5, dy: -0.5)).stroke()
    }
}

// MARK: Rows

/// One layer: thumbnail, name, a detail line when the layer is hidden or not
/// plain Normal at 100%, and the eye target. Dragging the row reorders layers.
public final class LayerRow: PanelRow {
    public let docLayer: DocInfo.Layer
    public let thumb = LayerThumbView()
    public private(set) var eye: ShellButton!
    var onToggleVisible: ((Int) -> Void)?
    var onOpenProperties: ((Int) -> Void)?
    weak var dragOwner: LayersPanelView?
    private var downPoint: NSPoint?
    public private(set) var isDragging = false

    init(index: Int, layer: DocInfo.Layer, pixelSize: CGSize) {
        self.docLayer = layer
        let detail = LayerRow.detail(layer)
        super.init(index: index, title: layer.name, detail: detail, symbol: nil, height: 49)
        thumb.pixelSize = pixelSize
        thumb.setAccessibilityElement(false)
        addSubview(thumb)
        eye = ShellButton(symbol: layer.visible ? "eye" : "eye.slash",
                          label: layer.visible ? "Hide \(layer.name)" : "Show \(layer.name)",
                          tooltip: layer.visible ? "Visible: click to hide" : "Hidden: click to show",
                          size: 24, pointSize: 13, target: nil, action: nil)
        eye.translatesAutoresizingMaskIntoConstraints = true
        for c in eye.constraints { c.isActive = false }
        eye.target = self
        eye.action = #selector(eyeClicked(_:))
        eye.contentTintColor = layer.visible ? Theme.ink : Theme.muted
        eye.setAccessibilityValue(layer.visible ? "visible" : "hidden")
        addSubview(eye)
        setAccessibilityValue(layer.visible ? "visible" : "hidden")
        setAccessibilityHelp("Drag to reorder. Double-click for Layer Properties.")
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("not used") }

    static func detail(_ l: DocInfo.Layer) -> String? {
        var parts: [String] = []
        if !l.visible { parts.append("Hidden") }
        if l.blend != "normal" { parts.append(LayerBlend.title(l.blend)) }
        if l.opacity != 255 { parts.append("\(Int((Double(l.opacity) * 100 / 255).rounded()))%") }
        return parts.isEmpty ? nil : parts.joined(separator: " · ")
    }

    public override func layout() {
        super.layout()
        // Thumbnail 40 x 40 pt, 8 pt in; text after it; eye before the diamond.
        thumb.frame = NSRect(x: 8, y: ((bounds.height - 40) / 2).rounded(), width: 40, height: 40)
        eye.frame = NSRect(x: bounds.width - 52, y: ((bounds.height - 24) / 2).rounded(), width: 24, height: 24)
        let x: CGFloat = 56, w = bounds.width - x - 56
        if detailLabel.superview != nil {
            titleLabel.frame = NSRect(x: x, y: bounds.height / 2 - 17, width: w, height: 16)
            detailLabel.frame = NSRect(x: x, y: bounds.height / 2 + 1, width: w, height: 15)
        } else {
            titleLabel.frame = NSRect(x: x, y: (bounds.height - 16) / 2, width: w, height: 16)
        }
    }

    @objc private func eyeClicked(_ sender: Any?) { onToggleVisible?(index) }

    public override func mouseDown(with event: NSEvent) {
        downPoint = event.locationInWindow
        isDragging = false
    }

    public override func mouseDragged(with event: NSEvent) {
        guard let start = downPoint else { return }
        let p = event.locationInWindow
        if !isDragging, hypot(p.x - start.x, p.y - start.y) < 3 { return }
        isDragging = true
        dragOwner?.dragMoved(self, to: dragOwner?.list.convert(p, from: nil) ?? .zero)
    }

    public override func mouseUp(with event: NSEvent) {
        defer { downPoint = nil; isDragging = false }
        if isDragging {
            dragOwner?.dragEnded(self, at: dragOwner?.list.convert(event.locationInWindow, from: nil) ?? .zero)
        } else if event.clickCount >= 2 {
            onOpenProperties?(index)
        } else {
            onClick?(index)
        }
    }
}

/// The 14 blend modes: engine names and paint.net titles, in paint.net order.
public enum LayerBlend {
    public static let modes: [(id: String, title: String)] = [
        ("normal", "Normal"), ("multiply", "Multiply"), ("additive", "Additive"), ("color_burn", "Color Burn"),
        ("color_dodge", "Color Dodge"), ("reflect", "Reflect"), ("glow", "Glow"), ("overlay", "Overlay"),
        ("difference", "Difference"), ("negation", "Negation"), ("lighten", "Lighten"), ("darken", "Darken"),
        ("screen", "Screen"), ("xor", "Xor"),
    ]

    public static func title(_ id: String) -> String { modes.first { $0.id == id }?.title ?? id }
}

// MARK: Panel

@MainActor
public final class LayersPanelView: RowListPanelView {
    weak var workspace: Workspace?
    public let thumbnails = LayerThumbnailCache()
    /// Buttons by engine command id (Properties is "layer.properties").
    public private(set) var buttons: [String: ShellButton] = [:]
    public var addButton: ShellButton { buttons["layer.add"]! }
    public var deleteButton: ShellButton { buttons["layer.delete"]! }
    /// The insertion line shown while a row is dragged.
    let dropLine = FilledView(fill: Theme.accent)
    public private(set) var dropGap: Int?
    private var reloadAfterDrag = false
    public var layerRows: [LayerRow] { rows.compactMap { $0 as? LayerRow } }

    init(workspace: Workspace) {
        self.workspace = workspace
        super.init(rowHeight: 49)
        let items: [(String, String, String)] = [
            ("layer.add", "plus.square", "Add New Layer"),
            ("layer.delete", "trash", "Delete Layer"),
            ("layer.duplicate", "plus.square.on.square", "Duplicate Layer"),
            ("layer.merge_down", "arrow.down.to.line", "Merge Layer Down"),
            ("layer.move_up", "arrow.up", "Move Layer Up"),
            ("layer.move_down", "arrow.down", "Move Layer Down"),
            ("layer.properties", "slider.horizontal.3", "Layer Properties"),
        ]
        for (id, symbol, label) in items {
            let tip = id == "layer.add" ? "\(label) (⇧⌘N)" : id == "layer.duplicate" ? "\(label) (⇧⌘D)"
                : id == "layer.delete" ? "\(label) (⇧⌘⌫)" : id == "layer.properties" ? "\(label) (F4)" : label
            let b = ShellButton(symbol: symbol, label: label, tooltip: tip, size: 24, pointSize: 13, target: self,
                                action: #selector(action(_:)))
            b.identifier = NSUserInterfaceItemIdentifier(id)
            buttons[id] = b
            actions.addArrangedSubview(b)
        }
        dropLine.isHidden = true
        list.addSubview(dropLine)
        thumbnails.onReady = { [weak self] key in self?.thumbnailReady(key) }
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("not used") }

    /// Commands are off while a sheet (Layer Properties) is up on the document.
    var busy: Bool { FocusRouter.target(in: workspace?.active?.windowController?.window) == .dialog }

    func reload() {
        if layerRows.contains(where: \.isDragging) {
            reloadAfterDrag = true
            return
        }
        guard let e = workspace?.active else {
            setRows([])
            for b in buttons.values { b.isEnabled = false }
            return
        }
        let layers = e.info.layers
        let size = CGSize(width: max(e.info.width, 1), height: max(e.info.height, 1))
        let busy = self.busy
        // Top layer first, like paint.net.
        let rows: [PanelRow] = layers.indices.reversed().map { i in
            let row = LayerRow(index: i, layer: layers[i], pixelSize: size)
            row.isCurrent = i == e.info.activeLayer
            row.thumb.image = thumbnails.image(for: layers[i], in: e.document)
            row.onClick = { [weak self] index in self?.select(index) }
            row.onToggleVisible = { [weak self] index in self?.toggleVisible(index) }
            row.onOpenProperties = { [weak self] index in
                self?.select(index)
                self?.workspace?.active?.windowController?.showLayerProperties(index: index)
            }
            row.dragOwner = self
            row.eye.isEnabled = !busy
            return row
        }
        setRows(rows)
        for (id, b) in buttons {
            b.isEnabled = !busy && (id == "layer.properties" ? !layers.isEmpty : e.isEnabled(id))
        }
        scrollToCurrent()
    }

    private func thumbnailReady(_ key: LayerThumbnailCache.Key) {
        guard let e = workspace?.active, e.document.id == key.doc else { return }
        for row in layerRows where row.docLayer.id == key.layer && row.docLayer.revision == key.revision {
            row.thumb.image = thumbnails.cached(key)
        }
    }

    func select(_ index: Int) {
        guard !busy, let e = workspace?.active, index != e.info.activeLayer else { return }
        e.run("layer.set_active", ["index": index])
    }

    func toggleVisible(_ index: Int) {
        guard !busy, let e = workspace?.active, index < e.info.layers.count else { return }
        e.run("layer.set_visible", ["index": index, "visible": !e.info.layers[index].visible])
    }

    @objc func action(_ sender: NSButton) {
        guard !busy, let id = sender.identifier?.rawValue, let wc = workspace?.active?.windowController else { return }
        switch id {
        case "layer.add": wc.addLayer()
        case "layer.properties": wc.showLayerProperties(index: wc.editor.info.activeLayer)
        default: wc.editor.run(id)
        }
    }

    // MARK: Drag reorder

    /// The gap (0 = above the top row) nearest to a point in the list.
    func gap(at p: NSPoint) -> Int {
        min(max(Int((p.y / rowHeight).rounded()), 0), rows.count)
    }

    func dragMoved(_ row: LayerRow, to p: NSPoint) {
        guard !busy else { return }
        let g = gap(at: p)
        dropGap = g
        dropLine.frame = NSRect(x: 0, y: CGFloat(g) * rowHeight - 1, width: list.bounds.width, height: 2)
        dropLine.isHidden = false
        list.scrollToVisible(dropLine.frame.insetBy(dx: 0, dy: -rowHeight / 2))
    }

    /// Drops the row at the gap under `p`: one `layer.move` step, or nothing
    /// when the row would land where it is.
    func dragEnded(_ row: LayerRow, at p: NSPoint) {
        dropLine.isHidden = true
        dropGap = nil
        defer {
            if reloadAfterDrag {
                reloadAfterDrag = false
                DispatchQueue.main.async { [weak self] in self?.reload() }
            }
        }
        guard !busy, let e = workspace?.active, let to = Self.dropIndex(from: row.index, gap: gap(at: p),
                                                                         count: e.info.layers.count)
        else { return }
        e.run("layer.move", ["from": row.index, "to": to])
    }

    /// Rows are top layer first. Moving layer `from` into display gap `gap`
    /// gives the layer index it lands at, or nil for no move.
    public static func dropIndex(from: Int, gap: Int, count: Int) -> Int? {
        let fromRow = count - 1 - from
        let toRow = gap > fromRow ? gap - 1 : gap
        let to = count - 1 - min(max(toRow, 0), count - 1)
        return to == from ? nil : to
    }
}
