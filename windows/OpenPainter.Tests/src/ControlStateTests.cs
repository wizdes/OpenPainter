// Hover, press and focus on every control type, by a headless pointer and the real render, in light,
// dark and high contrast. The contrast test walks every window (PLAN 4.2). The real-window look is
// the harness's `alt-states` part.
using Avalonia;
using Avalonia.Controls;
using Avalonia.Controls.Presenters;
using Avalonia.Controls.Primitives;
using Avalonia.Headless;
using Avalonia.Headless.XUnit;
using Avalonia.Input;
using Avalonia.Media;
using Avalonia.Threading;
using Avalonia.VisualTree;
using OpenPainter.Bridge;
using OpenPainter.Shell;
using UiLook;
using Xunit;

namespace OpenPainter.Tests;

public sealed class ControlStateTests
{
    /// Colors from C:\Windows\Resources\Ease of Access Themes on this PC, Windows 11 build 26300:
    /// Window, WindowText, HotTrackingColor, GrayText, Hilight, HilightText, ButtonFace, ButtonText.
    static readonly (string Name, string Theme)[] ContrastThemes =
    [
        ("hcblack", Theme("32 32 32", "255 255 255", "117 233 252", "166 166 166", "142 227 240", "38 59 80", "32 32 32", "255 255 255")),
        ("hc1", Theme("45 50 54", "255 255 255", "112 235 222", "166 166 166", "161 191 222", "33 45 59", "45 50 54", "182 246 240")),
        ("hc2", Theme("0 0 0", "255 255 255", "128 128 255", "166 166 166", "214 180 253", "43 43 43", "0 0 0", "255 238 50")),
        ("hcwhite", Theme("255 250 239", "61 61 61", "28 94 117", "103 103 103", "144 57 9", "255 245 227", "255 250 239", "32 32 32")),
    ];

    static string Theme(string window, string text, string hot, string gray, string high, string highText, string face, string faceText) =>
        $"[Theme]\nDisplayName=x\n[Control Panel\\Colors]\nWindow={window}\nWindowText={text}\nHotTrackingColor={hot}\nGrayText={gray}\nHilight={high}\nHilightText={highText}\nButtonFace={face}\nButtonText={faceText}\n[Other]\nWindow=1 2 3\n";

    /// A window with a real engine document, in a theme or a contrast theme.
    sealed class Scene : IDisposable
    {
        readonly string? _file;
        public EngineClient Client { get; } = new();
        public Editor Editor { get; }
        public MainWindow Window { get; }

        public Scene(string look)
        {
            var contrast = ContrastThemes.FirstOrDefault(t => t.Name == look);
            if (contrast.Name is not null)
            {
                _file = Path.GetTempFileName();
                File.WriteAllText(_file, contrast.Theme);
            }
            Editor = new Editor(Client, Client.ImportPixels(Fixture.Width, Fixture.Height, Fixture.Flat(200, 40, 10)));
            Window = new MainWindow(Shells.NoOptions with { Theme = _file is null ? look : "dark", TestContrastTheme = _file }, headless: true) { Width = 1280, Height = 832 };
            Window.Show();
            Dispatcher.UIThread.RunJobs();
            Window.Bind(Editor);
            Editor.Run("layer.add", EngineParams.Of(("name", "Top")));
            Settle();
        }

        public void Settle()
        {
            Editor.Settle();
            Editor.Thumbnails.WaitForRenders();
            Editor.WaitForPreview();
            foreach (var panel in Window.Panels.Panels.Values) panel.UpdateLayout();
            Window.Dialog?.UpdateLayout();
            Dispatcher.UIThread.RunJobs();
        }

        public void Dispose()
        {
            Window.Close();
            Editor.Dispose();
            Client.Dispose();
            if (_file is not null) File.Delete(_file);
        }
    }

