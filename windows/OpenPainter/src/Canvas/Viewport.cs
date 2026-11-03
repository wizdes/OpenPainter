// Where the document sits in the canvas view: mac/OpenPainterKit/Sources/Viewport.swift.
// Plain math with no window, so tests run it with no screen. View space is in layout
// points, from the view's top-left corner, with y growing down. Document space is in
// document pixels.
using Avalonia;

namespace OpenPainter.Canvas;

public sealed class Viewport
{
    public Size DocumentSize { get; set; }
    public Size ViewSize { get; set; }

    /// Points per document pixel, within ZoomPresets' limits.
    public double Zoom { get; private set; }

    /// View position of the document's top-left corner.
    public Point Origin { get; private set; }

    Rect? _clearRect;

    public Viewport(Size documentSize, Size viewSize, double zoom = 1)
    {
        DocumentSize = documentSize;
        ViewSize = viewSize;
        Zoom = ZoomPresets.Clamp(zoom);
        ClampOrigin();
    }

    /// The part of the view not covered by panels. A document smaller than the view is
    /// centered here, as far as the view's edges allow. Null means the whole view.
    public Rect? ClearRect
    {
        get => _clearRect;
        set { _clearRect = value; ClampOrigin(); }
    }

    public Size ContentSize => new(DocumentSize.Width * Zoom, DocumentSize.Height * Zoom);
    public Point ViewCenter => new(ViewSize.Width / 2, ViewSize.Height / 2);

    public Point DocumentPoint(Point view) => new((view.X - Origin.X) / Zoom, (view.Y - Origin.Y) / Zoom);
    public Point ViewPoint(Point document) => new(document.X * Zoom + Origin.X, document.Y * Zoom + Origin.Y);

    static double Rounded(double v) => Math.Round(v, MidpointRounding.AwayFromZero);

    /// On each axis: a document smaller than the view is centered, and a larger one
    /// cannot be scrolled past its edges.
    static double Axis(double origin, double content, double view, double? middle)
    {
        if (content <= view)
            return middle is { } m ? Math.Min(Math.Max(Rounded(m - content / 2), 0), view - content) : Rounded((view - content) / 2);
        return Math.Min(0, Math.Max(view - content, origin));
    }

    public void ClampOrigin()
    {
        var content = ContentSize;
        Origin = new Point(
            Axis(Origin.X, content.Width, ViewSize.Width, _clearRect?.Center.X),
            Axis(Origin.Y, content.Height, ViewSize.Height, _clearRect?.Center.Y));
    }

    public void Resize(Size size)
    {
        var keep = DocumentPoint(ViewCenter);
        ViewSize = size;
        CenterOn(keep);
    }

    /// Sets the zoom and keeps the document point under `p` there, as far as the scroll limits allow.
    public void SetZoom(double zoom, Point p)
    {
        var d = DocumentPoint(p);
        Zoom = ZoomPresets.Clamp(zoom);
        Origin = new Point(p.X - d.X * Zoom, p.Y - d.Y * Zoom);
        ClampOrigin();
    }

    /// Sets the zoom around the view's center.
    public void SetZoom(double zoom) => SetZoom(zoom, ViewCenter);

    /// Scrolls so document point `d` is at the view's center.
    public void CenterOn(Point d)
    {
        Origin = new Point(ViewCenter.X - d.X * Zoom, ViewCenter.Y - d.Y * Zoom);
        ClampOrigin();
    }

    /// Moves the document by a number of points. Positive x moves it right.
    public void Pan(double dx, double dy)
    {
        Origin = new Point(Origin.X + dx, Origin.Y + dy);
        ClampOrigin();
    }

    public void ZoomIn(Point? at = null) => Step(ZoomPresets.NextLarger(Zoom), at);
    public void ZoomOut(Point? at = null) => Step(ZoomPresets.NextSmaller(Zoom), at);

    /// A Zoom tool click: step, then center on the clicked document point.
    void Step(double zoom, Point? at)
    {
        if (at is not { } p)
        {
            SetZoom(zoom);
            return;
        }
        var d = DocumentPoint(p);
        Zoom = ZoomPresets.Clamp(zoom);
        CenterOn(d);
    }

    /// View > Zoom to Window.
    public void ZoomToFit()
    {
        var w = ViewSize.Width;
        var h = ViewSize.Height;
        if (_clearRect is { } r)
        {
            // The clear rectangle already holds the 16 point image margins, and Fit
            // takes 10 points off again, so give it those back.
            w = Math.Min(r.Width + 10, w);
            h = Math.Min(r.Height + 10, h);
        }
        Zoom = ZoomPresets.Fit(DocumentSize.Width, DocumentSize.Height, w, h);
        ClampOrigin();
    }

    /// View > Actual Size, around the view's center.
    public void ActualSize() => SetZoom(1);

    /// Fits a document rectangle, plus one pixel on each side, and centers on it.
    public void ZoomToDocumentRect(Rect rect)
    {
        var z = Math.Min(ViewSize.Width / (rect.Width + 2), ViewSize.Height / (rect.Height + 2));
        Zoom = ZoomPresets.Clamp(z);
        CenterOn(rect.Center);
    }
}
