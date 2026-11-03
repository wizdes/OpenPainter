// The Colors panel: mac/OpenPainterKit/Sources/Shell/ColorsPanel.swift, drawn as W6's Panels.Colors.
//
// - Primary and secondary swatches. The active slot has the notch, and clicking the other swatch
//   moves it. Swap and reset to black and white.
// - The hex field and the wheel edit the active slot. A wheel right-click sets the other slot.
//   Wheel keys: Ctrl keeps the radius, Alt keeps the spoke, Shift snaps to 15 degree spokes.
// - More and Less show the RGB, HSV and alpha sliders with fields.
// - The palette: 32 swatches compact, all 96 when expanded, scrolling. A click sets the active
//   slot, and a right-click or Shift-click the other. The palette menu loads, saves and resets.
//
// Swatch and palette bytes are document colors, drawn the same in both themes. Only frames, text
// and markers follow the theme.
using System.Text;
using Avalonia;
using Avalonia.Automation;
using Avalonia.Controls;
using Avalonia.Controls.Primitives;
using Avalonia.Input;
using Avalonia.Interactivity;
using Avalonia.Layout;
using Avalonia.Media;
using Avalonia.Media.Imaging;
using Avalonia.Platform;
using Avalonia.Platform.Storage;
using OpenPainter.Catalog;
using UiLook;

namespace OpenPainter.Shell;

public enum ColorChannel { Red, Green, Blue, Hue, Saturation, Value, Alpha }

/// What the panel keeps across a theme change, which rebuilds it.
public sealed class ColorsMemory
{
    /// The HSV of the active color. Kept across edits, so a grey or black color does not lose
    /// its hue and saturation.
    public Hsv Hsv { get; set; } = new(0, 0, 0);
    public bool Expanded { get; set; }
    public bool AddMode { get; set; }

    /// The last palette report, for the panel's tip and tests.
    public string? LastReport { get; set; }
}

/// Exact document-color drawing. A color with alpha below 255 shows over a small checkerboard.
static class ColorDrawing
{
    const double Cell = 6;
    static readonly Color Light = Colors.White, Dark = Color.FromRgb(204, 204, 204);

    public static void Swatch(DrawingContext context, DocColor c, Rect r)
    {
        if (c.A < 255) Checker(context, r);
        context.FillRectangle(new SolidColorBrush(c.Exact), r);
    }

    public static void Checker(DrawingContext context, Rect r)
    {
        context.FillRectangle(new SolidColorBrush(Light), r);
        var dark = new SolidColorBrush(Dark);
        var row = 0;
        for (var y = r.Top; y < r.Bottom; y += Cell, row++)
            for (var x = r.Left + (row % 2 == 0 ? 0 : Cell); x < r.Right; x += 2 * Cell)
                context.FillRectangle(dark, new Rect(x, y, Math.Min(Cell, r.Right - x), Math.Min(Cell, r.Bottom - y)));
    }
}

/// Hover and press on the drawn controls that have no fill of their own: an outline, in Ink and
/// then in the accent. A swatch's own color must not change, because it is document color.
static class DrawnStates
{
    public static void Outline(DrawingContext context, Palette p, Rect bounds, bool hover, bool pressed)
    {
        if (!hover && !pressed) return;
        context.DrawRectangle(new Pen(Ui.B(pressed ? p.Accent : p.Ink), 1.5), bounds.Deflate(.75));
    }
}

/// The primary (top left) and secondary (bottom right) swatches. The notch marks the active slot.
public sealed class SwatchPair : Control
{
    public static readonly Rect PrimaryRect = new(0, 0, 32, 32), SecondaryRect = new(22, 22, 32, 32);

    static SwatchPair() => AffectsRender<SwatchPair>(IsPointerOverProperty);
    readonly Palette _palette;
    readonly ToolState _tools;
    bool _pressed;

    public event Action<ColorSlot>? Picked;

    public SwatchPair(Palette p, ToolState tools)
    {
        _palette = p;
        _tools = tools;
        Width = 54;
        Height = 54;
        Focusable = true;
        AutomationProperties.SetName(this, "Primary and secondary colors");
        AutomationProperties.SetHelpText(this, "Press to make the other color active");
    }

    /// The slot under a point. The primary square is on top.
    public static ColorSlot? Slot(Point p)
    {
        if (PrimaryRect.Contains(p)) return ColorSlot.Primary;
        if (SecondaryRect.Contains(p)) return ColorSlot.Secondary;
        return null;
    }

    public override void Render(DrawingContext context)
    {
        // Pointer tests need something to hit in the swatches' gap too.
        context.FillRectangle(Brushes.Transparent, new Rect(Bounds.Size));
        foreach (var (r, slot) in new[] { (SecondaryRect, ColorSlot.Secondary), (PrimaryRect, ColorSlot.Primary) })
        {
            ColorDrawing.Swatch(context, _tools.Color(slot), r);
            context.DrawRectangle(new Pen(Ui.B(_palette.PanelLine)), r.Deflate(.5));
            if (slot != _tools.ActiveSlot) continue;
            context.DrawGeometry(Ui.B(_palette.Ink), null, new PolylineGeometry(new[] { r.TopLeft, r.TopLeft + new Point(9, 0), r.TopLeft + new Point(0, 9) }, true));
        }
        DrawnStates.Outline(context, _palette, new Rect(Bounds.Size), IsPointerOver, _pressed);
    }

