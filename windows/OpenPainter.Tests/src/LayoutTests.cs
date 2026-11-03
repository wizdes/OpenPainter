// Stage 2's layout with no real window: the rows, the command row, the options row, the
// strip, the panels, the zoom controls and the icons. Words and order are read from the
// Mac's source, so they cannot drift.
using System.Security.Cryptography;
using System.Text.RegularExpressions;
using Avalonia;
using Avalonia.Controls;
using Avalonia.Controls.Chrome;
using Avalonia.Headless;
using Avalonia.Headless.XUnit;
using Avalonia.Input;
using Avalonia.Interactivity;
using Avalonia.Threading;
using Avalonia.VisualTree;
using OpenPainter.Canvas;
using OpenPainter.Catalog;
using OpenPainter.Shell;
using UiLook;
using Xunit;
using CommandBar = OpenPainter.Shell.CommandBar;

namespace OpenPainter.Tests;

public sealed class LayoutTests
{
    const string MacShell = "mac/OpenPainterKit/Sources/Shell/";

    static Rect BoundsIn(Control control, Visual root)
    {
        var at = control.TranslatePoint(default, root);
        Assert.True(at.HasValue, $"{control.GetType().Name} is not in the window");
        return new Rect(at.Value, control.Bounds.Size);
    }

    static void Settle(Window window)
    {
        for (var i = 0; i < 4; i++)
        {
            Dispatcher.UIThread.RunJobs();
            window.UpdateLayout();
        }
    }

    static MainWindow Ready(string? theme = null, double width = 1280, double height = 832)
    {
        Icons.Folder = Repo.File("windows/assets/icons");
        var w = Shells.Open(theme, width, height);
        Settle(w);
        return w;
    }

    static void Click(Button button) => button.RaiseEvent(new RoutedEventArgs(Button.ClickEvent));

    static MacCatalog Catalog() => MacCatalog.Embedded();

    // Rows.

    [AvaloniaFact]
    public void TheRowsStackAtTheLooksHeights()
    {
        var w = Ready();
        Assert.Equal(new Rect(0, 32, 1280, 36), BoundsIn(w.Options, w));
        Assert.Equal(new Rect(0, 68, 1280, 64), BoundsIn(w.Strip, w));
        Assert.Equal(new Rect(0, 132, 1280, 672), BoundsIn(w.Stage, w));
        Assert.Equal(804, BoundsIn(w.Status, w).Y);
        w.Close();
    }

    [AvaloniaFact]
    public void OnlyTheStageGrowsWithTheWindow()
    {
        var w = Ready(width: 1600, height: 1000);
        Assert.Equal(new Rect(0, 132, 1600, 1000 - 132 - 28), BoundsIn(w.Stage, w));
        Assert.Equal(36, w.Options.Bounds.Height);
        Assert.Equal(64, w.Strip.Bounds.Height);
        w.Close();
    }

    [AvaloniaFact]
    public void TheCanvasStopNeverTakesAClickFromTheRowsItSpans()
    {
        var w = Ready();
        var button = w.Options.Commands["wand"];
        var hit = w.InputHitTest(BoundsIn(button, w).Center);
        Assert.NotSame(w.CanvasInput, hit);
        Assert.NotSame(w.CanvasInput, w.InputHitTest(BoundsIn(w.Strip, w).Center));
        w.Close();
    }

    [AvaloniaFact]
    public void TabGoesThroughTheRowsInReadingOrder()
    {
        var w = Ready();
        w.Options.ToolPicker.Focus(NavigationMethod.Tab);
        var seen = new List<object?>();
        for (var i = 0; i < 40; i++)
        {
            w.KeyPress(Key.Tab, RawInputModifiers.None, PhysicalKey.Tab, null);
            Settle(w);
            var at = w.FocusManager?.GetFocusedElement();
            seen.Add(at);
            if (ReferenceEquals(at, w.CanvasInput)) break;
        }
        Assert.Same(w.CanvasInput, seen[^1]);
        Assert.IsType<ImageStripItem>(seen[^2]);
        w.KeyPress(Key.Tab, RawInputModifiers.None, PhysicalKey.Tab, null);
        Settle(w);
        Assert.True(w.Status.IsVisualAncestorOf((Visual)w.FocusManager!.GetFocusedElement()!));
        w.Close();
    }

    // The command row.

    [AvaloniaFact]
    public void TheCommandRowIsTheMacsSevenThenMore()
    {
        var source = Repo.Read(MacShell + "CommandBar.swift");
        var bar = source[source.IndexOf("static let barCommands", StringComparison.Ordinal)..source.IndexOf("static let moreCommands", StringComparison.Ordinal)];
        var keys = Regex.Matches(bar, "key: \"(\\w+)\"").Select(m => m.Groups[1].Value).ToList();
        Assert.Equal(["new", "open", "save", "undo", "redo", "crop", "wand"], keys);

        var w = Ready();
        var row = w.Options;
        Assert.Equal(keys, row.Commands.Keys);
        var xs = keys.Select(k => BoundsIn(row.Commands[k], w).X).Append(BoundsIn(row.MoreCommands, w).X).ToList();
        Assert.Equal(12, xs[0]);
        var size = Tokens.CommandTarget;
        // Groups of 3, 2 and 2, then More close to the last, as W6 draws them.
        Assert.Equal(new[] { 6.0, 6, 12, 6, 12, 6, 6 }, xs.Zip(xs.Skip(1), (a, b) => b - a - size));
        foreach (var k in keys) Assert.Equal(size, row.Commands[k].Bounds.Width);
        w.Close();
    }

