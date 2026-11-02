// The four floating panels: Tools, History, Layers and Colors.
// Sizes and row layouts follow the Mac's PanelViews.swift, HistoryPanel.swift,
// LayersPanel.swift and ColorsPanel.swift.
using Avalonia;
using Avalonia.Controls;
using Avalonia.Layout;
using Avalonia.Media;
using Avalonia.Media.Imaging;
using static UiLook.Ui;

namespace UiLook;

public static class Panels
{
    /// The icon box and the selected dot of a Tools panel cell, relative to the cell.
    public static Rect ToolIconBox => new(2, 2, Tokens.IconSize, Tokens.IconSize);
    public static Rect ToolDotBox => new(22, 0, 4, 4);

    /// A panel frame: shadow, outline, title row with the close button on the right, and a body.
    public static Control Frame(Palette p, string title, Control body, double width, double height)
    {
        var grid = new Grid { Width = width, Height = height, RowDefinitions = new RowDefinitions($"{Tokens.PanelTitleHeight},*") };
        var titleRow = new Grid();
        titleRow.Children.Add(new Border { Background = B(p.TitleBar), CornerRadius = new CornerRadius(Tokens.PanelCorner, Tokens.PanelCorner, 0, 0) });
        titleRow.Children.Add(new TextBlock
        {
            Text = title,
            FontSize = Tokens.FontSmall,
            Foreground = B(p.Muted),
            HorizontalAlignment = HorizontalAlignment.Center,
            VerticalAlignment = VerticalAlignment.Center,
        });
        titleRow.Children.Add(new IconView("window-close", p.Muted, 12)
        {
            HorizontalAlignment = HorizontalAlignment.Right,
            VerticalAlignment = VerticalAlignment.Center,
            Margin = new Thickness(0, 0, 7, 0),
        });
        titleRow.Children.Add(new Border { Height = 1, Background = B(p.PanelEdge), VerticalAlignment = VerticalAlignment.Bottom });
        grid.Children.Add(titleRow);
        Grid.SetRow(body, 1);
        grid.Children.Add(body);
        // The shadow is a sibling of the content, not its parent: when rendering to a bitmap,
        // Avalonia 12.1.4 draws a shadowed Border's child at the wrong scale.
        return new Grid
        {
            Width = width,
            Height = height,
            Children =
            {
                new Border
                {
                    Background = B(p.Panel),
                    CornerRadius = new CornerRadius(Tokens.PanelCorner),
                    BoxShadow = new BoxShadows(new BoxShadow { OffsetY = 3, Blur = 12, Color = Tokens.Shadow }),
                },
                grid,
                new Border
                {
                    BorderBrush = B(p.PanelEdge),
                    BorderThickness = new Thickness(1),
                    CornerRadius = new CornerRadius(Tokens.PanelCorner),
                    IsHitTestVisible = false,
                },
            },
        };
    }

    public static Control Tools(Palette p, Scene s)
    {
        var body = new Canvas();
        const double cell = Tokens.PanelTarget, gap = 2, inset = 5;
        var x0 = Math.Round((Tokens.ToolsWidth - (2 * cell + gap)) / 2);
        for (var i = 0; i < Scenes.Tools.Length; i++)
        {
            var (_, icon, _) = Scenes.Tools[i];
            var x = x0 + i % 2 * (cell + gap);
            var y = inset + i / 2 * (cell + gap);
            var on = icon == s.ToolIcon;
            if (on)
            {
                At(body, Box(p.Wash, 3), x, y, cell, cell);
                At(body, Box(p.Accent), x, y, 2, cell);
                // The dot sits outside the icon box.
                At(body, new Border { Background = B(p.Accent), CornerRadius = new CornerRadius(2) }, x + ToolDotBox.X, y + ToolDotBox.Y, ToolDotBox.Width, ToolDotBox.Height);
            }
            At(body, new IconView(icon, p.Ink), x + 2, y + 2);
        }
        return Frame(p, "Tools", body, Tokens.ToolsWidth, Tokens.ToolsHeight);
    }

    static Control ListRow(Palette p, double height, bool current, Control content)
    {
        var grid = new Grid { Height = height, Background = current ? B(p.Wash) : Brushes.Transparent };
        grid.Children.Add(content);
        if (current)
        {
            grid.Children.Add(new Border { Width = 2, Background = B(p.Accent), HorizontalAlignment = HorizontalAlignment.Left });
            grid.Children.Add(new Drawn((c, size) =>
            {
                var cx = size.Width - 12;
                var cy = size.Height / 2;
                var g = new PolylineGeometry(new[] { new Point(cx, cy - 4), new Point(cx + 4, cy), new Point(cx, cy + 4), new Point(cx - 4, cy) }, true);
                c.DrawGeometry(B(p.Accent), null, g);
            }));
        }
        grid.Children.Add(new Border { Height = 1, Background = B(p.Line), VerticalAlignment = VerticalAlignment.Bottom });
        return grid;
    }

