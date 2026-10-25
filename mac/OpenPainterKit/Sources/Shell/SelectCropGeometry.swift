// Pure geometry for Rectangle Select and the Crop tool (M23): how a drag
// becomes a rectangle (any size, fixed ratio, fixed size, Shift for a square),
// which combine mode the modifier keys pick, the eight handles, the crop
// aspect presets and the straighten rotation. No AppKit state, so tests can
// check every rule directly.
//
// Rectangle Select follows paint.net
// (https://www.getpaint.net/doc/latest/RectangleSelectionTool.html): the press
// is one corner and the pointer the other; Shift keeps a square; Fixed Ratio
// locks the ratio; Fixed Size locks the size and the drag moves it; with Fixed
// Ratio or Fixed Size a drag off the canvas is clamped to the canvas edge and
// keeps the ratio or size.
//
// Modifier keys (https://www.getpaint.net/doc/latest/KeyboardMouseCommands.html,
// "Selection Tools"), with Ctrl as Command and Alt as Option, and the right
// button or Control-click as the Mac right-click:
//   Command-drag            Add (union)
//   Option-drag             Subtract
//   Command-right-drag      Invert (xor)
//   Option-right-drag       Intersect
// Without Command or Option, the mode in the options row is used.
import AppKit

public enum RectSelectStyle: String, CaseIterable, Sendable {
    case normal, fixedRatio, fixedSize
    public var label: String {
        switch self {
        case .normal: "Normal"
        case .fixedRatio: "Fixed Ratio"
        case .fixedSize: "Fixed Size"
        }
    }
}

/// One of the eight resize handles: corners and edge midpoints.
public enum RectHandle: CaseIterable, Sendable {
    case topLeft, top, topRight, right, bottomRight, bottom, bottomLeft, left

    /// Where the handle sits as a fraction of the rectangle (y down).
    var unit: CGPoint {
        switch self {
        case .topLeft: CGPoint(x: 0, y: 0)
        case .top: CGPoint(x: 0.5, y: 0)
        case .topRight: CGPoint(x: 1, y: 0)
        case .right: CGPoint(x: 1, y: 0.5)
        case .bottomRight: CGPoint(x: 1, y: 1)
        case .bottom: CGPoint(x: 0.5, y: 1)
        case .bottomLeft: CGPoint(x: 0, y: 1)
        case .left: CGPoint(x: 0, y: 0.5)
        }
    }

    public func point(in r: CGRect) -> CGPoint {
        CGPoint(x: r.minX + r.width * unit.x, y: r.minY + r.height * unit.y)
    }

    var movesX: Bool { unit.x != 0.5 }
    var movesY: Bool { unit.y != 0.5 }
}

public enum SelectGeometry {
    /// The combine mode for a drag: modifier keys first, else the options row.
    public static func mode(
        modifiers: NSEvent.ModifierFlags, right: Bool, toolbar: ToolSettings.SelectionMode
    ) -> ToolSettings.SelectionMode {
        let command = modifiers.contains(.command), option = modifiers.contains(.option)
        if command && right { return .invert }
        if option && right { return .intersect }
        if command { return .add }
        if option { return .subtract }
        return toolbar
    }

    /// True when a lasso's points enclose some area: three or more points and
    /// a nonzero shoelace area (a click or a straight scribble does not).
    public static func isArea(_ points: [CGPoint]) -> Bool {
        guard points.count >= 3 else { return false }
        var twice: CGFloat = 0
        for (i, a) in points.enumerated() {
            let b = points[(i + 1) % points.count]
            twice += a.x * b.y - b.x * a.y
        }
        return abs(twice) / 2 >= 0.5
    }

    /// `selection.lasso` points: "x y x y ..." in document pixels.
    public static func lassoText(_ points: [CGPoint]) -> String {
        points.map { "\(Double($0.x)) \(Double($0.y))" }.joined(separator: " ")
    }

    static func clampPoint(_ p: CGPoint, _ canvas: CGSize) -> CGPoint {
        CGPoint(x: min(max(p.x, 0), canvas.width), y: min(max(p.y, 0), canvas.height))
    }

    static func rounded(_ r: CGRect) -> CGRect {
        let x0 = r.minX.rounded(), y0 = r.minY.rounded()
        let x1 = r.maxX.rounded(), y1 = r.maxY.rounded()
        return CGRect(x: x0, y: y0, width: x1 - x0, height: y1 - y0)
    }

