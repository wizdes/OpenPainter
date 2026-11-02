// The window: title row with the menu bar and caption buttons, tool options row, image
// strip, stage with the canvas, status bar and panels. Positions match the Mac shots:
// the 1280 by 832 window sits 48 pixels in from each edge of the 1376 by 928 picture.
using Avalonia;
using Avalonia.Controls;
using Avalonia.Controls.Shapes;
using Avalonia.Layout;
using Avalonia.Media;
using Avalonia.Media.Imaging;
using static UiLook.Ui;

namespace UiLook;

public static class Shell
{
    /// The image rectangle, relative to the window: PanelLayout.clearRect's result for
    /// these panels, 4096 by 3072 fitted into it.
    public static readonly Rect CanvasRect = new(272, 185.25, 754, 565.5);
    const double StageTop = Tokens.TitleHeight + Tokens.OptionsHeight + Tokens.StripHeight;
    const double StageBottom = Tokens.WindowHeight - Tokens.StatusHeight;

    /// `omitPanel` leaves one panel out, by title. Only tests use it, to prove they notice.
    public static Control Build(Palette p, Scene s, Bitmap image, Bitmap canvas, string? omitPanel = null)
    {
        var picture = new Canvas { Width = Tokens.ShotWidth, Height = Tokens.ShotHeight, Background = Brushes.Transparent };
        // The window's shadow, then the window itself, clipped to its corners.
        At(picture, new Border
        {
            CornerRadius = new CornerRadius(Tokens.WindowCorner),
            Background = B(p.Stage),
            BoxShadow = new BoxShadows(new BoxShadow { OffsetY = 8, Blur = 24, Color = Tokens.Shadow }),
        }, Tokens.ShotMargin, Tokens.ShotMargin, Tokens.WindowWidth, Tokens.WindowHeight);
        var win = new Canvas { Width = Tokens.WindowWidth, Height = Tokens.WindowHeight };
        At(picture, new Border { CornerRadius = new CornerRadius(Tokens.WindowCorner), ClipToBounds = true, Child = win },
            Tokens.ShotMargin, Tokens.ShotMargin, Tokens.WindowWidth, Tokens.WindowHeight);

        At(win, Box(p.Stage), 0, StageTop, Tokens.WindowWidth, StageBottom - StageTop);
        At(win, TitleRow(p, s), 0, 0, Tokens.WindowWidth, Tokens.TitleHeight);
        At(win, OptionsRow(p, s), 0, Tokens.TitleHeight, Tokens.WindowWidth, Tokens.OptionsHeight);
        At(win, Strip(p, image), 0, Tokens.TitleHeight + Tokens.OptionsHeight, Tokens.WindowWidth, Tokens.StripHeight);
        At(win, StatusBar(p, s), 0, StageBottom, Tokens.WindowWidth, Tokens.StatusHeight);

        var img = new Image { Source = canvas, Stretch = Stretch.Fill };
        RenderOptions.SetBitmapInterpolationMode(img, BitmapInterpolationMode.HighQuality);
        At(win, img, CanvasRect.X, CanvasRect.Y, CanvasRect.Width, CanvasRect.Height);
        CanvasOverlays(win, p, s);

        if (s.Dialog != DialogKind.None) At(win, Box(Tokens.Scrim), 0, 0, Tokens.WindowWidth, Tokens.WindowHeight);

        // Panels sit above the dim, as the Mac's separate panel windows do.
        const double gap = Tokens.PanelClearance;
        if (omitPanel != "Tools") At(win, Panels.Tools(p, s), gap, StageTop + gap);
        if (omitPanel != "Colors") At(win, Panels.Colors(p, s, expanded: false), gap, StageBottom - gap - Tokens.ColorsHeight);
        if (omitPanel != "History") At(win, Panels.History(p, s), Tokens.WindowWidth - gap - Tokens.ListPanelWidth, StageTop + gap);
        if (omitPanel != "Layers") At(win, Panels.Layers(p, s, image), Tokens.WindowWidth - gap - Tokens.ListPanelWidth, StageBottom - gap - Tokens.ListPanelHeight);

        if (s.Dialog == DialogKind.Curves) Dialogs.Curves(win, p);
        if (s.Dialog == DialogKind.HueSaturation) Dialogs.HueSaturation(win, p);
        return picture;
    }

