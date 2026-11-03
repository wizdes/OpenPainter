// The critical flows of docs/windows/flows.md, one test each, against the real engine, through the
// keys, menus and panels a person uses. `cargo xtask check --tier milestone` runs each by name on the
// AOT build.
using System.Text.RegularExpressions;
using Avalonia;
using Avalonia.Controls;
using Avalonia.Headless;
using Avalonia.Headless.XUnit;
using Avalonia.Input;
using Avalonia.Threading;
using OpenPainter.Shell;
using Xunit;

namespace OpenPainter.Tests;

public sealed class FlowTests
{
    static void Press(TopLevel w, Key key, RawInputModifiers mods = RawInputModifiers.None)
    {
        w.KeyPress(key, mods, PhysicalKey.None, null);
        Dispatcher.UIThread.RunJobs();
    }

    const RawInputModifiers Ctrl = RawInputModifiers.Control, Shift = RawInputModifiers.Shift, Alt = RawInputModifiers.Alt;

    [AvaloniaFact]
    public void Flow01StartTheAppAndSeeTheDocument()
    {
        using var f = new Fixture();
        var w = f.Window;
        Assert.Equal("Untitled - OpenPainter", w.TitleBar.TitleText);
        Assert.Equal("Untitled", w.Strip.Items[0].Image.Name);
        Assert.True(w.Strip.Items[0].Image.Active);
        Assert.Equal($"{Fixture.Width} × {Fixture.Height}", w.Status.ImageValue.Text?.Replace(" px", "").Trim());
        foreach (var id in new[] { PanelLayout.Tools, PanelLayout.History, PanelLayout.Layers, PanelLayout.Colors })
        {
            Assert.True(w.Panels.IsShown(id), id);
            Assert.True(w.Panels.Panels[id].IsVisible, id);
        }
        Assert.Single(f.Layers.LayerRows);
        Assert.Single(f.History.HistoryRows);
    }

    [AvaloniaFact]
    public void Flow02BuildALayerStackFromTheMenus()
    {
        using var f = new Fixture();
        var w = f.Window;
        w.CanvasInput.Focus();
        Press(w, Key.N, Ctrl | Shift);
        f.Settle();
        Press(w, Key.D, Ctrl | Shift);
        f.Settle();
        Assert.Equal(3, f.Editor.Info.Layers.Count);
        Assert.Equal(2, f.Editor.Info.ActiveLayer);
        Press(w, Key.PageDown, Alt);
        f.Settle();
        Assert.Equal(1, f.Editor.Info.ActiveLayer);
        var moving = f.Editor.Info.Layers[1].Id;
        Item(w, "Layers > Move Layer to Top").RaiseEvent(new Avalonia.Interactivity.RoutedEventArgs(MenuItem.ClickEvent));
        f.Settle();
        Assert.Equal(moving, f.Editor.Info.Layers[2].Id);
        Item(w, "Layers > Toggle Layer Visibility").RaiseEvent(new Avalonia.Interactivity.RoutedEventArgs(MenuItem.ClickEvent));
        f.Settle();
        Assert.False(f.Editor.Info.Layers[2].Visible);
        Item(w, "Layers > Merge Layer Down").RaiseEvent(new Avalonia.Interactivity.RoutedEventArgs(MenuItem.ClickEvent));
        f.Settle();
        Assert.Equal(2, f.Editor.Info.Layers.Count);
        Press(w, Key.Delete, Ctrl | Shift);
        f.Settle();
        Assert.Single(f.Editor.Info.Layers);
        // The panel saw each step.
        Assert.Single(f.Layers.LayerRows);
        Assert.Equal(f.Editor.Info.HistoryLabels.Count, f.History.HistoryRows.Count);
    }

