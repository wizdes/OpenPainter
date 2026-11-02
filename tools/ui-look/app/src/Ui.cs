// Small helpers that make real Avalonia controls in the look's sizes and token colors.
// Each helper sets values on one control. Fluent's own templates still draw it.
using Avalonia;
using Avalonia.Controls;
using Avalonia.Layout;
using Avalonia.Media;

namespace UiLook;

public static class Ui
{
    public static IBrush B(Color c) => new SolidColorBrush(c);

    public static T At<T>(Canvas parent, T child, double x, double y, double w = double.NaN, double h = double.NaN) where T : Control
    {
        Canvas.SetLeft(child, x);
        Canvas.SetTop(child, y);
        if (!double.IsNaN(w)) child.Width = w;
        if (!double.IsNaN(h)) child.Height = h;
        parent.Children.Add(child);
        return child;
    }

    public static Border Box(Color fill, double corner = 0) => new() { Background = B(fill), CornerRadius = new CornerRadius(corner) };

    public static TextBlock Label(string text, Color color, double size = Tokens.FontBody, FontWeight weight = FontWeight.Normal) => new()
    {
        Text = text,
        Foreground = B(color),
        FontSize = size,
        FontWeight = weight,
        VerticalAlignment = VerticalAlignment.Center,
        TextTrimming = TextTrimming.CharacterEllipsis,
    };

    /// A text field: Fluent's TextBox with Windows 11's stronger bottom line on top.
    public static Control Field(Palette p, string text, double width, double height = Tokens.FieldHeight, TextAlignment align = TextAlignment.Center) => new Grid
    {
        Width = width,
        Height = height,
        VerticalAlignment = VerticalAlignment.Center,
        Children =
        {
            FieldBox(p, text, width, height, align),
            new Border
            {
                BorderBrush = B(p.FieldBottom),
                BorderThickness = new Thickness(0, 0, 0, 1),
                CornerRadius = new CornerRadius(Tokens.ControlCorner),
                IsHitTestVisible = false,
            },
        },
    };

    static TextBox FieldBox(Palette p, string text, double width, double height, TextAlignment align) => new()
    {
        Text = text,
        Width = width,
        Height = height,
        MinWidth = 0,
        MinHeight = 0,
        FontSize = Tokens.FontBody,
        Foreground = B(p.Ink),
        Background = B(p.Field),
        BorderBrush = B(p.ControlEdge),
        BorderThickness = new Thickness(1),
        CornerRadius = new CornerRadius(Tokens.ControlCorner),
        Padding = new Thickness(6, 0),
        TextAlignment = align,
        VerticalContentAlignment = VerticalAlignment.Center,
        VerticalAlignment = VerticalAlignment.Center,
    };

    public static ComboBox Choice(Palette p, object content, double width, double height = Tokens.ChoiceHeight, double padLeft = 10)
    {
        var box = new ComboBox
        {
            Width = width,
            Height = height,
            MinHeight = 0,
            MinWidth = 0,
            FontSize = Tokens.FontBody,
            Foreground = B(p.Ink),
            Background = B(p.Control),
            BorderBrush = B(p.ControlEdge),
            BorderThickness = new Thickness(1),
            CornerRadius = new CornerRadius(Tokens.ControlCorner),
            Padding = new Thickness(padLeft, 0, 0, 0),
            VerticalAlignment = VerticalAlignment.Center,
            VerticalContentAlignment = VerticalAlignment.Center,
            HorizontalContentAlignment = HorizontalAlignment.Left,
        };
        box.Items.Add(content);
        box.SelectedIndex = 0;
        // Fluent's arrow is drawn for 14 pixel text. This look uses 12.
        box.TemplateApplied += (_, e) =>
        {
            var glyph = e.NameScope.Find<Control>("DropDownGlyph") ?? throw new InvalidOperationException("Fluent's ComboBox template has no DropDownGlyph");
            glyph.Width = Tokens.ChoiceArrow;
            glyph.Height = Tokens.ChoiceArrow;
        };
        return box;
    }

    public static Button Push(Palette p, string text, double width, double height = Tokens.ButtonHeight, bool accent = false) => new()
    {
        Content = text,
        Width = width,
        Height = height,
        MinHeight = 0,
        FontSize = Tokens.FontBody,
        Foreground = B(accent ? p.OnAccent : p.Ink),
        Background = B(accent ? p.Accent : p.Control),
        BorderBrush = B(accent ? p.Accent : p.ControlEdge),
        BorderThickness = new Thickness(1),
        CornerRadius = new CornerRadius(Tokens.ControlCorner),
        Padding = new Thickness(0),
        HorizontalContentAlignment = HorizontalAlignment.Center,
        VerticalContentAlignment = VerticalAlignment.Center,
        VerticalAlignment = VerticalAlignment.Center,
    };

    public static CheckBox Check(Palette p, string text)
    {
        var box = new CheckBox
        {
            Content = text,
            IsChecked = true,
            MinHeight = 0,
            MinWidth = 0,
            FontSize = Tokens.FontBody,
            Foreground = B(p.Ink),
            Padding = new Thickness(6, 0, 0, 0),
            VerticalAlignment = VerticalAlignment.Center,
            VerticalContentAlignment = VerticalAlignment.Center,
        };
        box.Resources["CheckBoxCheckBackgroundFillChecked"] = B(p.Control);
        box.Resources["CheckBoxCheckBackgroundStrokeChecked"] = B(p.Control);
        box.Resources["CheckBoxCheckGlyphForegroundChecked"] = B(p.Ink);
        box.Resources["CheckBoxForegroundChecked"] = B(p.Ink);
        return box;
    }

