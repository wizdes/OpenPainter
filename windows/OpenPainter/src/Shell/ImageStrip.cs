// The 64 pixel image strip: ImageStripView.swift. One item per open image, with a 56 by
// 40 preview and its name, an asterisk on a modified image, a 2 pixel outline and a dot on
// the active one, and a close target on hover or focus. The strip scrolls when the items do
// not fit. Only one image can be open until W10, so Close is present and off, and items
// do not reorder yet.
using Avalonia;
using Avalonia.Automation;
using Avalonia.Controls;
using Avalonia.Controls.Primitives;
using Avalonia.Input;
using Avalonia.Layout;
using Avalonia.Media;
using Avalonia.Media.Imaging;
using UiLook;

namespace OpenPainter.Shell;

public sealed record StripImage(string Name, bool Modified, bool Active, Bitmap? Preview, double ImageWidth, double ImageHeight)
{
    public string ShownName => Modified ? $"{Name} *" : Name;
}

public sealed class ImageStrip : Grid
{
    public const double Gap = 8;   // ImageStripView.gap.
    readonly Palette _palette;
    readonly StackPanel _items = new() { Orientation = Orientation.Horizontal, Spacing = Gap, Margin = new Thickness(Gap, 0, Gap, 0) };

    public ScrollViewer Scroller { get; }
    public IReadOnlyList<ImageStripItem> Items => _items.Children.OfType<ImageStripItem>().ToList();

    public ImageStrip(Palette palette)
    {
        _palette = palette;
        Name = "ImageStrip";
        Height = Tokens.StripHeight;
        Background = Ui.B(palette.Chrome);
        AutomationProperties.SetName(this, "Open images");
        Scroller = new ScrollViewer
        {
            HorizontalScrollBarVisibility = ScrollBarVisibility.Hidden,
            VerticalScrollBarVisibility = ScrollBarVisibility.Disabled,
            Content = _items,
            Margin = new Thickness(0, 0, 0, 1),
        };
        Children.Add(Scroller);
        Children.Add(new Border { Height = 1, Background = Ui.B(palette.Line), VerticalAlignment = VerticalAlignment.Bottom });
    }

    public void Show(IEnumerable<StripImage> images)
    {
        _items.Children.Clear();
        foreach (var image in images) _items.Children.Add(new ImageStripItem(image, _palette));
    }
}

public sealed class ImageStripItem : Grid
{
    const double PreviewWidth = 56, PreviewHeight = 40, PreviewTop = 4, NameTop = 45, NameHeight = 17, DotAt = 6, DotSize = 5;
    const double CloseTarget = 24, CloseIcon = 12;

    public StripImage Image { get; }
    public IconCommand CloseButton { get; }

    public ImageStripItem(StripImage image, Palette p)
    {
        Image = image;
        Width = Tokens.ImageStripItemWidth;
        Height = Tokens.StripHeight - 1;
        Focusable = true;
        Background = Brushes.Transparent;
        AutomationProperties.SetName(this, image.Name);
        AutomationProperties.SetHelpText(this, (image.Modified ? "modified" : "saved") + (image.Active ? ", active" : ""));
        ToolTip.SetTip(this, image.Name);

        var canvas = new Avalonia.Controls.Canvas();
        Children.Add(canvas);
        if (image.Active)
        {
            Ui.At(canvas, new Border { Background = Ui.B(p.Wash), BorderBrush = Ui.B(p.Accent), BorderThickness = new Thickness(2), CornerRadius = new CornerRadius(6) }, 0, 0, Width, Height);
            Ui.At(canvas, new Border { Background = Ui.B(p.Accent), CornerRadius = new CornerRadius(DotSize / 2) }, DotAt, DotAt, DotSize, DotSize);
        }

        // Whole pixels, so the preview's edge stays sharp.
        var fit = image.ImageWidth > 0 && image.ImageHeight > 0 ? Math.Min(PreviewWidth / image.ImageWidth, PreviewHeight / image.ImageHeight) : 1;
        var w = Math.Max(1, Math.Round(image.ImageWidth * fit));
        var h = Math.Max(1, Math.Round(image.ImageHeight * fit));
        var x = Math.Round((Width - w) / 2);
        var y = PreviewTop + Math.Round((PreviewHeight - h) / 2);
        if (image.Preview is { } bitmap)
        {
            var preview = new Avalonia.Controls.Image { Source = bitmap, Stretch = Stretch.Fill, Name = "Preview" };
            RenderOptions.SetBitmapInterpolationMode(preview, BitmapInterpolationMode.HighQuality);
            Ui.At(canvas, preview, x, y, w, h);
        }
        else
        {
            Ui.At(canvas, Ui.Box(p.Line), x, y, w, h);
        }
        Ui.At(canvas, new Border { BorderBrush = Ui.B(p.Line), BorderThickness = new Thickness(1) }, x, y, w, h);

        var name = Ui.Label(image.ShownName, p.Ink, weight: image.Active ? Tokens.StrongWeight : FontWeight.Normal);
        name.TextAlignment = TextAlignment.Center;
        name.Name = "ImageName";
        Ui.At(canvas, name, 2, NameTop, Width - 4, NameHeight);

        CloseButton = new IconCommand(p, "window-close", "Close image", target: CloseTarget, iconSize: CloseIcon, enabled: false)
        {
            HorizontalAlignment = HorizontalAlignment.Right,
            VerticalAlignment = VerticalAlignment.Top,
            IsVisible = false,
        };
        Children.Add(CloseButton);

        var surface = image.Active ? p.Wash : p.Chrome;
        void Fill(bool down) => Background = down ? Ui.B(ControlStates.Pressed(p, surface)) : IsPointerOver ? Ui.B(ControlStates.Hover(p, surface)) : Brushes.Transparent;
        PointerEntered += (_, _) =>
        {
            UpdateClose();
            Fill(false);
        };
        PointerExited += (_, _) =>
        {
            UpdateClose();
            Fill(false);
        };
        PointerPressed += (_, _) => Fill(true);
        PointerReleased += (_, _) => Fill(false);
        GotFocus += (_, _) => UpdateClose();
        LostFocus += (_, _) => UpdateClose();
    }

    /// The close target shows on hover or focus.
    void UpdateClose() => CloseButton.IsVisible = IsPointerOver || IsFocused || IsKeyboardFocusWithin;
}
