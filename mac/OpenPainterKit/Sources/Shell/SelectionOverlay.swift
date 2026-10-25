// The canvas overlay (M23): marching ants for the engine's selection outline
// and for a rectangle, ellipse or lasso (M29) being dragged, the Rectangle Select handles, and the Crop
// tool's frame (dimmed outside, thirds lines, handles). Paths are kept in
// document pixels and moved into view points when the viewport changes, so
// the outline is right at every zoom. The ants move by animating the dash
// phase of a Core Animation shape layer: the window server draws it, and the
// path is not rebuilt for each frame.
import AppKit
import QuartzCore

@MainActor
public final class SelectionOverlayView: NSView {
    /// Dash length in points (white under black, as paint.net draws them).
    static let dash: CGFloat = 4
    /// Seconds for the dash to move one full period.
    static let period: CFTimeInterval = 0.6

    let antsUnder = CAShapeLayer()
    let antsOver = CAShapeLayer()
    let pendingUnder = CAShapeLayer()
    let pendingOver = CAShapeLayer()
    let handles = CAShapeLayer()
    let cropDim = CAShapeLayer()
    let cropFrame = CAShapeLayer()
    let cropThirds = CAShapeLayer()

    /// The selection outline in document pixels (nil: nothing selected).
    public private(set) var selectionPath: CGPath?
    /// A rectangle being dragged (document pixels).
    public var pending: CGRect? { didSet { if pending != oldValue { redraw() } } }
    /// True: `pending` is drawn as the ellipse inside it (Ellipse Select, M29).
    public var pendingEllipse = false { didSet { if pendingEllipse != oldValue { redraw() } } }
    /// A lasso being dragged (document pixels), drawn closed back to its
    /// first point (Lasso Select, M29).
    public var pendingLasso: [CGPoint]? { didSet { if pendingLasso != oldValue { redraw() } } }
    /// The rectangle whose handles show (document pixels).
    public var handleRect: CGRect? { didSet { if handleRect != oldValue { redraw() } } }
    /// The crop frame's corners on the original image (document pixels), and
    /// its handle points; nil when the Crop tool is off.
    public var crop: (corners: [CGPoint], handles: [CGPoint])? { didSet { redraw() } }
    public private(set) var viewport: Viewport?

    public override init(frame: NSRect) {
        super.init(frame: frame)
        wantsLayer = true
        layer?.masksToBounds = true
        for l in [cropDim, cropThirds, cropFrame, antsUnder, antsOver, pendingUnder, pendingOver, handles] {
            l.fillColor = nil
            l.lineWidth = 1
            l.contentsScale = 2
            l.actions = ["path": NSNull(), "position": NSNull(), "bounds": NSNull(), "hidden": NSNull()]
            layer?.addSublayer(l)
        }
        for under in [antsUnder, pendingUnder] { under.strokeColor = NSColor.white.cgColor }
        for over in [antsOver, pendingOver] {
            over.strokeColor = NSColor.black.cgColor
            over.lineDashPattern = [NSNumber(value: Double(Self.dash)), NSNumber(value: Double(Self.dash))]
        }
        handles.fillColor = NSColor.white.cgColor
        handles.strokeColor = Theme.hex(0x1973C8).cgColor
        cropDim.fillColor = NSColor(white: 0, alpha: 0.45).cgColor
        cropDim.fillRule = .evenOdd
        cropFrame.strokeColor = NSColor.white.cgColor
        cropFrame.lineWidth = 1.5
        cropThirds.strokeColor = NSColor(white: 1, alpha: 0.55).cgColor
        startAnts()
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("not used") }

    public override var isFlipped: Bool { true }
    public override func hitTest(_ point: NSPoint) -> NSView? { nil }

    /// The animation that moves the black dashes; one per ants layer.
    func startAnts() {
        for l in [antsOver, pendingOver] {
            let a = CABasicAnimation(keyPath: "lineDashPhase")
            a.fromValue = 0
            a.toValue = Self.dash * 2
            a.duration = Self.period
            a.repeatCount = .infinity
            l.add(a, forKey: "ants")
        }
    }

    /// True while the dash phase is animating (tests).
    public var antsAnimating: Bool { antsOver.animation(forKey: "ants") != nil }

