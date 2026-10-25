// The 64 pt image strip: one item per open image with a 56 x 40 pt preview and
// a one-line 12 pt name, an asterisk on modified images, a 2 pt outline and a
// dot on the active one, a close target on hover or focus. Click switches,
// drag reorders, and the strip scrolls when the items do not fit.
import AppKit

@MainActor
public final class ImageStripView: FilledView {
    weak var workspace: Workspace?
    private let scroll = NSScrollView()
    private let content = FlippedView()
    public private(set) var items: [ImageStripItem] = []
    static let itemWidth: CGFloat = 104
    static let gap: CGFloat = 8

    init(workspace: Workspace) {
        self.workspace = workspace
        super.init(fill: Theme.chrome, dividers: [.bottom])
        scroll.documentView = content
        scroll.hasHorizontalScroller = true
        scroll.hasVerticalScroller = false
        scroll.autohidesScrollers = true
        scroll.scrollerStyle = .overlay
        scroll.drawsBackground = false
        scroll.verticalScrollElasticity = .none
        addSubview(scroll)
        setAccessibilityRole(.list)
        setAccessibilityLabel("Open images")
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("not used") }

    public override func layout() {
        super.layout()
        scroll.frame = NSRect(x: 0, y: 0, width: bounds.width, height: bounds.height - 1)
        layoutItems()
    }

    /// Rebuilds the items from the workspace's editors, in order.
    public func reload() {
        guard let ws = workspace else { return }
        let editors = ws.editors
        if items.count != editors.count || zip(items, editors).contains(where: { $0.editor !== $1 }) {
            for i in items { i.removeFromSuperview() }
            items = editors.map { e in
                let item = ImageStripItem(editor: e, strip: self)
                content.addSubview(item)
                return item
            }
        }
        for i in items { i.update(active: i.editor === ws.active) }
        layoutItems()
    }

    private func layoutItems() {
        let h = scroll.bounds.height
        var x = Self.gap
        for i in items where i.dragOffset == nil {
            i.frame = NSRect(x: x, y: 0, width: Self.itemWidth, height: h)
            x += Self.itemWidth + Self.gap
        }
        content.frame = NSRect(x: 0, y: 0, width: max(x, scroll.contentSize.width), height: h)
    }

    /// The index a dragged item at strip x-position `x` should move to.
    func index(forX x: CGFloat) -> Int {
        let slot = Int(((x - Self.gap) / (Self.itemWidth + Self.gap)).rounded(.down))
        return min(max(slot, 0), max(items.count - 1, 0))
    }

    func itemDragEnded(_ item: ImageStripItem, at x: CGFloat) {
        guard let ws = workspace, let editor = item.editor else { return }
        ws.move(editor, to: index(forX: x + Self.itemWidth / 2))
        layoutItems()
    }
}

final class FlippedView: NSView {
    override var isFlipped: Bool { true }
}

@MainActor
public final class ImageStripItem: NSView {
    public private(set) weak var editor: Editor?
    weak var strip: ImageStripView?
    public let nameLabel = NSTextField(labelWithString: "")
    public private(set) var closeButton: ShellButton!
    public private(set) var isActive = false
    var dragOffset: CGFloat?
    private var downPoint: CGPoint?
    private var hovering = false

