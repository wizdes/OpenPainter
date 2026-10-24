// Zoom steps ported from Paint.NET 3.36.7, src/Core/ScaleFactor.cs (PresetValues,
// GetNextLarger, GetNextSmaller, FromDouble) in the OpenPDN mirror at commit
// cca476b0df2a2f70996e6b9486ec45327631568c.
// MIT License, Copyright (C) dotPDN LLC, Rick Brewster, Tom Jackson, and contributors.
// See THIRD_PARTY.md.
import Foundation

public enum ZoomPresets {
    /// paint.net's preset ratios, 1% to 3200%, ascending. 1.0 is always present.
    public static let values: [Double] = [
        0.01, 0.02, 0.03, 0.04, 0.05, 0.06, 0.08, 0.12, 0.16, 0.25, 0.33, 0.50, 0.66, 1,
        2, 3, 4, 5, 6, 7, 8, 12, 16, 24, 32,
    ]
    /// ScaleFactor.MinValue (1/100) and MaxValue (32/1).
    public static let minimum = 0.01
    public static let maximum = 32.0

    public static func clamp(_ zoom: Double) -> Double {
        min(max(zoom, minimum), maximum)
    }

    /// ScaleFactor.FromDouble: floor to thousandths (paint.net stores n/1000).
    static func fromDouble(_ value: Double) -> Double {
        clamp(Double(Int((value * 1000).rounded(.down))) / 1000)
    }

    /// GetNextLarger: the first preset at or above `zoom + 0.005`, or the largest.
    public static func nextLarger(_ zoom: Double) -> Double {
        let ratio = zoom + 0.005
        let index = values.firstIndex { ratio <= $0 } ?? values.count - 1
        return fromDouble(values[index])
    }

    /// GetNextSmaller: the preset before the first one at or above `zoom - 0.005`,
    /// or the smallest.
    public static func nextSmaller(_ zoom: Double) -> Double {
        let ratio = zoom - 0.005
        let found = values.firstIndex { ratio <= $0 } ?? -1
        return fromDouble(values[max(found - 1, 0)])
    }

    /// DocumentView.ZoomToWindow: the largest scale that fits the document inside
    /// the view less a 10-point margin on each axis, never above 100%.
    public static func fit(document: CGSize, view: CGSize) -> Double {
        guard document.width > 0, document.height > 0 else { return 1 }
        let sx = Double(max(view.width - 10, 1)) / Double(document.width)
        let sy = Double(max(view.height - 10, 1)) / Double(document.height)
        return clamp(min(sx, sy, 1))
    }

    /// The percent text paint.net shows ("33%", "100%", "1600%"); 1 decimal below 10%.
    public static func label(_ zoom: Double) -> String {
        let percent = zoom * 100
        if percent < 10, percent.rounded() != percent {
            return String(format: "%.1f%%", percent)
        }
        return "\(Int(percent.rounded()))%"
    }
}
