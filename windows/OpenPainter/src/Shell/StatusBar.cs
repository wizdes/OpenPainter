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

    /// The pointer's position in whole document pixels, or a dash when it is off the canvas.
    public void ShowCursor(Point? at) =>
        CursorValue.Text = at is { } p ? $"{(long)Math.Floor(p.X)}, {(long)Math.Floor(p.Y)}" : "–";

    void UpdateHint() => Hint.IsVisible = HintFits(_right.Bounds.Width + _right.Margin.Right, Bounds.Width);

    /// A very narrow window drops the hint before it squeezes any value.
    public static bool HintFits(double valuesWidth, double barWidth) => valuesWidth + HintMargin <= barWidth;
}