    init(editor: Editor, strip: ImageStripView) {
        self.editor = editor
        self.strip = strip
        super.init(frame: .zero)
        nameLabel.font = Theme.font(12)
        nameLabel.textColor = Theme.ink
        nameLabel.alignment = .center
        nameLabel.lineBreakMode = .byTruncatingTail
        nameLabel.maximumNumberOfLines = 1
        nameLabel.cell?.truncatesLastVisibleLine = true
        addSubview(nameLabel)
        closeButton = ShellButton(symbol: "xmark", label: "Close image", size: 24, pointSize: 10, target: self,
                                  action: #selector(closeClicked(_:)))
        closeButton.translatesAutoresizingMaskIntoConstraints = true
        for c in closeButton.constraints { c.isActive = false }
        closeButton.isHidden = true
        addSubview(closeButton)
        setAccessibilityElement(true)
        setAccessibilityRole(.button)
        addTrackingArea(NSTrackingArea(rect: .zero, options: [.mouseEnteredAndExited, .activeAlways, .inVisibleRect],
                                       owner: self, userInfo: nil))
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("not used") }

    public override var isFlipped: Bool { true }
    public override var acceptsFirstResponder: Bool { true }
    public override var canBecomeKeyView: Bool { true }

    /// The name as shown, with an asterisk when the image is modified.
    public var shownName: String { nameLabel.stringValue }

    func update(active: Bool) {
        guard let e = editor else { return }
        isActive = active
        nameLabel.stringValue = e.info.dirty ? "\(e.name) *" : e.name
        nameLabel.font = Theme.font(12, active ? .semibold : .regular)
        toolTip = e.name
        setAccessibilityLabel(e.name)
        setAccessibilityValue((e.info.dirty ? "modified" : "saved") + (active ? ", active" : ""))
        setAccessibilitySelected(active)
        needsDisplay = true
    }

    public override func layout() {
        super.layout()
        // Preview 4...44, name 46...62: the 16 pt line fits 12 pt text without clipping.
        nameLabel.frame = NSRect(x: 2, y: 45, width: bounds.width - 4, height: 17)
        closeButton.frame = NSRect(x: bounds.width - 24, y: 0, width: 24, height: 24)
    }

    var previewRect: NSRect { NSRect(x: (bounds.width - 56) / 2, y: 4, width: 56, height: 40) }

    public override func draw(_ dirtyRect: NSRect) {
        let r = previewRect
        if isActive {
            let outline = NSBezierPath(roundedRect: bounds.insetBy(dx: 1, dy: 1), xRadius: 6, yRadius: 6)
            Theme.wash.setFill()
            outline.fill()
            Theme.accent.setStroke()
            outline.lineWidth = 2
            outline.stroke()
            Theme.accent.setFill()
            NSBezierPath(ovalIn: NSRect(x: 6, y: 6, width: 5, height: 5)).fill()
        }
        if let img = editor?.thumbnail {
            let w = CGFloat(img.width), h = CGFloat(img.height)
            let s = min(r.width / w, r.height / h)
            let fit = NSRect(x: r.midX - w * s / 2, y: r.midY - h * s / 2, width: w * s, height: h * s)
            NSImage(cgImage: img, size: fit.size).draw(in: fit)
        } else {
            Theme.line.setFill()
            r.fill()
        }
        Theme.line.setStroke()
        NSBezierPath(rect: r.insetBy(dx: 0.5, dy: 0.5)).stroke()
        if window?.firstResponder === self {
            Theme.accent.setStroke()
            let ring = NSBezierPath(roundedRect: bounds.insetBy(dx: 2, dy: 2), xRadius: 5, yRadius: 5)
            ring.lineWidth = 2
            ring.stroke()
        }
    }

    public override func mouseEntered(with event: NSEvent) {
        hovering = true
        closeButton.isHidden = false
    }

    public override func mouseExited(with event: NSEvent) {
        hovering = false
        closeButton.isHidden = window?.firstResponder !== self
    }

    public override func becomeFirstResponder() -> Bool {
        closeButton.isHidden = false
        needsDisplay = true
        return true
    }

    public override func resignFirstResponder() -> Bool {
        closeButton.isHidden = !hovering
        needsDisplay = true
        return true
    }

    public override func keyDown(with event: NSEvent) {
        switch event.keyCode {
        case 36, 49: activate()  // Return, Space
        default: super.keyDown(with: event)
        }
    }

    public override func accessibilityPerformPress() -> Bool {
        activate()
        return true
    }

    func activate() {
        guard let e = editor else { return }
        strip?.workspace?.activate(e)
    }

    public override func mouseDown(with event: NSEvent) {
        downPoint = superview?.convert(event.locationInWindow, from: nil)
        dragOffset = nil
    }

    public override func mouseDragged(with event: NSEvent) {
        guard let down = downPoint, let p = superview?.convert(event.locationInWindow, from: nil) else { return }
        if dragOffset == nil, abs(p.x - down.x) < 4 { return }
        if dragOffset == nil { dragOffset = down.x - frame.minX }
        frame.origin.x = p.x - (dragOffset ?? 0)
        superview?.addSubview(self)  // draw above the others while dragging
    }

    public override func mouseUp(with event: NSEvent) {
        defer { downPoint = nil }
        if dragOffset != nil {
            let x = frame.minX
            dragOffset = nil
            strip?.itemDragEnded(self, at: x)
        } else {
            activate()
        }
    }

    @objc private func closeClicked(_ sender: Any?) {
        guard let editor else { return }
        if let host = editor.host {
            host.closeEditor()  // asks to save a modified image first
        } else {
            editor.windowController?.closeWindow()
        }
    }
}