    [AvaloniaFact]
    public void Flow03WalkTheHistory()
    {
        using var f = new Fixture();
        var w = f.Window;
        w.CanvasInput.Focus();
        f.Editor.WaitForPreview();
        Dispatcher.UIThread.RunJobs();
        var plain = f.Editor.Preview!.Value.Rgba;
        var plainShown = w.Strip.Items[0].Image.Preview;
        Assert.NotNull(plainShown);
        Assert.False(w.TitleBar.IsModifiedShown);
        // The Adjustments menu comes in W11, so the one step that changes pixels is an engine command.
        f.Run("image.invert_colors");
        Press(w, Key.N, Ctrl | Shift);
        f.Settle();
        Press(w, Key.N, Ctrl | Shift);
        f.Settle();
        f.Editor.WaitForPreview();
        Dispatcher.UIThread.RunJobs();
        Assert.Equal(4, f.Editor.Info.HistoryLabels.Count);
        Assert.True(w.TitleBar.IsModifiedShown);
        var inverted = f.Editor.Preview!.Value.Rgba;
        var invertedShown = w.Strip.Items[0].Image.Preview;
        Assert.NotEqual(plain, inverted);
        Assert.NotSame(plainShown, invertedShown);
        Press(w, Key.Z, Ctrl);
        f.Settle();
        Assert.Equal(2, f.Editor.Info.HistoryIndex);
        Press(w, Key.Y, Ctrl);
        f.Settle();
        Assert.Equal(3, f.Editor.Info.HistoryIndex);
        Press(w, Key.Z, Ctrl);
        f.Settle();
        Press(w, Key.Z, Ctrl | Shift);
        f.Settle();
        Assert.Equal(3, f.Editor.Info.HistoryIndex);
        // A press and release on the first History row.
        f.Press(f.History.HistoryRows[0]);
        f.Settle();
        Assert.Equal(0, f.Editor.Info.HistoryIndex);
        Assert.True(f.History.HistoryRows[3].Undone);
        // The modified mark clears, and the strip shows the first picture again.
        Assert.False(w.TitleBar.IsModifiedShown);
        f.Editor.WaitForPreview();
        Dispatcher.UIThread.RunJobs();
        Assert.Equal(plain, f.Editor.Preview!.Value.Rgba);
        Assert.NotSame(invertedShown, w.Strip.Items[0].Image.Preview);
    }

    [AvaloniaFact]
    public void Flow04ChooseColors()
    {
        using var f = new Fixture();
        var w = f.Window;
        var tools = w.Tools;
        w.CanvasInput.Focus();
        Press(w, Key.D);
        Assert.Equal(new DocColor(0, 0, 0), tools.Primary);
        Press(w, Key.X);
        Assert.Equal(new DocColor(255, 255, 255), tools.Primary);
        Press(w, Key.C);
        Assert.Equal(ColorSlot.Secondary, tools.ActiveSlot);
        Press(w, Key.C);
        Assert.Equal(ColorSlot.Primary, tools.ActiveSlot);
        var colors = w.Panels.Panels[PanelLayout.Colors];
        var hex = w.Panels.Colors.HexField;
        hex.Focus();
        hex.SelectAll();
        colors.KeyTextInput("336699");
        Dispatcher.UIThread.RunJobs();
        Assert.Equal("336699", hex.Text);
        Press(colors, Key.Enter);
        Assert.Equal(new DocColor(0x33, 0x66, 0x99), tools.Primary);
        // The keys came back to the canvas: the same letter types no more and picks a tool.
        Assert.True(w.CanvasInput.IsFocused);
        Press(w, Key.E);
        Assert.Equal("eraser", tools.Current);
        // A press and release on a palette swatch.
        Dispatcher.UIThread.RunJobs();
        Gesture.Click(w.Panels.Colors.Swatchboard.Cells[3], new Point(12, 12));
        Assert.Equal(tools.Palette.Colors[3], tools.Primary);
    }

