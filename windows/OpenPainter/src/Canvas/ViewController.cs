// What the canvas does with input: the Zoom and Pan tools, the held space bar, the wheel
// and the Fit rule. It follows MetalCanvasView.swift and has no window, so tests drive it
// with no screen. CanvasHost feeds it from window messages on the UI thread and each
// change goes straight to op_view_set, so nothing here waits for a UI frame.
using Avalonia;

namespace OpenPainter.Canvas;

public enum CanvasTool { Inactive, Pan, Zoom }
public enum CanvasCursor { Arrow, Move, Cross }
public enum CanvasButton { Left, Right }

public sealed class ViewController
{
    const double WheelZoomPerNotch = 1.12;   // paint.net: Ctrl + wheel zooms by 1.12 per notch.
    const double PointsPerLine = 16;         // MetalCanvasView.scrollWheel.
    const double ZoomClickLimit = 10;        // A smaller drag is a click.
    const double LeastLayoutSize = 20;       // The first Fit waits for a real size.

    bool _fitOnFirstLayout = true;
    bool _spaceHeld;
    Point? _dragStart, _lastDrag;
    bool _altAtPress;

    public Viewport View { get; } = new(new Size(1, 1), new Size(1, 1));

    /// True while the zoom is the Fit zoom: a size or panel change fits again. Any other zoom change clears it.
    public bool IsFitted { get; private set; } = true;

    public CanvasTool Tool { get; set; } = CanvasTool.Inactive;
    public CanvasTool EffectiveTool => _spaceHeld ? CanvasTool.Pan : Tool;
    public bool Dragging => _dragStart is not null;

    public CanvasCursor Cursor => EffectiveTool switch
    {
        CanvasTool.Pan => CanvasCursor.Move,
        CanvasTool.Zoom => CanvasCursor.Cross,
        _ => CanvasCursor.Arrow,
    };

    /// The view changed. The window sends the new state to the engine.
    public event Action? Changed;

    /// The Zoom tool's drag rectangle in view points, or null when it ends.
    public event Action<Rect?>? ZoomRectangle;

    /// The pointer's document position, or null when it leaves.
    public event Action<Point?>? PointerAt;

    void Change(Action<Viewport> body)
    {
        var before = View.Zoom;
        body(View);
        if (View.Zoom != before) IsFitted = false;
        Changed?.Invoke();
    }

    public void Resize(Size size)
    {
        View.Resize(size);
        if (_fitOnFirstLayout && size.Width > LeastLayoutSize && size.Height > LeastLayoutSize)
        {
            _fitOnFirstLayout = false;
            View.ZoomToFit();
        }
        else if (IsFitted)
        {
            View.ZoomToFit();
        }
        Changed?.Invoke();
    }

    public void SetClearRect(Rect? clear)
    {
        if (clear == View.ClearRect) return;
        View.ClearRect = clear;
        if (IsFitted) View.ZoomToFit();
        Changed?.Invoke();
    }

    /// A new image: the view takes its size and fits again.
    public void SetDocumentSize(Size size)
    {
        if (size == View.DocumentSize || size.Width <= 0 || size.Height <= 0) return;
        View.DocumentSize = size;
        View.ZoomToFit();
        IsFitted = true;
        Changed?.Invoke();
    }

    public void Fit()
    {
        View.ZoomToFit();
        IsFitted = true;
        Changed?.Invoke();
    }

    public void ZoomIn() => Change(v => v.ZoomIn());
    public void ZoomOut() => Change(v => v.ZoomOut());
    public void ActualSize() => Change(v => v.ActualSize());
    public void SetZoom(double zoom) => Change(v => v.SetZoom(zoom));

    /// One turn of the wheel. Ctrl zooms around the pointer. Otherwise the view scrolls
    /// by lines, and Shift turns up and down into left and right.
    public void Wheel(Point p, double notchesX, double notchesY, double linesPerNotch, bool ctrl, bool shift)
    {
        if (ctrl)
        {
            Change(v => v.SetZoom(v.Zoom * Math.Pow(WheelZoomPerNotch, notchesY), p));
            return;
        }
        var dx = notchesX * linesPerNotch * PointsPerLine;
        var dy = notchesY * linesPerNotch * PointsPerLine;
        if (shift && dx == 0) (dx, dy) = (dy, 0);
        Change(v => v.Pan(dx, dy));
    }

    public void PointerDown(Point p, CanvasButton button, bool alt)
    {
        if (button == CanvasButton.Right)
        {
            // paint.net's Zoom tool: a right click steps out.
            if (EffectiveTool == CanvasTool.Zoom) Change(v => v.ZoomOut(p));
            return;
        }
        _dragStart = p;
        _lastDrag = p;
        _altAtPress = alt;
    }

    public void PointerMove(Point p)
    {
        PointerAt?.Invoke(View.DocumentPoint(p));
        if (_dragStart is not { } start || _lastDrag is not { } last) return;
        _lastDrag = p;
        switch (EffectiveTool)
        {
            case CanvasTool.Pan:
                Change(v => v.Pan(p.X - last.X, p.Y - last.Y));
                break;
            case CanvasTool.Zoom:
                ZoomRectangle?.Invoke(Span(start, p));
                break;
        }
    }

    public void PointerUp(Point p)
    {
        if (_dragStart is not { } start) return;
        var tool = EffectiveTool;
        var alt = _altAtPress;
        EndDrag();
        if (tool != CanvasTool.Zoom) return;
        var rect = Span(start, p);
        if (Math.Sqrt(rect.Width * rect.Width + rect.Height * rect.Height) < ZoomClickLimit)
        {
            // paint.net: a click steps in and recenters on the point. Alt takes the Mac's Option-click.
            if (alt) Change(v => v.ZoomOut(p));
            else Change(v => v.ZoomIn(p));
            return;
        }
        Change(v => v.ZoomToDocumentRect(new Rect(v.DocumentPoint(rect.TopLeft), v.DocumentPoint(rect.BottomRight))));
    }

    public void PointerLeft() => PointerAt?.Invoke(null);

    /// The press ended with no release we can use: another window took the mouse.
    public void CancelDrag()
    {
        if (_dragStart is not null) EndDrag();
    }

    void EndDrag()
    {
        _dragStart = null;
        _lastDrag = null;
        ZoomRectangle?.Invoke(null);
    }

    public void SpaceDown() => _spaceHeld = true;
    public void SpaceUp() => _spaceHeld = false;

    /// Focus left the canvas, so its key-up may never come.
    public void FocusLost()
    {
        _spaceHeld = false;
        CancelDrag();
    }

    static Rect Span(Point a, Point b) =>
        new(Math.Min(a.X, b.X), Math.Min(a.Y, b.Y), Math.Abs(b.X - a.X), Math.Abs(b.Y - a.Y));
}