    [AvaloniaFact]
    public void CommandsHaveTheMacsWordsAndTheCatalogsKeys()
    {
        var source = Repo.Read(MacShell + "CommandBar.swift");
        var menus = WindowsMenus.Build(Catalog());
        var bar = CommandBar.Bar(menus, Catalog().Tools).SelectMany(g => g).ToList();
        foreach (var c in bar.Where(c => c.Key != "wand"))
        {
            var row = Regex.Match(source, $"key: \"{c.Key}\", symbol: \"[^\"]*\", title: \"([^\"]*)\", shortcut: \"([^\"]*)\"");
            Assert.True(row.Success, $"no row for {c.Key}");
            Assert.Equal(row.Groups[1].Value, c.Title);
            Assert.Equal(WindowsKeys.FromMac(row.Groups[2].Value), c.Shortcut);
        }
        var wand = bar.Single(c => c.Key == "wand");
        Assert.Equal(("Magic Wand", "Magic Wand (S)"), (wand.Title, wand.Tooltip));
        Assert.Equal("New (Ctrl+N)", bar[0].Tooltip);
    }

    /// Each icon must be the one W6's record gives that use.
    [AvaloniaFact]
    public void CommandIconsAreTheRecordsIcons()
    {
        var record = Repo.Read("docs/windows/icons.toml");
        string IconFor(string use)
        {
            var m = Regex.Match(record, $"use = \"{Regex.Escape(use)}\"\\n(?:[^\\n]*\\n)*?file = \"windows/assets/icons/([^\"]+)\\.svg\"");
            Assert.True(m.Success, $"no row for {use}");
            return m.Groups[1].Value;
        }
        var menus = WindowsMenus.Build(Catalog());
        foreach (var c in CommandBar.Bar(menus, Catalog().Tools).SelectMany(g => g)) Assert.Equal(IconFor($"bar:{c.Key}"), c.Icon);
        foreach (var c in CommandBar.More(menus).OfType<BarCommand>()) Assert.Equal(IconFor($"bar:{c.Key}"), c.Icon);
    }

    [AvaloniaFact]
    public void CommandsThatDoNotWorkYetAreOffAndTheWandWorks()
    {
        var w = Ready();
        foreach (var key in new[] { "new", "open", "save", "undo", "redo", "crop" }) Assert.False(w.Options.Commands[key].IsEnabled, key);
        var wand = w.Options.Commands["wand"];
        Assert.True(wand.IsEnabled);
        Assert.False(wand.On);
        Click(wand);
        Settle(w);
        Assert.Equal(ToolState.MagicWand, w.Tools.Current);
        Assert.True(w.Options.Commands["wand"].On);
        w.Tools.Current = ToolState.Pan;
        Assert.False(w.Options.Commands["wand"].On);
        w.Close();
    }

    [AvaloniaFact]
    public void TheMoreMenuIsTheMacsCommandsThenThePanels()
    {
        var source = Repo.Read(MacShell + "CommandBar.swift");
        var more = source[source.IndexOf("static let moreCommands", StringComparison.Ordinal)..source.IndexOf("/// Builds the buttons", StringComparison.Ordinal)];
        var titles = Regex.Matches(more, "title: \"([^\"]+)\"").Select(m => m.Groups[1].Value).ToList();
        Assert.Equal(["Print", "Cut", "Copy", "Paste", "Deselect", "Pixel Grid", "Rulers"], titles);

        var w = Ready();
        var items = w.Options.MoreMenu.Items.OfType<MenuItem>().Select(i => (string)i.Header!).ToList();
        Assert.Equal(titles.Concat(Catalog().Panels.Select(p => p.Title)), items);
        Assert.Equal(4, w.Options.MoreMenu.Items.OfType<Separator>().Count());
        foreach (var item in w.Options.MoreMenu.Items.OfType<MenuItem>())
            Assert.Equal(Catalog().Panels.Any(p => p.Title == (string)item.Header!), item.IsEnabled);
        w.Close();
    }

    // The options row.

    /// The words each row must hold, in order, from the Mac's rebuild().
    [Fact]
    public void TheRowsHoldTheMacsWordsInTheMacsOrder()
    {
        var source = Repo.Read(MacShell + "OptionsBarView.swift");
        string Case(string name, string next)
        {
            var from = source.IndexOf($"case {name}", source.IndexOf("switch set {", StringComparison.Ordinal), StringComparison.Ordinal);
            var to = source.IndexOf($"case {next}", from, StringComparison.Ordinal);
            Assert.True(from > 0 && to > from, $"OptionsBarView.rebuild changed near {name}");
            return source[from..to];
        }
        static List<string> Labels(string text) => Regex.Matches(text, "Labels\\.label\\(\"([^\"]+)\"\\)").Select(m => m.Groups[1].Value).ToList();
        List<string> Ours(string tool, string options) => OptionRows.For(tool, options).SelectMany(g => g).OfType<OptionLabel>().Select(l => l.Text).ToList();

        var brushCase = Case(".brush, .eraser", ".pencil");
        Assert.Equal(Labels(brushCase), Ours("paintbrush", "brush"));
        Assert.Equal(Labels(brushCase).Where(l => l != "Blend mode"), Ours("eraser", "eraser"));
        Assert.Equal(Labels(Case(".pencil", ".flood")), Ours("pencil", "pencil"));
        Assert.Equal(Labels(Case(".flood", ".picker")), Ours("magicWand", "flood"));
        Assert.Equal(Labels(Case(".picker", ".text")), Ours("colorPicker", "picker"));
        Assert.Equal(["This tool has no options."], Ours("zoom", "none"));

        var text = Repo.Read(MacShell + "TextOptionsControls.swift");
        var add = text[text.IndexOf("func add(", StringComparison.Ordinal)..text.IndexOf("func sync()", StringComparison.Ordinal)];
        var labels = Labels(add);
        Assert.Equal(labels, Ours("text", "text"));

        var select = Repo.Read(MacShell + "SelectCropTool.swift");
        Assert.Contains("[Labels.label(\"Selection mode\"), modePopup, Labels.gap(8), Labels.label(\"Style\"), stylePopup, Labels.gap(8)]", select);
        Assert.Equal(["Selection mode", "Style"], Ours("ellipseSelect", "none"));
        Assert.Equal(["Selection mode"], Ours("lassoSelect", "none"));
        Assert.Contains("Labels.label(\"Resampling\"), resamplingPopup", Repo.Read(MacShell + "MoveTool.swift"));
        Assert.Equal(["Resampling"], Ours("moveSelection", "none"));
    }