    protected override void OnPointerPressed(PointerPressedEventArgs e)
    {
        base.OnPointerPressed(e);
        if (!e.GetCurrentPoint(this).Properties.IsLeftButtonPressed) return;
        _pressed = true;
        InvalidateVisual();
        if (Slot(e.GetPosition(this)) is { } slot) Picked?.Invoke(slot);
        e.Handled = true;
    }

    protected override void OnPointerReleased(PointerReleasedEventArgs e)
    {
        base.OnPointerReleased(e);
        _pressed = false;
        InvalidateVisual();
    }

    protected override void OnPointerCaptureLost(PointerCaptureLostEventArgs e)
    {
        base.OnPointerCaptureLost(e);
        _pressed = false;
        InvalidateVisual();
    }

    protected override void OnKeyDown(KeyEventArgs e)
    {
        base.OnKeyDown(e);
        if (e.Key is not (Key.Space or Key.Enter)) return;
        Picked?.Invoke(ToolState.Other(_tools.ActiveSlot));
        e.Handled = true;
    }
}

/// The HSV wheel: hue around, saturation outward, drawn at full value. The ring marker shows the
/// active color's hue and saturation.
public sealed class ColorWheel : Control
{
    public const double Diameter = 52;

    static ColorWheel() => AffectsRender<ColorWheel>(IsPointerOverProperty);
    static readonly Dictionary<int, WriteableBitmap> Images = [];
    readonly Palette _palette;
    bool _rightDrag, _down;
    Hsv _hsv = new(0, 0, 100);

    /// dx and dy in radius units with y up, the modifiers, and whether it was the right button.
    public event Action<double, double, WheelModifiers, bool>? Picked;

    public ColorWheel(Palette p)
    {
        _palette = p;
        Width = Diameter;
        Height = Diameter;
        Focusable = true;
        AutomationProperties.SetName(this, "Color wheel");
        ToolTip.SetTip(this, "Click: active color. Right-click: the other color. Ctrl keeps saturation, Alt keeps hue, Shift snaps to 15° spokes.");
    }

    public Hsv Hsv
    {
        get => _hsv;
        set
        {
            if (_hsv == value) return;
            _hsv = value;
            AutomationProperties.SetItemStatus(this, $"hue {Math.Round(value.H, MidpointRounding.AwayFromZero)}, saturation {Math.Round(value.S, MidpointRounding.AwayFromZero)}");
            InvalidateVisual();
        }
    }

    /// A square image of the disc: top rows first, BGRA, premultiplied, rim antialiased.
    public static byte[] DiscBytes(int size)
    {
        var bytes = new byte[size * size * 4];
        var r = size / 2.0;
        for (var y = 0; y < size; y++)
        {
            for (var x = 0; x < size; x++)
            {
                double dx = (x + .5 - r) / r, dy = (r - y - .5) / r, d = Math.Sqrt(dx * dx + dy * dy);
                if (d > 1 + 1 / r) continue;
                var h = Math.Atan2(dy, dx) * 180 / Math.PI;
                if (h < 0) h += 360;
                var c = new Hsv(h, Math.Min(d, 1) * 100, 100).Color();
                var a = Math.Clamp((1 - d) * r + .5, 0, 1);
                var i = (y * size + x) * 4;
                bytes[i] = (byte)Math.Round(c.B * a, MidpointRounding.AwayFromZero);
                bytes[i + 1] = (byte)Math.Round(c.G * a, MidpointRounding.AwayFromZero);
                bytes[i + 2] = (byte)Math.Round(c.R * a, MidpointRounding.AwayFromZero);
                bytes[i + 3] = (byte)Math.Round(a * 255, MidpointRounding.AwayFromZero);
            }
        }
        return bytes;
    }

    static WriteableBitmap Image(int size)
    {
        if (Images.TryGetValue(size, out var ready)) return ready;
        var bitmap = new WriteableBitmap(new PixelSize(size, size), new Vector(96, 96), PixelFormat.Bgra8888, AlphaFormat.Premul);
        var bytes = DiscBytes(size);
        using (var locked = bitmap.Lock())
            for (var row = 0; row < size; row++)
                System.Runtime.InteropServices.Marshal.Copy(bytes, row * size * 4, locked.Address + row * locked.RowBytes, size * 4);
        Images[size] = bitmap;
        return bitmap;
    }

    public override void Render(DrawingContext context)
    {
        var scale = (VisualRoot as TopLevel)?.RenderScaling ?? 1;
        var bounds = new Rect(Bounds.Size);
        var image = Image((int)Math.Round(Bounds.Width * scale));
        context.DrawImage(image, new Rect(image.Size), bounds);
        context.DrawEllipse(null, new Pen(Ui.B(_palette.PanelLine)), bounds.Deflate(.5));
        // The ring marker: a dark ring inside a light ring, visible on any hue.
        var (mx, my) = ColorWheelMath.Point(_hsv);
        var r = Bounds.Width / 2;
        var c = new Point(r + mx * r, r - my * r);
        context.DrawEllipse(null, new Pen(Brushes.White, 2), c, 5, 5);
        context.DrawEllipse(null, new Pen(Brushes.Black, 2), c, 3.2, 3.2);
        // Hover and press: the rim grows stronger, over the disc's edge.
        if (IsPointerOver || _down) context.DrawEllipse(null, new Pen(Ui.B(_down ? _palette.Accent : _palette.Ink), 2), bounds.Center, r - 1, r - 1);
    }

