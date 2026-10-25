// The document window's content: the Metal canvas, classic always-visible scroll
// bars, and the Zoom tool's drag rectangle. The canvas is not inside an
// NSScrollView; the scroll bars only mirror and drive the canvas viewport.
import AppKit

@MainActor
public final class CanvasContainerView: NSView {
    public let canvas: MetalCanvasView
    private let horizontal = NSScroller()
    private let vertical = NSScroller()
    private let overlay = ZoomRectangleView()
    /// Called after every viewport change (for a zoom label).
    public var onViewportChange: ((Viewport) -> Void)?
    /// The brush preview's width in document pixels and its color.
    public var strokeWidthPixels: Double = 2
    public var strokeColor: NSColor = .black

    public init(document: Document, documentSize: CGSize) {
        canvas = MetalCanvasView(document: document, documentSize: documentSize)
        super.init(frame: NSRect(x: 0, y: 0, width: 800, height: 600))
        for s in [horizontal, vertical] {
            s.scrollerStyle = .legacy
            s.controlSize = .regular
            s.target = self
            s.action = #selector(scrolled(_:))
            addSubview(s)
        }
        addSubview(canvas)
        addSubview(overlay)
        canvas.onViewportChange = { [weak self] v in self?.viewportChanged(v) }
        canvas.onZoomRectangle = { [weak self] r in self?.overlay.rectangle = r }
        canvas.onStrokePreview = { [weak self] points in
            guard let self else { return }
            self.overlay.strokeWidth = self.strokeWidthPixels * self.canvas.viewport.zoom
            self.overlay.strokeColor = self.strokeColor
            self.overlay.stroke = points
        }
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("not used") }

    public override var isFlipped: Bool { true }

    private var scrollerWidth: CGFloat {
        NSScroller.scrollerWidth(for: .regular, scrollerStyle: .legacy)
    }

    public override func layout() {
        super.layout()
        let w = scrollerWidth
        let canvasFrame = NSRect(x: 0, y: 0, width: max(bounds.width - w, 1), height: max(bounds.height - w, 1))
        canvas.frame = canvasFrame
        overlay.frame = canvasFrame
        vertical.frame = NSRect(x: canvasFrame.maxX, y: 0, width: w, height: canvasFrame.height)
        horizontal.frame = NSRect(x: 0, y: canvasFrame.maxY, width: canvasFrame.width, height: w)
    }

    private func viewportChanged(_ v: Viewport) {
        let p = v.knobProportion
        let f = v.scrollFraction
        horizontal.knobProportion = p.width
        horizontal.doubleValue = Double(f.x)
        horizontal.isEnabled = p.width < 1
        vertical.knobProportion = p.height
        vertical.doubleValue = Double(f.y)
        vertical.isEnabled = p.height < 1
        onViewportChange?(v)
    }

    @objc private func scrolled(_ sender: NSScroller) {
        var v = canvas.viewport
        let isH = sender === horizontal
        let page = isH ? v.viewSize.width * 0.9 : v.viewSize.height * 0.9
        let line: CGFloat = 32
        func step(_ d: CGFloat) {
            v.pan(by: isH ? CGSize(width: -d, height: 0) : CGSize(width: 0, height: -d))
        }
        switch sender.hitPart {
        case .knob, .knobSlot:
            if isH { v.setScrollFraction(x: sender.doubleValue) } else { v.setScrollFraction(y: sender.doubleValue) }
        case .incrementPage: step(page)
        case .decrementPage: step(-page)
        case .incrementLine: step(line)
        case .decrementLine: step(-line)
        default: return
        }
        canvas.setViewport(v)
    }
}

/// Draws the Zoom tool's rectangle; never takes mouse events.
final class ZoomRectangleView: NSView {
    var rectangle: CGRect? {
        didSet { needsDisplay = true }
    }
    /// The brush drag in progress, drawn as a line until the engine paints it.
    var stroke: [CGPoint]? {
        didSet { needsDisplay = true }
    }
    var strokeWidth: Double = 2
    var strokeColor: NSColor = .black

    override var isFlipped: Bool { true }
    override func hitTest(_ point: NSPoint) -> NSView? { nil }

    override func draw(_ dirtyRect: NSRect) {
        if let points = stroke, let first = points.first {
            let path = NSBezierPath()
            path.move(to: first)
            for p in points.dropFirst() { path.line(to: p) }
            if points.count == 1 { path.line(to: CGPoint(x: first.x + 0.01, y: first.y)) }
            path.lineWidth = max(1, strokeWidth)
            path.lineCapStyle = .round
            path.lineJoinStyle = .round
            strokeColor.setStroke()
            path.stroke()
        }
        guard let r = rectangle else { return }
        let path = NSBezierPath(rect: r.insetBy(dx: 0.5, dy: 0.5))
        path.lineWidth = 1
        NSColor.white.setStroke()
        path.stroke()
        path.setLineDash([4, 4], count: 2, phase: 0)
        NSColor.black.setStroke()
        path.stroke()
    }
}
