// The tool list, the tool options and the shared current-tool state.
// Order and shortcut letters follow paint.net 5.1's Tools window
// (https://www.getpaint.net/doc/latest/ToolsWindow.html): two columns, read
// left to right, top to bottom.
import AppKit

public enum PaintTool: String, CaseIterable, Sendable {
    case rectangleSelect, moveSelectedPixels
    case lassoSelect, moveSelection
    case ellipseSelect, zoom
    case magicWand, pan
    case paintBucket, gradient
    case paintbrush, eraser
    case pencil, colorPicker
    case cloneStamp, recolor
    case text, lineCurve
    case shapes

    public var label: String {
        switch self {
        case .rectangleSelect: "Rectangle Select"
        case .moveSelectedPixels: "Move Selected Pixels"
        case .lassoSelect: "Lasso Select"
        case .moveSelection: "Move Selection"
        case .ellipseSelect: "Ellipse Select"
        case .zoom: "Zoom"
        case .magicWand: "Magic Wand"
        case .pan: "Pan"
        case .paintBucket: "Paint Bucket"
        case .gradient: "Gradient"
        case .paintbrush: "Paintbrush"
        case .eraser: "Eraser"
        case .pencil: "Pencil"
        case .colorPicker: "Color Picker"
        case .cloneStamp: "Clone Stamp"
        case .recolor: "Recolor"
        case .text: "Text"
        case .lineCurve: "Line / Curve"
        case .shapes: "Shapes"
        }
    }

    /// paint.net's single-key shortcut. Pressing it again cycles tools that share it.
    public var key: Character {
        switch self {
        case .rectangleSelect, .lassoSelect, .ellipseSelect, .magicWand: "s"
        case .moveSelectedPixels, .moveSelection: "m"
        case .zoom: "z"
        case .pan: "h"
        case .paintBucket: "f"
        case .gradient: "g"
        case .paintbrush: "b"
        case .eraser: "e"
        case .pencil: "p"
        case .colorPicker: "k"
        case .cloneStamp: "l"
        case .recolor: "r"
        case .text: "t"
        case .lineCurve, .shapes: "o"
        }
    }

    /// SF Symbol drawn in the Tools panel and the options-bar picker.
    public var symbol: String {
        switch self {
        case .rectangleSelect: "rectangle.dashed"
        case .moveSelectedPixels: "arrow.up.and.down.and.arrow.left.and.right"
        case .lassoSelect: "lasso"
        case .moveSelection: "arrow.up.left.and.down.right.and.arrow.up.right.and.down.left"
        case .ellipseSelect: "circle.dashed"
        case .zoom: "magnifyingglass"
        case .magicWand: "wand.and.stars"
        case .pan: "hand.raised"
        case .paintBucket: "drop"
        case .gradient: "square.bottomhalf.filled"
        case .paintbrush: "paintbrush.pointed"
        case .eraser: "eraser"
        case .pencil: "pencil"
        case .colorPicker: "eyedropper"
        case .cloneStamp: "square.on.square.dashed"
        case .recolor: "paintbrush"
        case .text: "textformat"
        case .lineCurve: "line.diagonal"
        case .shapes: "square.on.circle"
        }
    }

    /// Tools that work in this version. The rest are selectable and say "coming soon".
    public var isFunctional: Bool {
        switch self {
        case .zoom, .pan, .paintbrush, .eraser, .pencil, .magicWand, .paintBucket, .colorPicker, .rectangleSelect: true
        default: false
        }
    }

    /// The engine's `tool.stroke` tool name, for brush tools.
    public var strokeName: String? {
        switch self {
        case .paintbrush: "paintbrush"
        case .eraser: "eraser"
        case .pencil: "pencil"
        default: nil
        }
    }

    public enum OptionSet: Sendable { case none, brush, eraser, pencil, flood, picker }

    public var options: OptionSet {
        switch self {
        case .paintbrush: .brush
        case .eraser: .eraser
        case .pencil: .pencil
        case .magicWand, .paintBucket: .flood
        case .colorPicker: .picker
        default: .none
        }
    }

    /// The short hint at the left of the status bar.
    public var hint: String {
        guard isFunctional else { return "\(label) is coming soon" }
        switch self {
        case .zoom: return "Zoom: click to zoom in, Option-click to zoom out, drag a rectangle"
        case .pan: return "Pan: drag to move the view"
        case .magicWand: return "Magic Wand: click to select a region of similar color"
        case .rectangleSelect:
            return "Rectangle Select: drag a rectangle; Command adds, Option subtracts, Shift makes a square"
        case .paintBucket: return "Paint Bucket: click to fill with the primary color, right-click for the secondary"
        case .colorPicker: return "Color Picker: click sets the primary color, right-click the secondary; Command-click samples the image"
        default: return "\(label): drag to paint"
        }
    }

