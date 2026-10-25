// Classic v2 appearance tokens (docs/design/SPEC.md, "Appearance tokens").
// Each token is a dynamic NSColor with the light and dark values from the spec.
import AppKit

public enum Theme {
    static func hex(_ v: UInt32) -> NSColor {
        NSColor(
            srgbRed: CGFloat((v >> 16) & 0xFF) / 255, green: CGFloat((v >> 8) & 0xFF) / 255,
            blue: CGFloat(v & 0xFF) / 255, alpha: 1)
    }

    static func token(_ name: String, light: UInt32, dark: UInt32) -> NSColor {
        NSColor(name: NSColor.Name(name)) { appearance in
            appearance.bestMatch(from: [.darkAqua, .aqua]) == .darkAqua ? hex(dark) : hex(light)
        }
    }

    public static let chrome = token("chrome", light: 0xF5F5F6, dark: 0x353A42)
    public static let options = token("options", light: 0xE9EBED, dark: 0x2E333B)
    public static let panel = token("panel", light: 0xF9F9FA, dark: 0x363B43)
    public static let field = token("field", light: 0xFFFFFF, dark: 0x262B32)
    public static let ink = token("ink", light: 0x202329, dark: 0xEEF0F3)
    public static let muted = token("muted", light: 0x5C626C, dark: 0xBFC5CE)
    public static let line = token("line", light: 0xCDD1D6, dark: 0x515862)
    public static let panelLine = token("panel-line", light: 0x9DA5AD, dark: 0x8E98A3)
    public static let stage = token("stage", light: 0x777777, dark: 0x252525)
    public static let accent = token("accent", light: 0x1973C8, dark: 0x76B8FF)
    public static let wash = token("wash", light: 0xDCECFF, dark: 0x244967)
    public static let onAccent = token("on-accent", light: 0x101D2B, dark: 0x101D2B)

    // Sizes from the spec, in points.
    public static let optionsHeight: CGFloat = 36
    public static let stripHeight: CGFloat = 64
    public static let statusHeight: CGFloat = 28
    public static let target: CGFloat = 24
    public static let commandTarget: CGFloat = 28

    public static func font(_ size: CGFloat, _ weight: NSFont.Weight = .regular) -> NSFont {
        .systemFont(ofSize: size, weight: weight)
    }
}

/// A plain view that fills itself with a theme color and redraws on appearance change.
public class FilledView: NSView {
    public var fill: NSColor { didSet { needsDisplay = true } }
    /// Draws a 1 pt `line` divider on these edges (y-down: top is minY).
    public var dividers: NSRectEdge.Set = []

    public init(fill: NSColor, dividers: NSRectEdge.Set = []) {
        self.fill = fill
        self.dividers = dividers
        super.init(frame: .zero)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("not used") }

    public override var isFlipped: Bool { true }

    public override func draw(_ dirtyRect: NSRect) {
        fill.setFill()
        bounds.fill()
        Theme.line.setFill()
        if dividers.contains(.top) { NSRect(x: 0, y: 0, width: bounds.width, height: 1).fill() }
        if dividers.contains(.bottom) { NSRect(x: 0, y: bounds.height - 1, width: bounds.width, height: 1).fill() }
    }
}

extension NSRectEdge {
    public struct Set: OptionSet, Sendable {
        public let rawValue: Int
        public init(rawValue: Int) { self.rawValue = rawValue }
        public static let top = Set(rawValue: 1)
        public static let bottom = Set(rawValue: 2)
    }
}
