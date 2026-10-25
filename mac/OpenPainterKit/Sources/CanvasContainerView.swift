// The document window's content: the Metal canvas, classic always-visible scroll
// bars, and the Zoom tool's drag rectangle. Brush strokes need no overlay: the
// engine draws its live preview into the canvas (M38). The canvas is not inside an
// NSScrollView; the scroll bars only mirror and drive the canvas viewport.
import AppKit

@MainActor
public final class CanvasContainerView: NSView {
    public let canvas: MetalCanvasView
    private let horizontal = NSScroller()
    private let vertical = NSScroller()
    private let overlay = ZoomRectangleView()
    /// View > Rulers (M56): along the top and left edges of the canvas.
    public let horizontalRuler = RulerView(axis: .horizontal)
    public let verticalRuler = RulerView(axis: .vertical)
    private let rulerCorner = RulerCornerView()
    public var rulersVisible = false {
        didSet {
            guard rulersVisible != oldValue else { return }
            needsLayout = true
            layoutSubtreeIfNeeded()
        }
    }
    /// The rulers' units and the document's pixels per inch.
    public var rulerUnit: MeasurementUnit = .pixels {
        didSet { updateRulers() }
    }
    public var rulerDPI: Double = 96 {
        didSet { updateRulers() }
    }
    /// The selection bounds (document pixels) the rulers highlight.
    public var rulerSelection: CGRect? {
        didSet { if rulerSelection != oldValue { updateRulers() } }
    }
    /// The pointer (document pixels) the rulers mark.
    public var rulerCursor: CGPoint? {
        didSet { if rulerCursor != oldValue { updateRulers() } }
    }
    /// Called after every viewport change (for a zoom label).
    public var onViewportChange: ((Viewport) -> Void)?

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
        for r in [horizontalRuler, verticalRuler, rulerCorner] as [NSView] {
            r.isHidden = true
            addSubview(r)
        }
        canvas.onViewportChange = { [weak self] v in self?.viewportChanged(v) }
        canvas.onZoomRectangle = { [weak self] r in self?.overlay.rectangle = r }
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
        let r: CGFloat = rulersVisible ? RulerView.thickness : 0
        let canvasFrame = NSRect(
            x: r, y: r, width: max(bounds.width - w - r, 1), height: max(bounds.height - w - r, 1))
        canvas.frame = canvasFrame
        overlay.frame = canvasFrame
        vertical.frame = NSRect(x: canvasFrame.maxX, y: canvasFrame.minY, width: w, height: canvasFrame.height)
        horizontal.frame = NSRect(x: canvasFrame.minX, y: canvasFrame.maxY, width: canvasFrame.width, height: w)
        for v in [horizontalRuler, verticalRuler, rulerCorner] as [NSView] { v.isHidden = !rulersVisible }
        horizontalRuler.frame = NSRect(x: r, y: 0, width: canvasFrame.width, height: r)
        verticalRuler.frame = NSRect(x: 0, y: r, width: r, height: canvasFrame.height)
        rulerCorner.frame = NSRect(x: 0, y: 0, width: r, height: r)
        updateRulers()
    }

    /// Gives the rulers the current zoom, scroll, units, selection and pointer.
    func updateRulers() {
        let v = canvas.viewport
        let scale = RulerScale(unit: rulerUnit, dpi: rulerDPI, zoom: v.zoom)
        rulerCorner.unit = rulerUnit
        horizontalRuler.scale = scale
        verticalRuler.scale = scale
        horizontalRuler.origin = v.origin.x
        verticalRuler.origin = v.origin.y
        if let s = rulerSelection, s.width > 0, s.height > 0 {
            let a = v.viewPoint(ofDocument: s.origin), b = v.viewPoint(ofDocument: CGPoint(x: s.maxX, y: s.maxY))
            horizontalRuler.highlight = a.x...b.x
            verticalRuler.highlight = a.y...b.y
        } else {
            horizontalRuler.highlight = nil
            verticalRuler.highlight = nil
        }
        let c = rulerCursor.map { v.viewPoint(ofDocument: $0) }
        horizontalRuler.marker = c?.x
        verticalRuler.marker = c?.y
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
        updateRulers()
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

    override var isFlipped: Bool { true }
    override func hitTest(_ point: NSPoint) -> NSView? { nil }

    override func draw(_ dirtyRect: NSRect) {
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