    public static Slider Slide(Palette p, double min, double max, double value, double width)
    {
        var slider = new Slider
        {
            Minimum = min,
            Maximum = max,
            Value = value,
            Width = width,
            MinHeight = 0,
            VerticalAlignment = VerticalAlignment.Center,
            Margin = new Thickness(0, 4, 0, 0),   // Fluent's template sits high once the margins above are gone.
        };
        slider.Resources["SliderTrackFill"] = B(p.Line);
        slider.Resources["SliderTrackValueFill"] = B(p.SliderValue);
        slider.Resources["SliderContainerBackground"] = Brushes.Transparent;
        slider.Resources["SliderPreContentMargin"] = new GridLength(0);
        slider.Resources["SliderPostContentMargin"] = new GridLength(0);
        slider.Resources["SliderHorizontalHeight"] = 20.0;
        // Fluent's knob has one color and no ring, so this draws its own: white with a darker ring.
        slider.TemplateApplied += (_, e) =>
        {
            var track = e.NameScope.Find<Avalonia.Controls.Primitives.Track>("PART_Track") ?? throw new InvalidOperationException("Fluent's Slider template has no PART_Track");
            var thumb = track.Thumb ?? throw new InvalidOperationException("Fluent's Slider track has no thumb");
            thumb.Width = Tokens.KnobSize;
            thumb.Height = Tokens.KnobSize;
            thumb.Template = new Avalonia.Controls.Templates.FuncControlTemplate<Avalonia.Controls.Primitives.Thumb>((_, _) => new Border
            {
                Background = B(p.Knob),
                BorderBrush = B(p.KnobRing),
                BorderThickness = new Thickness(1),
                CornerRadius = new CornerRadius(Tokens.KnobSize / 2),
            });
        };
        return slider;
    }

    /// A command target with one icon. `on` draws the Mac's pressed look: wash, outline, 2 pixel bottom edge.
    public static Control IconButton(Palette p, string icon, Color color, double target = Tokens.CommandTarget, double iconSize = Tokens.IconSize, bool on = false)
    {
        var grid = new Grid { Width = target, Height = target, VerticalAlignment = VerticalAlignment.Center };
        if (on)
        {
            grid.Children.Add(new Border
            {
                Background = B(p.Wash),
                BorderBrush = B(p.Accent),
                BorderThickness = new Thickness(1),
                CornerRadius = new CornerRadius(Tokens.ControlCorner),
            });
            grid.Children.Add(new Border
            {
                Background = B(p.Accent),
                Height = 2,
                Margin = new Thickness(3, 0),
                VerticalAlignment = VerticalAlignment.Bottom,
            });
        }
        grid.Children.Add(new IconView(icon, color, iconSize) { HorizontalAlignment = HorizontalAlignment.Center, VerticalAlignment = VerticalAlignment.Center });
        return grid;
    }

    public static StackPanel Row(double spacing = 0) => new() { Orientation = Orientation.Horizontal, Spacing = spacing, VerticalAlignment = VerticalAlignment.Center };

    public static Color Hex(string hex)
    {
        var v = Convert.ToUInt32(hex.TrimStart('#'), 16);
        return Color.FromRgb((byte)(v >> 16), (byte)(v >> 8), (byte)v);
    }

    /// HSV to color: hue in degrees, saturation and value from 0 to 1.
    public static Color Hsv(double h, double s, double v)
    {
        h = ((h % 360) + 360) % 360;
        var c = v * s;
        var x = c * (1 - Math.Abs((h / 60) % 2 - 1));
        var m = v - c;
        var (r, g, b) = h switch
        {
            < 60 => (c, x, 0.0),
            < 120 => (x, c, 0.0),
            < 180 => (0.0, c, x),
            < 240 => (0.0, x, c),
            < 300 => (x, 0.0, c),
            _ => (c, 0.0, x),
        };
        return Color.FromRgb(To8(r + m), To8(g + m), To8(b + m));
    }

    public static byte To8(double v) => (byte)Math.Round(Math.Clamp(v, 0, 1) * 255);

    public static (double H, double S, double V) ToHsv(Color c)
    {
        double r = c.R / 255.0, g = c.G / 255.0, b = c.B / 255.0;
        double max = Math.Max(r, Math.Max(g, b)), min = Math.Min(r, Math.Min(g, b)), d = max - min;
        double h = d == 0 ? 0 : max == r ? 60 * (((g - b) / d) % 6) : max == g ? 60 * ((b - r) / d + 2) : 60 * ((r - g) / d + 4);
        if (h < 0) h += 360;
        return (h, max == 0 ? 0 : d / max, max);
    }
}

/// A checkerboard, for transparent layers and the alpha slider.
public sealed class Checker : Control
{
    readonly double _cell;
    public Checker(double cell) { _cell = cell; IsHitTestVisible = false; }

    public override void Render(DrawingContext context)
    {
        var light = new SolidColorBrush(Tokens.CheckerLight);
        var dark = new SolidColorBrush(Tokens.CheckerDark);
        context.FillRectangle(light, new Rect(Bounds.Size));
        var row = 0;
        for (double y = 0; y < Bounds.Height; y += _cell, row++)
            for (double x = row % 2 == 0 ? 0 : _cell; x < Bounds.Width; x += 2 * _cell)
                context.FillRectangle(dark, new Rect(x, y, Math.Min(_cell, Bounds.Width - x), Math.Min(_cell, Bounds.Height - y)));
    }
}

/// A shape drawn in code: dashed outlines, the diamond, the swatch notch and the curve.
public sealed class Drawn : Control
{
    readonly Action<DrawingContext, Size> _draw;
    public Drawn(Action<DrawingContext, Size> draw) { _draw = draw; IsHitTestVisible = false; }
    public override void Render(DrawingContext context) => _draw(context, Bounds.Size);
}
