// Small controls the shell shares: 28 pt command buttons with 18 pt line
// drawings, the pressed panel toggles, and labels in the spec's type sizes.
import AppKit

/// A borderless command target with an SF Symbol line drawing.
public class ShellButton: NSButton {
    public let size: CGFloat

    public init(symbol: String, label: String, tooltip: String? = nil, size: CGFloat = Theme.commandTarget,
                pointSize: CGFloat = 18, target: AnyObject?, action: Selector?) {
        self.size = size
        super.init(frame: NSRect(x: 0, y: 0, width: size, height: size))
        let config = NSImage.SymbolConfiguration(pointSize: pointSize, weight: .regular)
        image = NSImage(systemSymbolName: symbol, accessibilityDescription: label)?.withSymbolConfiguration(config)
        imagePosition = .imageOnly
        isBordered = false
        bezelStyle = .regularSquare
        contentTintColor = Theme.ink
        self.target = target
        self.action = action
        toolTip = tooltip ?? label
        setAccessibilityLabel(label)
        translatesAutoresizingMaskIntoConstraints = false
        widthAnchor.constraint(equalToConstant: size).isActive = true
        heightAnchor.constraint(equalToConstant: size).isActive = true
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("not used") }

    public override var intrinsicContentSize: NSSize { NSSize(width: size, height: size) }
}

/// A toggle that shows its pressed state with a filled wash, an outline and a
/// 2 pt bottom stroke (panel toggles and selected tools).
public final class ToggleButton: ShellButton {
    public var isOn: Bool {
        get { state == .on }
        set {
            state = newValue ? .on : .off
            setAccessibilityValue(newValue ? "shown" : "hidden")
            needsDisplay = true
        }
    }

    public override init(symbol: String, label: String, tooltip: String? = nil, size: CGFloat = Theme.commandTarget,
                         pointSize: CGFloat = 18, target: AnyObject?, action: Selector?) {
        super.init(symbol: symbol, label: label, tooltip: tooltip, size: size, pointSize: pointSize,
                   target: target, action: action)
        setButtonType(.pushOnPushOff)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("not used") }

    public override func draw(_ dirtyRect: NSRect) {
        if state == .on {
            let r = bounds.insetBy(dx: 0.5, dy: 0.5)
            let path = NSBezierPath(roundedRect: r, xRadius: 4, yRadius: 4)
            Theme.wash.setFill()
            path.fill()
            Theme.accent.setStroke()
            path.lineWidth = 1
            path.stroke()
            Theme.accent.setFill()
            let y = isFlipped ? bounds.maxY - 2 : bounds.minY
            NSRect(x: 3, y: y, width: bounds.width - 6, height: 2).fill()
        }
        super.draw(dirtyRect)
    }
}

enum Labels {
    static func label(_ text: String, size: CGFloat = 12, color: NSColor = Theme.muted,
                      weight: NSFont.Weight = .regular) -> NSTextField {
        let l = NSTextField(labelWithString: text)
        l.font = Theme.font(size, weight)
        l.textColor = color
        l.lineBreakMode = .byTruncatingTail
        return l
    }

    static func field(_ text: String, width: CGFloat, height: CGFloat = 24) -> NSTextField {
        let f = NSTextField(string: text)
        f.font = Theme.font(12)
        f.textColor = Theme.ink
        f.alignment = .center
        f.translatesAutoresizingMaskIntoConstraints = false
        f.widthAnchor.constraint(equalToConstant: width).isActive = true
        f.heightAnchor.constraint(equalToConstant: height).isActive = true
        return f
    }

    static func popup(_ items: [String], width: CGFloat? = nil) -> NSPopUpButton {
        let p = NSPopUpButton(frame: .zero, pullsDown: false)
        p.addItems(withTitles: items)
        p.font = Theme.font(12)
        p.controlSize = .regular
        p.translatesAutoresizingMaskIntoConstraints = false
        if let width { p.widthAnchor.constraint(equalToConstant: width).isActive = true }
        return p
    }

    static func gap(_ w: CGFloat) -> NSView {
        let v = NSView()
        v.translatesAutoresizingMaskIntoConstraints = false
        v.widthAnchor.constraint(equalToConstant: w).isActive = true
        return v
    }
}