    /// Windows keys to the paint.net wheel modifiers.
    public static WheelModifiers Modifiers(KeyModifiers keys)
    {
        var m = WheelModifiers.None;
        if (keys.HasFlag(KeyModifiers.Control)) m |= WheelModifiers.KeepRadius;
        if (keys.HasFlag(KeyModifiers.Alt)) m |= WheelModifiers.KeepSpoke;
        if (keys.HasFlag(KeyModifiers.Shift)) m |= WheelModifiers.SnapSpokes;
        return m;
    }

    void Pick(Point at, KeyModifiers keys)
    {
        var r = Bounds.Width / 2;
        Picked?.Invoke((at.X - r) / r, (r - at.Y) / r, Modifiers(keys), _rightDrag);
    }

    protected override void OnPointerPressed(PointerPressedEventArgs e)
    {
        base.OnPointerPressed(e);
        var point = e.GetCurrentPoint(this).Properties;
        if (!point.IsLeftButtonPressed && !point.IsRightButtonPressed) return;
        _rightDrag = point.IsRightButtonPressed;
        _down = true;
        InvalidateVisual();
        e.Pointer.Capture(this);
        Pick(e.GetPosition(this), e.KeyModifiers);
        e.Handled = true;
    }

    protected override void OnPointerMoved(PointerEventArgs e)
    {
        base.OnPointerMoved(e);
        if (_down) Pick(e.GetPosition(this), e.KeyModifiers);
    }

    protected override void OnPointerReleased(PointerReleasedEventArgs e)
    {
        base.OnPointerReleased(e);
        _down = false;
        InvalidateVisual();
        e.Pointer.Capture(null);
    }

    protected override void OnPointerCaptureLost(PointerCaptureLostEventArgs e)
    {
        base.OnPointerCaptureLost(e);
        _down = false;
        InvalidateVisual();
    }

    /// Left and right turn the hue by 1 degree (Shift: 15). Up and down change the saturation by
    /// 1 (Shift: 10).
    protected override void OnKeyDown(KeyEventArgs e)
    {
        base.OnKeyDown(e);
        var big = e.KeyModifiers.HasFlag(KeyModifiers.Shift);
        var (h, s) = (_hsv.H, _hsv.S);
        switch (e.Key)
        {
            case Key.Left: h -= big ? 15 : 1; break;
            case Key.Right: h += big ? 15 : 1; break;
            case Key.Down: s -= big ? 10 : 1; break;
            case Key.Up: s += big ? 10 : 1; break;
            default: return;
        }
        h = (h % 360 + 360) % 360;
        s = Math.Clamp(s, 0, 100);
        var a = h * Math.PI / 180;
        _rightDrag = false;
        Picked?.Invoke(Math.Cos(a) * s / 100, Math.Sin(a) * s / 100, WheelModifiers.None, false);
        e.Handled = true;
    }
}

/// A horizontal slider whose track shows the channel's colors and whose thumb is a ring. A drag
/// or click sets the value, and arrow keys step by one.
public sealed class GradientSlider : Control
{
    static GradientSlider() => AffectsRender<GradientSlider>(IsPointerOverProperty);

    readonly Palette _palette;
    bool _down;
    double _value;
    IReadOnlyList<DocColor> _stops = [];

    public double Minimum { get; }
    public double Maximum { get; }
    public bool Checkered { get; init; }

    public event Action<double>? Changed;

    public GradientSlider(Palette p, double minimum, double maximum)
    {
        _palette = p;
        Minimum = minimum;
        Maximum = maximum;
        Focusable = true;
    }

    public double Value
    {
        get => _value;
        set
        {
            if (_value == value) return;
            _value = value;
            AutomationProperties.SetItemStatus(this, ((int)Math.Round(value, MidpointRounding.AwayFromZero)).ToString());
            InvalidateVisual();
        }
    }

    public IReadOnlyList<DocColor> Stops
    {
        get => _stops;
        set
        {
            if (_stops.SequenceEqual(value)) return;
            _stops = value;
            InvalidateVisual();
        }
    }

    public Rect Track => new(5, 8, Math.Max(Bounds.Width - 10, 1), 8);

    public override void Render(DrawingContext context)
    {
        var t = Track;
        context.FillRectangle(Brushes.Transparent, new Rect(Bounds.Size));
        if (Checkered) ColorDrawing.Checker(context, t);
        if (_stops.Count >= 2)
        {
            var gradient = new LinearGradientBrush { StartPoint = new RelativePoint(0, .5, RelativeUnit.Relative), EndPoint = new RelativePoint(1, .5, RelativeUnit.Relative) };
            for (var i = 0; i < _stops.Count; i++) gradient.GradientStops.Add(new GradientStop(_stops[i].Exact, i / (double)(_stops.Count - 1)));
            context.FillRectangle(gradient, t);
        }
        context.DrawRectangle(new Pen(Ui.B(_palette.PanelLine)), t.Inflate(.5));
        var f = Math.Clamp((_value - Minimum) / (Maximum - Minimum), 0, 1);
        var center = new Point(t.X + f * t.Width, Bounds.Height / 2);
        context.DrawEllipse(null, new Pen(Brushes.White, 2), center, 5, 5);
        context.DrawEllipse(null, new Pen(Ui.B(_down ? _palette.Accent : _palette.Ink), IsPointerOver || _down ? 2 : 1), center, 6.5, 6.5);
    }

    public double ValueAt(double x)
    {
        var f = Math.Clamp((x - Track.X) / Track.Width, 0, 1);
        return Math.Round(Minimum + f * (Maximum - Minimum), MidpointRounding.AwayFromZero);
    }

