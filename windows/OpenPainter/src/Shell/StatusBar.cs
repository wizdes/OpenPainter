// The 28 pixel status bar, with the Mac's words and order (StatusBarView.swift):
// the tool hint on the left; Selection, Cursor, Image, Profile, Units, the zoom
// field, Fit, 100% and the zoom slider on the right. On a tight window the hint
// hides first. The values and buttons get their math in a later stage of W7.
using Avalonia;
using Avalonia.Automation;
using Avalonia.Controls;
using Avalonia.Layout;
using UiLook;

namespace OpenPainter.Shell;

public sealed class StatusBar : Grid
{
    /// The space StatusBarView.swift keeps before it hides the hint.
    const double HintMargin = 40;
    const double MinimumZoom = 0.01, MaximumZoom = 32;   // ZoomPresets.minimum and maximum.

    readonly StackPanel _right;

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
        ZoomSlider = Ui.Slide(p, Math.Log(MinimumZoom), Math.Log(MaximumZoom), 0, 96);
        AutomationProperties.SetName(ZoomSlider, "Zoom slider");
        _right.Children.Add(ZoomSlider);
        SetColumn(_right, 1);
        Children.Add(_right);

        SizeChanged += (_, _) => UpdateHint();
    }

    void UpdateHint() => Hint.IsVisible = HintFits(_right.Bounds.Width + _right.Margin.Right, Bounds.Width);

    /// A very narrow window drops the hint before it squeezes any value.
    public static bool HintFits(double valuesWidth, double barWidth) => valuesWidth + HintMargin <= barWidth;
}