    [Fact]
    public void EveryCatalogToolHasARowAndAnIcon()
    {
        Assert.Equal(19, Catalog().Tools.Count);
        foreach (var tool in Catalog().Tools)
        {
            Assert.NotEmpty(OptionRows.For(tool.Id, tool.Options));
            Assert.NotNull(tool.Icon);
            Assert.True(File.Exists(Repo.File($"windows/assets/icons/{tool.Icon}.svg")), tool.Icon);
        }
    }

    [Fact]
    public void AnUnknownOptionSetFailsByName()
    {
        var e = Assert.Throws<InvalidDataException>(() => OptionRows.For("x", "nonsense"));
        Assert.Contains("nonsense", e.Message);
    }

    [AvaloniaFact]
    public void ThePickerListsTheCatalogsToolsAndChangesTheRow()
    {
        var w = Ready();
        var picker = w.Options.ToolPicker;
        Assert.Equal(Catalog().Tools.Select(t => t.Label), picker.Items.Cast<CatalogTool>().Select(t => t.Label));
        Assert.Equal("paintbrush", ((CatalogTool)picker.SelectedItem!).Id);
        Assert.Equal("Paintbrush: drag to paint", w.Status.Hint.Text);
        picker.SelectedItem = picker.Items.Cast<CatalogTool>().Single(t => t.Id == "ellipseSelect");
        Settle(w);
        Assert.Equal("ellipseSelect", w.Tools.Current);
        Assert.Same(picker, w.Options.ToolPicker);
        Assert.Equal(3, w.Options.Groups.Count);
        Assert.Equal("Ellipse Select: drag an ellipse; Ctrl adds, Alt subtracts, Shift makes a circle", w.Status.Hint.Text);
        w.Close();
    }

    [AvaloniaFact]
    public void TheHintsAreTheMacsInWindowsWords()
    {
        var source = Repo.Read(MacShell + "ToolState.swift");
        var from = source.IndexOf("public var hint: String", StringComparison.Ordinal);
        var body = source[from..source.IndexOf("/// The next tool for a key press", StringComparison.Ordinal)];
        var cases = Regex.Matches(body, "case \\.(\\w+):\\s*return \"([^\"]+)\"");
        Assert.True(cases.Count >= 11, $"only {cases.Count} hints read");
        var tools = Catalog().Tools;
        foreach (Match m in cases)
        {
            var hint = ToolState.Hint(tools.Single(t => t.Id == m.Groups[1].Value));
            Assert.Equal(ToolState.WindowsWords(m.Groups[2].Value), hint);
            Assert.DoesNotContain("Command", hint);
            Assert.DoesNotContain("Option", hint);
        }
        Assert.Equal("Gradient is coming soon", ToolState.Hint(tools.Single(t => t.Id == "gradient")));
        Assert.Equal("Paintbrush: drag to paint", ToolState.Hint(tools.Single(t => t.Id == "paintbrush")));
    }

    [Fact]
    public void ALetterPicksAToolAndCyclesToolsThatShareIt()
    {
        var tools = new ToolState(Catalog().Tools);
        Assert.Equal("zoom", tools.ToolForKey('z'));
        Assert.Equal("pan", tools.ToolForKey('H'));
        Assert.Null(tools.ToolForKey('q'));
        tools.Current = tools.ToolForKey('s')!;
        Assert.Equal("rectangleSelect", tools.Current);
        tools.Current = tools.ToolForKey('s')!;
        Assert.Equal("lassoSelect", tools.Current);
        tools.Current = tools.ToolForKey('s')!;
        Assert.Equal("ellipseSelect", tools.Current);
        tools.Current = tools.ToolForKey('s')!;
        Assert.Equal("magicWand", tools.Current);
        tools.Current = tools.ToolForKey('s')!;
        Assert.Equal("rectangleSelect", tools.Current);
    }

    [Fact]
    public void OnlyZoomAndPanActOnTheCanvas()
    {
        var tools = new ToolState(Catalog().Tools);
        foreach (var tool in Catalog().Tools)
        {
            tools.Current = tool.Id;
            var want = tool.Id switch { "zoom" => CanvasTool.Zoom, "pan" => CanvasTool.Pan, _ => CanvasTool.Inactive };
            Assert.Equal(want, tools.CanvasMode);
        }
        Assert.Throws<ArgumentException>(() => tools.Current = "nope");
    }

    [Fact]
    public void FitMathFindsGroupsThatAreNotWhole()
    {
        (double, double)[] groups = [(12, 182), (206, 400), (424, 600), (624, 800)];
        Assert.Empty(OptionsFit.Hidden(groups, 0, 900));
        Assert.Equal([3], OptionsFit.Hidden(groups, 0, 700));
        Assert.Equal([2, 3], OptionsFit.Hidden(groups, 0, 500));
        Assert.Equal([0, 3], OptionsFit.Hidden(groups, 100, 600));
        Assert.Equal((0.0, 76.0), OptionsFit.Covers(groups, 0, 700));
        Assert.Equal((82.0, 0.0), OptionsFit.Covers(groups, 100, 800));
        Assert.Equal((0.0, 0.0), OptionsFit.Covers(groups, 0, 900));
        Assert.True(OptionsFit.Overflows(1001, 1000));
        Assert.False(OptionsFit.Overflows(1000, 1000));
    }