    void Drag(PointerEventArgs e)
    {
        var v = ValueAt(e.GetPosition(this).X);
        if (v != _value) Changed?.Invoke(v);
    }

    protected override void OnPointerPressed(PointerPressedEventArgs e)
    {
        base.OnPointerPressed(e);
        if (!e.GetCurrentPoint(this).Properties.IsLeftButtonPressed) return;
        _down = true;
        InvalidateVisual();
        e.Pointer.Capture(this);
        Drag(e);
        e.Handled = true;
    }

    protected override void OnPointerMoved(PointerEventArgs e)
    {
        base.OnPointerMoved(e);
        if (_down) Drag(e);
    }

    protected override void OnPointerReleased(PointerReleasedEventArgs e)
    {
        base.OnPointerReleased(e);
        _down = false;
        InvalidateVisual();
        e.Pointer.Capture(null);
    }

    protected override void OnPointerCaptureLost(PointerCaptureLostEventArgs e)
    {
        base.OnPointerCaptureLost(e);
        _down = false;
        InvalidateVisual();
    }

    protected override void OnKeyDown(KeyEventArgs e)
    {
        base.OnKeyDown(e);
        switch (e.Key)
        {
            case Key.Left or Key.Down: Changed?.Invoke(Math.Max(_value - 1, Minimum)); break;
            case Key.Right or Key.Up: Changed?.Invoke(Math.Min(_value + 1, Maximum)); break;
            default: return;
        }
        e.Handled = true;
    }
}

/// One More row: a label, a gradient slider and a number field.
public sealed class ChannelRow : Avalonia.Controls.Canvas
{
    readonly Action _refused;
    readonly double _rowWidth;
    string _shown = "";

    public ColorChannel Channel { get; }
    public GradientSlider Slider { get; }
    public TextBox Field { get; }
    public event Action<double>? Changed;

    /// Enter finished the field: GradientSliderRow's makeFirstResponder(nil) on the Mac.
    public event Action? Entered;

    public static string Label(ColorChannel c) => "RGBHSVA"[(int)c].ToString();
    public static string Title(ColorChannel c) => c.ToString();
    public static double Maximum(ColorChannel c) => c switch { ColorChannel.Hue => 360, ColorChannel.Saturation or ColorChannel.Value => 100, _ => 255 };

    public ChannelRow(Palette p, ColorChannel channel, double rowWidth, Action refused)
    {
        Channel = channel;
        _rowWidth = rowWidth;
        _refused = refused;
        Width = rowWidth;
        Height = 24;
        var label = Ui.At(this, Ui.Label(Label(channel), p.Muted, weight: FontWeight.SemiBold), 0, 4, 14, 16);
        label.IsHitTestVisible = false;
        Slider = new GradientSlider(p, 0, Maximum(channel)) { Checkered = channel == ColorChannel.Alpha };
        AutomationProperties.SetName(Slider, Title(channel));
        Slider.Changed += v => Changed?.Invoke(v);
        Ui.At(this, Slider, 18, 0, rowWidth - 18 - 50, 24);
        var field = Ui.Field(p, "0", 44);
        Field = ((Grid)field).Children.OfType<TextBox>().Single();
        AutomationProperties.SetName(Field, Title(channel));
        Ui.At(this, field, _rowWidth - 44, 0);
        Field.KeyDown += (_, e) =>
        {
            if (e.Key != Key.Enter) return;
            Commit();
            Entered?.Invoke();
            e.Handled = true;
        };
        Field.LostFocus += (_, _) => Commit();
    }

    public void Show(double value, IReadOnlyList<DocColor> gradient)
    {
        Slider.Value = value;
        Slider.Stops = gradient;
        if (!Field.IsFocused) ShowText();
    }

    void ShowText()
    {
        _shown = ((int)Math.Round(Slider.Value, MidpointRounding.AwayFromZero)).ToString();
        Field.Text = _shown;
    }

    /// Whole numbers in the channel's range. Anything else is refused and the value shows again.
    /// Ending an edit with no change does nothing.
    void Commit()
    {
        if (Field.Text != _shown)
        {
            if (ColorText.Channel(Field.Text ?? "", 0, Maximum(Channel)) is { } v) Changed?.Invoke(v);
            else _refused();
        }
        ShowText();
    }
}

/// One palette swatch. A click sets the active color, and a right-click or Shift-click the other.
public sealed class PaletteCell : Control
{
    static PaletteCell() => AffectsRender<PaletteCell>(IsPointerOverProperty);

    readonly Palette _palette;
    readonly PaletteGrid _owner;
    bool _pressed;
    public int Index { get; }

    public PaletteCell(Palette p, PaletteGrid owner, int index)
    {
        _palette = p;
        _owner = owner;
        Index = index;
        Width = PaletteGrid.Cell;
        Height = PaletteGrid.Cell;
        Focusable = true;
    }

    public DocColor Color => Index < _owner.Colors.Count ? _owner.Colors[Index] : ColorPalette.Padding;
    public bool IsChosen => _owner.Chosen == Index;

    public void UpdateWords()
    {
        AutomationProperties.SetName(this, $"Palette color {Index + 1}, {Color.Hex}");
        ToolTip.SetTip(this, $"{Color.Hex}: click for the active color, right-click or Shift-click for the other");
    }

