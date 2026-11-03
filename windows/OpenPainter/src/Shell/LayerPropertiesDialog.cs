// Layer Properties: mac/OpenPainterKit/Sources/Shell/LayerProperties.swift. Name, visible, blend
// mode (all 14) and opacity (a slider and a 0 to 255 field) as an engine preview session. Each
// change updates the preview, so the canvas shows it live. OK commits one history step. Cancel
// ends the session and leaves the document and history as they were.
//
// The Mac shows a sheet. A Windows dialog is an owned window, so it stays above the canvas, with
// a title row that drags it (W6). `MainWindow.ShowLayerProperties` holds the rest of the app off
// while it is up, the way a modal dialog does.
using System.Globalization;
using System.Runtime.InteropServices;
using Avalonia;
using Avalonia.Automation;
using Avalonia.Controls;
using Avalonia.Controls.Chrome;
using Avalonia.Input;
using Avalonia.Layout;
using Avalonia.Media;
using Avalonia.Styling;
using OpenPainter.Bridge;
using UiLook;

namespace OpenPainter.Shell;

public sealed partial class LayerPropertiesDialog : Window
{
    public const double ClientWidth = 340, BodyHeight = 214;
    const uint DwmCornerPreference = 33;
    const int DwmCornerRound = 2;

    [LibraryImport("dwmapi.dll")] private static partial int DwmSetWindowAttribute(nint hwnd, uint attribute, ref int value, int size);

    public readonly record struct Values(string Name, bool Visible, string Blend, int Opacity);

    readonly Editor _editor;
    bool _showing, _finished;

    public int LayerIndex { get; }
    public Values Original { get; }
    public Values Current { get; private set; }

    /// True once the engine session has begun, on the first change.
    public bool SessionStarted { get; private set; }

    public TextBox NameField { get; private set; } = null!;
    public CheckBox VisibleBox { get; private set; } = null!;
    public ComboBox BlendPopup { get; private set; } = null!;
    public Slider OpacitySlider { get; private set; } = null!;
    public TextBox OpacityField { get; private set; } = null!;
    public Button OkButton { get; private set; } = null!;
    public Button CancelButton { get; private set; } = null!;
    public IconCommand CloseButton { get; private set; } = null!;
    public Control TitleRow { get; private set; } = null!;

    /// The dialog ended, with true for OK.
    public event Action<bool>? Finished;

    /// The screens' work areas in layout points, so a drag keeps the title row reachable.
    public Func<IReadOnlyList<Rect>>? WorkAreas { get; set; }

    public LayerPropertiesDialog(Editor editor, int index, Palette palette)
    {
        _editor = editor;
        LayerIndex = index;
        var l = editor.Info.Layers[index];
        Original = new Values(l.Name, l.Visible, l.Blend, l.Opacity);
        Current = Original;
        Name = "LayerPropertiesDialog";
        Title = "Layer Properties";
        Width = ClientWidth;
        Height = BodyHeight + Tokens.DialogTitleHeight;
        CanResize = false;
        ShowInTaskbar = false;
        WindowStartupLocation = WindowStartupLocation.CenterOwner;
        FontFamily = new FontFamily(Tokens.FontFamily);
        ExtendClientAreaToDecorationsHint = true;
        ExtendClientAreaTitleBarHeightHint = Tokens.DialogTitleHeight;
        WindowDecorationsTheme = new ControlTheme(typeof(WindowDrawnDecorations))
        {
            Setters = { new Setter(WindowDrawnDecorations.TemplateProperty, new NoDrawnDecorations()) },
        };
        AutomationProperties.SetName(this, "Layer Properties");
        ApplyLook(palette);
        Opened += (_, _) =>
        {
            RoundCorners();
            NameField.Focus();
            NameField.SelectAll();
        };
        Closing += (_, e) =>
        {
            // The system's close and Alt+F4 are Cancel.
            if (!_finished) Finish(false);
        };
        PositionChanged += (_, _) => Fatal.Guard("DIALOG_MOVED", KeepReachable);
    }

    void RoundCorners()
    {
        if (TryGetPlatformHandle() is not { HandleDescriptor: "HWND", Handle: var hwnd }) return;
        var round = DwmCornerRound;
        var status = DwmSetWindowAttribute(hwnd, DwmCornerPreference, ref round, sizeof(int));
        // Windows 10 has no such attribute, and there its square corners are right.
        if (status != 0 && OperatingSystem.IsWindowsVersionAtLeast(10, 0, 22000))
            throw new InvalidOperationException($"DIALOG_CORNER: 0x{status:x8}");
    }