    public override func layout() {
        super.layout()
        for l in [cropDim, cropThirds, cropFrame, antsUnder, antsOver, pendingUnder, pendingOver, handles] {
            l.frame = bounds
        }
        redraw()
    }

    public func setViewport(_ v: Viewport) {
        guard v != viewport else { return }
        viewport = v
        redraw()
    }

    public func setSelectionPath(_ p: CGPath?) {
        selectionPath = p
        redraw()
    }

    var transform: CGAffineTransform {
        guard let v = viewport else { return .identity }
        return CGAffineTransform(a: v.zoom, b: 0, c: 0, d: v.zoom, tx: v.origin.x, ty: v.origin.y)
    }

    func view(_ p: CGPoint) -> CGPoint { p.applying(transform) }

    /// The selection outline in view points, as drawn (tests).
    public var shownSelectionPath: CGPath? { antsOver.path }

    func redraw() {
        // AppKit can drop layer animations when the view joins a window (seen
        // once the window lost its NSToolbar); start the dashes again.
        if !antsAnimating { startAnts() }
        var t = transform
        antsUnder.path = selectionPath?.copy(using: &t)
        antsOver.path = antsUnder.path
        if let r = pending, r.width > 0 || r.height > 0 {
            let p = pendingEllipse ? CGPath(ellipseIn: r.applying(t), transform: nil)
                : CGPath(rect: r.applying(t), transform: nil)
            pendingUnder.path = p
            pendingOver.path = p
        } else if let pts = pendingLasso, pts.count >= 2 {
            let p = CGMutablePath()
            p.addLines(between: pts.map { $0.applying(t) })
            p.closeSubpath()
            pendingUnder.path = p
            pendingOver.path = p
        } else {
            pendingUnder.path = nil
            pendingOver.path = nil
        }
        let h = CGMutablePath()
        let size: CGFloat = 7
        func handle(_ p: CGPoint) {
            h.addRect(CGRect(x: (p.x - size / 2).rounded() + 0.5, y: (p.y - size / 2).rounded() + 0.5,
                             width: size - 1, height: size - 1))
        }
        if let r = handleRect, pending == nil {
            let vr = r.applying(t)
            for k in RectHandle.allCases { handle(k.point(in: vr)) }
        }
        if let crop {
            let q = crop.corners.map(view)
            let quad = CGMutablePath()
            quad.addLines(between: q)
            quad.closeSubpath()
            let dim = CGMutablePath()
            dim.addRect(bounds)
            dim.addPath(quad)
            cropDim.path = dim
            cropFrame.path = quad
            let thirds = CGMutablePath()
            if q.count == 4 {
                func lerp(_ a: CGPoint, _ b: CGPoint, _ f: CGFloat) -> CGPoint {
                    CGPoint(x: a.x + (b.x - a.x) * f, y: a.y + (b.y - a.y) * f)
                }
                for f in [1.0 / 3, 2.0 / 3] {
                    thirds.move(to: lerp(q[0], q[1], f))
                    thirds.addLine(to: lerp(q[3], q[2], f))
                    thirds.move(to: lerp(q[0], q[3], f))
                    thirds.addLine(to: lerp(q[1], q[2], f))
                }
            }
            cropThirds.path = thirds
            for p in crop.handles { handle(view(p)) }
        } else {
            cropDim.path = nil
            cropFrame.path = nil
            cropThirds.path = nil
        }
        handles.path = h
    }

    /// Builds a document-pixel path from the engine's `doc.selection_path`
    /// answer: closed rings and straight edge runs.
    public static func path(fromEngine json: [String: Any]) -> CGPath? {
        guard json["active"] as? Bool == true else { return nil }
        let p = CGMutablePath()
        for ring in json["paths"] as? [[NSNumber]] ?? [] where ring.count >= 4 {
            let pts = stride(from: 0, to: ring.count - 1, by: 2).map {
                CGPoint(x: ring[$0].doubleValue, y: ring[$0 + 1].doubleValue)
            }
            p.addLines(between: pts)
            p.closeSubpath()
        }
        let s = json["segments"] as? [NSNumber] ?? []
        var i = 0
        while i + 3 < s.count {
            p.move(to: CGPoint(x: s[i].doubleValue, y: s[i + 1].doubleValue))
            p.addLine(to: CGPoint(x: s[i + 2].doubleValue, y: s[i + 3].doubleValue))
            i += 4
        }
        return p
    }
}
