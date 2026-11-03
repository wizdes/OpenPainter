// Where panels go and what they leave clear: mac/OpenPainterKit/Sources/Shell/PanelLayout.swift,
// in layout points with y growing down, as Windows has it. Plain math with no window.
using Avalonia;
using UiLook;

namespace OpenPainter.Shell;

public static class PanelLayout
{
    public const double Clearance = Tokens.PanelClearance;   // PanelLayout.clearance.
    public const double ImageMargin = 16;                    // PanelLayout.imageMargin.

    public const string Tools = "tools", History = "history", Layers = "layers", Colors = "colors";

    /// The default frame size of each panel: PanelKind.defaultSize.
    public static Size DefaultSize(string panel) => panel switch
    {
        Tools => new Size(Tokens.ToolsWidth, Tokens.ToolsHeight),
        Colors => new Size(Tokens.ColorsWidth, Tokens.ColorsHeight),
        History or Layers => new Size(Tokens.ListPanelWidth, Tokens.ListPanelHeight),
        _ => throw new ArgumentException($"PANEL: no panel named {panel}"),
    };

    /// PanelKind.minimumSize.
    public static Size MinimumSize(string panel) => panel switch
    {
        Tools => new Size(84, 94),
        Colors => new Size(244, 246),
        History or Layers => new Size(226, 160),
        _ => throw new ArgumentException($"PANEL: no panel named {panel}"),
    };

    /// Default frames inside the stage: Tools top left, Colors bottom left, History top
    /// right, Layers bottom right. When a side does not fit, the top panel shortens down
    /// to its minimum.
    public static Dictionary<string, Rect> Defaults(Rect stage)
    {
        var frames = new Dictionary<string, Rect>();
        foreach (var (top, bottom, left) in new[] { (Tools, Colors, true), (History, Layers, false) })
        {
            var b = DefaultSize(bottom);
            var t = DefaultSize(top);
            var room = stage.Height - 3 * Clearance - b.Height;
            if (t.Height > room) t = t.WithHeight(Math.Max(MinimumSize(top).Height, room));
            var tx = left ? stage.X + Clearance : stage.Right - Clearance - t.Width;
            var bx = left ? stage.X + Clearance : stage.Right - Clearance - b.Width;
            frames[top] = new Rect(tx, stage.Y + Clearance, t.Width, t.Height);
            frames[bottom] = new Rect(bx, stage.Bottom - Clearance - b.Height, b.Width, b.Height);
        }
        return frames;
    }

    /// PanelLayout.titleStrip: the top of a frame, which a pointer must be able to reach.
    const double TitleStrip = 24, LeastTitleWidth = 60, LeastTitleHeight = 20;

    /// PanelLayout.clamp: keeps a frame inside `bounds`, shrinking it only when it is larger.
    public static Rect Clamp(Rect frame, Rect bounds)
    {
        var width = Math.Min(frame.Width, bounds.Width);
        var height = Math.Min(frame.Height, bounds.Height);
        return new Rect(
            Math.Min(Math.Max(frame.X, bounds.X), bounds.Right - width),
            Math.Min(Math.Max(frame.Y, bounds.Y), bounds.Bottom - height),
            width, height);
    }

    /// PanelLayout.isReachable: the frame's title strip is mostly on one screen.
    public static bool IsReachable(Rect frame, IEnumerable<Rect> screens)
    {
        var title = new Rect(frame.X, frame.Y, frame.Width, TitleStrip);
        return screens.Any(s =>
        {
            var i = s.Intersect(title);
            return i.Width >= Math.Min(LeastTitleWidth, title.Width) && i.Height >= LeastTitleHeight;
        });
    }

    /// The screen a frame overlaps most, or else the one nearest to it.
    static Rect BestScreen(Rect frame, IReadOnlyList<Rect> screens)
    {
        static double Area(Rect a, Rect b)
        {
            var i = a.Intersect(b);
            return i.Width * i.Height;
        }
        static double Distance(Rect a, Rect b) => Math.Sqrt(Math.Pow(a.Center.X - b.Center.X, 2) + Math.Pow(a.Center.Y - b.Center.Y, 2));
        var best = screens[0];
        foreach (var s in screens.Skip(1))
        {
            var (area, bestArea) = (Area(s, frame), Area(best, frame));
            if (area > bestArea || (area == bestArea && Distance(s, frame) < Distance(best, frame))) best = s;
        }
        return best;
    }

    /// PanelLayout.onScreen: moves a frame no screen can reach fully inside the best screen.
    public static Rect OnScreen(Rect frame, IReadOnlyList<Rect> screens) =>
        screens.Count == 0 || IsReachable(frame, screens) ? frame : Clamp(frame, BestScreen(frame, screens));

    /// For a drag: the shortest move that keeps the title strip reachable. A move fully inside
    /// the screen, as OnScreen makes, would jump back and forth under a pointer held past the
    /// edge, because Windows places a dragged window by the pointer on every move.
    public static Rect Reachable(Rect frame, IReadOnlyList<Rect> screens)
    {
        if (screens.Count == 0 || IsReachable(frame, screens)) return frame;
        var s = BestScreen(frame, screens);
        var overlap = Math.Min(LeastTitleWidth, frame.Width);
        var x = Math.Min(Math.Max(frame.X, s.X + overlap - frame.Width), s.Right - overlap);
        var y = Math.Min(Math.Max(frame.Y, s.Y + LeastTitleHeight - TitleStrip), s.Bottom - LeastTitleHeight);
        return new Rect(x, y, frame.Width, frame.Height);
    }

    /// The rectangle the document fits into: the stage less the space the visible panels
    /// take on its left and right, less 16 points on every side.
    public static Rect ClearRect(Rect stage, IEnumerable<Rect> panels)
    {
        double left = 0, right = 0;
        foreach (var p in panels)
        {
            if (!p.Intersects(stage)) continue;
            if (p.Center.X < stage.Center.X) left = Math.Max(left, p.Right - stage.X);
            else right = Math.Max(right, stage.Right - p.X);
        }
        var width = Math.Max(stage.Width - left - right - 2 * ImageMargin, 1);
        var height = Math.Max(stage.Height - 2 * ImageMargin, 1);
        return new Rect(stage.X + left + ImageMargin, stage.Y + ImageMargin, width, height);
    }
}