    public static Control History(Palette p, Scene s)
    {
        var body = new Grid { RowDefinitions = new RowDefinitions("*,1," + Tokens.ActionRowHeight) };
        var list = new StackPanel();
        for (var i = 0; i < s.HistoryCount; i++)
        {
            var e = Scenes.History[i];
            var current = i == s.HistoryCurrent;
            var row = new Canvas();
            At(row, new IconView(e.Icon, p.Ink), 7, 3.5);
            var label = Label(e.Title, p.Ink, weight: current ? Tokens.StrongWeight : FontWeight.Normal);
            At(row, label, 36, 5, Tokens.ListPanelWidth - 60, 16);
            list.Children.Add(ListRow(p, Tokens.HistoryRowHeight, current, row));
        }
        body.Children.Add(list);
        var line = Box(p.Line);
        Grid.SetRow(line, 1);
        body.Children.Add(line);
        var enabled = s.HistoryCurrent > 0;
        var actions = new Grid { Margin = new Thickness(8, 0), ColumnDefinitions = new ColumnDefinitions("Auto,*,Auto") };
        var undo = Row(4);
        undo.Children.Add(new IconView("cmd-undo", enabled ? p.Ink : p.DisabledText, Tokens.SmallIconSize));
        undo.Children.Add(Label("Undo", enabled ? p.Ink : p.DisabledText));
        actions.Children.Add(undo);
        var redo = Row(4);
        redo.Children.Add(Label("Redo", p.DisabledText));
        redo.Children.Add(new IconView("cmd-redo", p.DisabledText, Tokens.SmallIconSize));
        Grid.SetColumn(redo, 2);
        actions.Children.Add(redo);
        Grid.SetRow(actions, 2);
        body.Children.Add(actions);
        return Frame(p, "History", body, Tokens.ListPanelWidth, Tokens.ListPanelHeight);
    }

    public static Control Layers(Palette p, Scene s, Bitmap image)
    {
        var body = new Grid { RowDefinitions = new RowDefinitions("*,1," + Tokens.ActionRowHeight) };
        var list = new StackPanel();
        var layers = Scenes.Layers(s);
        for (var i = 0; i < layers.Length; i++)
        {
            var l = layers[i];
            var current = i == 0;
            var row = new Canvas();
            const double tw = 40, th = 30;
            At(row, new Checker(4), 8, 9, tw, th);
            if (l.HasImage)
            {
                var thumb = new Image { Source = image, Stretch = Stretch.Fill };
                RenderOptions.SetBitmapInterpolationMode(thumb, BitmapInterpolationMode.HighQuality);
                At(row, thumb, 8, 9, tw, th);
            }
            At(row, new Border { BorderBrush = B(p.Line), BorderThickness = new Thickness(1) }, 8, 9, tw, th);
            At(row, Label(l.Name, p.Ink, weight: current ? Tokens.StrongWeight : FontWeight.Normal), 58, 16, Tokens.ListPanelWidth - 114, 16);
            At(row, new IconView("layer-visible", s.Dialog == DialogKind.None ? p.Ink : p.Muted), Tokens.ListPanelWidth - 50, 14.5);
            list.Children.Add(ListRow(p, Tokens.LayerRowHeight, current, row));
        }
        body.Children.Add(list);
        var line = Box(p.Line);
        Grid.SetRow(line, 1);
        body.Children.Add(line);
        // Off buttons as the Mac shots show them. With a dialog open, the Mac's row shows all seven on.
        var many = layers.Length > 1 || s.Dialog != DialogKind.None;
        var canRaise = s.Dialog != DialogKind.None;
        var actions = Row(4);
        actions.Margin = new Thickness(8, 0);
        var enabledActions = new[] { true, many, true, many, canRaise, many, true };
        for (var i = 0; i < Scenes.LayerActions.Length; i++)
            actions.Children.Add(IconButton(p, Scenes.LayerActions[i].Icon, enabledActions[i] ? p.Ink : p.Disabled, Tokens.PanelTarget));
        Grid.SetRow(actions, 2);
        body.Children.Add(actions);
        return Frame(p, "Layers", body, Tokens.ListPanelWidth, Tokens.ListPanelHeight);
    }

