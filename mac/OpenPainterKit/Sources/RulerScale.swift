// Units of measurement and ruler tick math (M56). Pure math, no AppKit state.
//
// paint.net's View menu (https://www.getpaint.net/doc/latest/ViewMenu.html):
// the units are Pixels, Inches or Centimeters; inches and centimeters come from
// the image's resolution (DPI). The rulers along the top and left edges of the
// canvas always use the current units.
import Foundation

/// View > Pixels, Inches, Centimeters.
public enum MeasurementUnit: String, CaseIterable, Sendable {
    case pixels, inches, centimeters

    /// The menu title.
    public var title: String {
        switch self {
        case .pixels: "Pixels"
        case .inches: "Inches"
        case .centimeters: "Centimeters"
        }
    }

    /// The short name (status bar, ruler corner).
    public var abbreviation: String {
        switch self {
        case .pixels: "px"
        case .inches: "in"
        case .centimeters: "cm"
        }
    }

    /// Document pixels in one unit at `dpi` pixels per inch (1 for pixels).
    public func pixelsPerUnit(dpi: Double) -> Double {
        switch self {
        case .pixels: 1
        case .inches: dpi
        case .centimeters: dpi / 2.54
        }
    }

    /// A pixel length or coordinate in this unit.
    public func value(ofPixels pixels: Double, dpi: Double) -> Double {
        pixels / pixelsPerUnit(dpi: dpi)
    }

    /// A length or coordinate in this unit, in pixels.
    public func pixels(ofValue value: Double, dpi: Double) -> Double {
        value * pixelsPerUnit(dpi: dpi)
    }

    /// The text for a value: whole pixels, 2 decimals for inches and centimeters.
    public func format(_ value: Double) -> String {
        if self == .pixels { return "\(Int(value.rounded(.down)))" }
        return String(format: "%.2f", value)
    }
}

/// One ruler tick. `position` is in view points along the ruler, `value` in units.
public struct RulerTick: Equatable, Sendable {
    public enum Level: Int, Sendable { case major = 0, middle = 1, minor = 2 }
    public let position: Double
    public let value: Double
    public let level: Level
}

/// The tick spacing for one zoom, resolution and unit. A major tick (with a
/// label) lands on a 1, 2 or 5 times a power of ten unit value, the smallest
/// such step at least `minMajorSpacing` points apart. Minor ticks split it into
/// the most parts that stay `minMinorSpacing` points apart (10, 5 or 2 parts for
/// a step of 1; 4 or 2 for 2; 5 for 5). Pixel steps are whole pixels.
public struct RulerScale: Equatable, Sendable {
    public let unit: MeasurementUnit
    public let dpi: Double
    /// Points per document pixel.
    public let zoom: Double
    /// Points per unit.
    public let pointsPerUnit: Double
    /// Units between labelled ticks.
    public let majorStep: Double
    /// Minor parts per major step (1 when no minor ticks fit).
    public let subdivisions: Int

    public static let defaultMajorSpacing = 48.0
    public static let defaultMinorSpacing = 4.0

    public init(unit: MeasurementUnit, dpi: Double, zoom: Double,
                minMajorSpacing: Double = defaultMajorSpacing, minMinorSpacing: Double = defaultMinorSpacing) {
        let dpi = dpi.isFinite && dpi > 0 ? dpi : 96
        self.unit = unit
        self.dpi = dpi
        self.zoom = zoom
        let ppu = zoom * unit.pixelsPerUnit(dpi: dpi)
        pointsPerUnit = ppu
        var step = 1.0
        var mantissa = 1
        search: for exponent in -4...9 {
            for m in [1, 2, 5] {
                let s = Double(m) * pow(10, Double(exponent))
                if unit == .pixels, s < 1 { continue }
                if s * ppu >= minMajorSpacing - 1e-9 {
                    step = s
                    mantissa = m
                    break search
                }
            }
        }
        majorStep = step
        let parts: [Int] = switch mantissa {
        case 1: [10, 5, 2]
        case 2: [4, 2]
        default: [5]
        }
        subdivisions = parts.first { n in
            let minor = step / Double(n)
            if unit == .pixels, minor < 1 - 1e-9 || abs(minor - minor.rounded()) > 1e-9 { return false }
            return minor * ppu >= minMinorSpacing - 1e-9
        } ?? 1
    }

    public var minorStep: Double { majorStep / Double(subdivisions) }

    /// Decimals a major label needs (0 for steps of 1 and up).
    public var labelDecimals: Int { max(0, Int(-floor(log10(majorStep) + 1e-9))) }

    public func label(_ value: Double) -> String {
        let v = abs(value) < minorStep / 1000 ? 0 : value
        return String(format: "%.\(labelDecimals)f", v)
    }

    /// The view position of a unit value when document pixel 0 is at `origin`.
    public func position(ofValue value: Double, origin: Double) -> Double {
        origin + value * pointsPerUnit
    }

    /// Every tick from view position 0 to `length` when document pixel 0 is at
    /// view position `origin`. Values left of the document are negative.
    public func ticks(origin: Double, length: Double) -> [RulerTick] {
        guard pointsPerUnit > 0, length > 0 else { return [] }
        let minor = minorStep
        let first = Int(floor((0 - origin) / pointsPerUnit / minor - 1e-9))
        let last = Int(ceil((length - origin) / pointsPerUnit / minor + 1e-9))
        guard last >= first, last - first < 100_000 else { return [] }
        var out: [RulerTick] = []
        out.reserveCapacity(last - first + 1)
        let half = subdivisions % 2 == 0 ? subdivisions / 2 : 0
        for i in first...last {
            let value = Double(i) * minor
            let p = position(ofValue: value, origin: origin)
            guard p >= -1e-6, p <= length + 1e-6 else { continue }
            let r = ((i % subdivisions) + subdivisions) % subdivisions
            let level: RulerTick.Level = r == 0 ? .major : (half > 0 && r % half == 0 ? .middle : .minor)
            out.append(RulerTick(position: p, value: value, level: level))
        }
        return out
    }
}