    public override void Render(DrawingContext context)
    {
        var bounds = new Rect(Bounds.Size);
        context.FillRectangle(Brushes.Transparent, bounds);
        var r = bounds.Deflate(2);
        ColorDrawing.Swatch(context, Color, r);
        context.DrawRectangle(new Pen(Ui.B(_palette.Line)), r.Deflate(.5));
        DrawnStates.Outline(context, _palette, bounds, IsPointerOver, _pressed);
        if (!IsChosen) return;
        context.DrawRectangle(new Pen(Ui.B(_palette.Accent), 2), bounds.Deflate(1));
        // The check mark, white over a dark edge so it shows on any color.
        var points = new[] { new Point(7, 12.5), new Point(10.5, 16), new Point(17, 8.5) };
        var check = new PolylineGeometry(points, false);
        context.DrawGeometry(null, new Pen(new SolidColorBrush(Avalonia.Media.Color.FromArgb(0xB3, 0, 0, 0)), 3.5, lineCap: PenLineCap.Round, lineJoin: PenLineJoin.Round), check);
        context.DrawGeometry(null, new Pen(Brushes.White, 1.75, lineCap: PenLineCap.Round, lineJoin: PenLineJoin.Round), check);
    }

    protected override void OnPointerPressed(PointerPressedEventArgs e)
    {
        base.OnPointerPressed(e);
        var point = e.GetCurrentPoint(this).Properties;
        if (!point.IsRightButtonPressed && !point.IsLeftButtonPressed) return;
        _pressed = true;
        InvalidateVisual();
        _owner.Pick(Index, point.IsRightButtonPressed || e.KeyModifiers.HasFlag(KeyModifiers.Shift));
        e.Handled = true;
    }

    protected override void OnPointerReleased(PointerReleasedEventArgs e)
    {
        base.OnPointerReleased(e);
        _pressed = false;
        InvalidateVisual();
    }

    protected override void OnPointerCaptureLost(PointerCaptureLostEventArgs e)
    {
        base.OnPointerCaptureLost(e);
        _pressed = false;
        InvalidateVisual();
    }

    /// Space or Enter sets the active color, Shift-Space the other. The arrows move in the grid.
    protected override void OnKeyDown(KeyEventArgs e)
    {
        base.OnKeyDown(e);
        if (e.Key is Key.Space or Key.Enter)
        {
            _owner.Pick(Index, e.KeyModifiers.HasFlag(KeyModifiers.Shift));
            e.Handled = true;
            return;
        }
        int? step = e.Key switch { Key.Left => -1, Key.Right => 1, Key.Down => PaletteGrid.Columns, Key.Up => -PaletteGrid.Columns, _ => null };
        if (step is not { } by || Index + by < 0 || Index + by >= _owner.Cells.Count) return;
        _owner.Cells[Index + by].Focus(NavigationMethod.Directional);
        e.Handled = true;
    }
}

/// The palette swatches: 8 columns of 24 pixel targets with 2 pixel gaps, scrolling when they do
/// not fit. The swatch equal to the active color has an outline and a check mark.
public sealed class PaletteGrid : Grid
{
    public const double Cell = 24, Gap = 2;
    public const int Columns = 8;

    readonly Palette _palette;
    readonly Avalonia.Controls.Canvas _grid = new() { Name = "PaletteCells" };
    readonly Border _addOutline;
    readonly List<PaletteCell> _cells = [];
    IReadOnlyList<DocColor> _colors = [];
    int? _chosen;

    public static double GridWidth => Columns * Cell + (Columns - 1) * Gap;
    public static double HeightOf(int rows) => rows * Cell + Math.Max(rows - 1, 0) * Gap;

    public event Action<int, bool>? Picked;
    public IReadOnlyList<PaletteCell> Cells => _cells;
    public ScrollViewer Scroller { get; }
    public IReadOnlyList<DocColor> Colors => _colors;

    public PaletteGrid(Palette p)
    {
        _palette = p;
        Scroller = new ScrollViewer
        {
            HorizontalScrollBarVisibility = ScrollBarVisibility.Disabled,
            VerticalScrollBarVisibility = ScrollBarVisibility.Auto,
            Content = _grid,
        };
        Children.Add(Scroller);
        // Add mode: an accent outline round the grid says "click a slot".
        _addOutline = new Border
        {
            Name = "AddOutline",
            IsHitTestVisible = false,
            IsVisible = false,
            Child = new Drawn((c, size) => c.DrawRectangle(new Pen(Ui.B(p.Accent), 2, new DashStyle([2, 1.5], 0)), new Rect(size).Deflate(1))),
        };
        Children.Add(_addOutline);
    }

    public bool AddMode
    {
        get => _addOutline.IsVisible;
        set => _addOutline.IsVisible = value;
    }

    public int? Chosen
    {
        get => _chosen;
        set
        {
            if (_chosen == value) return;
            _chosen = value;
            foreach (var cell in _cells) cell.InvalidateVisual();
        }
    }

    public void Show(IReadOnlyList<DocColor> colors, double x0)
    {
        _colors = colors;
        if (_cells.Count != colors.Count)
        {
            _grid.Children.Clear();
            _cells.Clear();
            for (var i = 0; i < colors.Count; i++)
            {
                var cell = new PaletteCell(_palette, this, i);
                Avalonia.Controls.Canvas.SetLeft(cell, x0 + i % Columns * (Cell + Gap));
                Avalonia.Controls.Canvas.SetTop(cell, i / Columns * (Cell + Gap));
                _cells.Add(cell);
                _grid.Children.Add(cell);
            }
            _grid.Height = HeightOf((colors.Count + Columns - 1) / Columns);
        }
        foreach (var cell in _cells)
        {
            cell.UpdateWords();
            cell.InvalidateVisual();
        }
    }