    [AvaloniaFact]
    public void Flow05ChooseAToolAndItsOptions()
    {
        using var f = new Fixture();
        var w = f.Window;
        var tools = w.Tools;
        w.CanvasInput.Focus();
        void OptionsRowFollows() => Assert.Equal(tools.CurrentTool, w.Options.ToolPicker.SelectedItem);
        Press(w, Key.P);
        Assert.Equal("pencil", tools.Current);
        Assert.StartsWith("Pencil", w.Status.Hint.Text);
        OptionsRowFollows();
        Press(w, Key.B);
        Assert.Equal("paintbrush", tools.Current);
        OptionsRowFollows();
        Press(w, Key.OemCloseBrackets);
        Press(w, Key.OemCloseBrackets, Ctrl);
        Assert.Equal(8, tools.Settings.BrushWidth);
        Assert.Equal("8", w.Options.Option<TextBox>(OptionBinding.BrushWidth).Text);
        Press(w, Key.OemOpenBrackets);
        Assert.Equal(7, tools.Settings.BrushWidth);
        Assert.Equal("7", w.Options.Option<TextBox>(OptionBinding.BrushWidth).Text);
        // Repeat presses walk the group, and Shift walks it the other way.
        Press(w, Key.S);
        Assert.Equal("rectangleSelect", tools.Current);
        OptionsRowFollows();
        Press(w, Key.S);
        Assert.Equal("lassoSelect", tools.Current);
        OptionsRowFollows();
        Press(w, Key.S, Shift);
        Assert.Equal("rectangleSelect", tools.Current);
        OptionsRowFollows();
    }

    [AvaloniaFact]
    public void Flow06MoveAroundTheImage()
    {
        using var f = new Fixture();
        var w = f.Window;
        w.View.SetDocumentSize(new Size(4096, 3072));
        Dispatcher.UIThread.RunJobs();
        w.CanvasInput.Focus();
        Press(w, Key.D0, Ctrl);
        Assert.Equal(1, w.View.View.Zoom);
        Press(w, Key.OemPlus, Ctrl);
        Assert.Equal(2, w.View.View.Zoom);
        Press(w, Key.OemMinus, Ctrl);
        Assert.Equal(1, w.View.View.Zoom);
        Press(w, Key.B, Ctrl);
        Assert.NotEqual(1, w.View.View.Zoom);
        Choose(w, "View > Inches");
        Assert.Equal(MeasurementUnit.Inches, w.Status.Unit);
        Assert.Equal(MeasurementUnit.Inches, w.Unit);
        Choose(w, "View > Pixels");
        Assert.Equal(MeasurementUnit.Pixels, w.Status.Unit);
    }

    [AvaloniaFact]
    public void Flow07ArrangeThePanels()
    {
        using var f = new Fixture();
        var w = f.Window;
        w.CanvasInput.Focus();
        var frames = w.Panels.ShownFrames().ToList();
        foreach (var (key, id) in new[] { (Key.F5, PanelLayout.Tools), (Key.F6, PanelLayout.History), (Key.F7, PanelLayout.Layers), (Key.F8, PanelLayout.Colors) })
        {
            Press(w, key);
            Assert.False(w.Panels.IsShown(id), id);
            Press(w, key);
            Assert.True(w.Panels.IsShown(id), id);
        }
        // Next Panel moves the keyboard to the Tools panel, and Previous Panel brings it back.
        Press(w, Key.OemTilde, Ctrl);
        var tools = w.Panels.Panels[PanelLayout.Tools];
        Assert.NotNull(tools.FocusManager?.GetFocusedElement());
        Press(tools, Key.OemTilde, Ctrl | Shift);
        Assert.True(w.CanvasInput.IsFocused);
        Press(w, Key.F8);
        Assert.False(w.Panels.IsShown(PanelLayout.Colors));
        Choose(w, "Window > Reset Panel Layout");
        Assert.True(w.Panels.IsShown(PanelLayout.Colors));
        Assert.True(frames.SequenceEqual(w.Panels.ShownFrames()), "Reset Panel Layout left a panel in another place");
    }