    static unsafe byte[] Frame(Window window, out PixelSize size)
    {
        window.UpdateLayout();
        Dispatcher.UIThread.RunJobs();
        AvaloniaHeadlessPlatform.ForceRenderTimerTick();
        Dispatcher.UIThread.RunJobs();
        var frame = window.CaptureRenderedFrame() ?? throw new InvalidOperationException("no rendered frame");
        size = frame.PixelSize;
        var bytes = new byte[size.Width * size.Height * 4];
        fixed (byte* first = bytes) frame.CopyPixels(new PixelRect(size), (nint)first, bytes.Length, size.Width * 4);
        return bytes;
    }

    /// The bytes of a control's box, with some room round it for a mark that sits outside.
    static byte[] Box(Window window, Control control, double room = 3)
    {
        var bytes = Frame(window, out var size);
        var at = control.TranslatePoint(default, window) ?? throw new InvalidOperationException($"{control} is not in the window");
        var left = Math.Max(0, (int)(at.X - room));
        var top = Math.Max(0, (int)(at.Y - room));
        var right = Math.Min(size.Width, (int)(at.X + control.Bounds.Width + room));
        var bottom = Math.Min(size.Height, (int)(at.Y + control.Bounds.Height + room));
        var box = new byte[(right - left) * (bottom - top) * 4];
        for (var y = top; y < bottom; y++)
            Array.Copy(bytes, (y * size.Width + left) * 4, box, (y - top) * (right - left) * 4, (right - left) * 4);
        return box;
    }

    static int Changed(byte[] a, byte[] b)
    {
        Assert.Equal(a.Length, b.Length);
        var count = 0;
        for (var i = 0; i < a.Length; i += 4)
            if (a[i] != b[i] || a[i + 1] != b[i + 1] || a[i + 2] != b[i + 2]) count++;
        return count;
    }

    static Point Middle(Control control, Window window) =>
        control.TranslatePoint(new Point(control.Bounds.Width / 2, control.Bounds.Height / 2), window) ?? throw new InvalidOperationException($"{control} is not in the window");

    /// A place in the window with no control under it.
    static readonly Point Away = new(1, 1);

    /// Hover, then press, each with a change on screen. Then the pointer leaves, and the look comes back.
    static void CheckPointer(string name, Window window, Control control, List<string> wrong, bool press = true)
    {
        var away = new Point(window.Bounds.Width - 2, 1);
        window.MouseMove(away, RawInputModifiers.None);
        Dispatcher.UIThread.RunJobs();
        var rest = Box(window, control);
        var at = Middle(control, window);
        window.MouseMove(at, RawInputModifiers.None);
        Dispatcher.UIThread.RunJobs();
        var hover = Box(window, control);
        if (Changed(rest, hover) == 0) wrong.Add($"{name}: hover draws nothing new");
        if (press)
        {
            window.MouseDown(at, MouseButton.Left, RawInputModifiers.LeftMouseButton);
            Dispatcher.UIThread.RunJobs();
            if (Changed(hover, Box(window, control)) == 0) wrong.Add($"{name}: press draws nothing new");
            window.MouseMove(away, RawInputModifiers.LeftMouseButton);
            window.MouseUp(away, MouseButton.Left, RawInputModifiers.None);
        }
        else window.MouseMove(away, RawInputModifiers.None);
        Dispatcher.UIThread.RunJobs();
    }

    static void CheckFocus(string name, Window window, Control control, List<string> wrong)
    {
        // A press above may have left focus here by the pointer. Moving it off with the pointer shows no ring.
        var other = window.GetVisualDescendants().OfType<InputElement>().First(x => x is not MenuItem && x.Focusable && x.IsEffectivelyEnabled && x.IsEffectivelyVisible && !ReferenceEquals(x, control) && !control.IsVisualAncestorOf(x) && !x.IsVisualAncestorOf(control));
        other.Focus(NavigationMethod.Pointer);
        Dispatcher.UIThread.RunJobs();
        var rest = Box(window, control);
        control.Focus(NavigationMethod.Tab);
        Dispatcher.UIThread.RunJobs();
        if (!control.IsFocused && !control.IsKeyboardFocusWithin) wrong.Add($"{name}: takes no keyboard focus");
        else if (Changed(rest, Box(window, control)) == 0) wrong.Add($"{name}: focus draws nothing new");
        other.Focus(NavigationMethod.Pointer);
        Dispatcher.UIThread.RunJobs();
    }