    public void Pick(int index, bool other) => Picked?.Invoke(index, other);
}

public sealed class ColorsPanel : Avalonia.Controls.Canvas
{
    const double Pad = 10, RowY = 72, MoreY = 102, ChannelStep = 26, Gutter = 8;
    /// This body starts after the window's 1 pixel outline, and the Mac's numbers count from the
    /// window's edge.
    const double Edge = 1;
    const int GradientSteps = 16;

    public const string LoadItem = "Load Palette…", SaveItem = "Save Palette As…", ResetItem = "Reset to Default Palette";

    readonly PanelContext _context;
    readonly ColorsMemory _memory;
    readonly ToolState _tools;
    readonly Action<bool> _resize;
    readonly Dictionary<ColorChannel, ChannelRow> _channels = [];
    string _shownHex = "";

    public SwatchPair Swatches { get; }
    public IconCommand SwapButton { get; }
    public IconCommand ResetButton { get; }
    public TextBox HexField { get; }
    public ColorWheel Wheel { get; }
    public Button MoreButton { get; }
    public IconCommand AddColorButton { get; }
    public IconCommand PaletteButton { get; }
    public ContextMenu PaletteMenu { get; }
    public PaletteGrid Swatchboard { get; }
    public IReadOnlyDictionary<ColorChannel, ChannelRow> Channels => _channels;
    public bool IsExpanded => _memory.Expanded;
    public string? LastReport => _memory.LastReport;
    public Hsv Hsv => _memory.Hsv;

    public ColorsPanel(Palette p, PanelContext context, ColorsMemory memory, Action<bool> resize)
    {
        _context = context;
        _memory = memory;
        _tools = context.Tools;
        _resize = resize;
        Name = "ColorsBody";
        Focusable = true;
        Background = Ui.B(p.Panel);
        var width = Tokens.ColorsWidth;

        Swatches = Put(new SwatchPair(p, _tools), Pad, Pad);
        Swatches.Picked += slot => _tools.ActiveSlot = slot;
        SwapButton = Put(new IconCommand(p, "colors-swap", "Swap colors", "Swap colors (X)", Tokens.PanelTarget), 68, Pad);
        SwapButton.Click += (_, _) => _tools.SwapColors();
        ResetButton = Put(new IconCommand(p, "colors-reset", "Reset colors", "Black and white", Tokens.PanelTarget), 68, Pad + 30);
        ResetButton.Click += (_, _) => _tools.ResetColors();
        Put(Ui.Label("Hex", p.Muted), 100, Pad + 1, 72, 15);
        var hex = Ui.Field(p, "", 72, Tokens.FieldHeight, TextAlignment.Left);
        HexField = ((Grid)hex).Children.OfType<TextBox>().Single();
        ToolTip.SetTip(HexField, "RRGGBB, or RRGGBBAA with alpha");
        Put(hex, 100, Pad + 18);
        HexField.KeyDown += (_, e) =>
        {
            if (e.Key != Key.Enter) return;
            HexEntered();
            // ColorsPanel.swift: "Give the keyboard back to the document window".
            GiveBack();
            e.Handled = true;
        };
        HexField.LostFocus += (_, _) => HexEntered();

        Wheel = Put(new ColorWheel(p), width - Pad - ColorWheel.Diameter, Pad);
        Wheel.Picked += WheelPicked;

        MoreButton = Put(Ui.Push(p, "More »", 84), Pad - 2, RowY);
        AutomationProperties.SetName(MoreButton, "More");
        MoreButton.Click += (_, _) => SetExpanded(!_memory.Expanded);
        AddColorButton = Put(new IconCommand(p, "colors-add", "Add color to palette", "Add the active color: then click a palette swatch to replace it", Tokens.PanelTarget), width - Pad - 52, RowY);
        AddColorButton.Click += (_, _) =>
        {
            _memory.AddMode = !_memory.AddMode;
            Sync();
        };
        PaletteButton = Put(new IconCommand(p, "cmd-more", "Palette menu", "Load, save or reset the palette", Tokens.PanelTarget), width - Pad - 24, RowY);
        PaletteMenu = new ContextMenu { Placement = PlacementMode.BottomEdgeAlignedLeft, Name = "PaletteMenu" };
        MenuBuilder.Fill(PaletteMenu.Items, MenuEntries(), p, new Dictionary<string, Action>
        {
            [LoadItem] = LoadChosen,
            [ResetItem] = ResetPalette,
            // Save Palette As… comes with Open and Save in W10, so it is present and off here.
        });
        PaletteButton.Click += (_, _) => PaletteMenu.Open(PaletteButton);

        Swatchboard = new PaletteGrid(p);
        Swatchboard.Picked += PalettePicked;
        Put(Swatchboard, 0, MoreY, width);

        var rowWidth = width - 2 * Pad;
        var y = MoreY;
        foreach (var channel in Enum.GetValues<ColorChannel>())
        {
            var row = Put(new ChannelRow(p, channel, rowWidth, Refuse), Pad, y);
            row.Changed += v => ChannelChanged(channel, v);
            row.Entered += GiveBack;
            _channels[channel] = row;
            y += ChannelStep;
        }

        SizeChanged += (_, _) => Place();
        AttachedToVisualTree += (_, _) =>
        {
            _tools.ValuesChanged += Sync;
            Sync();
        };
        DetachedFromVisualTree += (_, _) => _tools.ValuesChanged -= Sync;
        Sync();
        Place();
    }