    /// The rectangle a drag from `start` to `current` makes, in whole document
    /// pixels. `ratio` is width:height for Fixed Ratio, `size` the pixel size for
    /// Fixed Size. `square` is Shift.
    public static func dragRect(
        from start: CGPoint, to current: CGPoint, style: RectSelectStyle, ratio: CGSize, size: CGSize,
        square: Bool, canvas: CGSize
    ) -> CGRect {
        switch style {
        case .fixedSize:
            let w = min(max(size.width.rounded(), 1), canvas.width)
            let h = min(max(size.height.rounded(), 1), canvas.height)
            // The drag moves the rectangle: its top-left corner follows the pointer.
            let x = min(max(current.x.rounded(), 0), canvas.width - w)
            let y = min(max(current.y.rounded(), 0), canvas.height - h)
            return CGRect(x: x, y: y, width: w, height: h)
        case .fixedRatio where ratio.width > 0 && ratio.height > 0:
            return ratioRect(from: start, to: current, ratio: ratio.width / ratio.height, canvas: canvas)
        default:
            if square {
                return ratioRect(from: start, to: current, ratio: 1, canvas: canvas)
            }
            let a = clampPoint(start, canvas), b = clampPoint(current, canvas)
            return rounded(CGRect(x: a.x, y: a.y, width: b.x - a.x, height: b.y - a.y).standardized)
        }
    }

    /// A rectangle with width / height = `ratio` from `start` toward
    /// `current`, as large as the pointer asks and clamped to the canvas
    /// without losing the ratio.
    public static func ratioRect(from start: CGPoint, to current: CGPoint, ratio: CGFloat, canvas: CGSize) -> CGRect {
        let a = clampPoint(start, canvas)
        let dx = current.x - a.x, dy = current.y - a.y
        let sx: CGFloat = dx < 0 ? -1 : 1, sy: CGFloat = dy < 0 ? -1 : 1
        // Room toward the drag direction.
        let roomX = sx > 0 ? canvas.width - a.x : a.x
        let roomY = sy > 0 ? canvas.height - a.y : a.y
        var h = max(abs(dx) / ratio, abs(dy))
        h = min(h, roomY, roomX / ratio)
        let w = h * ratio
        let r = CGRect(x: a.x, y: a.y, width: sx * w, height: sy * h).standardized
        // Round the corner at the press, then keep the ratio with the size.
        let x0 = r.minX.rounded(), y0 = r.minY.rounded()
        var rw = r.width.rounded(), rh = r.height.rounded()
        rw = min(rw, canvas.width - x0)
        rh = min(rh, canvas.height - y0)
        return CGRect(x: x0, y: y0, width: rw, height: rh)
    }

    /// The handle under `p` (view points), if any, for a rectangle shown at
    /// `viewRect`. Handles take a 12 pt square target.
    public static func handle(at p: CGPoint, viewRect: CGRect, target: CGFloat = 12) -> RectHandle? {
        RectHandle.allCases.first { h in
            let c = h.point(in: viewRect)
            return abs(p.x - c.x) <= target / 2 && abs(p.y - c.y) <= target / 2
        }
    }

    /// The rectangle after dragging `handle` to `p` (document pixels). The
    /// opposite edge or corner stays. `ratio` (width / height) keeps a fixed
    /// ratio when given.
    public static func resize(
        _ r: CGRect, handle: RectHandle, to p: CGPoint, ratio: CGFloat?, canvas: CGSize
    ) -> CGRect {
        let q = clampPoint(p, canvas)
        var x0 = r.minX, y0 = r.minY, x1 = r.maxX, y1 = r.maxY
        if handle.movesX { if handle.unit.x == 0 { x0 = q.x } else { x1 = q.x } }
        if handle.movesY { if handle.unit.y == 0 { y0 = q.y } else { y1 = q.y } }
        var out = CGRect(x: min(x0, x1), y: min(y0, y1), width: abs(x1 - x0), height: abs(y1 - y0))
        if let ratio, ratio > 0 {
            // Keep the ratio. The opposite corner or edge (the anchor) stays.
            let anchor = RectHandle.allCases.first { $0.unit == CGPoint(x: 1 - handle.unit.x, y: 1 - handle.unit.y) }!
                .point(in: r)
            var w = max(out.width, 1), h = max(out.height, 1)
            if handle.movesX && handle.movesY {
                if w / h > ratio { h = w / ratio } else { w = h * ratio }
            } else if handle.movesX {
                h = w / ratio
            } else {
                w = h * ratio
            }
            let dirX: CGFloat = q.x >= anchor.x ? 1 : -1
            let dirY: CGFloat = q.y >= anchor.y ? 1 : -1
            let roomX = handle.movesX
                ? (dirX > 0 ? canvas.width - anchor.x : anchor.x) : 2 * min(r.midX, canvas.width - r.midX)
            let roomY = handle.movesY
                ? (dirY > 0 ? canvas.height - anchor.y : anchor.y) : 2 * min(r.midY, canvas.height - r.midY)
            let s = min(1, roomX / w, roomY / h)
            w *= s
            h *= s
            let x = handle.movesX ? (dirX > 0 ? anchor.x : anchor.x - w) : r.midX - w / 2
            let y = handle.movesY ? (dirY > 0 ? anchor.y : anchor.y - h) : r.midY - h / 2
            out = CGRect(x: x, y: y, width: w, height: h)
        }
        return rounded(out)
    }
}