    static void CheckAllStates(string look)
    {
        using var s = new Scene(look);
        var w = s.Window;
        var panels = w.Panels;
        var wrong = new List<string>();

        // The main window.
        var wand = w.Options.Commands["wand"];
        CheckPointer("icon button in the options row", w, wand, wrong);
        CheckFocus("icon button in the options row", w, wand, wrong);
        CheckPointer("tool picker", w, w.Options.ToolPicker, wrong, press: false);
        CheckFocus("tool picker", w, w.Options.ToolPicker, wrong);
        var width = w.Options.Option<TextBox>(OptionBinding.BrushWidth);
        CheckPointer("text field", w, width, wrong, press: false);
        CheckFocus("text field", w, width, wrong);
        var item = w.Strip.Items[0];
        CheckPointer("image strip item", w, item, wrong);
        CheckFocus("image strip item", w, item, wrong);
        var file = w.TitleBar.Menus.Items.OfType<MenuItem>().First();
        CheckPointer("top menu", w, file, wrong, press: false);
        CheckPointer("caption button", w, w.TitleBar.MinimizeButton, wrong, press: false);

        // Tools.
        var tools = panels.Panels[PanelLayout.Tools];
        var cell = panels.Tools.Cells["eraser"];
        CheckPointer("tool cell", tools, cell, wrong);
        CheckFocus("tool cell", tools, cell, wrong);

        // History.
        var history = panels.Panels[PanelLayout.History];
        var text = panels.History.UndoButton;
        CheckPointer("text button", history, text, wrong);
        CheckFocus("text button", history, text, wrong);
        var step = panels.History.HistoryRows[^1];
        CheckPointer("history row", history, step, wrong);
        s.Settle();
        CheckFocus("history row", history, panels.History.HistoryRows[0], wrong);

        // Layers.
        var layers = panels.Panels[PanelLayout.Layers];
        var layerRow = panels.Layers.LayerRows[1];
        CheckPointer("layer row", layers, layerRow, wrong);
        s.Settle();
        CheckFocus("layer row", layers, panels.Layers.LayerRows[1], wrong);
        var eye = panels.Layers.LayerRows[1].Eye;
        CheckFocus("eye button", layers, eye, wrong);

        // Colors.
        var colors = panels.Panels[PanelLayout.Colors];
        var view = panels.Colors;
        CheckPointer("swatch pair", colors, view.Swatches, wrong);
        CheckFocus("swatch pair", colors, view.Swatches, wrong);
        CheckPointer("wheel", colors, view.Wheel, wrong);
        CheckFocus("wheel", colors, view.Wheel, wrong);
        CheckPointer("palette cell", colors, view.Swatchboard.Cells[5], wrong);
        CheckFocus("palette cell", colors, view.Swatchboard.Cells[5], wrong);
        CheckPointer("plain button", colors, view.MoreButton, wrong);
        CheckFocus("plain button", colors, view.MoreButton, wrong);
        CheckPointer("hex field", colors, view.HexField, wrong, press: false);
        CheckFocus("hex field", colors, view.HexField, wrong);
        view.SetExpanded(true);
        s.Settle();
        var slider = view.Channels[ColorChannel.Red].Slider;
        CheckPointer("color slider", colors, slider, wrong);
        CheckFocus("color slider", colors, slider, wrong);

        // The dialog's Fluent controls.
        var dialog = w.ShowLayerProperties(0) ?? throw new InvalidOperationException("no dialog");
        s.Settle();
        foreach (var control in dialog.GetVisualDescendants().OfType<Control>().Where(c => c is CheckBox or Slider or ComboBox).GroupBy(c => c.GetType()).Select(g => g.First()).ToList())
        {
            var kind = $"dialog {control.GetType().Name}";
            if (control is not ComboBox) CheckPointer(kind, dialog, control, wrong, press: control is CheckBox);
            CheckFocus(kind, dialog, control, wrong);
        }
        CheckPointer("OK", dialog, dialog.OkButton, wrong, press: false);
        CheckFocus("OK", dialog, dialog.OkButton, wrong);
        dialog.Cancel();
        s.Settle();
        Assert.True(wrong.Count == 0, string.Join("; ", wrong));
    }

