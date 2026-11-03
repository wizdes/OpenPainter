// The 28 pixel status bar, with the Mac's words and order (StatusBarView.swift):
// the tool hint on the left; Selection, Cursor, Image, Profile, Units, the zoom
// field, Fit, 100% and the zoom slider on the right. On a tight window the hint
// hides first.
using Avalonia;
using Avalonia.Automation;
using Avalonia.Controls;
using Avalonia.Input;
using Avalonia.Layout;
using OpenPainter.Canvas;
using UiLook;

namespace OpenPainter.Shell;

/// The units for the status bar: MeasurementUnit in RulerScale.swift.
public enum MeasurementUnit { Pixels, Inches, Centimeters }

public static class Measurement
{
    public static readonly MeasurementUnit[] All = [MeasurementUnit.Pixels, MeasurementUnit.Inches, MeasurementUnit.Centimeters];

    public static string Title(MeasurementUnit u) => u.ToString();
    public static string Abbreviation(MeasurementUnit u) => u switch { MeasurementUnit.Pixels => "px", MeasurementUnit.Inches => "in", _ => "cm" };

    /// Document pixels in one unit at `dpi` pixels per inch.
    public static double PixelsPerUnit(MeasurementUnit u, double dpi) => u switch { MeasurementUnit.Pixels => 1, MeasurementUnit.Inches => dpi, _ => dpi / 2.54 };

    /// Whole pixels, 2 decimals for inches and centimeters.
    public static string Format(MeasurementUnit u, double pixels, double dpi)
    {
        var value = pixels / PixelsPerUnit(u, dpi);
        return u == MeasurementUnit.Pixels
            ? ((long)Math.Floor(value)).ToString(System.Globalization.CultureInfo.InvariantCulture)
            : value.ToString("F2", System.Globalization.CultureInfo.InvariantCulture);
    }
}

public sealed class StatusBar : Grid
{
    /// The space StatusBarView.swift keeps before it hides the hint.
    const double HintMargin = 40;

    readonly StackPanel _right;
    bool _showing;
    string _shownText = "100%";

    /// The field holds something other than what the view last showed there.
    bool Typing => ZoomField.Text != _shownText;
    double _zoom = 1;
    MeasurementUnit _unit = MeasurementUnit.Pixels;
    double _dpi = 96;
    Size? _imageSize;
    Rect? _selection;
    Point? _cursor;

    public TextBlock Hint { get; }
    public TextBlock SelectionValue { get; }
    public TextBlock CursorValue { get; }
    public TextBlock ImageValue { get; }
    public TextBlock ProfileValue { get; }
    public ComboBox Units { get; }
    public TextBox ZoomField { get; }
    public Button FitButton { get; }
    public Button ActualButton { get; }
    public Slider ZoomSlider { get; }

    /// A new zoom from the field or the slider, or the 100% button.
    public event Action<double>? ZoomChosen;

    /// The Fit button.
    public event Action? FitChosen;

    /// The units list picked a unit.
    public event Action<MeasurementUnit>? UnitChosen;

    /// The units the values show in, and the document's pixels per inch.
    public MeasurementUnit Unit
    {
        get => _unit;
        set
        {
            _unit = value;
            var at = Array.IndexOf(Measurement.All, value);
            if (Units.SelectedIndex != at) Units.SelectedIndex = at;
            ShowValues();
        }
    }

    public double Dpi
    {
        get => _dpi;
        set
        {
            _dpi = value > 0 && double.IsFinite(value) ? value : 96;
            ShowValues();
        }
    }