    [AvaloniaFact]
    public void TheBrushRowOverflowsAtTheMacSizeAndShowsMore()
    {
        var w = Ready();
        var row = w.Options;
        var cell = Assert.IsAssignableFrom<Control>(row.MoreOptions.Parent);
        Assert.True(cell.IsVisible);
        Assert.Equal(Tokens.OverflowWidth, cell.Bounds.Width);
        Assert.Equal(1280, BoundsIn(cell, w).Right);
        Assert.Contains(row.Groups.Count - 1, row.HiddenGroups());
        Assert.DoesNotContain(0, row.HiddenGroups());
        Assert.True(row.RightCover > 0, "a cut word shows at the row's end");
        w.Close();
    }

    [AvaloniaFact]
    public void AWideWindowHasNoMoreButtonAndNoCover()
    {
        var w = Ready(width: 1900);
        var cell = Assert.IsAssignableFrom<Control>(w.Options.MoreOptions.Parent);
        Assert.False(cell.IsVisible);
        Assert.Empty(w.Options.HiddenGroups());
        Assert.Equal(0, w.Options.RightCover);
        w.Close();
    }

    [AvaloniaFact]
    public void ARowThatFitsHasNoMoreButton()
    {
        var w = Ready();
        w.Tools.Current = "ellipseSelect";
        Settle(w);
        var cell = Assert.IsAssignableFrom<Control>(w.Options.MoreOptions.Parent);
        Assert.False(cell.IsVisible);
        w.Close();
    }

    [AvaloniaFact]
    public void MoreOpensTheHiddenGroupsAndClosingPutsThemBack()
    {
        var w = Ready();
        var row = w.Options;
        var hidden = row.HiddenGroups();
        var groups = row.Groups;
        var picker = BoundsIn(row.ToolPicker, w);
        Click(row.MoreOptions);
        Settle(w);
        Assert.True(row.MorePopup.IsOpen);
        Assert.Equal(hidden.Count, row.MoreGroups.Children.Count);
        foreach (var index in hidden) Assert.Contains(groups[index], row.MoreGroups.Children);
        Assert.Equal(picker, BoundsIn(row.ToolPicker, w));
        Click(row.MoreOptions);
        Settle(w);
        Assert.False(row.MorePopup.IsOpen);
        Assert.Empty(row.MoreGroups.Children);
        Assert.Equal(groups, row.Groups);
        w.Close();
    }

    [AvaloniaFact]
    public void ChangingToolWithMoreOpenClosesItCleanly()
    {
        var w = Ready();
        w.Options.OpenMore();
        Settle(w);
        w.Tools.Current = "zoom";
        Settle(w);
        Assert.False(w.Options.MorePopup.IsOpen);
        Assert.Empty(w.Options.MoreGroups.Children);
        Assert.Equal(2, w.Options.Groups.Count);
        w.Close();
    }

    [AvaloniaFact]
    public void TheRowScrollsSidewaysToItsEndAndNotPast()
    {
        var w = Ready();
        var row = w.Options;
        row.ScrollBy(100);
        Settle(w);
        Assert.Equal(100, row.Scroller.Offset.X);
        row.ScrollBy(100000);
        Settle(w);
        var max = row.Scroller.Extent.Width - row.Scroller.Viewport.Width;
        Assert.Equal(max, row.Scroller.Offset.X);
        Assert.DoesNotContain(row.Groups.Count - 1, row.HiddenGroups());
        row.ScrollBy(-100000);
        Settle(w);
        Assert.Equal(0, row.Scroller.Offset.X);
        w.Close();
    }

    [AvaloniaFact]
    public void TheWheelOverTheOptionsScrollsThem()
    {
        var w = Ready();
        var row = w.Options;
        var over = BoundsIn(row.Scroller, w).Center;
        w.MouseMove(over, RawInputModifiers.None);
        Settle(w);
        w.MouseWheel(over, new Vector(0, -1), RawInputModifiers.None);
        Settle(w);
        Assert.True(row.Scroller.Offset.X > 0, $"offset {row.Scroller.Offset.X}");
        w.Close();
    }

    [AvaloniaFact]
    public void ScrollingCoversAGroupCutAtTheStart()
    {
        var w = Ready();
        var row = w.Options;
        row.ScrollBy(50);
        Settle(w);
        Assert.True(row.LeftCover > 0);
        Assert.Contains(0, row.HiddenGroups());
        w.Close();
    }

    [AvaloniaFact]
    public void AClickOutsideTheRowStillLeavesItWorking()
    {
        var w = Ready();
        w.DismissPopups();
        Assert.False(w.Options.MorePopup.IsOpen);
        w.Options.OpenMore();
        Settle(w);
        w.DismissPopups();
        Settle(w);
        Assert.False(w.Options.MorePopup.IsOpen);
        Assert.Empty(w.Options.MoreGroups.Children);
        w.Close();
    }

    // The strip.

    [AvaloniaFact]
    public void TheStripDrawsTheActiveImageAtTheLooksPlace()
    {
        var w = Ready();
        var item = Assert.Single(w.Strip.Items);
        Assert.Equal(new Rect(ImageStrip.Gap, 68, Tokens.ImageStripItemWidth, 63), BoundsIn(item, w));
        Assert.Equal("Untitled", item.Image.ShownName);
        var name = item.GetVisualDescendants().OfType<TextBlock>().Single(t => t.Name == "ImageName");
        Assert.Equal(Tokens.StrongWeight, name.FontWeight);
        Assert.Equal("Open images", Avalonia.Automation.AutomationProperties.GetName(w.Strip));
        Assert.Equal("Untitled", Avalonia.Automation.AutomationProperties.GetName(item));
        w.Close();
    }

    [AvaloniaFact]
    public void AModifiedImageShowsAnAsterisk()
    {
        var w = Ready();
        w.Strip.Show([new StripImage("a.png", Modified: true, Active: false, null, 100, 100)]);
        Settle(w);
        var item = Assert.Single(w.Strip.Items);
        var name = item.GetVisualDescendants().OfType<TextBlock>().Single(t => t.Name == "ImageName");
        Assert.Equal("a.png *", name.Text);
        Assert.NotEqual(Tokens.StrongWeight, name.FontWeight);
        w.Close();
    }

