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