    [AvaloniaFact]
    public void Flow08WorkWithTheKeyboardAlone()
    {
        using var f = new Fixture();
        var w = f.Window;
        w.CanvasInput.Focus();
        Press(w, Key.N, Ctrl | Shift);
        f.Settle();
        // Next Panel reaches the Layers panel, and Tab goes on to the lower row.
        Press(w, Key.OemTilde, Ctrl);
        Press(w.Panels.Panels[PanelLayout.Tools], Key.OemTilde, Ctrl);
        Press(w.Panels.Panels[PanelLayout.History], Key.OemTilde, Ctrl);
        var layers = w.Panels.Panels[PanelLayout.Layers];
        Assert.Same(f.Layers.LayerRows[0], layers.FocusManager?.GetFocusedElement());
        var tabs = 0;
        while (!ReferenceEquals(layers.FocusManager?.GetFocusedElement(), f.Layers.LayerRows[1]))
        {
            Assert.True(++tabs < 8, "Tab did not reach the lower row");
            Press(layers, Key.Tab);
        }
        // The arrow keys move between the rows, and Enter picks one.
        Press(layers, Key.Up);
        Assert.Same(f.Layers.LayerRows[0], layers.FocusManager?.GetFocusedElement());
        Press(layers, Key.Down);
        Assert.Same(f.Layers.LayerRows[1], layers.FocusManager?.GetFocusedElement());
        Press(layers, Key.Enter);
        f.Settle();
        Assert.Equal(0, f.Editor.Info.ActiveLayer);
        Assert.Same(f.Layers.LayerRows[1], layers.FocusManager?.GetFocusedElement());
        // Alt with the menu's letter, as the keys really come: Alt down, the letter, Alt up.
        layers.KeyPressQwerty(PhysicalKey.AltLeft, Alt);
        Press(layers, Key.L, Alt);
        layers.KeyReleaseQwerty(PhysicalKey.AltLeft, RawInputModifiers.None);
        Dispatcher.UIThread.RunJobs();
        var menu = w.TitleBar.Menus.Items.OfType<MenuItem>().Single(m => (string?)m.Header == "Layers");
        Assert.True(w.TitleBar.Menus.IsOpen);
        Assert.True(menu.IsSubMenuOpen);
        // An arrow moves to the first item that is on, and Enter runs it.
        Press(w, Key.Down);
        var focused = w.FocusManager?.GetFocusedElement() as CatalogMenuItem;
        Assert.NotNull(focused);
        Assert.Equal("Add New Layer", focused.Header);
        Press(w, Key.Enter);
        f.Settle();
        Assert.Equal(3, f.Editor.Info.Layers.Count);
        Assert.False(w.TitleBar.Menus.IsOpen);
        // A letter moves too.
        layers.KeyPressQwerty(PhysicalKey.AltLeft, Alt);
        Press(layers, Key.L, Alt);
        layers.KeyReleaseQwerty(PhysicalKey.AltLeft, RawInputModifiers.None);
        Dispatcher.UIThread.RunJobs();
        Press(w, Key.D);
        focused = w.FocusManager?.GetFocusedElement() as CatalogMenuItem;
        Assert.NotNull(focused);
        Assert.StartsWith("D", (string)focused.Header!);
        // Esc closes the open menu, then the bar.
        Press(w, Key.Escape);
        Assert.False(menu.IsSubMenuOpen);
        Press(w, Key.Escape);
        Assert.False(w.TitleBar.Menus.IsOpen);
        // The keys came back: a row still has the keyboard, and a tool letter works from the panel.
        Assert.IsType<LayerRow>(layers.FocusManager?.GetFocusedElement());
        Press(layers, Key.P);
        Assert.Equal("pencil", w.Tools.Current);
    }

