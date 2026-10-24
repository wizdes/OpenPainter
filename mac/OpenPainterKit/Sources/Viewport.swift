// Where the document sits in the canvas view. Pure math, no AppKit state.
import Foundation

/// View space is in points with the origin at the view's top-left corner and y
/// growing down. Document space is in document pixels.
public struct Viewport: Equatable, Sendable {
    public var documentSize: CGSize
    public var viewSize: CGSize
    /// Points per document pixel, within ZoomPresets.minimum...maximum.
    public private(set) var zoom: Double
    /// View position of the document's top-left corner.
    public private(set) var origin: CGPoint

    public init(documentSize: CGSize, viewSize: CGSize, zoom: Double = 1) {
        self.documentSize = documentSize
        self.viewSize = viewSize
        self.zoom = ZoomPresets.clamp(zoom)
        self.origin = .zero
        clampOrigin()
    }

    public var contentSize: CGSize {
        CGSize(width: documentSize.width * zoom, height: documentSize.height * zoom)
    }

    public func documentPoint(atView p: CGPoint) -> CGPoint {
        CGPoint(x: (p.x - origin.x) / zoom, y: (p.y - origin.y) / zoom)
    }

    public func viewPoint(ofDocument d: CGPoint) -> CGPoint {
        CGPoint(x: d.x * zoom + origin.x, y: d.y * zoom + origin.y)
    }

    public var viewCenter: CGPoint { CGPoint(x: viewSize.width / 2, y: viewSize.height / 2) }

    /// On each axis: a document smaller than the view is centered; a larger one
    /// cannot be scrolled past its edges.
    public mutating func clampOrigin() {
        func axis(_ o: CGFloat, _ content: CGFloat, _ view: CGFloat) -> CGFloat {
            if content <= view { return ((view - content) / 2).rounded() }
            return min(0, max(view - content, o))
        }
        origin = CGPoint(
            x: axis(origin.x, contentSize.width, viewSize.width),
            y: axis(origin.y, contentSize.height, viewSize.height))
    }

    public mutating func resize(to size: CGSize) {
        let keep = documentPoint(atView: viewCenter)
        viewSize = size
        center(on: keep)
    }

    /// Sets the zoom and keeps the document point under `p` there, as far as the
    /// scroll limits allow.
    public mutating func setZoom(_ newZoom: Double, keeping p: CGPoint) {
        let d = documentPoint(atView: p)
        zoom = ZoomPresets.clamp(newZoom)
        origin = CGPoint(x: p.x - d.x * zoom, y: p.y - d.y * zoom)
        clampOrigin()
    }

    /// Sets the zoom around the view's center.
    public mutating func setZoom(_ newZoom: Double) {
        setZoom(newZoom, keeping: viewCenter)
    }

    /// Scrolls so document point `d` is at the view's center (RecenterView).
    public mutating func center(on d: CGPoint) {
        origin = CGPoint(x: viewCenter.x - d.x * zoom, y: viewCenter.y - d.y * zoom)
        clampOrigin()
    }

    /// Moves the document by `delta` points (positive x moves it right).
    public mutating func pan(by delta: CGSize) {
        origin = CGPoint(x: origin.x + delta.width, y: origin.y + delta.height)
        clampOrigin()
    }

    public mutating func zoomIn(at p: CGPoint? = nil) {
        step(ZoomPresets.nextLarger(zoom), at: p)
    }

    public mutating func zoomOut(at p: CGPoint? = nil) {
        step(ZoomPresets.nextSmaller(zoom), at: p)
    }

    /// Zoom tool click: step, then center on the clicked document point.
    private mutating func step(_ z: Double, at p: CGPoint?) {
        guard let p else { return setZoom(z) }
        let d = documentPoint(atView: p)
        zoom = ZoomPresets.clamp(z)
        center(on: d)
    }

    /// View > Zoom to Window.
    public mutating func zoomToFit() {
        zoom = ZoomPresets.fit(document: documentSize, view: viewSize)
        clampOrigin()
    }

    /// View > Actual Size (100%), around the view's center.
    public mutating func actualSize() {
        setZoom(1)
    }

    /// DocumentWorkspace.ZoomToRectangle: fit the rectangle (plus one pixel on
    /// each side) and center on it. `rect` is in document pixels.
    public mutating func zoom(toDocumentRect rect: CGRect) {
        let r = rect.standardized
        let z = min(
            Double(viewSize.width) / Double(r.width + 2),
            Double(viewSize.height) / Double(r.height + 2))
        zoom = ZoomPresets.clamp(z)
        center(on: CGPoint(x: r.midX, y: r.midY))
    }

    // MARK: Scroll bars

    /// Knob size as a fraction of the track, 1 when the axis cannot scroll.
    public var knobProportion: CGSize {
        CGSize(
            width: min(1, viewSize.width / max(contentSize.width, 1)),
            height: min(1, viewSize.height / max(contentSize.height, 1)))
    }

    /// Scroll position from 0 (left or top edge) to 1 (right or bottom edge).
    public var scrollFraction: CGPoint {
        func axis(_ o: CGFloat, _ content: CGFloat, _ view: CGFloat) -> CGFloat {
            content > view ? -o / (content - view) : 0
        }
        return CGPoint(
            x: axis(origin.x, contentSize.width, viewSize.width),
            y: axis(origin.y, contentSize.height, viewSize.height))
    }

    public mutating func setScrollFraction(x: CGFloat? = nil, y: CGFloat? = nil) {
        if let x { origin.x = -x * (contentSize.width - viewSize.width) }
        if let y { origin.y = -y * (contentSize.height - viewSize.height) }
        clampOrigin()
    }
}
