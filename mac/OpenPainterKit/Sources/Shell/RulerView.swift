// View > Rulers (M56): a ruler along the top and the left edge of the canvas,
// in the current units, following zoom and scroll. As in paint.net
// (https://www.getpaint.net/doc/latest/ViewMenu.html), the selection's bounds
// are highlighted and the pointer position is marked. Colors are theme tokens,
// so the rulers follow the appearance.
import AppKit

@MainActor
public final class RulerView: NSView {
    public enum Axis: Sendable { case horizontal, vertical }

    /// Thickness in points.
    public static let thickness: CGFloat = 20
    public let axis: Axis
    public var scale = RulerScale(unit: .pixels, dpi: 96, zoom: 1) { didSet { if scale != oldValue { needsDisplay = true } } }
    /// View position of document pixel 0 along the ruler (the viewport origin).
    public var origin: CGFloat = 0 { didSet { if origin != oldValue { needsDisplay = true } } }
    /// The selection's bounds along this axis, in view points.
    public var highlight: ClosedRange<CGFloat>? { didSet { if highlight != oldValue { needsDisplay = true } } }
    /// The pointer along this axis, in view points.
    public var marker: CGFloat? { didSet { if marker != oldValue { needsDisplay = true } } }

    public init(axis: Axis) {
        self.axis = axis
        super.init(frame: .zero)
        setAccessibilityElement(true)
        setAccessibilityRole(.ruler)
        setAccessibilityLabel(axis == .horizontal ? "Horizontal ruler" : "Vertical ruler")
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("not used") }

    public override var isFlipped: Bool { true }
    public override func hitTest(_ point: NSPoint) -> NSView? { nil }

    /// Length along the axis in points.
    var length: CGFloat { axis == .horizontal ? bounds.width : bounds.height }

    public var ticks: [RulerTick] { scale.ticks(origin: Double(origin), length: Double(length)) }

    /// Tick length for each level, in points.
    static func tickLength(_ level: RulerTick.Level) -> CGFloat {
        switch level {
        case .major: thickness
        case .middle: 8
        case .minor: 4
        }
    }

    public override func draw(_ dirtyRect: NSRect) {
        Theme.chrome.setFill()
        bounds.fill()
        let horizontal = axis == .horizontal
        if let h = highlight {
            Theme.wash.setFill()
            let lo = max(h.lowerBound, 0), hi = min(h.upperBound, length)
            if hi > lo {
                (horizontal ? NSRect(x: lo, y: 0, width: hi - lo, height: bounds.height)
                    : NSRect(x: 0, y: lo, width: bounds.width, height: hi - lo)).fill()
            }
        }
        // The edge next to the canvas.
        Theme.line.setFill()
        (horizontal ? NSRect(x: 0, y: bounds.height - 1, width: bounds.width, height: 1)
            : NSRect(x: bounds.width - 1, y: 0, width: 1, height: bounds.height)).fill()
        let font = Theme.font(11)
        let attrs: [NSAttributedString.Key: Any] = [.font: font, .foregroundColor: Theme.muted]
        let side = Self.thickness
        for t in ticks {
            let p = (CGFloat(t.position)).rounded(.down) + 0.5
            let len = Self.tickLength(t.level)
            (t.level == .major ? Theme.ink : Theme.muted).setFill()
            if horizontal {
                NSRect(x: p - 0.5, y: side - len, width: 1, height: len).fill()
            } else {
                NSRect(x: side - len, y: p - 0.5, width: len, height: 1).fill()
            }
            guard t.level == .major else { continue }
            let text = NSAttributedString(string: scale.label(t.value), attributes: attrs)
            if horizontal {
                text.draw(at: NSPoint(x: p + 3, y: 1))
            } else {
                // Rotated a quarter turn, reading bottom to top, just after the tick.
                NSGraphicsContext.saveGraphicsState()
                let xf = NSAffineTransform()
                xf.translateX(by: 1, yBy: p + 3 + text.size().width)
                xf.rotate(byDegrees: -90)
                xf.concat()
                text.draw(at: NSPoint(x: 0, y: 0))
                NSGraphicsContext.restoreGraphicsState()
            }
        }
        if let m = marker, m >= 0, m <= length {
            Theme.accent.setFill()
            let p = m.rounded(.down)
            (horizontal ? NSRect(x: p, y: 0, width: 1, height: bounds.height)
                : NSRect(x: 0, y: p, width: bounds.width, height: 1)).fill()
        }
    }
}

/// The square where the two rulers meet; it shows the unit's short name.
@MainActor
final class RulerCornerView: FilledView {
    var unit: MeasurementUnit = .pixels { didSet { needsDisplay = true } }

    init() { super.init(fill: Theme.chrome) }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("not used") }

    override func hitTest(_ point: NSPoint) -> NSView? { nil }

    override func draw(_ dirtyRect: NSRect) {
        super.draw(dirtyRect)
        Theme.line.setFill()
        NSRect(x: 0, y: bounds.height - 1, width: bounds.width, height: 1).fill()
        NSRect(x: bounds.width - 1, y: 0, width: 1, height: bounds.height).fill()
        let text = NSAttributedString(
            string: unit.abbreviation, attributes: [.font: Theme.font(11), .foregroundColor: Theme.muted])
        let s = text.size()
        text.draw(at: NSPoint(x: (bounds.width - s.width) / 2, y: (bounds.height - s.height) / 2))
    }
}