    /// The next tool for a key press, cycling among tools that share the key.
    public static func tool(forKey key: Character, current: PaintTool) -> PaintTool? {
        let matches = allCases.filter { $0.key == key }
        guard let first = matches.first else { return nil }
        if let i = matches.firstIndex(of: current) { return matches[(i + 1) % matches.count] }
        return first
    }
}

/// Options for every tool. Values go to the engine with each stroke or fill.
public struct ToolSettings: Equatable, Sendable {
    public enum Blend: String, CaseIterable, Sendable {
        case normal, overwrite
        public var label: String { self == .normal ? "Normal" : "Overwrite" }
    }
    public enum FloodMode: String, CaseIterable, Sendable {
        case contiguous, global
        public var label: String { self == .contiguous ? "Contiguous" : "Global" }
    }
    public enum Sampling: String, CaseIterable, Sendable {
        case layer, image
        public var label: String { self == .layer ? "Layer" : "Image" }
    }
    public enum SelectionMode: String, CaseIterable, Sendable {
        case replace, add, subtract, intersect, invert
        public var label: String { rawValue.capitalized }
    }

    /// Brush width in pixels, 1...2000 (paint.net's range).
    public var brushWidth: Double = 2
    public var antialias = true
    public var blend = Blend.normal
    /// Opacity percent, 0...100.
    public var opacity: Double = 100
    /// Brush hardness percent, 0...100 (Paintbrush and Eraser; 100 is a hard edge).
    public var hardness: Double = 100
    /// Pen pressure changes the brush width (Paintbrush and Eraser).
    public var pressureSize = false
    /// Pen pressure changes the opacity (Paintbrush, Eraser and Pencil).
    public var pressureOpacity = false
    /// Tolerance percent, 0...100 (paint.net default 50%).
    public var tolerance: Double = 50
    public var floodMode = FloodMode.contiguous
    public var sampling = Sampling.layer
    public var selectionMode = SelectionMode.replace
    /// Color Picker options (M36).
    public var pickerSize = PickerSampleSize.one
    public var pickerSampling = Sampling.layer
    public var pickerAfterClick = PickerAfterClick.doNotSwitch

    public init() {}

    public static let widthRange: ClosedRange<Double> = 1...2000

    /// paint.net's brush-width steps for the - and + buttons.
    public static let widthSteps: [Double] = [
        1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 20, 25, 30, 35, 40, 45, 50, 55, 60, 65, 70, 75, 80,
        85, 90, 95, 100, 125, 150, 175, 200, 225, 250, 275, 300, 325, 350, 375, 400, 425, 450, 475, 500, 1000,
        1500, 2000,
    ]

    public mutating func stepWidth(up: Bool) {
        if up {
            brushWidth = Self.widthSteps.first { $0 > brushWidth } ?? Self.widthRange.upperBound
        } else {
            brushWidth = Self.widthSteps.last { $0 < brushWidth } ?? Self.widthRange.lowerBound
        }
    }
}

/// RGBA bytes of a document color. The bytes never change with the appearance.
public struct DocColor: Equatable, Sendable {
    public var r: UInt8, g: UInt8, b: UInt8, a: UInt8

    public init(r: UInt8, g: UInt8, b: UInt8, a: UInt8 = 255) {
        self.r = r
        self.g = g
        self.b = b
        self.a = a
    }

    /// Parses "#RRGGBB", "RRGGBB" or "RRGGBBAA".
    public init?(hex: String) {
        var s = hex.trimmingCharacters(in: .whitespaces)
        if s.hasPrefix("#") { s.removeFirst() }
        guard s.count == 6 || s.count == 8, s.allSatisfy(\.isHexDigit), let v = UInt32(s, radix: 16) else { return nil }
        if s.count == 6 {
            self.init(r: UInt8(v >> 16 & 0xFF), g: UInt8(v >> 8 & 0xFF), b: UInt8(v & 0xFF))
        } else {
            self.init(r: UInt8(v >> 24), g: UInt8(v >> 16 & 0xFF), b: UInt8(v >> 8 & 0xFF), a: UInt8(v & 0xFF))
        }
    }

    /// "#CF684D" (alpha is shown only when it is not 255).
    public var hex: String {
        let rgb = String(format: "#%02X%02X%02X", r, g, b)
        return a == 255 ? rgb : rgb + String(format: "%02X", a)
    }

    /// The engine's eight-digit RGBA form.
    public var engineHex: String { String(format: "%02x%02x%02x%02x", r, g, b, a) }