    [AvaloniaFact] public void EveryControlTypeShowsItsStatesInLight() => CheckAllStates("light");
    [AvaloniaFact] public void EveryControlTypeShowsItsStatesInDark() => CheckAllStates("dark");
    [AvaloniaFact] public void EveryControlTypeShowsItsStatesInHighContrast() => CheckAllStates("hcblack");
    [AvaloniaFact] public void EveryControlTypeShowsItsStatesInLightHighContrast() => CheckAllStates("hcwhite");

    [AvaloniaFact]
    public void TheFocusRingOnlyShowsForTheKeyboard()
    {
        using var s = new Scene("light");
        var colors = s.Window.Panels.Panels[PanelLayout.Colors];
        var cell = s.Window.Panels.Colors.Swatchboard.Cells[5];
        var rest = Box(colors, cell);
        cell.Focus(NavigationMethod.Pointer);
        Dispatcher.UIThread.RunJobs();
        Assert.True(cell.IsFocused);
        Assert.Equal(0, Changed(rest, Box(colors, cell)));
        s.Window.Panels.Colors.Swatches.Focus(NavigationMethod.Pointer);
        Dispatcher.UIThread.RunJobs();
        Assert.False(cell.IsFocused);
        cell.Focus(NavigationMethod.Tab);
        Dispatcher.UIThread.RunJobs();
        Assert.NotEqual(0, Changed(rest, Box(colors, cell)));
    }

    // Contrast: PLAN 4.2, a unit test at 4.5 to 1 for each window in light and dark.

    const double TextContrast = 4.5;

    /// The color behind a control: the nearest solid fill above it.
    static Color? Behind(Visual start)
    {
        for (Visual? v = start; v is not null; v = v.GetVisualParent())
        {
            var brush = v switch
            {
                Avalonia.Controls.Panel panel => panel.Background,
                Border border => border.Background,
                ContentPresenter presenter => presenter.Background,
                TemplatedControl control => control.Background,
                _ => null,
            };
            if (brush is ISolidColorBrush { Color.A: 255 } solid) return solid.Color;
        }
        return null;
    }

    /// Words a person must read, in one window, with a contrast under the rule. Text in an off
    /// control is exempt under WCAG, and DisabledText has its own row in Tokens.ContrastPairs.
    static IEnumerable<string> LowContrast(string name, Visual root)
    {
        var seen = 0;
        foreach (var text in root.GetVisualDescendants().OfType<TextBlock>())
        {
            if (!text.IsEffectivelyVisible || !text.IsEffectivelyEnabled || string.IsNullOrWhiteSpace(text.Text)) continue;
            if (text.Foreground is not ISolidColorBrush front || Behind(text) is not { } back) continue;
            seen++;
            var ratio = Tokens.Contrast(front.Color, back);
            if (ratio < TextContrast) yield return $"{name}: \"{text.Text}\" {front.Color} on {back} is {ratio:F2} to 1";
        }
        foreach (var box in root.GetVisualDescendants().OfType<TextBox>())
        {
            if (!box.IsEffectivelyVisible || !box.IsEffectivelyEnabled || box.Foreground is not ISolidColorBrush front || Behind(box) is not { } back) continue;
            seen++;
            var ratio = Tokens.Contrast(front.Color, back);
            if (ratio < TextContrast) yield return $"{name}: a field {front.Color} on {back} is {ratio:F2} to 1";
            // Reproduced first in a real Desert window: selected text kept its dark color on the dark highlight.
            if (box.SelectionForegroundBrush is not ISolidColorBrush picked || box.SelectionBrush is not ISolidColorBrush fill) yield return $"{name}: a field has no selection colors, so Fluent's own show";
            else if (Tokens.Contrast(picked.Color, fill.Color) < TextContrast) yield return $"{name}: selected text {picked.Color} on {fill.Color} is {Tokens.Contrast(picked.Color, fill.Color):F2} to 1";
        }
        if (seen == 0) yield return $"{name}: no text was checked, so this check looks broken";
    }

