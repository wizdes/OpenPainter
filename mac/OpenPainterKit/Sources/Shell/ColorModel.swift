// Color math and data for the Colors panel and the Color Picker tool (M36):
// HSV conversion, the color wheel's modifier rules, paint.net palette files and
// the default palette, and the Color Picker's options.
//
// Sources: the Colors window page
// (https://www.getpaint.net/doc/latest/ColorsWindow.html) for the wheel
// modifiers, the active slot and palette clicks; the palette page
// (https://www.getpaint.net/doc/latest/WorkingWithPalettes.html) for the file
// format; the Color Picker page
// (https://www.getpaint.net/doc/latest/ColorPicker.html) for the tool options.
import Foundation

// MARK: HSV

/// Hue 0..<360 degrees, saturation and value 0...100 (paint.net's HSV ranges).
public struct HSV: Equatable, Sendable {
    public var h: Double
    public var s: Double
    public var v: Double

    public init(h: Double, s: Double, v: Double) {
        self.h = h
        self.s = s
        self.v = v
    }

    public init(_ c: DocColor) {
        let r = Double(c.r) / 255, g = Double(c.g) / 255, b = Double(c.b) / 255
        let mx = max(r, g, b), mn = min(r, g, b), d = mx - mn
        var h = 0.0
        if d > 0 {
            if mx == r {
                h = 60 * ((g - b) / d).truncatingRemainder(dividingBy: 6)
            } else if mx == g {
                h = 60 * ((b - r) / d + 2)
            } else {
                h = 60 * ((r - g) / d + 4)
            }
        }
        if h < 0 { h += 360 }
        self.init(h: h, s: mx == 0 ? 0 : d / mx * 100, v: mx * 100)
    }

    /// The RGB bytes for this HSV, with the given alpha.
    public func color(alpha: UInt8 = 255) -> DocColor {
        let s = min(max(self.s, 0), 100) / 100, v = min(max(self.v, 0), 100) / 100
        var h = self.h.truncatingRemainder(dividingBy: 360)
        if h < 0 { h += 360 }
        let c = v * s
        let x = c * (1 - abs((h / 60).truncatingRemainder(dividingBy: 2) - 1))
        let m = v - c
        let (r, g, b): (Double, Double, Double) =
            switch Int(h / 60) {
            case 0: (c, x, 0)
            case 1: (x, c, 0)
            case 2: (0, c, x)
            case 3: (0, x, c)
            case 4: (x, 0, c)
            default: (c, 0, x)
            }
        func byte(_ u: Double) -> UInt8 { UInt8(min(max(((u + m) * 255).rounded(), 0), 255)) }
        return DocColor(r: byte(r), g: byte(g), b: byte(b), a: alpha)
    }
}

// MARK: Color wheel

/// The wheel's point math. Hue is the angle (0 degrees at the right, counter-
/// clockwise, red at 0), saturation the distance from the center (0 at the
/// center, 100 at the rim).
public enum ColorWheelMath {
    /// paint.net's wheel modifiers, mapped to Mac keys. paint.net's Ctrl is
    /// Command here, because Control-click is a right-click on a Mac; Alt is
    /// Option.
    public struct Modifiers: OptionSet, Sendable {
        public let rawValue: Int
        public init(rawValue: Int) { self.rawValue = rawValue }
        /// Command (paint.net Ctrl): keep the current radius; only the hue changes.
        public static let keepRadius = Modifiers(rawValue: 1)
        /// Option (paint.net Alt): keep the current spoke; only the saturation changes.
        public static let keepSpoke = Modifiers(rawValue: 2)
        /// Shift: snap the hue to the 15 degree spokes.
        public static let snapSpokes = Modifiers(rawValue: 4)
    }

    /// The spacing of the Shift spokes. The Colors page gives 15 degrees for
    /// Ctrl+Shift ("adding (or subtracting) 15 from the Hue value").
    public static let spokeStep = 15.0