    /// The Mac's dialog cannot leave the screen, and a modal window off screen would hold the
    /// app. So a drag keeps the title row reachable, as a panel's does.
    void KeepReachable()
    {
        if (WorkAreas is not { } screens || TryGetPlatformHandle() is not { HandleDescriptor: "HWND" }) return;
        var scale = RenderScaling;
        var at = this.PointToScreen(default);
        var frame = new Rect(at.X / scale, at.Y / scale, ClientSize.Width, ClientSize.Height);
        var kept = PanelLayout.Reachable(frame, screens());
        if (kept == frame) return;
        var edge = at - Position;
        Position = new PixelPoint((int)Math.Round(kept.X * scale) - edge.X, (int)Math.Round(kept.Y * scale) - edge.Y);
    }

    static TextBox FieldBox(Control field, Palette p)
    {
        var box = ((Grid)field).Children.OfType<TextBox>().Single();
        // Fluent's selection is dark on the light theme, over dark text.
        box.SelectionBrush = Ui.B(p.Accent);
        box.SelectionForegroundBrush = Ui.B(p.OnAccent);
        return box;
    }

    /// Light and dark differ only by the palette, so a theme change builds the look again.
    public void ApplyLook(Palette p)
    {
        Background = Ui.B(p.Panel);
        var row = new Grid { Name = "DialogTitleRow", Height = Tokens.DialogTitleHeight, Background = Ui.B(p.TitleBar), VerticalAlignment = VerticalAlignment.Top };
        WindowDecorationProperties.SetElementRole(row, WindowDecorationsElementRole.TitleBar);
        row.Children.Add(new Border { Height = 1, Background = Ui.B(p.PanelEdge), VerticalAlignment = VerticalAlignment.Bottom, IsHitTestVisible = false });
        var title = Ui.Label("Layer Properties", p.Ink);
        title.Margin = new Thickness(12, 0, 0, 0);
        title.HorizontalAlignment = HorizontalAlignment.Left;
        title.IsHitTestVisible = false;
        row.Children.Add(title);
        CloseButton = new IconCommand(p, "window-close", "Close Layer Properties", target: Tokens.DialogTitleHeight, iconSize: Tokens.SmallIconSize)
        {
            HorizontalAlignment = HorizontalAlignment.Right,
            VerticalAlignment = VerticalAlignment.Center,
            Focusable = false,
        };
        WindowDecorationProperties.SetElementRole(CloseButton, WindowDecorationsElementRole.User);
        CloseButton.Click += (_, _) => Cancel();
        row.Children.Add(CloseButton);
        TitleRow = row;

        // y grows down, 28 pixel rows 12 apart, in the Mac's places.
        var body = new Avalonia.Controls.Canvas { Width = ClientWidth, Height = BodyHeight };
        void RightLabel(string text, double y)
        {
            var label = Ui.Label(text, p.Ink);
            label.TextAlignment = TextAlignment.Right;
            Ui.At(body, label, 16, y + 4, 84, 16);
        }
        RightLabel("Name:", 20);
        var nameField = Ui.Field(p, Current.Name, 216, Tokens.FieldHeight, TextAlignment.Left);
        NameField = FieldBox(nameField, p);
        AutomationProperties.SetName(NameField, "Layer name");
        Ui.At(body, nameField, 108, 20);
        NameField.TextChanged += (_, _) => NameTyped();

        VisibleBox = Ui.Check(p, "Visible");
        VisibleBox.IsChecked = Current.Visible;
        Ui.At(body, VisibleBox, 106, 54);
        VisibleBox.IsCheckedChanged += (_, _) => ControlChanged();

        RightLabel("Blend mode:", 86);
        BlendPopup = Ui.Choice(p, LayerBlend.Modes[0].Title, 160);
        foreach (var (_, modeTitle) in LayerBlend.Modes.Skip(1)) BlendPopup.Items.Add(modeTitle);
        BlendPopup.SelectedIndex = BlendIndex(Current.Blend);
        AutomationProperties.SetName(BlendPopup, "Blend mode");
        Ui.At(body, BlendPopup, 106, 84);
        BlendPopup.SelectionChanged += (_, _) => ControlChanged();

        RightLabel("Opacity:", 122);
        OpacitySlider = Ui.Slide(p, 0, 255, Current.Opacity, 150);
        AutomationProperties.SetName(OpacitySlider, "Opacity");
        Ui.At(body, OpacitySlider, 108, 123);
        OpacitySlider.ValueChanged += (_, _) => SliderMoved();
        var opacityField = Ui.Field(p, Current.Opacity.ToString(CultureInfo.InvariantCulture), 56, Tokens.FieldHeight, TextAlignment.Right);
        OpacityField = FieldBox(opacityField, p);
        AutomationProperties.SetName(OpacityField, "Opacity value, 0 to 255");
        Ui.At(body, opacityField, 268, 122);
        OpacityField.TextChanged += (_, _) => OpacityTyped();
        OpacityField.LostFocus += (_, _) => ShowValues(Current);

        // OK comes before Cancel, as W6's dialogs have it for Windows.
        OkButton = Ui.Push(p, "OK", 80, Tokens.DialogButtonHeight, accent: true);
        // OK keeps its accent fill and its own text color in hover and press.
        OkButton.Resources["ButtonBackgroundPointerOver"] = Ui.B(ControlStates.Blend(p.Accent, p.OnAccent, .15));
        OkButton.Resources["ButtonBackgroundPressed"] = Ui.B(ControlStates.Blend(p.Accent, p.OnAccent, .28));
        OkButton.Resources["ButtonBorderBrushPointerOver"] = Ui.B(p.Accent);
        OkButton.Resources["ButtonBorderBrushPressed"] = Ui.B(p.Accent);
        OkButton.Resources["ButtonForegroundPointerOver"] = Ui.B(p.OnAccent);
        OkButton.Resources["ButtonForegroundPressed"] = Ui.B(p.OnAccent);
        OkButton.IsDefault = true;
        OkButton.Click += (_, _) => Ok();
        Ui.At(body, OkButton, 160, 170);
        CancelButton = Ui.Push(p, "Cancel", 80, Tokens.DialogButtonHeight);
        CancelButton.IsCancel = true;
        CancelButton.Click += (_, _) => Cancel();
        Ui.At(body, CancelButton, 244, 170);

        var root = new Grid { RowDefinitions = new RowDefinitions("Auto,*") };
        root.Children.Add(row);
        Grid.SetRow(body, 1);
        root.Children.Add(body);
        Content = new Grid
        {
            Children =
            {
                root,
                new Border { BorderBrush = Ui.B(p.PanelEdge), BorderThickness = new Thickness(1), IsHitTestVisible = false },
            },
        };
    }