    [AvaloniaFact]
    public void TheCloseTargetShowsOnHoverOrFocusAndIsOff()
    {
        var w = Ready();
        var item = Assert.Single(w.Strip.Items);
        Assert.False(item.CloseButton.IsVisible);
        Assert.False(item.CloseButton.IsEnabled);
        var center = BoundsIn(item, w).Center;
        w.MouseMove(center, RawInputModifiers.None);
        Settle(w);
        Assert.True(item.CloseButton.IsVisible);
        w.MouseMove(new Point(600, 500), RawInputModifiers.None);
        Settle(w);
        Assert.False(item.CloseButton.IsVisible);
        item.Focus(NavigationMethod.Tab);
        Settle(w);
        Assert.True(item.CloseButton.IsVisible);
        w.Close();
    }

    [AvaloniaFact]
    public void ManyImagesScrollInTheStrip()
    {
        var w = Ready();
        w.Strip.Show(Enumerable.Range(0, 30).Select(i => new StripImage($"{i}.png", false, i == 0, null, 10, 10)));
        Settle(w);
        Assert.Equal(30, w.Strip.Items.Count);
        Assert.Equal(112, w.Strip.Items[1].Bounds.X - w.Strip.Items[0].Bounds.X);
        Assert.True(w.Strip.Scroller.Extent.Width > w.Strip.Scroller.Viewport.Width);
        w.Close();
    }

    // Panels.

    [Fact]
    public void PanelSizesAreTheMacs()
    {
        var source = Repo.Read(MacShell + "PanelLayout.swift");
        var defaults = source[source.IndexOf("public var defaultSize", StringComparison.Ordinal)..source.IndexOf("public var minimumSize", StringComparison.Ordinal)];
        var minimums = source[source.IndexOf("public var minimumSize", StringComparison.Ordinal)..source.IndexOf("public var symbol", StringComparison.Ordinal)];
        foreach (var (pattern, id) in new[] { (".tools", "tools"), (".colors", "colors"), (".history, .layers", "history"), (".history, .layers", "layers") })
        {
            Size Read(string text)
            {
                var m = Regex.Match(text, $"case {Regex.Escape(pattern)}: CGSize\\(width: (\\d+), height: (\\d+)\\)");
                Assert.True(m.Success, $"no size for {pattern}");
                return new Size(double.Parse(m.Groups[1].Value), double.Parse(m.Groups[2].Value));
            }
            Assert.Equal(Read(defaults), PanelLayout.DefaultSize(id));
            Assert.Equal(Read(minimums), PanelLayout.MinimumSize(id));
        }
        Assert.Throws<ArgumentException>(() => PanelLayout.DefaultSize("nope"));
        Assert.Contains("clearance: CGFloat = 12", source);
        Assert.Contains("imageMargin: CGFloat = 16", source);
    }

    [Fact]
    public void PanelsTakeTheFourCornersOfTheStage()
    {
        var stage = new Rect(100, 200, 1280, 672);
        var f = PanelLayout.Defaults(stage);
        Assert.Equal(new Rect(112, 212, 84, 292), f["tools"]);
        Assert.Equal(new Rect(112, 200 + 672 - 12 - 246, 244, 246), f["colors"]);
        Assert.Equal(new Rect(100 + 1280 - 12 - 226, 212, 226, 245), f["history"]);
        Assert.Equal(new Rect(100 + 1280 - 12 - 226, 200 + 672 - 12 - 245, 226, 245), f["layers"]);
    }

    [Fact]
    public void ATopPanelShortensWhenASideDoesNotFit()
    {
        var f = PanelLayout.Defaults(new Rect(0, 0, 1280, 400));
        Assert.Equal(400 - 36 - 246, f["tools"].Height);
        var tiny = PanelLayout.Defaults(new Rect(0, 0, 1280, 100));
        Assert.Equal(PanelLayout.MinimumSize("tools").Height, tiny["tools"].Height);
        Assert.Equal(PanelLayout.MinimumSize("history").Height, tiny["history"].Height);
    }

    [Fact]
    public void TheClearRectDropsThePanelsAndTheMargin()
    {
        var stage = new Rect(0, 0, 1280, 672);
        Assert.Equal(new Rect(16, 16, 1248, 640), PanelLayout.ClearRect(stage, []));
        Assert.Equal(new Rect(272, 16, 754, 640), PanelLayout.ClearRect(stage, PanelLayout.Defaults(stage).Values));
        Assert.Equal(new Rect(16, 16, 1248, 640), PanelLayout.ClearRect(stage, [new Rect(5000, 0, 100, 100)]));
    }

    [AvaloniaFact]
    public void PanelsAreOwnedToolWindowsAtTheirPlaces()
    {
        var w = Ready();
        var stage = w.StageOnScreen();
        Assert.Equal(["tools", "history", "layers", "colors"], w.Panels.Panels.Keys);
        var want = PanelLayout.Defaults(stage);
        foreach (var (id, panel) in w.Panels.Panels)
        {
            Assert.True(panel.IsVisible, id);
            Assert.Same(w, panel.Owner);
            Assert.False(panel.ShowInTaskbar);
            Assert.False(panel.ShowActivated);
            Assert.Equal(want[id].Size, panel.ClientSize);
            Assert.Contains(panel, w.OwnedWindows);
        }
        w.Close();
    }