    /// Hue and saturation for a point given relative to the wheel center in
    /// units of the radius (y up), under `mods`, starting from `current`.
    public static func pick(dx: Double, dy: Double, current: HSV, mods: Modifiers) -> (h: Double, s: Double) {
        var angle = atan2(dy, dx) * 180 / .pi
        if angle < 0 { angle += 360 }
        let dist = min(hypot(dx, dy), 1) * 100
        if mods.contains(.keepSpoke) {
            // Project the point onto the current spoke; the far side is 0.
            let a = current.h * .pi / 180
            let along = (dx * cos(a) + dy * sin(a)) * 100
            return (current.h, min(max(along, 0), 100))
        }
        var h = angle
        if mods.contains(.snapSpokes) {
            h = (angle / spokeStep).rounded() * spokeStep
            if h >= 360 { h -= 360 }
        }
        let s = mods.contains(.keepRadius) ? current.s : dist
        return (h, s)
    }

    /// The marker position for a color, relative to the center in radius units (y up).
    public static func point(for hsv: HSV) -> (x: Double, y: Double) {
        let a = hsv.h * .pi / 180, r = min(max(hsv.s, 0), 100) / 100
        return (cos(a) * r, sin(a) * r)
    }
}

// MARK: Palette

/// A 96-color palette. Loads and saves paint.net palette text files: one
/// `aarrggbb` hex color per line, `;` starts a comment, at most 96 colors, and
/// missing slots are white.
public struct Palette: Equatable, Sendable {
    public static let size = 96
    public static let compactCount = 32
    public static let padding = DocColor(r: 255, g: 255, b: 255)

    public var colors: [DocColor]

    public init(colors: [DocColor]) {
        var c = Array(colors.prefix(Self.size))
        while c.count < Self.size { c.append(Self.padding) }
        self.colors = c
    }

    /// A line the parser skipped, for the report.
    public struct Issue: Equatable, Sendable {
        public var line: Int
        public var text: String
        public var reason: String
    }

    public struct ParseResult: Equatable, Sendable {
        public var palette: Palette
        /// How many colors the file gave (before padding; at most 96).
        public var colorCount: Int
        public var issues: [Issue]

        /// One line for the status bar or the panel.
        public var summary: String {
            var parts = ["Loaded \(colorCount) of \(Palette.size) colors"]
            let bad = issues.filter { $0.reason != Palette.overflowReason }
            if !bad.isEmpty {
                let lines = bad.prefix(5).map { String($0.line) }.joined(separator: ", ")
                parts.append("skipped \(bad.count) malformed line\(bad.count == 1 ? "" : "s") (\(lines)\(bad.count > 5 ? ", ..." : ""))")
            }
            if let over = issues.first(where: { $0.reason == Palette.overflowReason }) {
                let extra = issues.filter { $0.reason == Palette.overflowReason }.count
                parts.append("ignored \(extra) color\(extra == 1 ? "" : "s") after slot 96 (from line \(over.line))")
            }
            return parts.joined(separator: "; ") + "."
        }
    }

    static let overflowReason = "more than 96 colors"

    /// Parses palette text. Comment and blank lines are skipped quietly;
    /// malformed lines and colors past slot 96 are skipped and reported.
    public static func parse(_ text: String) -> ParseResult {
        var colors: [DocColor] = []
        var issues: [Issue] = []
        let lines = text.split(omittingEmptySubsequences: false, whereSeparator: \.isNewline)
        for (i, raw) in lines.enumerated() {
            var s = Substring(raw)
            if let semi = s.firstIndex(of: ";") { s = s[..<semi] }
            let t = s.trimmingCharacters(in: .whitespaces)
            if t.isEmpty { continue }
            guard let c = argb(t) else {
                issues.append(Issue(line: i + 1, text: String(raw), reason: "not an 8-digit aarrggbb hex color"))
                continue
            }
            if colors.count >= size {
                issues.append(Issue(line: i + 1, text: String(raw), reason: overflowReason))
                continue
            }
            colors.append(c)
        }
        return ParseResult(palette: Palette(colors: colors), colorCount: colors.count, issues: issues)
    }