    static void CheckWindows(string look)
    {
        using var s = new Scene(look);
        var w = s.Window;
        var wrong = new List<string>();
        wrong.AddRange(LowContrast($"{look} main window", w));
        foreach (var (id, panel) in w.Panels.Panels)
        {
            // Tools has icons and no words.
            if (id != PanelLayout.Tools) wrong.AddRange(LowContrast($"{look} {id} panel", panel));
            else wrong.AddRange(LowContrast($"{look} tools panel title", panel.TitleRow));
        }
        var dialog = w.ShowLayerProperties(0) ?? throw new InvalidOperationException("no dialog");
        s.Settle();
        wrong.AddRange(LowContrast($"{look} Layer Properties", dialog));
        dialog.Cancel();
        s.Settle();
        foreach (var top in w.TitleBar.Menus.Items.OfType<MenuItem>().Take(2))
        {
            top.Open();
            Dispatcher.UIThread.RunJobs();
            foreach (var item in top.Items.OfType<MenuItem>().Where(i => i.IsEffectivelyEnabled)) wrong.AddRange(LowContrast($"{look} {top.Header} menu item {item.Header}", item).Where(l => !l.EndsWith("looks broken", StringComparison.Ordinal)));
            top.Close();
            Dispatcher.UIThread.RunJobs();
        }
        foreach (var (pair, front, back) in ControlStates.Pairs(w.Palette))
        {
            var ratio = Tokens.Contrast(front, back);
            if (ratio < TextContrast) wrong.Add($"{look} {pair}: {front} on {back} is {ratio:F2} to 1");
        }
        Assert.Empty(wrong);
    }

    [AvaloniaFact] public void EveryWindowReadsAt45ToOneInLight() => CheckWindows("light");
    [AvaloniaFact] public void EveryWindowReadsAt45ToOneInDark() => CheckWindows("dark");
    [AvaloniaFact] public void EveryWindowReadsAt45ToOneInAquatic() => CheckWindows("hcblack");
    [AvaloniaFact] public void EveryWindowReadsAt45ToOneInDusk() => CheckWindows("hc1");
    [AvaloniaFact] public void EveryWindowReadsAt45ToOneInNightSky() => CheckWindows("hc2");
    [AvaloniaFact] public void EveryWindowReadsAt45ToOneInDesert() => CheckWindows("hcwhite");

    /// The walk must see a planted failure, or its passes mean nothing.
    [AvaloniaFact]
    public void AColorThatFailsTheRuleIsSeen()
    {
        var text = new TextBlock { Text = "Faint", Foreground = new SolidColorBrush(Color.FromRgb(0xA0, 0xA0, 0xA0)) };
        var window = new Window { Content = new Border { Background = Brushes.White, Child = text } };
        window.Show();
        Dispatcher.UIThread.RunJobs();
        var wrong = LowContrast("planted", window).ToList();
        Assert.Single(wrong);
        Assert.Contains("Faint", wrong[0]);
        text.Foreground = Brushes.Black;
        Assert.Empty(LowContrast("planted", window));
        window.Close();
    }