    [AvaloniaFact]
    public void Flow09ChangeALayerInLayerProperties()
    {
        using var f = new Fixture();
        var w = f.Window;
        w.CanvasInput.Focus();
        var pixels = f.Pixels();
        Press(w, Key.F4);
        var d = w.Dialog ?? throw new InvalidOperationException("F4 opened no dialog");
        f.Settle();
        var steps = f.Editor.Info.HistoryLabels.Count;
        // Everything by the keyboard: the name has focus with its text chosen, so typing replaces it.
        Assert.True(d.NameField.IsFocused);
        d.KeyTextInput("Sky");
        Dispatcher.UIThread.RunJobs();
        Assert.Equal("Sky", d.NameField.Text);
        void TabTo(Control control)
        {
            var tabs = 0;
            while (!control.IsFocused)
            {
                Assert.True(++tabs < 10, "Tab did not reach the control");
                Press(d, Key.Tab);
            }
        }
        TabTo(d.BlendPopup);
        Press(d, Key.Down);
        Assert.Equal("multiply", d.Current.Blend);
        TabTo(d.OpacityField);
        d.OpacityField.SelectAll();
        d.KeyTextInput("128");
        Dispatcher.UIThread.RunJobs();
        Assert.Equal(("Sky", 128, "multiply"), (d.Current.Name, d.Current.Opacity, d.Current.Blend));
        f.Settle();
        // Up to here the document holds nothing of this.
        Assert.Equal("Background", f.Editor.Info.Layers[0].Name);
        Assert.Equal(pixels, f.Pixels());
        Press(d, Key.Enter);
        f.Settle();
        Assert.Null(w.Dialog);
        var layer = f.Editor.Info.Layers[0];
        Assert.Equal(("Sky", 128, "multiply"), (layer.Name, layer.Opacity, layer.Blend));
        Assert.Equal(steps + 1, f.Editor.Info.HistoryLabels.Count);
        w.CanvasInput.Focus();
        Press(w, Key.Z, Ctrl);
        f.Settle();
        Assert.Equal("Background", f.Editor.Info.Layers[0].Name);
        // F4, a change, then Esc adds no step and changes nothing.
        Press(w, Key.Y, Ctrl);
        f.Settle();
        var count = f.Editor.Info.HistoryLabels.Count;
        var index = f.Editor.Info.HistoryIndex;
        Press(w, Key.F4);
        f.Settle();
        d = w.Dialog ?? throw new InvalidOperationException("F4 opened no dialog");
        d.KeyTextInput("Not kept");
        Dispatcher.UIThread.RunJobs();
        f.Settle();
        Assert.True(d.SessionStarted);
        Press(d, Key.Escape);
        f.Settle();
        Assert.Null(w.Dialog);
        Assert.Equal(count, f.Editor.Info.HistoryLabels.Count);
        Assert.Equal(index, f.Editor.Info.HistoryIndex);
        Assert.Equal("Sky", f.Editor.Info.Layers[0].Name);
    }

    /// Runs a menu item as a click on it does. A real pointer on a menu stays owed from W7.
    static void Choose(MainWindow w, string path)
    {
        Item(w, path).RaiseEvent(new Avalonia.Interactivity.RoutedEventArgs(MenuItem.ClickEvent));
        Dispatcher.UIThread.RunJobs();
    }

    static CatalogMenuItem Item(MainWindow w, string path)
    {
        static IEnumerable<CatalogMenuItem> All(IEnumerable<object?> items)
        {
            foreach (var item in items.OfType<MenuItem>())
            {
                if (item is CatalogMenuItem c) yield return c;
                foreach (var inner in All(item.Items)) yield return inner;
            }
        }
        return All(w.TitleBar.Menus.Items).Single(i => i.Entry.Path == path);
    }
}

/// Holds docs/windows/flows.md to the tests above.
public sealed partial class FlowListTests
{
    [GeneratedRegex(@"\[AvaloniaFact\]\s+public void Flow(\d\d)")] private static partial Regex FlowMark();

    static List<string[]> Rows() => ShortcutDoc.Table(Repo.Read("docs/windows/flows.md"), "# Critical flows");

    [Fact]
    public void TheListHas9To12NamedFlowsWithStepsAMilestoneAndAWayToRun()
    {
        var rows = Rows();
        Assert.InRange(rows.Count, 9, 12);
        for (var i = 0; i < rows.Count; i++)
        {
            Assert.Equal(6, rows[i].Length);
            Assert.Equal((i + 1).ToString(), rows[i][0]);
            Assert.True(rows[i][1].Length > 5, $"flow {i + 1} has no name");
            Assert.True(rows[i][2].Length > 10, $"flow {i + 1} has no steps");
            Assert.Matches(@"^W\d+$", rows[i][3]);
            Assert.Contains("headless", rows[i][4]);
        }
    }

    [Fact]
    public void AFlowThatRunsNowHasItsTestAndATestHasItsRow()
    {
        var rows = Rows();
        var tests = FlowMark().Matches(Repo.Read("windows/OpenPainter.Tests/src/FlowTests.cs")).Select(m => int.Parse(m.Groups[1].Value)).ToList();
        var running = rows.Where(r => r[5] == "yes").Select(r => int.Parse(r[0])).ToList();
        Assert.Equal(running, tests);
        foreach (var row in rows)
        {
            var milestone = int.Parse(row[3][1..]);
            Assert.True(row[5] == (milestone <= 8 ? "yes" : "no"), $"flow {row[0]} says {row[5]} and comes in {row[3]}");
        }
    }
}