    static int BlendIndex(string id)
    {
        for (var i = 0; i < LayerBlend.Modes.Length; i++) if (LayerBlend.Modes[i].Id == id) return i;
        throw new InvalidDataException($"LAYER_BLEND: the engine named {id}, which the list does not hold");
    }

    void ShowValues(Values v)
    {
        _showing = true;
        try
        {
            if (!NameField.IsFocused) NameField.Text = v.Name;
            VisibleBox.IsChecked = v.Visible;
            BlendPopup.SelectedIndex = BlendIndex(v.Blend);
            OpacitySlider.Value = v.Opacity;
            if (!OpacityField.IsFocused) OpacityField.Text = v.Opacity.ToString(CultureInfo.InvariantCulture);
        }
        finally
        {
            _showing = false;
        }
    }

    /// Sets the values and updates the preview.
    public void Set(Values v)
    {
        // A text change arrives a moment later, and the dialog may have ended by then.
        if (_finished) return;
        v = v with { Opacity = Math.Clamp(v.Opacity, 0, 255) };
        if (v == Current) return;
        Current = v;
        ShowValues(v);
        Preview();
    }

    void Preview()
    {
        // The engine refuses a preview that changes nothing, and it would keep showing the last
        // one. So values back at the start end the session, and the next change starts another.
        if (Current == Original)
        {
            if (SessionStarted) _editor.Run("preview.cancel");
            SessionStarted = false;
            return;
        }
        var parameters = EngineParams.Of(("index", LayerIndex), ("name", Current.Name), ("visible", Current.Visible), ("blend", Current.Blend), ("opacity", Current.Opacity));
        if (SessionStarted)
        {
            _editor.Run("preview.update", parameters);
            return;
        }
        SessionStarted = true;
        _editor.Run("preview.begin", EngineParams.Of(("command", "layer.properties"), ("params", parameters)));
    }

    void ControlChanged()
    {
        if (_showing) return;
        Set(Current with { Visible = VisibleBox.IsChecked == true, Blend = LayerBlend.Modes[Math.Max(BlendPopup.SelectedIndex, 0)].Id });
    }

    void SliderMoved()
    {
        if (_showing) return;
        var opacity = (int)Math.Round(OpacitySlider.Value);
        // LayerProperties.swift writes the field here whatever has focus, because ShowValues leaves
        // a focused field to the person typing in it.
        _showing = true;
        OpacityField.Text = opacity.ToString(CultureInfo.InvariantCulture);
        _showing = false;
        Set(Current with { Opacity = opacity });
    }

    void NameTyped()
    {
        if (!_showing) Set(Current with { Name = NameField.Text ?? "" });
    }

    /// A partly typed number waits, as the Mac's does. Leaving the field shows the real value.
    void OpacityTyped()
    {
        if (_showing || !int.TryParse(OpacityField.Text, NumberStyles.Integer, CultureInfo.InvariantCulture, out var typed)) return;
        Set(Current with { Opacity = typed });
    }

    public void Ok()
    {
        if (SessionStarted) _editor.Run("preview.commit");
        Finish(true);
        Close();
    }

    public void Cancel()
    {
        Finish(false);
        Close();
    }

    void Finish(bool ok)
    {
        if (_finished) return;
        _finished = true;
        if (!ok && SessionStarted) _editor.Run("preview.cancel");
        Finished?.Invoke(ok);
    }
}