    /// The walk must reach the words in each window, not a handful.
    [AvaloniaFact]
    public void TheWalkReachesTheWordsOfEachWindow()
    {
        using var s = new Scene("light");
        int Words(Visual root) => root.GetVisualDescendants().OfType<TextBlock>().Count(t => t.IsEffectivelyVisible && t.Foreground is ISolidColorBrush && Behind(t) is not null && !string.IsNullOrWhiteSpace(t.Text));
        Assert.True(Words(s.Window) >= 20, $"main: {Words(s.Window)}");
        Assert.True(Words(s.Window.Panels.Panels[PanelLayout.Colors]) >= 3, "colors");
        Assert.True(Words(s.Window.Panels.Panels[PanelLayout.Layers]) >= 3, "layers");
        Assert.True(Words(s.Window.Panels.Panels[PanelLayout.History]) >= 3, "history");
    }

    // High contrast.

    [Fact]
    public void AThemeFileGivesItsSystemColors()
    {
        var colors = HighContrast.FromThemeText(ContrastThemes[0].Theme, "hcblack");
        Assert.Equal(Color.FromRgb(32, 32, 32), colors.Window);
        Assert.Equal(Color.FromRgb(255, 255, 255), colors.WindowText);
        Assert.Equal(Color.FromRgb(142, 227, 240), colors.Highlight);
        Assert.True(HighContrast.IsDark(colors));
        Assert.False(HighContrast.IsDark(HighContrast.FromThemeText(ContrastThemes[3].Theme, "hcwhite")));
    }

    [Fact]
    public void AThemeFileWithoutAColorFailsByName()
    {
        var error = Assert.Throws<InvalidDataException>(() => HighContrast.FromThemeText("[Control Panel\\Colors]\nWindow=1 2 3\n", "broken"));
        Assert.Contains("broken", error.Message);
        Assert.Contains("WindowText", error.Message);
    }

    /// A high-contrast palette uses only the colors the system names.
    [Fact]
    public void AHighContrastPaletteUsesOnlyTheSystemsColors()
    {
        foreach (var (name, theme) in ContrastThemes)
        {
            var s = HighContrast.FromThemeText(theme, name);
            var p = HighContrast.Palette(s);
            Assert.Equal(ControlStates.ContrastName, p.Name);
            var allowed = new[] { s.Window, s.WindowText, s.GrayText, s.Highlight, s.HighlightText };
            foreach (var color in new[] { p.Chrome, p.Options, p.Panel, p.Field, p.Ink, p.Muted, p.Disabled, p.Line, p.PanelLine, p.Stage, p.TitleBar, p.Accent, p.Wash, p.Control, p.ControlEdge, p.Plot, p.Knob, p.KnobRing, p.SliderValue, p.DisabledText, p.FieldBottom, p.PanelEdge, p.OnAccent, p.Handle })
                Assert.Contains(color, allowed);
            foreach (var (pair, front, back, least) in Tokens.ContrastPairs(p))
                Assert.True(Tokens.Contrast(front, back) >= least, $"{name} {pair}: {Tokens.Contrast(front, back):F2}");
        }
    }

    [AvaloniaFact]
    public void AHighContrastWindowDrawsItsSurfacesInTheSystemsColors()
    {
        using var s = new Scene("hcblack");
        var window = Color.FromRgb(32, 32, 32);
        Assert.Equal(window, Assert.IsAssignableFrom<ISolidColorBrush>(s.Window.Background).Color);
        Assert.Equal(window, Assert.IsAssignableFrom<ISolidColorBrush>(s.Window.Panels.Panels[PanelLayout.Colors].Background).Color);
        Assert.Equal(ControlStates.ContrastName, s.Window.Palette.Name);
        // The document's pixels and swatches stay real: a swatch keeps its exact bytes.
        Assert.Equal(new DocColor(0xCF, 0x68, 0x4D), s.Window.Tools.Primary);
    }

    /// This run never asks to change the system's setting.
    [Fact]
    public void TheAppNeverWritesTheSystemsHighContrastSetting()
    {
        var source = Repo.Read("windows/OpenPainter/src/Shell/HighContrast.cs");
        Assert.Contains("GetHighContrast = 0x0042", source);
        Assert.DoesNotContain("0x0043", source);
    }
}