    /// The default palette, built as ColorModel.swift builds it: 96 colors.
    static List<Color> PaletteColors()
    {
        var c = new List<Color>();
        for (var i = 0; i < 8; i++) { var v = (byte)Math.Round(i * 255.0 / 7); c.Add(Color.FromRgb(v, v, v)); }
        for (var i = 0; i < 24; i++) c.Add(Hsv(i * 15, 1, 1));
        for (var i = 0; i < 24; i++) c.Add(Hsv(i * 15, 1, .6));
        for (var i = 0; i < 24; i++) c.Add(Hsv(i * 15, .4, 1));
        for (var i = 0; i < 8; i++) c.Add(Hsv(i * 45, 1, .35));
        for (var i = 0; i < 8; i++) c.Add(Hsv(i * 45, .15, 1));
        return c;
    }

    public static Control Colors(Palette p, Scene? s, bool expanded)
    {
        var primaryHex = s?.PrimaryHex ?? "#D41E3A";
        var secondaryHex = s?.SecondaryHex ?? "#FFFFFF";
        var chosen = s?.ChosenSwatch ?? -1;
        var primary = Hex(primaryHex);
        var secondary = Hex(secondaryHex);
        var (h, sat, val) = ToHsv(primary);
        const double pad = 10, rowY = 72, moreY = 102, channelRow = 26;
        var width = Tokens.ColorsWidth;
        var height = expanded ? Tokens.ColorsExpandedHeight : Tokens.ColorsHeight;
        var body = new Canvas();

        // Swatches. The active one has the notch.
        At(body, new Border { Background = B(secondary), BorderBrush = B(p.PanelLine), BorderThickness = new Thickness(1) }, pad + 22, pad + 22, 32, 32);
        At(body, new Border { Background = B(primary), BorderBrush = B(p.PanelLine), BorderThickness = new Thickness(1) }, pad, pad, 32, 32);
        At(body, new Drawn((c, _) => c.DrawGeometry(B(p.Ink), null,
            new PolylineGeometry(new[] { new Point(0, 0), new Point(9, 0), new Point(0, 9) }, true))), pad, pad, 9, 9);

        At(body, IconButton(p, "colors-swap", p.Ink, Tokens.PanelTarget), 68, pad);
        At(body, IconButton(p, "colors-reset", p.Ink, Tokens.PanelTarget), 68, pad + 30);
        At(body, Label("Hex", p.Muted), 100, pad + 1, 72, 15);
        At(body, Field(p, primaryHex, 72, Tokens.FieldHeight, TextAlignment.Left), 100, pad + 18);

        // The wheel and its ring marker.
        var wx = width - pad - 52;
        At(body, new Image { Source = Wheel.Bitmap(208) }, wx, pad, 52, 52);
        At(body, new Drawn((c, size) => c.DrawEllipse(null, new Pen(B(p.PanelLine)), new Rect(size).Deflate(.5))), wx, pad, 52, 52);
        var rad = h * Math.PI / 180;
        var mx = wx + 26 + Math.Cos(rad) * sat * 26;
        var my = pad + 26 - Math.Sin(rad) * sat * 26;
        At(body, new Drawn((c, _) =>
        {
            c.DrawEllipse(null, new Pen(Brushes.White, 2), new Point(7, 7), 5, 5);
            c.DrawEllipse(null, new Pen(Brushes.Black, 2), new Point(7, 7), 3.2, 3.2);
        }), mx - 7, my - 7, 14, 14);

        // More or Less, add color, palette menu.
        At(body, Push(p, expanded ? "« Less" : "More »", 84), pad - 2, rowY);
        At(body, IconButton(p, "colors-add", p.Ink, Tokens.PanelTarget), width - pad - 52, rowY);
        At(body, IconButton(p, "cmd-more", p.Ink, Tokens.PanelTarget), width - pad - 24, rowY);

        var y = moreY;
        if (expanded)
        {
            foreach (var row in Channels(primary, h, sat, val))
            {
                ChannelRow(body, p, row, pad, y, width - 2 * pad);
                y += channelRow;
            }
            y += 6;
        }

        // Palette: 8 columns of 24 pixel targets, cut off at the panel's bottom when expanded.
        var colors = PaletteColors();
        var count = expanded ? colors.Count : 32;
        var x0 = Math.Round((width - (8 * 24 + 7 * 2)) / 2);
        var bodyHeight = height - Tokens.PanelTitleHeight;
        var paletteClip = new Canvas { ClipToBounds = true };
        At(body, paletteClip, 0, y, width, Math.Max(bodyHeight - y - 8, 0));
        for (var i = 0; i < count; i++)
        {
            var cx = x0 + i % 8 * 26;
            var cy = i / 8 * 26;
            At(paletteClip, new Border { Background = B(colors[i]), BorderBrush = B(p.Line), BorderThickness = new Thickness(1) }, cx + 2, cy + 2, 20, 20);
            if (i == chosen)
            {
                At(paletteClip, new Border { BorderBrush = B(p.Accent), BorderThickness = new Thickness(2) }, cx, cy, 24, 24);
                At(paletteClip, new Drawn((c, _) =>
                {
                    var points = new[] { new Point(7, 12.5), new Point(10.5, 16), new Point(17, 8.5) };
                    c.DrawGeometry(null, new Pen(new SolidColorBrush(Color.FromArgb(0xB3, 0, 0, 0)), 3.5, lineCap: PenLineCap.Round, lineJoin: PenLineJoin.Round), new PolylineGeometry(points, false));
                    c.DrawGeometry(null, new Pen(Brushes.White, 1.75, lineCap: PenLineCap.Round, lineJoin: PenLineJoin.Round), new PolylineGeometry(points, false));
                }), cx, cy, 24, 24);
            }
        }
        return Frame(p, "Colors", body, width, height);
    }