    [AvaloniaFact]
    public void APanelIsEmptyWithATitleAndCloseOnTheRight()
    {
        var w = Ready();
        var panel = w.Panels.Panels["history"];
        Settle(panel);
        Assert.Equal("History", panel.Title);
        Assert.Equal(Tokens.PanelTitleHeight, panel.TitleRow.Bounds.Height);
        Assert.Equal(panel.ClientSize.Width, BoundsIn(panel.CloseButton, panel).Right);
        Assert.Contains("History", panel.GetVisualDescendants().OfType<TextBlock>().Select(t => t.Text));
        Assert.Equal("History panel", Avalonia.Automation.AutomationProperties.GetName(panel));
        Assert.Equal(WindowDecorationsElementRole.TitleBar, WindowDecorationProperties.GetElementRole(panel.TitleRow));
        Assert.Equal(WindowDecorationsElementRole.User, WindowDecorationProperties.GetElementRole(panel.CloseButton));
        Assert.Single(panel.GetVisualDescendants().OfType<IconView>());
        w.Close();
    }

    [AvaloniaFact]
    public void ClosingAPanelHidesItAndTheToggleBringsItBack()
    {
        var w = Ready();
        var panel = w.Panels.Panels["colors"];
        Click(panel.CloseButton);
        Settle(w);
        Assert.False(panel.IsVisible);
        Assert.False(w.Panels.IsShown("colors"));
        w.Panels.Toggle("colors");
        Settle(w);
        Assert.True(panel.IsVisible);
        Assert.Same(w, panel.Owner);
        panel.Close();
        Settle(w);
        Assert.False(panel.IsVisible);
        Assert.False(w.Panels.IsShown("colors"));
        w.Panels.Toggle("colors");
        Settle(w);
        Assert.True(panel.IsVisible);
        w.Close();
    }

    [AvaloniaFact]
    public void ClosingTheMainWindowClosesThePanels()
    {
        var w = Ready();
        var closed = 0;
        foreach (var panel in w.Panels.Panels.Values) panel.Closed += (_, _) => closed++;
        w.Close();
        Assert.Equal(4, closed);
    }

    [AvaloniaFact]
    public void HidingAPanelGivesItsRoomToTheImage()
    {
        var w = Ready();
        w.View.SetDocumentSize(new Size(4096, 3072));
        Settle(w);
        var before = w.View.View.ClearRect;
        Assert.Equal(272, before?.X);
        w.Panels.SetShown("colors", false);
        Assert.Equal(16 + 96, w.View.View.ClearRect?.X);
        w.Panels.SetShown("tools", false);
        Assert.Equal(16, w.View.View.ClearRect?.X);
        w.Panels.ResetLayout();
        Settle(w);
        Assert.Equal(before, w.View.View.ClearRect);
        Assert.True(w.Panels.Panels.Values.All(p => p.IsVisible));
        w.Close();
    }

    [AvaloniaFact]
    public void PanelTogglesInBothMenusShowTheState()
    {
        var w = Ready();
        var window = w.TitleBar.Menus.Items.OfType<MenuItem>().Single(m => (string?)m.Header == "Window");
        MenuItem Tools(IEnumerable<object?> items) => items.OfType<MenuItem>().Single(i => (string?)i.Header == "Tools");
        Assert.True(Tools(window.Items).IsChecked);
        Assert.True(Tools(w.Options.MoreMenu.Items).IsChecked);
        Assert.Equal(MenuItemToggleType.CheckBox, Tools(window.Items).ToggleType);
        w.Panels.SetShown("tools", false);
        Assert.False(Tools(window.Items).IsChecked);
        Assert.False(Tools(w.Options.MoreMenu.Items).IsChecked);
        Tools(window.Items).RaiseEvent(new RoutedEventArgs(MenuItem.ClickEvent));
        Settle(w);
        Assert.True(w.Panels.IsShown("tools"));
        w.Close();
    }

    [AvaloniaFact]
    public void AThemeChangeReachesEveryNewPartAndKeepsTheTool()
    {
        var w = Ready("light");
        w.Tools.Current = "text";
        Settle(w);
        w.RequestedThemeVariant = Avalonia.Styling.ThemeVariant.Dark;
        Settle(w);
        Assert.Equal(Tokens.Dark.Options, ((Avalonia.Media.ISolidColorBrush)w.Options.Background!).Color);
        Assert.Equal(Tokens.Dark.Chrome, ((Avalonia.Media.ISolidColorBrush)w.Strip.Background!).Color);
        Assert.Equal(Tokens.Dark.Panel, ((Avalonia.Media.ISolidColorBrush)w.Panels.Panels["tools"].Background!).Color);
        Assert.Equal("text", w.Tools.Current);
        Assert.Equal("text", ((CatalogTool)w.Options.ToolPicker.SelectedItem!).Id);
        Assert.Equal(new Rect(0, 132, 1280, 672), BoundsIn(w.Stage, w));
        Assert.Same(w.Status, w.Status);
        w.Tools.Current = "zoom";
        Settle(w);
        Assert.Equal(2, w.Options.Groups.Count);
        w.Close();
    }

    // Zoom controls.

    [AvaloniaFact]
    public void TheStatusBarFollowsAndDrivesTheZoom()
    {
        var w = Ready();
        w.View.SetDocumentSize(new Size(4096, 3072));
        Settle(w);
        var s = w.Status;
        Assert.Equal("18%", s.ZoomField.Text);
        Assert.Equal(Math.Log(754.0 / 4096), s.ZoomSlider.Value, 6);
        Click(s.ActualButton);
        Assert.Equal(1, w.View.View.Zoom);
        Assert.False(w.View.IsFitted);
        Assert.Equal("100%", s.ZoomField.Text);
        Assert.Equal(0, s.ZoomSlider.Value, 9);
        Click(s.FitButton);
        Assert.True(w.View.IsFitted);
        Assert.Equal("18%", s.ZoomField.Text);
        s.ZoomSlider.Value = Math.Log(2);
        Assert.Equal(2, w.View.View.Zoom, 9);
        Assert.Equal("200%", s.ZoomField.Text);
        w.Close();
    }