    static Control TitleRow(Palette p, Scene s)
    {
        var grid = new Grid { Background = B(p.TitleBar), ColumnDefinitions = new ColumnDefinitions("Auto,*,Auto") };
        var menu = new Menu { VerticalAlignment = VerticalAlignment.Center, Margin = new Thickness(6, 0, 0, 0), Background = Brushes.Transparent };
        // The Mac's menu bar order. The Mac's app menu has no Windows equal; see docs/windows/w6-look.md.
        foreach (var title in new[] { "File", "Edit", "View", "Image", "Layers", "Adjustments", "Effects", "Window", "Help" })
            menu.Items.Add(new MenuItem { Header = title, FontSize = Tokens.FontBody, Foreground = B(p.Ink), Padding = new Thickness(8, 5), MinHeight = 0 });
        grid.Children.Add(menu);

        var title2 = Row(6);
        title2.HorizontalAlignment = HorizontalAlignment.Center;
        if (s.Dirty) title2.Children.Add(new Border { Width = 6, Height = 6, CornerRadius = new CornerRadius(3), Background = B(p.Muted), VerticalAlignment = VerticalAlignment.Center });
        title2.Children.Add(Label(Scenes.Title, p.Muted, weight: FontWeight.SemiBold));
        Grid.SetColumn(title2, 1);
        grid.Children.Add(title2);

        var caption = Row();
        caption.Children.Add(Caption(p, c => c.DrawLine(new Pen(B(p.Ink)), new Point(0, 5), new Point(10, 5))));
        caption.Children.Add(Caption(p, c => c.DrawRectangle(new Pen(B(p.Ink)), new Rect(.5, .5, 9, 9), 1.5f)));
        caption.Children.Add(Caption(p, c =>
        {
            var pen = new Pen(B(p.Ink));
            c.DrawLine(pen, new Point(0, 0), new Point(10, 10));
            c.DrawLine(pen, new Point(10, 0), new Point(0, 10));
        }));
        Grid.SetColumn(caption, 2);
        grid.Children.Add(caption);
        return grid;
    }

    static Control Caption(Palette p, Action<DrawingContext> draw) =>
        new Grid
        {
            Width = Tokens.CaptionButtonWidth,
            Height = Tokens.TitleHeight,
            Children =
            {
                new Drawn((c, _) => draw(c)) { Width = 10, Height = 10, HorizontalAlignment = HorizontalAlignment.Center, VerticalAlignment = VerticalAlignment.Center },
            },
        };