    /// An sRGB color with exactly these bytes, for drawing a swatch.
    public var nsColor: NSColor {
        NSColor(
            srgbRed: CGFloat(r) / 255, green: CGFloat(g) / 255, blue: CGFloat(b) / 255, alpha: CGFloat(a) / 255)
    }
}

/// The shared current tool, its options, and the primary and secondary colors.
/// Views observe it with `observe`; every change calls every observer.
@MainActor
public final class ToolState {
    public static let shared = ToolState()

    public var tool: PaintTool = .paintbrush {
        didSet {
            guard tool != oldValue else { return }
            previousTool = oldValue
            changed()
        }
    }
    /// The tool before the current one (Color Picker's "Switch to previous tool").
    public private(set) var previousTool: PaintTool?
    public var settings = ToolSettings() { didSet { if settings != oldValue { changed() } } }
    public var primary = DocColor(r: 0xCF, g: 0x68, b: 0x4D) { didSet { if primary != oldValue { changed() } } }
    public var secondary = DocColor(r: 0xE9, g: 0xE7, b: 0xDC) { didSet { if secondary != oldValue { changed() } } }
    /// The slot with the notch: Colors panel edits and left-clicks go here.
    public var activeSlot = ColorSlot.primary { didSet { if activeSlot != oldValue { changed() } } }
    public var palette = Palette.standard { didSet { if palette != oldValue { changed() } } }

    private var observers: [(AnyObject?, () -> Void)] = []

    public init() {}

    /// Calls `body` after every change while `owner` is alive.
    public func observe(_ owner: AnyObject, _ body: @escaping () -> Void) {
        observers.append((owner, body))
    }

    public func swapColors() {
        let p = primary
        primary = secondary
        secondary = p
    }

    public func color(_ slot: ColorSlot) -> DocColor { slot == .primary ? primary : secondary }

    public func set(_ slot: ColorSlot, _ c: DocColor) {
        if slot == .primary { primary = c } else { secondary = c }
    }

    /// The black and white button: primary black, secondary white.
    public func resetColors() {
        primary = DocColor(r: 0, g: 0, b: 0)
        secondary = DocColor(r: 255, g: 255, b: 255)
    }

    /// The C key: move the notch to the other slot.
    public func toggleActiveSlot() { activeSlot = activeSlot.other }

    private func changed() {
        observers.removeAll { $0.0 == nil }
        for (_, body) in observers { body() }
    }

    /// Parameters for `tool.stroke` (without the points).
    /// `secondary` is a right-button (or Control) drag, which paints with the
    /// secondary color as in paint.net.
    public func strokeParams(tool: PaintTool, secondary useSecondary: Bool = false) -> [String: any Sendable] {
        let color = tool == .eraser ? DocColor(r: 0, g: 0, b: 0) : (useSecondary ? secondary : primary)
        return [
            "tool": tool.strokeName ?? "paintbrush",
            "width": settings.brushWidth,
            "color": color.engineHex,
            "opacity": (settings.opacity / 100 * 255).rounded(),
            "antialias": settings.antialias,
            "blend": settings.blend.rawValue,
        ]
    }

    /// Options for a live stroke (`op_stroke_begin`): the `tool.stroke` options
    /// plus hardness and the pen pressure toggles. The Pencil has no width,
    /// hardness or antialiasing in paint.net: it always draws 1 px, hard,
    /// aliased. The Eraser has no blend mode.
    public func liveStrokeParams(tool: PaintTool, secondary useSecondary: Bool = false) -> [String: any Sendable] {
        var p = strokeParams(tool: tool, secondary: useSecondary)
        p["hardness"] = tool == .pencil ? 1.0 : settings.hardness / 100
        p["pressure_size"] = tool != .pencil && settings.pressureSize
        p["pressure_opacity"] = settings.pressureOpacity
        if tool == .pencil {
            p["width"] = 1.0
            p["antialias"] = false
        }
        if tool == .eraser { p["blend"] = ToolSettings.Blend.normal.rawValue }
        return p
    }

    /// Parameters for `tool.magic_wand` or `tool.paint_bucket` at a document pixel.
    public func floodParams(tool: PaintTool, x: Int, y: Int, secondary useSecondary: Bool = false) -> [String: any Sendable] {
        var p: [String: any Sendable] = [
            "x": x, "y": y,
            "tolerance": settings.tolerance / 100,
            "contiguous": settings.floodMode == .contiguous,
            "sampling": settings.sampling.rawValue,
            "antialias": settings.antialias,
        ]
        if tool == .paintBucket {
            var c = useSecondary ? secondary : primary
            c.a = UInt8((Double(c.a) * settings.opacity / 100).rounded())
            p["color"] = c.engineHex
            p["blend"] = settings.blend.rawValue
        } else {
            p["combine"] = settings.selectionMode.rawValue
        }
        return p
    }
}