    /// Parses exactly eight hex digits as `aarrggbb`.
    public static func argb(_ s: String) -> DocColor? {
        guard s.count == 8, s.allSatisfy(\.isHexDigit), let v = UInt32(s, radix: 16) else { return nil }
        return DocColor(r: UInt8(v >> 16 & 0xFF), g: UInt8(v >> 8 & 0xFF), b: UInt8(v & 0xFF), a: UInt8(v >> 24))
    }

    public static func argbString(_ c: DocColor) -> String {
        String(format: "%02X%02X%02X%02X", c.a, c.r, c.g, c.b)
    }

    /// The file text: a short comment header and 96 `AARRGGBB` lines.
    public var fileText: String {
        var out = """
            ; OpenPainter palette (paint.net palette format)
            ; One color per line as 8 hex digits: aarrggbb. aa is alpha: FF is opaque, 00 is clear.
            ; Lines that start with a semicolon are comments. Up to 96 colors; missing slots are white.

            """
        for c in colors { out += Self.argbString(c) + "\n" }
        return out
    }

    /// OpenPainter's default palette: 96 saturated colors in 12 rows of 8. Its
    /// shape follows paint.net's default (greys first, then strong hues, then
    /// darker and lighter rows), but the colors are OpenPainter's own.
    public static let standard: Palette = {
        var c: [DocColor] = []
        // Row 1: black to white in 8 steps.
        for i in 0..<8 {
            let v = UInt8((Double(i) * 255 / 7).rounded())
            c.append(DocColor(r: v, g: v, b: v))
        }
        // Rows 2 to 4: 24 hues 15 degrees apart at full strength.
        for i in 0..<24 { c.append(HSV(h: Double(i) * 15, s: 100, v: 100).color()) }
        // Rows 5 to 7: the same hues, darker.
        for i in 0..<24 { c.append(HSV(h: Double(i) * 15, s: 100, v: 60).color()) }
        // Rows 8 to 10: the same hues, lighter.
        for i in 0..<24 { c.append(HSV(h: Double(i) * 15, s: 40, v: 100).color()) }
        // Row 11: 8 hues 45 degrees apart, very dark.
        for i in 0..<8 { c.append(HSV(h: Double(i) * 45, s: 100, v: 35).color()) }
        // Row 12: 8 hues 45 degrees apart, very light.
        for i in 0..<8 { c.append(HSV(h: Double(i) * 45, s: 15, v: 100).color()) }
        return Palette(colors: c)
    }()
}

// MARK: Color Picker options

public enum PickerSampleSize: Int, CaseIterable, Sendable {
    // The Color Picker page lists single pixel, 3x3, 5x5, 11x11, 31x31 and
    // 51x51; the M36 brief adds 7x7 and 9x9.
    case one = 1, three = 3, five = 5, seven = 7, nine = 9, eleven = 11, thirtyOne = 31, fiftyOne = 51

    public var label: String { self == .one ? "Single Pixel" : "\(rawValue) x \(rawValue) Region" }
}

public enum PickerAfterClick: String, CaseIterable, Sendable {
    case doNotSwitch, previousTool, pencil

    public var label: String {
        switch self {
        case .doNotSwitch: "Do not switch tool"
        case .previousTool: "Switch to previous tool"
        case .pencil: "Switch to Pencil tool"
        }
    }
}

/// Which color slot is active (has the notch). Palette and wheel clicks go to
/// the active slot; right-clicks go to the other one.
public enum ColorSlot: Sendable {
    case primary, secondary
    public var other: ColorSlot { self == .primary ? .secondary : .primary }
}