// MARK: Crop tool

/// The Crop tool's aspect presets. Portrait forms swap the numbers.
public enum CropAspect: String, CaseIterable, Sendable {
    case free, original, square, r4x3, r3x2, r16x9, r3x4, r2x3, r9x16

    public var label: String {
        switch self {
        case .free: "Free"
        case .original: "Original"
        case .square: "1:1"
        case .r4x3: "4:3"
        case .r3x2: "3:2"
        case .r16x9: "16:9"
        case .r3x4: "3:4"
        case .r2x3: "2:3"
        case .r9x16: "9:16"
        }
    }

    /// Width / height, or nil for Free.
    public func ratio(original: CGSize) -> CGFloat? {
        switch self {
        case .free: nil
        case .original: original.height > 0 ? original.width / original.height : nil
        case .square: 1
        case .r4x3: 4.0 / 3.0
        case .r3x2: 3.0 / 2.0
        case .r16x9: 16.0 / 9.0
        case .r3x4: 3.0 / 4.0
        case .r2x3: 2.0 / 3.0
        case .r9x16: 9.0 / 16.0
        }
    }
}

public enum CropGeometry {
    /// The largest rectangle of `ratio` (width / height) centered in `canvas`,
    /// in whole pixels: the long side fills the canvas, the other is rounded,
    /// and the leftover is split with the smaller half first.
    public static func largest(ratio: CGFloat?, canvas: CGSize) -> CGRect {
        let cw = canvas.width, ch = canvas.height
        guard let ratio, ratio > 0 else { return CGRect(origin: .zero, size: canvas) }
        var w = cw, h = ch
        if cw / ch > ratio {
            w = min(max((ch * ratio).rounded(), 1), cw)
        } else {
            h = min(max((cw / ratio).rounded(), 1), ch)
        }
        return CGRect(x: ((cw - w) / 2).rounded(.down), y: ((ch - h) / 2).rounded(.down), width: w, height: h)
    }

    /// The engine's Rotate / Zoom mapping (op-ops rotate_zoom.rs): a point of
    /// the rotated image (the crop frame's space) to the point of the original
    /// image it samples. The frame drawn over the original image uses it.
    public static func toOriginal(_ p: CGPoint, angle degrees: Double, canvas: CGSize) -> CGPoint {
        let a = degrees * .pi / 180
        let (s, c) = (sin(a), cos(a))
        let cx = canvas.width / 2, cy = canvas.height / 2
        let dx = p.x - cx, dy = p.y - cy
        return CGPoint(x: dx * c + dy * s + cx, y: -dx * s + dy * c + cy)
    }

    /// The inverse: a point of the original image to the rotated image.
    public static func toRotated(_ p: CGPoint, angle degrees: Double, canvas: CGSize) -> CGPoint {
        toOriginal(p, angle: -degrees, canvas: canvas)
    }

    /// The four corners of `frame` (rotated space) on the original image.
    public static func corners(_ frame: CGRect, angle: Double, canvas: CGSize) -> [CGPoint] {
        [
            CGPoint(x: frame.minX, y: frame.minY), CGPoint(x: frame.maxX, y: frame.minY),
            CGPoint(x: frame.maxX, y: frame.maxY), CGPoint(x: frame.minX, y: frame.maxY),
        ].map { toOriginal($0, angle: angle, canvas: canvas) }
    }

    /// `image.crop_rect` parameters for a frame and angle.
    public static func params(_ frame: CGRect, angle: Double) -> [String: any Sendable] {
        [
            "x": Int(frame.minX.rounded()), "y": Int(frame.minY.rounded()),
            "width": Int(frame.width.rounded()), "height": Int(frame.height.rounded()),
            "angle": angle,
        ]
    }
}
