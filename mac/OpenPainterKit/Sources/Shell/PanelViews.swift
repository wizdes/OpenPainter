// Panel contents. Tools is complete for this version; Colors, Layers and
// History are shells (M36 adds the wheel and palette, M20 the full Layers and
// History features).
import AppKit

// MARK: Tools

/// Two columns of 24 x 24 pt targets with 2 pt gaps. The selected tool has a
/// 2 pt edge and a dot. The list scrolls when the panel is short.
@MainActor
public final class ToolsPanelView: NSView {
    let tools: ToolState
    public private(set) var buttons: [PaintTool: ToolCell] = [:]
    private let scroll = NSScrollView()
    private let grid = FlippedView()
    static let cell: CGFloat = 24
    static let gap: CGFloat = 2
    static let inset: CGFloat = 5

    /// The content height that shows every tool with no empty space.
    public static var fittingHeight: CGFloat {
        let rows = CGFloat((PaintTool.allCases.count + 1) / 2)
        return rows * cell + (rows - 1) * gap + 2 * inset
    }

    public init(tools: ToolState) {
        self.tools = tools
        super.init(frame: .zero)
        wantsLayer = true
        scroll.documentView = grid
        scroll.hasVerticalScroller = true
        scroll.autohidesScrollers = true
        scroll.scrollerStyle = .overlay
        scroll.drawsBackground = false
        addSubview(scroll)
        for t in PaintTool.allCases {
            let b = ToolCell(tool: t, target: self, action: #selector(picked(_:)))
            buttons[t] = b
            grid.addSubview(b)
        }
        tools.observe(self) { [weak self] in self?.sync() }
        sync()
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("not used") }

    public override var isFlipped: Bool { true }

    public override func draw(_ dirtyRect: NSRect) {
        Theme.panel.setFill()
        bounds.fill()
    }

    public override func layout() {
        super.layout()
        scroll.frame = bounds
        let width = 2 * Self.cell + Self.gap
        let x0 = ((bounds.width - width) / 2).rounded()
        for (i, t) in PaintTool.allCases.enumerated() {
            let col = CGFloat(i % 2), row = CGFloat(i / 2)
            buttons[t]?.frame = NSRect(
                x: x0 + col * (Self.cell + Self.gap), y: Self.inset + row * (Self.cell + Self.gap),
                width: Self.cell, height: Self.cell)
        }
        grid.frame = NSRect(x: 0, y: 0, width: bounds.width, height: max(Self.fittingHeight, bounds.height))
    }

    func sync() {
        for (t, b) in buttons { b.isOn = t == tools.tool }
    }

    @objc private func picked(_ sender: ToolCell) {
        tools.tool = sender.tool
        sync()
    }
}

public final class ToolCell: ShellButton {
    public let tool: PaintTool
    public var isOn = false {
        didSet {
            state = isOn ? .on : .off
            setAccessibilitySelected(isOn)
            needsDisplay = true
        }
    }

    init(tool: PaintTool, target: AnyObject, action: Selector) {
        self.tool = tool
        let shortcut = String(tool.key).uppercased()
        super.init(symbol: tool.symbol, label: tool.label, tooltip: "\(tool.label) (\(shortcut))", size: 24,
                   pointSize: 14, target: target, action: action)
        translatesAutoresizingMaskIntoConstraints = true
        for c in constraints { c.isActive = false }
        setAccessibilityRole(.radioButton)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("not used") }

    public override func draw(_ dirtyRect: NSRect) {
        if isOn {
            let path = NSBezierPath(roundedRect: bounds.insetBy(dx: 0.5, dy: 0.5), xRadius: 3, yRadius: 3)
            Theme.wash.setFill()
            path.fill()
            Theme.accent.setFill()
            NSRect(x: 0, y: 0, width: 2, height: bounds.height).fill()  // 2 pt edge
            NSBezierPath(ovalIn: NSRect(x: bounds.width - 6, y: 2, width: 4, height: 4)).fill()  // dot
        }
        super.draw(dirtyRect)
    }
}

// The Colors panel (M36) is in ColorsPanel.swift.

// MARK: Rows for Layers and History
// (The Layers and History panels themselves are in LayersPanel.swift and
// HistoryPanel.swift.)

/// One list row: a 2 pt edge and a diamond when current, muted text when
/// `dimmed` (undone History steps), and an optional leading symbol.
public class PanelRow: NSView {
    public let index: Int
    public let titleLabel = NSTextField(labelWithString: "")
    public let detailLabel = NSTextField(labelWithString: "")
    let icon = NSImageView()
    public var isCurrent = false { didSet { setAccessibilitySelected(isCurrent); needsDisplay = true } }
    public var dimmed = false
    var onClick: ((Int) -> Void)?

    init(index: Int, title: String, detail: String?, symbol: String?, height: CGFloat) {
        self.index = index
        super.init(frame: NSRect(x: 0, y: 0, width: 200, height: height))
        titleLabel.stringValue = title
        titleLabel.font = Theme.font(12)
        titleLabel.lineBreakMode = .byTruncatingTail
        detailLabel.stringValue = detail ?? ""
        detailLabel.font = Theme.font(11)
        detailLabel.textColor = Theme.muted
        addSubview(titleLabel)
        if detail != nil { addSubview(detailLabel) }
        if let symbol {
            icon.image = NSImage(systemSymbolName: symbol, accessibilityDescription: nil)
            icon.contentTintColor = Theme.muted
            addSubview(icon)
        }
        setAccessibilityElement(true)
        setAccessibilityRole(.button)
        setAccessibilityLabel(title)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("not used") }