    sealed record Channel(string Label, double Fraction, int Value, Color[] Stops, bool Alpha = false);

    static IEnumerable<Channel> Channels(Color c, double h, double s, double v)
    {
        Color[] Stops(Func<double, Color> at) => Enumerable.Range(0, 17).Select(i => at(i / 16.0)).ToArray();
        yield return new("R", c.R / 255.0, c.R, Stops(t => Color.FromRgb(To8(t), c.G, c.B)));
        yield return new("G", c.G / 255.0, c.G, Stops(t => Color.FromRgb(c.R, To8(t), c.B)));
        yield return new("B", c.B / 255.0, c.B, Stops(t => Color.FromRgb(c.R, c.G, To8(t))));
        yield return new("H", h / 360, (int)Math.Round(h), Stops(t => Hsv(t * 360, 1, 1)));
        yield return new("S", s, (int)Math.Round(s * 100), Stops(t => Hsv(h, t, v)));
        yield return new("V", v, (int)Math.Round(v * 100), Stops(t => Hsv(h, s, t)));
        yield return new("A", 1, 255, Stops(t => Color.FromArgb(To8(t), c.R, c.G, c.B)), Alpha: true);
    }

    static void ChannelRow(Canvas body, Palette p, Channel ch, double x, double y, double rowWidth)
    {
        At(body, Label(ch.Label, p.Muted, weight: FontWeight.SemiBold), x, y + 4, 14, 16);
        At(body, Field(p, ch.Value.ToString(), 44), x + rowWidth - 44, y);
        var sliderWidth = rowWidth - 18 - 50;
        var tx = x + 18 + 5;
        var ty = y + 8;
        var tw = sliderWidth - 10;
        if (ch.Alpha) At(body, new Checker(6), tx, ty, tw, 8);
        var gradient = new LinearGradientBrush { StartPoint = new RelativePoint(0, .5, RelativeUnit.Relative), EndPoint = new RelativePoint(1, .5, RelativeUnit.Relative) };
        for (var i = 0; i < ch.Stops.Length; i++) gradient.GradientStops.Add(new GradientStop(ch.Stops[i], i / (double)(ch.Stops.Length - 1)));
        At(body, new Border { Background = gradient, BorderBrush = B(p.PanelLine), BorderThickness = new Thickness(1) }, tx - 1, ty - 1, tw + 2, 10);
        var thumb = tx + ch.Fraction * tw;
        At(body, new Drawn((c, _) =>
        {
            c.DrawEllipse(null, new Pen(Brushes.White, 2), new Point(8, 8), 5, 5);
            c.DrawEllipse(null, new Pen(B(p.Ink)), new Point(8, 8), 6.5, 6.5);
        }), thumb - 8, y + 4, 16, 16);
    }
}

/// The hue and saturation wheel: hue around, saturation outward, red to the right.
public static class Wheel
{
    public static unsafe WriteableBitmap Bitmap(int size)
    {
        var bmp = new WriteableBitmap(new PixelSize(size, size), new Vector(96, 96), Avalonia.Platform.PixelFormat.Bgra8888, Avalonia.Platform.AlphaFormat.Premul);
        using var buf = bmp.Lock();
        var r = size / 2.0;
        for (var y = 0; y < size; y++)
        {
            var row = (byte*)buf.Address + y * buf.RowBytes;
            for (var x = 0; x < size; x++)
            {
                var dx = (x + .5 - r) / r;
                var dy = (r - y - .5) / r;
                var dist = Math.Sqrt(dx * dx + dy * dy);
                var a = Math.Clamp((1 - dist) * r, 0, 1);
                var c = Ui.Hsv(Math.Atan2(dy, dx) * 180 / Math.PI, Math.Min(dist, 1), 1);
                row[x * 4] = (byte)(c.B * a);
                row[x * 4 + 1] = (byte)(c.G * a);
                row[x * 4 + 2] = (byte)(c.R * a);
                row[x * 4 + 3] = (byte)(255 * a);
            }
        }
        return bmp;
    }
}