    static IReadOnlyList<MenuEntry> MenuEntries() =>
    [
        new(EntryKind.Item, LoadItem, LoadItem, null, "", null, []),
        new(EntryKind.Item, SaveItem, SaveItem, null, "", null, []),
        new(EntryKind.Separator, "---", "", null, "", null, []),
        new(EntryKind.Item, ResetItem, ResetItem, null, "", null, []),
    ];

    T Put<T>(T child, double x, double y, double w = double.NaN) where T : Control => Ui.At(this, child, x - Edge, y, w);

    T Put<T>(T child, double x, double y, double w, double h) where T : Control => Ui.At(this, child, x - Edge, y, w, h);

    /// The content height that shows the compact body with 8 pixels below.
    public static double CompactContentHeight => MoreY + PaletteGrid.HeightOf(4) + Gutter;

    void Place()
    {
        var y = _memory.Expanded ? MoreY + ChannelStep * _channels.Count + 6 : MoreY;
        SetTop(Swatchboard, y);
        // The body stops 1 pixel above the window's edge, so the palette keeps the Mac's 8 below.
        Swatchboard.Height = Math.Max(Bounds.Height + Edge - y - Gutter, 0);
    }

    // State to views.

    public void Sync()
    {
        var c = _tools.Color(_tools.ActiveSlot);
        var hsv = _memory.Hsv;
        if (hsv.Color(c.A) != c)
        {
            var n = Hsv.Of(c);
            // Grey has no hue, so it keeps the last one.
            if (n.S == 0 || n.V == 0) n = n with { H = hsv.H };
            if (n.V == 0) n = n with { S = hsv.S };
            _memory.Hsv = hsv = n;
        }
        Swatches.InvalidateVisual();
        var active = _tools.ActiveSlot == ColorSlot.Primary ? "primary" : "secondary";
        AutomationProperties.SetItemStatus(Swatches, $"primary {_tools.Primary.Hex}, secondary {_tools.Secondary.Hex}, {active} active");
        if (!HexField.IsFocused)
        {
            HexField.Text = c.Hex;
            _shownHex = c.Hex;
        }
        AutomationProperties.SetName(HexField, _tools.ActiveSlot == ColorSlot.Primary ? "Primary color hex" : "Secondary color hex");
        Wheel.Hsv = hsv;
        foreach (var (channel, row) in _channels)
        {
            row.IsVisible = _memory.Expanded;
            row.Show(ChannelValue(channel, c, hsv), Gradient(channel, c, hsv));
        }
        var shown = _tools.Palette.Colors.Take(_memory.Expanded ? ColorPalette.Size : ColorPalette.CompactCount).ToList();
        Swatchboard.Show(shown, Math.Round((Tokens.ColorsWidth - PaletteGrid.GridWidth) / 2) - Edge);
        var at = _tools.Palette.Colors.ToList().IndexOf(c);
        Swatchboard.Chosen = at >= 0 && at < shown.Count ? at : null;
        Swatchboard.AddMode = _memory.AddMode;
        AddColorButton.On = _memory.AddMode;
        MoreButton.Content = _memory.Expanded ? "« Less" : "More »";
        AutomationProperties.SetName(MoreButton, _memory.Expanded ? "Less" : "More");
        if (_memory.LastReport is { } report) ToolTip.SetTip(Swatchboard, report);
    }

    static double ChannelValue(ColorChannel channel, DocColor c, Hsv hsv) => channel switch
    {
        ColorChannel.Red => c.R,
        ColorChannel.Green => c.G,
        ColorChannel.Blue => c.B,
        ColorChannel.Hue => Math.Round(hsv.H, MidpointRounding.AwayFromZero),
        ColorChannel.Saturation => Math.Round(hsv.S, MidpointRounding.AwayFromZero),
        ColorChannel.Value => Math.Round(hsv.V, MidpointRounding.AwayFromZero),
        _ => c.A,
    };

    /// The colors along a slider's track for the current color.
    static IReadOnlyList<DocColor> Gradient(ColorChannel channel, DocColor c, Hsv hsv)
    {
        var stops = new DocColor[GradientSteps + 1];
        for (var i = 0; i <= GradientSteps; i++)
        {
            var t = i / (double)GradientSteps;
            var b = (byte)Math.Round(t * 255, MidpointRounding.AwayFromZero);
            var d = c with { A = 255 };
            stops[i] = channel switch
            {
                ColorChannel.Red => d with { R = b },
                ColorChannel.Green => d with { G = b },
                ColorChannel.Blue => d with { B = b },
                ColorChannel.Hue => new Hsv(t * 360, 100, 100).Color(),
                ColorChannel.Saturation => new Hsv(hsv.H, t * 100, Math.Max(hsv.V, 1)).Color(),
                ColorChannel.Value => new Hsv(hsv.H, hsv.S, t * 100).Color(),
                _ => d with { A = b },
            };
        }
        return stops;
    }

    // Edits.

    /// Sets a slot to a color, keeping `hsv` as the panel's HSV when it gives exactly these bytes.
    void Apply(DocColor c, ColorSlot slot, Hsv? hsv = null)
    {
        if (slot == _tools.ActiveSlot && hsv is { } n) _memory.Hsv = n;
        _tools.SetColor(slot, c);
        Sync();
    }

    void Refuse() => _context.Refused?.Invoke();

    /// With no one to give it to, the field still lets go of the keyboard.
    void GiveBack()
    {
        Focus();
        _context.GiveKeyboardBack?.Invoke();
    }