    public override var isFlipped: Bool { true }

    public override func layout() {
        super.layout()
        let x: CGFloat = icon.image == nil ? 10 : 34
        icon.frame = NSRect(x: 8, y: (bounds.height - 16) / 2, width: 18, height: 16)
        titleLabel.textColor = dimmed ? Theme.muted : Theme.ink
        titleLabel.font = Theme.font(12, isCurrent ? .semibold : .regular)
        if detailLabel.superview != nil {
            titleLabel.frame = NSRect(x: x, y: bounds.height / 2 - 17, width: bounds.width - x - 24, height: 16)
            detailLabel.frame = NSRect(x: x, y: bounds.height / 2 + 1, width: bounds.width - x - 24, height: 15)
        } else {
            titleLabel.frame = NSRect(x: x, y: (bounds.height - 16) / 2, width: bounds.width - x - 24, height: 16)
        }
    }

    public override func draw(_ dirtyRect: NSRect) {
        if isCurrent {
            Theme.wash.setFill()
            bounds.fill()
            Theme.accent.setFill()
            NSRect(x: 0, y: 0, width: 2, height: bounds.height).fill()
            let d = NSBezierPath()
            let c = NSPoint(x: bounds.width - 12, y: bounds.midY)
            d.move(to: NSPoint(x: c.x, y: c.y - 4))
            d.line(to: NSPoint(x: c.x + 4, y: c.y))
            d.line(to: NSPoint(x: c.x, y: c.y + 4))
            d.line(to: NSPoint(x: c.x - 4, y: c.y))
            d.close()
            d.fill()
        }
        Theme.line.setFill()
        NSRect(x: 0, y: bounds.height - 1, width: bounds.width, height: 1).fill()
    }

    public override func mouseDown(with event: NSEvent) {}

    public override func mouseUp(with event: NSEvent) {
        onClick?(index)
    }

    public override func accessibilityPerformPress() -> Bool {
        onClick?(index)
        return true
    }

    // M59: with Keyboard navigation on, Tab reaches each row; Space or Return
    // picks it and the up and down arrows move to the row above or below.
    public override var acceptsFirstResponder: Bool { KeyboardAccess.enabled }
    public override var canBecomeKeyView: Bool { KeyboardAccess.enabled }
    public override var focusRingMaskBounds: NSRect { bounds }
    public override func drawFocusRingMask() { bounds.insetBy(dx: 1, dy: 1).fill() }

    public override func keyDown(with event: NSEvent) {
        if KeyboardAccess.isPress(event) { return onClick?(index) ?? () }
        if let step = KeyboardAccess.verticalStep(event),
           let rows = superview?.subviews.compactMap({ $0 as? PanelRow }).sorted(by: { $0.frame.minY < $1.frame.minY }),
           let at = rows.firstIndex(where: { $0 === self }), rows.indices.contains(at + step) {
            let next = rows[at + step]
            window?.makeFirstResponder(next)
            next.scrollToVisible(next.bounds)
            return
        }
        super.keyDown(with: event)
    }
}

/// A scrolling list of rows above an action bar.
@MainActor
public class RowListPanelView: NSView {
    let scroll = NSScrollView()
    let list = FlippedView()
    let actions = NSStackView()
    public private(set) var rows: [PanelRow] = []
    let rowHeight: CGFloat

    init(rowHeight: CGFloat) {
        self.rowHeight = rowHeight
        super.init(frame: .zero)
        scroll.documentView = list
        scroll.hasVerticalScroller = true
        scroll.autohidesScrollers = true
        scroll.drawsBackground = false
        addSubview(scroll)
        actions.orientation = .horizontal
        actions.spacing = 4
        actions.edgeInsets = NSEdgeInsets(top: 2, left: 8, bottom: 2, right: 8)
        actions.frame = NSRect(x: 0, y: 0, width: 226, height: 28)
        addSubview(actions)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("not used") }

    public override var isFlipped: Bool { true }

    public override func draw(_ dirtyRect: NSRect) {
        Theme.panel.setFill()
        bounds.fill()
        Theme.line.setFill()
        NSRect(x: 0, y: bounds.height - 29, width: bounds.width, height: 1).fill()
    }

    public override func layout() {
        super.layout()
        scroll.frame = NSRect(x: 0, y: 0, width: bounds.width, height: max(bounds.height - 29, 0))
        actions.frame = NSRect(x: 0, y: bounds.height - 28, width: bounds.width, height: 28)
        layoutRows()
    }

    func setRows(_ new: [PanelRow]) {
        // M59: a row with keyboard focus keeps it across a reload.
        let focused = (window?.firstResponder as? PanelRow).flatMap { rows.contains($0) ? $0.index : nil }
        for r in rows { r.removeFromSuperview() }
        rows = new
        for r in rows { list.addSubview(r) }
        layoutRows()
        if let focused, let r = rows.first(where: { $0.index == focused }) ?? rows.last { window?.makeFirstResponder(r) }
    }

    private func layoutRows() {
        let w = scroll.contentSize.width
        for (i, r) in rows.enumerated() {
            r.frame = NSRect(x: 0, y: CGFloat(i) * rowHeight, width: w, height: rowHeight)
        }
        list.frame = NSRect(x: 0, y: 0, width: w, height: max(CGFloat(rows.count) * rowHeight, scroll.contentSize.height))
    }

    func scrollToCurrent() {
        if let r = rows.first(where: \.isCurrent) { list.scrollToVisible(r.frame) }
    }
}