    [AvaloniaFact]
    public void TheZoomFieldTakesPercentsAndRefusesBadText()
    {
        var w = Ready();
        w.View.SetDocumentSize(new Size(4096, 3072));
        var s = w.Status;
        void Type(string text)
        {
            s.ZoomField.Text = text;
            s.ZoomField.RaiseEvent(new KeyEventArgs { RoutedEvent = InputElement.KeyDownEvent, Key = Key.Enter });
        }
        Type("250%");
        Assert.Equal(2.5, w.View.View.Zoom);
        Assert.Equal("250%", s.ZoomField.Text);
        Type("rubbish");
        Assert.Equal(2.5, w.View.View.Zoom);
        Assert.Equal("250%", s.ZoomField.Text);
        Type("0");
        Assert.Equal(2.5, w.View.View.Zoom);
        Assert.Equal("250%", s.ZoomField.Text);
        w.Close();
    }

    /// The field must not rely on focus: the native canvas takes focus with no word to Avalonia.
    [AvaloniaFact]
    public void TheZoomFieldFollowsTheViewUnlessTheUserIsTyping()
    {
        var w = Ready();
        w.View.SetDocumentSize(new Size(4096, 3072));
        var s = w.Status;
        s.ZoomField.Focus();
        Settle(w);
        w.View.SetZoom(3);
        Assert.Equal("300%", s.ZoomField.Text);
        s.ZoomField.Text = "12";
        w.View.SetZoom(4);
        Assert.Equal("12", s.ZoomField.Text);
        w.DismissPopups();   // A press on the canvas.
        Assert.Equal(0.12, w.View.View.Zoom);
        Assert.Equal("12%", s.ZoomField.Text);
        s.ZoomField.Text = "nope";
        w.DismissPopups();
        Assert.Equal("12%", s.ZoomField.Text);
        w.Close();
    }

    [AvaloniaFact]
    public void TheCursorReadsWholeDocumentPixels()
    {
        var w = Ready();
        w.Status.ShowCursor(new Point(12.7, 3.2));
        Assert.Equal("12, 3", w.Status.CursorValue.Text);
        w.Status.ShowCursor(null);
        Assert.Equal("–", w.Status.CursorValue.Text);
        w.Close();
    }

    [AvaloniaFact]
    public void ViewMenuZoomCommandsWork()
    {
        var w = Ready();
        w.View.SetDocumentSize(new Size(4096, 3072));
        Settle(w);
        var view = w.TitleBar.Menus.Items.OfType<MenuItem>().Single(m => (string?)m.Header == "View");
        MenuItem Item(string title) => view.Items.OfType<MenuItem>().Single(i => (string?)i.Header == title);
        void Run(MenuItem item)
        {
            Assert.True(item.IsEnabled, (string?)item.Header);
            item.RaiseEvent(new RoutedEventArgs(MenuItem.ClickEvent));
        }
        Run(Item("Actual Size"));
        Assert.Equal(1, w.View.View.Zoom);
        Run(Item("Zoom In"));
        Assert.Equal(2, w.View.View.Zoom);
        Run(Item("Zoom Out"));
        Assert.Equal(1, w.View.View.Zoom);
        Run(Item("Zoom to Window"));
        Assert.True(w.View.IsFitted);
        var presets = Item("Zoom Presets").Items.OfType<MenuItem>().ToList();
        Assert.Equal(ZoomPresets.Values.Length, presets.Count);
        foreach (var preset in presets)
        {
            var zoom = ZoomPresets.ForTitle((string)preset.Header!);
            Assert.NotNull(zoom);
            Run(preset);
            Assert.Equal(zoom, w.View.View.Zoom);
        }
        Assert.False(Item("Zoom to Selection").IsEnabled);
        w.Close();
    }

    [AvaloniaFact]
    public void ShortcutsWorkFromTheWindow()
    {
        var w = Ready();
        w.View.SetDocumentSize(new Size(4096, 3072));
        Settle(w);
        w.Focus();
        w.KeyPress(Key.F8, RawInputModifiers.None, PhysicalKey.F8, null);
        Settle(w);
        Assert.False(w.Panels.IsShown("colors"));
        w.KeyPress(Key.F8, RawInputModifiers.None, PhysicalKey.F8, null);
        Settle(w);
        Assert.True(w.Panels.IsShown("colors"));
        w.View.ActualSize();
        w.KeyPress(Key.OemPlus, RawInputModifiers.Control, PhysicalKey.Equal, null);
        Settle(w);
        Assert.Equal(2, w.View.View.Zoom);
        w.KeyPress(Key.OemMinus, RawInputModifiers.Control, PhysicalKey.Minus, null);
        Settle(w);
        Assert.Equal(1, w.View.View.Zoom);
        w.Close();
    }

    [AvaloniaFact]
    public void ALetterKeyPicksAToolOnlyThroughTheCanvasPath()
    {
        var w = Ready();
        Assert.True(w.UseToolKey('Z'));
        Assert.Equal("zoom", w.Tools.Current);
        Assert.Equal(CanvasTool.Zoom, w.View.Tool);
        Assert.False(w.UseToolKey('Q'));
        w.Focus();
        w.KeyPress(Key.H, RawInputModifiers.None, PhysicalKey.H, "h");
        Settle(w);
        Assert.Equal("zoom", w.Tools.Current);
        w.Close();
    }

    /// Every shortcut the menus show must be reachable, by both routes the keys come in.
    [Fact]
    public void EveryMenuKeyHasTheSameWordsFromBothKeyRoutes()
    {
        static IEnumerable<string> Keys(IEnumerable<MenuEntry> entries) =>
            entries.SelectMany(e => (e.Key is null ? [] : new[] { e.Key }).Concat(Keys(e.Items)));
        var keys = Keys(WindowsMenus.Build(Catalog())).Distinct().ToList();
        Assert.True(keys.Count > 30, $"only {keys.Count} keys");
        foreach (var key in keys)
        {
            var gesture = WindowsKeys.Gesture(key);
            Assert.Equal(key, WindowsKeys.FromAvalonia(gesture.Key, gesture.KeyModifiers));
        }
    }