    static Control OptionsRow(Palette p, Scene s)
    {
        var root = new Grid { Background = B(p.Options), ColumnDefinitions = new ColumnDefinitions("Auto,*,Auto"), ClipToBounds = true };
        root.Children.Add(new Border { Height = 1, Background = B(p.Line), VerticalAlignment = VerticalAlignment.Bottom, [Grid.ColumnSpanProperty] = 3 });

        var commands = Row();
        commands.Margin = new Thickness(12, 0, 0, 1);
        void Add(string icon, bool enabled, double after, bool on = false)
        {
            var c = IconButton(p, icon, enabled ? p.Ink : p.Disabled, on: on);
            c.Margin = new Thickness(0, 0, after, 0);
            commands.Children.Add(c);
        }
        // Save is off in every Mac shot. The wand button is pressed while the wand is the tool.
        var enabled = new[] { true, true, false, s.UndoEnabled, false, s.CropEnabled, true, true };
        var gaps = new[] { Tokens.CommandGap, Tokens.CommandGap, Tokens.CommandGroupGap, Tokens.CommandGap, Tokens.CommandGroupGap, Tokens.CommandGap, Tokens.CommandGap, Tokens.CommandGroupGap };
        for (var i = 0; i < Scenes.BarCommands.Length; i++)
        {
            var icon = Scenes.BarCommands[i].Icon;
            Add(icon, enabled[i], gaps[i], on: icon == "tool-magic-wand" && s.ToolIcon == icon);
        }
        commands.Children.Add(new Border { Width = 1, Height = 20, Background = B(p.Line), VerticalAlignment = VerticalAlignment.Center });
        root.Children.Add(commands);

        var row = Row(Tokens.OptionItemGap);
        row.Margin = new Thickness(12, 0, 0, 1);
        row.HorizontalAlignment = HorizontalAlignment.Left;
        Control Gap() => new Border { Width = Tokens.OptionGroupGap - 2 * Tokens.OptionItemGap };
        void Items(params Control[] items) { foreach (var i in items) row.Children.Add(i); }
        TextBlock L(string t) => Label(t, p.Muted);

        var toolLabel = s.Options switch { Options.Text => "Text", Options.Selection => "Ellipse Select", Options.Flood => "Magic Wand", _ => "Paintbrush" };
        var pickerContent = Row(6);
        pickerContent.Children.Add(new IconView(s.ToolIcon, p.Ink));
        pickerContent.Children.Add(Label(toolLabel, p.Ink));
        Items(Choice(p, pickerContent, 170, padLeft: 8), Gap());

        switch (s.Options)
        {
            case Options.Brush:
                Items(L("Brush width"), IconButton(p, "opt-minus", p.Ink, iconSize: Tokens.SmallIconSize), Field(p, "2", 52, Tokens.OptionHeight), IconButton(p, "opt-plus", p.Ink, iconSize: Tokens.SmallIconSize), L("px"), Gap());
                Items(L("Hardness"), Field(p, "100", 52, Tokens.OptionHeight), L("%"), Gap());
                Items(L("Antialiasing"), Check(p, "On"), Gap());
                Items(L("Blend mode"), Choice(p, "Normal", 102), Gap());
                Items(L("Opacity"), Field(p, "100", 52, Tokens.OptionHeight), L("%"), Gap());
                Items(L("Pen pressure"), Check(p, "Size"), Check(p, "Opacity"));
                break;
            case Options.Text:
                var size = Choice(p, "144", 64, Tokens.OptionHeight, padLeft: 8);   // A combo box with its arrow, as on the Mac.
                size.Background = B(p.Field);
                Items(L("Font"), Choice(p, "Segoe UI", 170), size, L("pt"), Gap());
                Items(IconButton(p, "text-bold", p.Ink, on: true), IconButton(p, "text-italic", p.Ink), IconButton(p, "text-underline", p.Ink), IconButton(p, "text-strikethrough", p.Ink), Gap());
                Items(IconButton(p, "text-align-left", p.Ink, on: true), IconButton(p, "text-align-center", p.Ink), IconButton(p, "text-align-right", p.Ink), Gap());
                Items(L("Antialiasing"), Check(p, "On"), Gap());
                Items(L("Blend mode"), Choice(p, "Normal", 102), Gap());
                Items(Push(p, "Finish", 64));
                break;
            case Options.Selection:
                Items(L("Selection mode"), Choice(p, "Replace", 97), Gap());
                Items(L("Style"), Choice(p, "Normal", 110));
                break;
            case Options.Flood:
                Items(L("Tolerance"), Slide(p, 0, 100, 50, 96), Field(p, "50", 52, Tokens.OptionHeight), L("%"), Gap());
                Items(L("Tolerance alpha"), Choice(p, "Premultiplied", 124), Gap());
                Items(L("Selection mode"), Choice(p, "Replace", 97), Gap());
                Items(L("Sampling"), Choice(p, "Layer", 90));
                break;
        }
        var clip = new Border { ClipToBounds = true, Child = row };
        Grid.SetColumn(clip, 1);
        root.Children.Add(clip);

        // When the options do not fit, a visible button at the row's end says there is more.
        // W7 makes it open the rest; see docs/windows/w6-look.md.
        if (s.Options != Options.Selection)
        {
            var more = new Grid { Width = Tokens.OverflowWidth, Margin = new Thickness(0, 0, 0, 1), Background = B(p.Options) };
            more.Children.Add(new Border { Width = 1, Height = 20, Background = B(p.Line), HorizontalAlignment = HorizontalAlignment.Left, VerticalAlignment = VerticalAlignment.Center });
            more.Children.Add(new IconView("opt-overflow", p.Ink, Tokens.SmallIconSize) { HorizontalAlignment = HorizontalAlignment.Center, VerticalAlignment = VerticalAlignment.Center });
            Grid.SetColumn(more, 2);
            root.Children.Add(more);
        }
        return root;
    }

    static Control Strip(Palette p, Bitmap image)
    {
        var strip = new Canvas { Background = B(p.Chrome) };
        At(strip, Box(p.Line), 0, Tokens.StripHeight - 1, Tokens.WindowWidth, 1);
        const double x = 8, h = 63;
        At(strip, new Border { Background = B(p.Wash), BorderBrush = B(p.Accent), BorderThickness = new Thickness(2), CornerRadius = new CornerRadius(6) }, x, 0, Tokens.ImageStripItemWidth, h);
        At(strip, new Border { Background = B(p.Accent), CornerRadius = new CornerRadius(3) }, x + 6, 6, 5, 5);
        const double pw = 53.3, ph = 40;
        var px = x + (Tokens.ImageStripItemWidth - pw) / 2;
        var thumb = new Image { Source = image, Stretch = Stretch.Fill };
        RenderOptions.SetBitmapInterpolationMode(thumb, BitmapInterpolationMode.HighQuality);
        At(strip, thumb, px, 4, pw, ph);
        var name = Label(Scenes.StripTitle, p.Ink, weight: Tokens.StrongWeight);
        name.TextAlignment = TextAlignment.Center;
        At(strip, name, x + 2, 45, Tokens.ImageStripItemWidth - 4, 17);
        return strip;
    }

