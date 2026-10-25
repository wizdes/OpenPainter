// The Text tool's own options. Antialiasing and blend mode are shared with the
// other tools (`ToolSettings.antialias`, `ToolSettings.blend`), as in paint.net.
import AppKit

public struct TextToolSettings: Equatable, Sendable {
    public var family = TextStyleSpec.defaultFamily
    /// Points; one point is 96 / 72 pixels.
    public var size: Double = 12
    public var bold = false
    public var italic = false
    public var underline = false
    public var strikethrough = false
    public var alignment = TextStyleSpec.Alignment.left

    public init() {}

    /// The size menu's steps (paint.net's list).
    public static let sizes: [Double] = [8, 9, 10, 11, 12, 14, 16, 18, 20, 22, 24, 26, 28, 36, 48, 72, 84, 96, 108, 144,
                                         192, 216, 288]
}

extension ToolState {
    /// The layout style for the current options.
    public var textStyle: TextStyleSpec {
        let t = settings.text
        return TextStyleSpec(
            family: t.family, pointSize: t.size, bold: t.bold, italic: t.italic, underline: t.underline,
            strikethrough: t.strikethrough, alignment: t.alignment, antialias: settings.antialias)
    }
}