    public StatusBar(Palette p)
    {
        Name = "StatusBar";
        Height = Tokens.StatusHeight;
        Background = Ui.B(p.Chrome);
        ColumnDefinitions = new ColumnDefinitions("*,Auto");
        var line = new Border { Height = 1, Background = Ui.B(p.Line), VerticalAlignment = VerticalAlignment.Top };
        SetColumnSpan(line, 2);
        Children.Add(line);

        Hint = Ui.Label("", p.Muted);
        Hint.Margin = new Thickness(10, 0, 10, 0);
        Children.Add(Hint);

        _right = Ui.Row(10);
        _right.Margin = new Thickness(0, 0, 10, 0);
        TextBlock Pair(string label, string value)
        {
            var pair = Ui.Row(4);
            pair.Children.Add(Ui.Label(label, p.Muted));
            var text = Ui.Label(value, p.Ink);
            AutomationProperties.SetName(text, label);
            pair.Children.Add(text);
            _right.Children.Add(pair);
            return text;
        }
        SelectionValue = Pair("Selection", "None");
        CursorValue = Pair("Cursor", "–");
        ImageValue = Pair("Image", "–");
        ProfileValue = Pair("Profile", "sRGB");

        var units = Ui.Row(4);
        units.Children.Add(Ui.Label("Units", p.Muted));
        Units = Ui.Choice(p, "px", 58, 22);
        Units.Items.Add("in");
        Units.Items.Add("cm");
        AutomationProperties.SetName(Units, "Units");
        ToolTip.SetTip(Units, "Units: pixels, inches or centimeters (View menu)");
        Units.SelectionChanged += (_, _) =>
        {
            if (Units.SelectedIndex >= 0 && Measurement.All[Units.SelectedIndex] != _unit) UnitChosen?.Invoke(Measurement.All[Units.SelectedIndex]);
        };
        units.Children.Add(Units);
        _right.Children.Add(units);

        var zoomField = Ui.Field(p, "100%", 56);
        ZoomField = ((Grid)zoomField).Children.OfType<TextBox>().Single();
        ZoomField.Name = "ZoomField";
        AutomationProperties.SetName(ZoomField, "Zoom");
        _right.Children.Add(zoomField);
        FitButton = Ui.Push(p, "Fit", 34);
        ActualButton = Ui.Push(p, "100%", 52);
        _right.Children.Add(FitButton);
        _right.Children.Add(ActualButton);
        ZoomSlider = Ui.Slide(p, Math.Log(ZoomPresets.Minimum), Math.Log(ZoomPresets.Maximum), 0, 96);
        AutomationProperties.SetName(ZoomSlider, "Zoom slider");
        _right.Children.Add(ZoomSlider);
        SetColumn(_right, 1);
        Children.Add(_right);

        SizeChanged += (_, _) => UpdateHint();

        FitButton.Click += (_, _) => FitChosen?.Invoke();
        ActualButton.Click += (_, _) => ZoomChosen?.Invoke(1);
        ZoomField.KeyDown += (_, e) =>
        {
            if (e.Key != Key.Enter) return;
            CommitTyping();
            e.Handled = true;
        };
        ZoomField.LostFocus += (_, _) => CommitTyping();
        ZoomSlider.PropertyChanged += (_, e) =>
        {
            if (e.Property == Avalonia.Controls.Primitives.RangeBase.ValueProperty && !_showing)
                ZoomChosen?.Invoke(ZoomPresets.Clamp(Math.Exp(ZoomSlider.Value)));
        };
    }

    /// Shows a zoom without reporting a change. The field keeps what the user is typing.
    public void ShowZoom(double zoom)
    {
        _zoom = zoom;
        _showing = true;
        if (!Typing) ShowText(ZoomPresets.Label(zoom));
        ZoomSlider.Value = Math.Log(zoom);
        _showing = false;
    }

    /// Takes what the user typed in the zoom field. Bad input shows the real zoom again, as
    /// on the Mac. The native canvas takes focus with no word to Avalonia, so a press there
    /// calls this too, and the field does not rely on its focus state.
    public void CommitTyping()
    {
        if (!Typing) return;
        var zoom = ZoomPresets.ParseField(ZoomField.Text);
        ShowText(ZoomPresets.Label(_zoom));
        if (zoom is { } chosen) ZoomChosen?.Invoke(chosen);
    }

    void ShowText(string text)
    {
        _shownText = text;
        ZoomField.Text = text;
    }

    /// The pointer's position in document pixels, or null when it is off the canvas.
    public void ShowCursor(Point? at)
    {
        _cursor = at;
        ShowValues();
    }

    /// The image's size in document pixels, or null with no image.
    public void ShowImage(Size? size)
    {
        _imageSize = size;
        ShowValues();
    }

    /// The selection's bounds in document pixels, or null with no selection.
    public void ShowSelection(Rect? bounds)
    {
        _selection = bounds;
        ShowValues();
    }

    /// StatusBarView.update: every value, in the current unit.
    void ShowValues()
    {
        string V(double pixels) => Measurement.Format(_unit, pixels, _dpi);
        var abbreviation = Measurement.Abbreviation(_unit);
        SelectionValue.Text = _selection is { } s ? $"{V(s.Width)} × {V(s.Height)} {abbreviation}" : "None";
        CursorValue.Text = _cursor is { } c ? $"{V(c.X)}, {V(c.Y)}" : "–";
        ImageValue.Text = _imageSize is { } i ? $"{V(i.Width)} × {V(i.Height)} {abbreviation}" : "–";
    }

    /// The document's profile: the short name, and the full one as a tip.
    public void ShowProfile(string shortName, string full)
    {
        ProfileValue.Text = shortName;
        ToolTip.SetTip(ProfileValue, full);
    }

    void UpdateHint() => Hint.IsVisible = HintFits(_right.Bounds.Width + _right.Margin.Right, Bounds.Width);

    /// A very narrow window drops the hint before it squeezes any value.
    public static bool HintFits(double valuesWidth, double barWidth) => valuesWidth + HintMargin <= barWidth;
}
