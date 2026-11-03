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
        var plain = w.Strip.Items[0].Image.Preview;
        Assert.False(w.TitleBar.IsModifiedShown);
        Press(w, Key.N, Ctrl | Shift);
        f.Settle();
        f.Run("image.invert_colors");
        Press(w, Key.N, Ctrl | Shift);
        f.Settle();
        Assert.Equal(4, f.Editor.Info.HistoryLabels.Count);
        Assert.True(w.TitleBar.IsModifiedShown);
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
        f.History.Jump(0);
        f.Settle();
        Assert.Equal(0, f.Editor.Info.HistoryIndex);
        Assert.True(f.History.HistoryRows[3].Undone);
        f.Editor.WaitForPreview();
        Dispatcher.UIThread.RunJobs();
        Assert.NotNull(w.Strip.Items[0].Image.Preview);
        Assert.NotNull(plain);
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
        var colors = w.Panels.Panels[PanelLayout.Colors];
        var hex = w.Panels.Colors.HexField;
        hex.Focus();
        hex.Text = "336699";
        Press(colors, Key.Enter);
        Assert.Equal(new DocColor(0x33, 0x66, 0x99), tools.Primary);
        Assert.True(w.CanvasInput.IsFocused);
        Press(w, Key.E);
        Assert.Equal("eraser", tools.Current);
        Dispatcher.UIThread.RunJobs();
        var view = w.Panels.Colors;
        view.Swatchboard.Pick(3, false);
        Assert.Equal(tools.Palette.Colors[3], tools.Primary);
    }

    [AvaloniaFact]
    public void Flow05ChooseAToolAndItsOptions()
    {
        using var f = new Fixture();
        var w = f.Window;
        var tools = w.Tools;
        w.CanvasInput.Focus();
        Press(w, Key.P);
        Assert.Equal("pencil", tools.Current);
        Assert.StartsWith("Pencil", w.Status.Hint.Text);
        Press(w, Key.B);
        Assert.Equal("paintbrush", tools.Current);
        Press(w, Key.OemCloseBrackets);
        Press(w, Key.OemCloseBrackets, Ctrl);
        Assert.Equal(8, tools.Settings.BrushWidth);
        Assert.Equal("8", w.Options.Option<TextBox>(OptionBinding.BrushWidth).Text);
        Press(w, Key.S);
        Press(w, Key.S);
        Assert.Equal("lassoSelect", tools.Current);
        Press(w, Key.S, Shift);
        Assert.Equal("rectangleSelect", tools.Current);
        Assert.Same(tools.CurrentTool, tools.CurrentTool);
        Assert.Equal(tools.CurrentTool, w.Options.ToolPicker.SelectedItem);
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
        w.Unit = MeasurementUnit.Inches;
        Assert.Equal(MeasurementUnit.Inches, w.Status.Unit);
        w.Unit = MeasurementUnit.Pixels;
    }

    [AvaloniaFact]
    public void Flow07ArrangeThePanels()
    {
        using var f = new Fixture();
        var w = f.Window;
        w.CanvasInput.Focus();
        foreach (var (key, id) in new[] { (Key.F5, PanelLayout.Tools), (Key.F6, PanelLayout.History), (Key.F7, PanelLayout.Layers), (Key.F8, PanelLayout.Colors) })
        {
            Press(w, key);
            Assert.False(w.Panels.IsShown(id), id);
            Press(w, key);
            Assert.True(w.Panels.IsShown(id), id);
        }
        Press(w, Key.OemTilde, Ctrl);
        Assert.True(w.Panels.Panels[PanelLayout.Tools].IsKeyboardFocusWithin || w.Panels.Panels[PanelLayout.Tools].IsFocused);
        var back = w.CyclePanels(forward: false);
        Assert.Same(w, back);
        w.Panels.SetShown(PanelLayout.Colors, false);
        w.Panels.ResetLayout();
        Assert.True(w.Panels.IsShown(PanelLayout.Colors));
    }

    [AvaloniaFact]
    public void Flow08WorkWithTheKeyboardAlone()
    {
        using var f = new Fixture();
        var w = f.Window;
        w.CanvasInput.Focus();
        Press(w, Key.N, Ctrl | Shift);
        f.Settle();
        // Into the Layers rows through Next Panel, then pick the lower layer.
        Press(w, Key.OemTilde, Ctrl);
        Press(w, Key.OemTilde, Ctrl);
        Press(w, Key.OemTilde, Ctrl);
        var layers = w.Panels.Panels[PanelLayout.Layers];
        Assert.Same(f.Layers.LayerRows[0], layers.FocusManager?.GetFocusedElement());
        Press(layers, Key.Down);
        Press(layers, Key.Enter);
        f.Settle();
        Assert.Equal(0, f.Editor.Info.ActiveLayer);
        // Alt and the menu's letter, an arrow, Enter: the first item that is on runs.
        Press(layers, Key.L, Alt);
        var menu = w.TitleBar.Menus.Items.OfType<MenuItem>().Single(m => (string?)m.Header == "Layers");
        Assert.True(menu.IsSubMenuOpen);
        Press(w, Key.A);
        var focused = w.FocusManager?.GetFocusedElement() as CatalogMenuItem;
        Assert.NotNull(focused);
        Assert.Equal("Add New Layer", focused.Header);
        Press(w, Key.Enter);
        f.Settle();
        Assert.Equal(3, f.Editor.Info.Layers.Count);
        // Esc closes a menu, and the keys come back to the document.
        w.OpenMenuByLetter('E', fromCanvas: false);
        Dispatcher.UIThread.RunJobs();
        Press(w, Key.Escape);
        Press(w, Key.Escape);
        Assert.False(w.TitleBar.Menus.Items.OfType<MenuItem>().Any(m => m.IsSubMenuOpen));
    }

    [AvaloniaFact]
    public void Flow09ChangeALayerInLayerProperties()
    {
        using var f = new Fixture();
        var w = f.Window;
        w.CanvasInput.Focus();
        Press(w, Key.F4);
        var d = w.Dialog ?? throw new InvalidOperationException("F4 opened no dialog");
        f.Settle();
        var steps = f.Editor.Info.HistoryLabels.Count;
        d.Set(d.Current with { Name = "Sky", Opacity = 128, Blend = "multiply" });
        f.Settle();
        d.Ok();
        f.Settle();
        Assert.Null(w.Dialog);
        var layer = f.Editor.Info.Layers[0];
        Assert.Equal(("Sky", 128, "multiply"), (layer.Name, layer.Opacity, layer.Blend));
        Assert.Equal(steps + 1, f.Editor.Info.HistoryLabels.Count);
        w.CanvasInput.Focus();
        Press(w, Key.Z, Ctrl);
        f.Settle();
        Assert.Equal("Background", f.Editor.Info.Layers[0].Name);
        // F4 then Cancel adds no step.
        Press(w, Key.Y, Ctrl);
        f.Settle();
        var count = f.Editor.Info.HistoryLabels.Count;
        Press(w, Key.F4);
        f.Settle();
        w.Dialog!.Cancel();
        f.Settle();
        Assert.Equal(count, f.Editor.Info.HistoryLabels.Count);
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