    static Control StatusBar(Palette p, Scene s)
    {
        var grid = new Grid { Background = B(p.Chrome), ColumnDefinitions = new ColumnDefinitions("*,Auto") };
        grid.Children.Add(new Border { Height = 1, Background = B(p.Line), VerticalAlignment = VerticalAlignment.Top, [Grid.ColumnSpanProperty] = 2 });
        var hint = Label(s.StatusHint, p.Muted);
        hint.Margin = new Thickness(10, 0, 10, 0);
        grid.Children.Add(hint);

        var right = Row(10);
        right.Margin = new Thickness(0, 0, 10, 0);
        void Pair(string label, string value)
        {
            var pair = Row(4);
            pair.Children.Add(Label(label, p.Muted));
            pair.Children.Add(Label(value, p.Ink));
            right.Children.Add(pair);
        }
        Pair("Selection", s.SelectionText);
        Pair("Cursor", "–");
        Pair("Image", "4096 × 3072 px");
        Pair("Profile", "sRGB (none)");
        var units = Row(4);
        units.Children.Add(Label("Units", p.Muted));
        units.Children.Add(Choice(p, "px", 58, 22));
        right.Children.Add(units);
        right.Children.Add(Field(p, "18%", 56));
        right.Children.Add(Push(p, "Fit", 34));
        right.Children.Add(Push(p, "100%", 52));
        right.Children.Add(Slide(p, 0, 1, .38, 96));
        Grid.SetColumn(right, 1);
        grid.Children.Add(right);
        return grid;
    }

    static void CanvasOverlays(Canvas win, Palette p, Scene s)
    {
        // Window-relative. Measured from the Mac shots: layout pixel = shot pixel / 2 - 48.
        const double textX = 320, textY = 214;
        if (s.Canvas is CanvasFx.None or CanvasFx.Text)
        {
            var text = new TextBlock
            {
                Text = "Spring in Holland",
                FontFamily = new FontFamily(Tokens.FontFamily),
                FontSize = Tokens.FontCanvasText,
                FontWeight = FontWeight.Bold,
                Foreground = B(Tokens.TextOnCanvas),
            };
            At(win, text, textX, textY - 5);
        }
        if (s.Canvas == CanvasFx.Text)
        {
            At(win, Ants(new Rect(.5, .5, 304, 54), ellipse: false), 317, 210, 305, 55);
            At(win, Box(Colors.White), 618, 212, 1, 51);                // The caret.
            At(win, new Border { Background = B(p.Handle), BorderBrush = Brushes.White, BorderThickness = new Thickness(1), CornerRadius = new CornerRadius(2) }, 622, 263, 14, 14);
        }
        if (s.Canvas == CanvasFx.Selection)
        {
            // 1200 by 900 image pixels at the canvas zoom, as in the Mac shot.
            const double x = 548.5, y = 415, w = 221, h = 165.7;
            At(win, Ants(new Rect(.5, .5, w, h), ellipse: true), x, y, w + 1, h + 1);
            foreach (var (fx, fy) in new[] { (0.0, 0.0), (.5, 0.0), (1.0, 0.0), (0.0, .5), (1.0, .5), (0.0, 1.0), (.5, 1.0), (1.0, 1.0) })
                At(win, new Border { Background = Brushes.White, BorderBrush = B(p.Accent), BorderThickness = new Thickness(1) }, x + fx * w - 3, y + fy * h - 3, 7, 7);
        }
    }

    /// A marching-ants outline, visible on any color.
    static Control Ants(Rect r, bool ellipse) => new Drawn((c, _) =>
    {
        // A solid white line with black dashes on it, as the Mac draws it.
        var pens = new[] { new Pen(B(Colors.White), 1), new Pen(B(Colors.Black), 1, new DashStyle(new[] { Tokens.SelectionDash, Tokens.SelectionDash }, 0)) };
        foreach (var pen in pens)
        {
            if (ellipse) c.DrawEllipse(null, pen, r.Center, r.Width / 2, r.Height / 2);
            else c.DrawRectangle(pen, r);
        }
    });

    /// The Colors panel alone, as the Mac's panel-colors shot shows it.
    public static Control ColorsExpanded(Palette p)
    {
        var picture = new Canvas { Width = 308, Height = 534, Background = Brushes.Transparent };
        At(picture, Panels.Colors(p, null, expanded: true), 32, 32);
        return picture;
    }
}
