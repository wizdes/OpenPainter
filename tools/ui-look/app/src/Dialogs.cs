// The Curves and Hue / Saturation dialogs. Positions are measured from the Mac shots.
// OK comes before Cancel, as Windows dialogs do it.
using Avalonia;
using Avalonia.Controls;
using Avalonia.Layout;
using Avalonia.Media;
using static UiLook.Ui;

namespace UiLook;

public static class Dialogs
{
    const double DialogX = 420, DialogWidth = 440, LabelRight = 133, ControlX = 144;

    static Canvas Frame(Canvas win, Palette p, double y, double height)
    {
        var body = new Canvas { Width = DialogWidth, Height = height };
        // The shadow is a sibling of the content; see Panels.Frame.
        At(win, new Border
        {
            Background = B(p.Panel),
            CornerRadius = new CornerRadius(Tokens.DialogCorner),
            BoxShadow = new BoxShadows(new BoxShadow { OffsetY = 8, Blur = 28, Color = Tokens.Shadow }),
        }, DialogX, y, DialogWidth, height);
        At(win, body, DialogX, y);
        return body;
    }

    static void Buttons(Canvas body, Palette p, double y)
    {
        At(body, Push(p, "OK", 80, Tokens.DialogButtonHeight), DialogWidth - 16 - 164, y);
        At(body, Push(p, "Cancel", 80, Tokens.DialogButtonHeight), DialogWidth - 16 - 80, y);
    }

    static void RightLabel(Canvas body, Palette p, string text, double centerY)
    {
        var label = Label(text, p.Ink);
        label.TextAlignment = TextAlignment.Right;
        At(body, label, 0, centerY - 8, LabelRight, 16);
    }

    public static void Curves(Canvas win, Palette p)
    {
        var body = Frame(win, p, 221, 390);
        RightLabel(body, p, "Transfer map:", 34);
        At(body, Choice(p, "Luminosity", 130, 24), ControlX, 22);
        RightLabel(body, p, "Curve:", 69);

        const double gx = ControlX, gy = 56.5, size = 256;
        At(body, new Drawn((c, _) =>
        {
            c.FillRectangle(B(p.Plot), new Rect(.5, .5, size, size));
            var grid = new Pen(B(p.Line));
            for (var i = 1; i < 4; i++)
            {
                var t = .5 + i * size / 4;
                c.DrawLine(grid, new Point(t, .5), new Point(t, size + .5));
                c.DrawLine(grid, new Point(.5, t), new Point(size + .5, t));
            }
            c.DrawRectangle(new Pen(B(p.PanelLine)), new Rect(.5, .5, size, size));
            c.DrawLine(new Pen(B(p.Line), 1, new DashStyle(new[] { 3.0, 3.0 }, 0)), new Point(.5, size + .5), new Point(size + .5, .5));

            // The curve through the Mac shot's four points, as a smooth line.
            var pts = new[] { new Point(0, 0), new Point(.272, .221), new Point(.722, .80), new Point(1, 1) };
            var curve = new StreamGeometry();
            using (var g = curve.Open())
            {
                Point P(double t, int i)
                {
                    var a = pts[Math.Max(i - 1, 0)];
                    var b = pts[i];
                    var d = pts[i + 1];
                    var e = pts[Math.Min(i + 2, pts.Length - 1)];
                    double t2 = t * t, t3 = t2 * t;
                    double X(double p0, double p1, double p2, double p3) =>
                        .5 * (2 * p1 + (p2 - p0) * t + (2 * p0 - 5 * p1 + 4 * p2 - p3) * t2 + (3 * p1 - p0 - 3 * p2 + p3) * t3);
                    return new Point(.5 + X(a.X, b.X, d.X, e.X) * size, .5 + size - X(a.Y, b.Y, d.Y, e.Y) * size);
                }
                g.BeginFigure(new Point(.5, size + .5), false);
                for (var i = 0; i < pts.Length - 1; i++)
                    for (var n = 1; n <= 24; n++)
                        g.LineTo(P(n / 24.0, i));
                g.EndFigure(false);
            }
            c.DrawGeometry(null, new Pen(B(p.Ink), 2), curve);
            foreach (var pt in pts)
                c.DrawRectangle(B(p.Plot), new Pen(B(p.Ink)), new Rect(.5 + pt.X * size - 3.5, .5 + size - pt.Y * size - 3.5, 7, 7));
        }), gx, gy, size + 1, size + 1);

        var hint = Label("Click to add a point. Alt-click removes one.", p.Muted, Tokens.FontSmall);
        At(body, hint, gx + 2, 315, 256, 16);
        Buttons(body, p, 346);
    }

    public static void HueSaturation(Canvas win, Palette p)
    {
        var body = Frame(win, p, 326, 180);
        void Row(string label, double centerY, double min, double max, double value, string text)
        {
            RightLabel(body, p, label, centerY);
            var slider = Slide(p, min, max, value, 208);
            At(body, slider, ControlX, centerY - 11);
            var field = Field(p, text, 64, Tokens.FieldHeight, TextAlignment.Right);
            At(body, field, 360, centerY - 12);
        }
        Row("Hue:", 34, -180, 180, -22, "-22");
        Row("Saturation:", 70, 0, 200, 130, "130");
        Row("Lightness:", 106, -100, 100, 0, "0");
        Buttons(body, p, 138);
    }
}