    [Theory]
    [InlineData(0x5A, true, false, false, "Ctrl+Z")]
    [InlineData(0x5A, true, true, false, "Ctrl+Shift+Z")]
    [InlineData(0x52, true, false, true, "Ctrl+Alt+R")]
    [InlineData(0x31, true, false, true, "Ctrl+Alt+1")]
    [InlineData(0x77, false, false, false, "F8")]
    [InlineData(0xBB, true, false, false, "Ctrl++")]
    [InlineData(0x6B, true, false, false, "Ctrl++")]
    [InlineData(0xBD, true, false, false, "Ctrl+-")]
    [InlineData(0xDE, true, false, false, "Ctrl+'")]
    [InlineData(0xC0, true, false, false, "Ctrl+`")]
    [InlineData(0x2E, false, false, false, "Delete")]
    public void WindowKeysBecomeTheCatalogsWords(int vk, bool ctrl, bool shift, bool alt, string words) =>
        Assert.Equal(words, WindowsKeys.FromVirtualKey(vk, ctrl, shift, alt));

    [Fact]
    public void AKeyTheMenusNeverNameHasNoWords()
    {
        Assert.Null(WindowsKeys.FromVirtualKey(0x20, false, false, false));
        Assert.Null(WindowsKeys.FromAvalonia(Key.Space, KeyModifiers.None));
    }

    // Icons.

    [AvaloniaFact]
    public void EveryIconSitsOnWholePixels()
    {
        foreach (var (tool, theme) in new[] { ("paintbrush", "light"), ("text", "dark"), ("magicWand", "light") })
        {
            var w = Ready(theme);
            w.Tools.Current = tool;
            Settle(w);
            var icons = w.GetVisualDescendants().OfType<IconView>().Where(i => i.IsEffectivelyVisible).ToList();
            Assert.True(icons.Count >= 9, $"{tool}: only {icons.Count} icons");
            foreach (var icon in icons)
            {
                var at = BoundsIn(icon, w);
                Assert.Equal(Math.Round(at.X), at.X);
                Assert.Equal(Math.Round(at.Y), at.Y);
                Assert.Equal(icon.Width, at.Width);
            }
            w.Close();
        }
    }

    [Fact]
    public void ASmallIconDrawsItsOwnArtNeverScaled()
    {
        Icons.Folder = Repo.File("windows/assets/icons");
        foreach (var (name, size) in new[] { ("window-close", 12.0), ("window-close", 16.0), ("cmd-undo", 16.0), ("cmd-redo", 16.0), ("opt-minus", 16.0), ("opt-plus", 16.0), ("opt-overflow", 16.0), ("cmd-new", 20.0) })
            Assert.Equal(size, Icons.Load(name, size).ViewBox);
        var e = Assert.ThrowsAny<IOException>(() => Icons.Load("cmd-new", 16));
        Assert.Contains("cmd-new", e.Message);
    }

    [Fact]
    public void SmallArtIsTheUnchangedFluentFile()
    {
        var rows = File.ReadAllLines(Repo.File("windows/assets/icons/small-art.txt")).Where(l => !l.StartsWith('#')).Select(l => l.Split(' ')).ToList();
        Assert.Equal(7, rows.Count);
        foreach (var row in rows)
        {
            var path = Repo.File($"windows/assets/icons/{row[0]}");
            Assert.True(File.Exists(path), row[0]);
            Assert.Equal(row[2], Convert.ToHexStringLower(SHA256.HashData(File.ReadAllBytes(path))));
            var size = row[0].Split('/')[0];
            Assert.EndsWith($"_{size}_regular", row[1]);
        }
        var onDisk = Directory.GetFiles(Repo.File("windows/assets/icons/16"), "*.svg").Concat(Directory.GetFiles(Repo.File("windows/assets/icons/12"), "*.svg"));
        Assert.Equal(rows.Count, onDisk.Count());
    }

    [AvaloniaFact]
    public void AMenuIconDrawsAtItsArtSizeWithNoScale()
    {
        var w = Ready();
        var file = w.TitleBar.Menus.Items.OfType<MenuItem>().First();
        file.Open();
        Settle(w);
        var open = file.Items.OfType<CatalogMenuItem>().Single(i => (string?)i.Header == "Open…");
        Assert.True(file.IsSubMenuOpen);
        open.ApplyTemplate();
        Settle(w);
        var icon = Assert.IsType<IconView>(open.Icon);
        Assert.Equal(Tokens.IconSize, icon.Width);
        var box = open.GetVisualDescendants().OfType<Control>().Single(c => c.Name == CatalogMenuItem.IconPart);
        Assert.Equal(Tokens.IconSize, box.Width);
        Assert.Equal(Tokens.IconSize, icon.Bounds.Width);
        Assert.Equal(Tokens.IconSize, icon.Bounds.Height);
        file.Close();
        w.Close();
    }

    // Start options for the shots.

    [Fact]
    public void TheShotHooksAreRead()
    {
        var o = StartOptions.Parse(["--test-tool", "zoom", "--test-open-more", "--test-open-menu", "View"]);
        Assert.Equal("zoom", o.TestTool);
        Assert.True(o.TestOpenMore);
        Assert.Equal("View", o.TestOpenMenu);
    }

    [AvaloniaFact]
    public void AnUnknownTestToolFailsByName()
    {
        var e = Assert.Throws<ArgumentException>(() => new MainWindow(Shells.NoOptions with { TestTool = "nope" }, headless: true));
        Assert.Contains("nope", e.Message);
    }

    /// W6 asks this of dialogs. No dialog exists yet, so the rule waits in W8's first dialog.
    [Fact]
    public void NoDialogExistsYetToDrag()
    {
        var sources = Directory.GetFiles(Repo.File("windows/OpenPainter/src"), "*.cs", SearchOption.AllDirectories).Select(File.ReadAllText);
        Assert.DoesNotContain(sources, s => s.Contains("ShowDialog", StringComparison.Ordinal));
    }
}