    void HexEntered()
    {
        var text = HexField.Text ?? "";
        // Ending an edit that changed nothing must not write the text back: the color may have
        // changed elsewhere while the field kept its old text.
        if (text != _shownHex)
        {
            if (DocColor.FromHex(text) is { } c) Apply(c, _tools.ActiveSlot);
            else if (ColorText.TrimSpaces(text).Length != 0) Refuse();
        }
        var now = _tools.Color(_tools.ActiveSlot).Hex;
        HexField.Text = now;
        _shownHex = now;
    }

    public void WheelPicked(double dx, double dy, WheelModifiers mods, bool other)
    {
        var slot = other ? ToolState.Other(_tools.ActiveSlot) : _tools.ActiveSlot;
        var based = other ? Hsv.Of(_tools.Color(slot)) : _memory.Hsv;
        var (h, s) = ColorWheelMath.Pick(dx, dy, based, mods);
        // The wheel picks hue and saturation. Value stays, except that black would hide the
        // pick, so value 0 becomes 100.
        var n = new Hsv(h, s, based.V == 0 ? 100 : based.V);
        Apply(n.Color(_tools.Color(slot).A), slot, n);
    }

    public void ChannelChanged(ColorChannel channel, double v)
    {
        var slot = _tools.ActiveSlot;
        var c = _tools.Color(slot);
        var b = (byte)Math.Clamp(Math.Round(v, MidpointRounding.AwayFromZero), 0, 255);
        switch (channel)
        {
            case ColorChannel.Red: c = c with { R = b }; break;
            case ColorChannel.Green: c = c with { G = b }; break;
            case ColorChannel.Blue: c = c with { B = b }; break;
            case ColorChannel.Alpha: c = c with { A = b }; break;
            default:
                var n = _memory.Hsv;
                if (channel == ColorChannel.Hue) n = n with { H = v >= 360 ? 0 : v };
                if (channel == ColorChannel.Saturation) n = n with { S = v };
                if (channel == ColorChannel.Value) n = n with { V = v };
                Apply(n.Color(c.A), slot, n);
                return;
        }
        Apply(c, slot);
    }

    void PalettePicked(int index, bool other)
    {
        if (index < 0 || index >= _tools.Palette.Colors.Count) return;
        if (_memory.AddMode)
        {
            // Add mode: put the active color into the clicked slot, once.
            _tools.Palette = _tools.Palette.With(index, _tools.Color(_tools.ActiveSlot));
            _memory.AddMode = false;
            Sync();
            return;
        }
        Apply(_tools.Palette.Colors[index], other ? ToolState.Other(_tools.ActiveSlot) : _tools.ActiveSlot);
    }

    public void SetExpanded(bool on)
    {
        if (on == _memory.Expanded) return;
        _memory.Expanded = on;
        Sync();
        Place();
        _resize(on);
    }

    // Palette files.

    /// Opens the system's picker, and reads what comes back. An error reaches the status bar.
    async void LoadChosen()
    {
        try
        {
            var top = TopLevel.GetTopLevel(this) ?? throw new InvalidOperationException("the panel has no window");
            var pick = _context.PickPaletteFile ?? PickWithSystem;
            if (await pick(top) is { } path) LoadPalette(path);
        }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException or InvalidOperationException or NotSupportedException)
        {
            ReportText($"Could not open a palette: {e.Message}");
        }
    }

    static async Task<string?> PickWithSystem(TopLevel top)
    {
        var files = await top.StorageProvider.OpenFilePickerAsync(new FilePickerOpenOptions
        {
            AllowMultiple = false,
            FileTypeFilter = [FilePickerFileTypes.TextPlain],
        });
        if (files.Count == 0) return null;
        return files[0].TryGetLocalPath() ?? throw new NotSupportedException($"{files[0].Name} is not a local file");
    }

    void ResetPalette()
    {
        _tools.Palette = ColorPalette.Standard;
        ReportText("Palette reset to the default.");
    }

    /// Loads a paint.net palette file. Malformed lines are skipped and reported. Null means the
    /// file could not be read.
    public PaletteParse? LoadPalette(string path)
    {
        var name = Path.GetFileName(path);
        string text;
        try
        {
            var bytes = File.ReadAllBytes(path);
            try
            {
                text = new UTF8Encoding(false, true).GetString(bytes);
            }
            catch (DecoderFallbackException)
            {
                // Not UTF-8. The Mac then reads it as Latin-1, which takes any bytes.
                text = Encoding.Latin1.GetString(bytes);
            }
        }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException)
        {
            ReportText($"Could not open {name}: {e.Message}");
            return null;
        }
        var result = ColorPalette.Parse(text);
        if (result.ColorCount == 0)
        {
            ReportText($"{name} has no palette colors; the palette did not change. {result.Summary}");
            return result;
        }
        _tools.Palette = result.Palette;
        ReportText($"{name}: {result.Summary}");
        return result;
    }

    /// Saves the palette as a paint.net palette file, replacing it whole or not at all.
    public bool SavePalette(string path)
    {
        var temporary = path + ".part";
        try
        {
            File.WriteAllText(temporary, _tools.Palette.FileText, new UTF8Encoding(false));
            File.Move(temporary, path, overwrite: true);
            ReportText($"Saved the palette to {Path.GetFileName(path)}.");
            return true;
        }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException)
        {
            ReportText($"Could not save the palette: {e.Message}");
            return false;
        }
    }

    void ReportText(string text)
    {
        _memory.LastReport = text;
        ToolTip.SetTip(Swatchboard, text);
        _context.Report?.Invoke(text);
    }
}
