// W8 done-test 3: the Mac's keys work with Ctrl for Command, and docs/windows/shortcuts-windows.md
// holds each difference and its reason. A difference that is not in the table fails by name.
using System.Text.Json;
using System.Text.RegularExpressions;
using Avalonia.Controls;
using Avalonia.Headless;
using Avalonia.Headless.XUnit;
using Avalonia.Input;
using OpenPainter.Catalog;
using OpenPainter.Shell;
using Xunit;

namespace OpenPainter.Tests;

public static partial class ShortcutDoc
{
    const string Path = "docs/windows/shortcuts-windows.md";
    public const string Vectors = "windows/tests/vectors/shortcut-words.json";
    public const string MacList = "docs/shortcuts.md";

    /// The rows of the table under a heading, as cells.
    public static List<string[]> Table(string text, string heading)
    {
        var at = text.IndexOf(heading, StringComparison.Ordinal);
        if (at < 0) throw new InvalidDataException($"no heading {heading}");
        var rows = new List<string[]>();
        var started = false;
        foreach (var line in text[(at + heading.Length)..].Split('\n'))
        {
            if (line.StartsWith("## ", StringComparison.Ordinal)) break;
            if (!line.StartsWith('|'))
            {
                if (started) break;
                continue;
            }
            started = true;
            var cells = line.Trim().Trim('|').Split('|').Select(c => c.Trim()).ToArray();
            if (cells[0].StartsWith("---", StringComparison.Ordinal)) continue;
            rows.Add(cells);
        }
        return rows.Skip(1).ToList();
    }

    public static string Text() => Repo.Read(Path);

    /// The rule written out here, not by the code under test: ⌘ and ⌃ to Ctrl, ⌥ to Alt, ⇧ to Shift.
    public static string Rule(string mac)
    {
        bool ctrl = false, shift = false, alt = false;
        var at = 0;
        for (; at < mac.Length && mac[at] is '⌘' or '⌃' or '⌥' or '⇧'; at++)
        {
            if (mac[at] is '⌘' or '⌃') ctrl = true;
            else if (mac[at] == '⌥') alt = true;
            else shift = true;
        }
        return (ctrl ? "Ctrl+" : "") + (shift ? "Shift+" : "") + (alt ? "Alt+" : "") + mac[at..];
    }

    /// The Mac's menu rows: where, and its key.
    public static List<(string Name, string Mac)> MacMenuRows()
    {
        using var file = JsonDocument.Parse(Repo.Read(Vectors));
        return file.RootElement.GetProperty("vectors").EnumerateArray()
            .Select(v => (v.GetProperty("name").GetString()!, v.GetProperty("in")[0].GetString()!)).ToList();
    }

    public static IEnumerable<MenuEntry> Flatten(IEnumerable<MenuEntry> items)
    {
        foreach (var item in items)
        {
            if (item.Kind == EntryKind.Separator) continue;
            yield return item;
            foreach (var inner in Flatten(item.Items)) yield return inner;
        }
    }

    /// Where Windows shows a Mac menu row: its own path, or where the Mac's app menu moved it.
    public static string? WindowsKeyOf(string macPath, IReadOnlyList<MenuEntry> menus)
    {
        if (WindowsMenus.Removed.ContainsKey(macPath)) return null;
        var moved = WindowsMenus.Moved.FirstOrDefault(m => m.MacPath == macPath);
        var path = moved.MacPath is null ? macPath : $"{moved.Menu} > {moved.Title}";
        return Flatten(menus).Single(e => e.Path == path).Key;
    }

    /// Where a Mac menu row shows on Windows, or null when Windows has no such item.
    static string? WindowsPathOf(string macPath)
    {
        if (WindowsMenus.Removed.ContainsKey(macPath)) return null;
        var moved = WindowsMenus.Moved.FirstOrDefault(m => m.MacPath == macPath);
        return moved.MacPath is null ? macPath : $"{moved.Menu} > {moved.Title}";
    }

    /// Each difference this build really has, by where. `menus` lets a test plant a fault.
    public static Dictionary<string, string> MenuDifferences(IReadOnlyCollection<string> working, IReadOnlyList<MenuEntry>? menus = null)
    {
        menus ??= WindowsMenus.Build(MacCatalog.Embedded());
        var found = new Dictionary<string, string>();
        var shownKeys = Flatten(menus).Select(e => e.Key).OfType<string>().ToHashSet();
        var extra = working.Where(k => !shownKeys.Contains(k) && !HiddenTwin().IsMatch(k)).ToList();
        foreach (var (name, mac) in MacMenuRows())
        {
            var rule = Rule(mac);
            var shown = WindowsKeyOf(name, menus);
            var words = shown ?? "none";
            if (name == "Edit > Redo") words += string.Concat(extra.Where(k => k == MainWindow.RedoBeside).Select(k => $", {k}"));
            if (words != rule) found[name] = words;
        }
        foreach (var key in extra.Where(k => k != MainWindow.RedoBeside)) found[$"key {key}"] = key;
        // A Windows item with a key that the Mac's list of keys does not have.
        var macPlaces = MacMenuRows().Select(r => WindowsPathOf(r.Name)).OfType<string>().ToHashSet();
        foreach (var entry in Flatten(menus).Where(e => e.Key is not null && !macPlaces.Contains(e.Path))) found[entry.Path] = entry.Key!;
        return found;
    }

    [GeneratedRegex(@"^Ctrl\+Alt\+[1-4]$")] public static partial Regex HiddenTwin();
}

public sealed class ShortcutTableTests
{
    static List<string[]> Differences() => ShortcutDoc.Table(ShortcutDoc.Text(), "## Differences from the Mac");

    /// The rows the table has, by where, with Windows keys that a test can compare.
    static Dictionary<string, string> TableRows() => Differences().Where(IsMenuRow).ToDictionary(r => r[0], r => r[3]);

    /// A menu row names its place as "Menu > Item". The others are held by NonMenuRowProofs.
    static bool IsMenuRow(string[] row) => row[0].Contains(" > ", StringComparison.Ordinal);

    /// Names what is wrong, so a failure says which key to explain.
    public static List<string> Unexplained(IReadOnlyDictionary<string, string> actual, IReadOnlyDictionary<string, string> table)
    {
        var wrong = new List<string>();
        foreach (var (where, keys) in actual)
            if (!table.TryGetValue(where, out var row)) wrong.Add($"{where}: Windows has {keys}, and the table has no row");
            else if (row != keys) wrong.Add($"{where}: Windows has {keys}, and the table says {row}");
        return wrong;
    }

    [AvaloniaFact]
    public void EveryMenuKeyMatchesTheMacOrItsDifferenceIsInTheTable()
    {
        var w = Shells.Open();
        var actual = ShortcutDoc.MenuDifferences(w.WorkingKeys);
        var table = TableRows();
        Assert.Empty(Unexplained(actual, table));
        // A row for something that no longer differs must go too.
        Assert.Empty(table.Keys.Except(actual.Keys).Select(k => $"{k}: the table has a row, and Windows does not differ"));
        Assert.True(actual.Count >= 8, $"only {actual.Count} differences were found, so the check looks broken");
        w.Close();
    }

    [Fact]
    public void ADifferenceThatIsNotInTheTableFailsByName()
    {
        var wrong = Unexplained(new Dictionary<string, string> { ["File > Save"] = "Ctrl+Q", ["Edit > Undo"] = "F2" }, new Dictionary<string, string> { ["Edit > Undo"] = "F3" });
        Assert.Equal(2, wrong.Count);
        Assert.Contains("File > Save", wrong[0]);
        Assert.Contains("Ctrl+Q", wrong[0]);
        Assert.Contains("F2", wrong[1]);
    }

    [Fact]
    public void EveryRowInTheTableGivesAReason()
    {
        var rows = Differences();
        Assert.True(rows.Count >= 15, $"only {rows.Count} rows");
        foreach (var row in rows) Assert.True(row.Length == 5 && row[4].Length > 10, $"{row[0]} has no reason");
    }

    static IReadOnlyList<MenuEntry> Plant(IEnumerable<MenuEntry> menus, string path, string key) =>
        menus.Select(e => e.Path == path ? e with { Key = key } : e with { Items = Plant(e.Items, path, key) }).ToList();

    /// A key on an item that has none on the Mac is a difference too.
    [AvaloniaFact]
    public void AKeyPlantedOnAnItemWithNoMacKeyFailsByName()
    {
        var w = Shells.Open();
        var planted = Plant(WindowsMenus.Build(MacCatalog.Embedded()), "Layers > Merge Layer Down", "Ctrl+E");
        var wrong = Unexplained(ShortcutDoc.MenuDifferences(w.WorkingKeys, planted), TableRows());
        Assert.Equal(["Layers > Merge Layer Down: Windows has Ctrl+E, and the table has no row"], wrong);
        w.Close();
    }

    /// A key that works and that no menu shows is a difference too.
    [AvaloniaFact]
    public void AHiddenKeyPlantedInTheWorkingListFailsByName()
    {
        var w = Shells.Open();
        var wrong = Unexplained(ShortcutDoc.MenuDifferences([.. w.WorkingKeys, "Ctrl+E"]), TableRows());
        Assert.Equal(["key Ctrl+E: Windows has Ctrl+E, and the table has no row"], wrong);
        w.Close();
    }

    /// The rows that are not menu items: each one's keys, pressed. Without this they were read by nothing.
    static readonly Dictionary<string, (string Keys, Action<Fixture> Proof)> NonMenuRowProofs = new()
    {
        ["Canvas: Finish the crop, a move or a fill; else Deselect"] = ("Enter", f =>
        {
            f.Window.CanvasInput.Focus();
            f.Editor.Report("a message");
            Press(f.Window, Key.Enter);
            Assert.Null(f.Editor.Message);
        }),
        ["Dialogs: Default button; Cancel"] = ("Enter; Esc", f =>
        {
            var d = f.Window.ShowLayerProperties(0)!;
            Avalonia.Threading.Dispatcher.UIThread.RunJobs();
            d.Set(d.Current with { Opacity = 9 });
            d.KeyPress(Key.Escape, RawInputModifiers.None, PhysicalKey.Escape, null);
            f.Settle();
            Assert.Null(f.Window.Dialog);
            Assert.Equal(255, f.Editor.Info.Layers[0].Opacity);
            d = f.Window.ShowLayerProperties(0)!;
            Avalonia.Threading.Dispatcher.UIThread.RunJobs();
            d.Set(d.Current with { Opacity = 9 });
            d.KeyPress(Key.Enter, RawInputModifiers.None, PhysicalKey.Enter, null);
            f.Settle();
            Assert.Null(f.Window.Dialog);
            Assert.Equal(9, f.Editor.Info.Layers[0].Opacity);
        }),
        ["Panels: Pick the focused row, swatch or button"] = ("Space or Enter", f =>
        {
            f.AddLayer();
            var panel = f.Window.Panels.Panels[PanelLayout.Layers];
            f.Layers.LayerRows[1].Focus(NavigationMethod.Tab);
            PressIn(panel, Key.Space);
            f.Settle();
            Assert.Equal(0, f.Editor.Info.ActiveLayer);
            f.Layers.LayerRows[0].Focus(NavigationMethod.Tab);
            PressIn(panel, Key.Enter);
            f.Settle();
            Assert.Equal(1, f.Editor.Info.ActiveLayer);
        }),
        ["Canvas: Reset colors to black and white"] = ("D", f =>
        {
            f.Window.CanvasInput.Focus();
            f.Window.Tools.Primary = new DocColor(1, 2, 3);
            Press(f.Window, Key.D);
            Assert.Equal(new DocColor(0, 0, 0), f.Window.Tools.Primary);
            Assert.Equal(new DocColor(255, 255, 255), f.Window.Tools.Secondary);
        }),
        ["Menu bar: open it"] = ("Alt, F10", f =>
        {
            var w = f.Window;
            w.Focus();
            w.KeyPressQwerty(PhysicalKey.AltLeft, RawInputModifiers.Alt);
            w.KeyReleaseQwerty(PhysicalKey.AltLeft, RawInputModifiers.None);
            Avalonia.Threading.Dispatcher.UIThread.RunJobs();
            Assert.True(w.TitleBar.Menus.IsOpen);
            w.TitleBar.Menus.Close();
            Avalonia.Threading.Dispatcher.UIThread.RunJobs();
            Press(w, Key.F10);
            Assert.True(w.TitleBar.Menus.IsOpen);
            w.TitleBar.Menus.Close();
            Avalonia.Threading.Dispatcher.UIThread.RunJobs();
        }),
        ["Menu bar: open a menu"] = ("Alt+F, E, V, I, L, A, C, W, H", f =>
        {
            var w = f.Window;
            w.Focus();
            foreach (var (title, letter) in MenuLetters.Letters)
            {
                Press(w, Enum.Parse<Key>(letter.ToString()), RawInputModifiers.Alt);
                Assert.True(w.TitleBar.Menus.Items.OfType<MenuItem>().Single(m => (string?)m.Header == title).IsSubMenuOpen, title);
                w.TitleBar.Menus.Close();
                Avalonia.Threading.Dispatcher.UIThread.RunJobs();
            }
        }),
        ["Open menu: move to an item"] = ("a letter", f =>
        {
            var w = f.Window;
            w.OpenMenuByLetter('L', fromCanvas: false);
            Avalonia.Threading.Dispatcher.UIThread.RunJobs();
            Press(w, Key.A);
            Assert.Equal("Add New Layer", (w.FocusManager?.GetFocusedElement() as CatalogMenuItem)?.Header);
            w.TitleBar.Menus.Close();
            Avalonia.Threading.Dispatcher.UIThread.RunJobs();
        }),
    };

    static void PressIn(Avalonia.Controls.Window w, Key key)
    {
        w.KeyPress(key, RawInputModifiers.None, PhysicalKey.None, null);
        Avalonia.Threading.Dispatcher.UIThread.RunJobs();
    }

    /// Names a row with no proof, a proof with no row, or a row whose keys the proof does not press.
    public static List<string> NonMenuRowProblems(IEnumerable<string[]> rows, IReadOnlyDictionary<string, string> proofs)
    {
        var problems = new List<string>();
        var names = new HashSet<string>();
        foreach (var row in rows.Where(r => !IsMenuRow(r)))
        {
            names.Add(row[0]);
            if (!proofs.TryGetValue(row[0], out var keys)) problems.Add($"{row[0]}: the table has this row, and no test presses its keys");
            else if (keys != row[3]) problems.Add($"{row[0]}: the table says {row[3]}, and the test presses {keys}");
        }
        foreach (var name in proofs.Keys.Where(k => !names.Contains(k))) problems.Add($"{name}: a test presses its keys, and the table has no row");
        return problems;
    }

    [AvaloniaFact]
    public void EveryRowThatIsNotAMenuItemIsHeldByItsRealKeys()
    {
        var proofs = NonMenuRowProofs.ToDictionary(p => p.Key, p => p.Value.Keys);
        Assert.Empty(NonMenuRowProblems(Differences(), proofs));
        foreach (var (name, proof) in NonMenuRowProofs)
        {
            using var f = new Fixture();
            try
            {
                proof.Proof(f);
            }
            catch (Exception e) when (e is not Xunit.Sdk.XunitException)
            {
                throw new InvalidOperationException($"{name}: {e.Message}", e);
            }
        }
    }

    [Fact]
    public void ANonMenuRowWithADifferentKeyOrNoProofFailsByName()
    {
        var proofs = new Dictionary<string, string> { ["Canvas: A"] = "Enter", ["Canvas: Gone"] = "X" };
        var wrong = NonMenuRowProblems([["Canvas: A", "↩", "↩", "Tab", "a reason"], ["Canvas: B", "none", "none", "D", "a reason"]], proofs);
        Assert.Equal(3, wrong.Count);
        Assert.Contains("Canvas: A: the table says Tab", wrong[0]);
        Assert.Contains("Canvas: B: the table has this row", wrong[1]);
        Assert.Contains("Canvas: Gone: a test presses", wrong[2]);
    }

    /// Every Mac menu key either works or the table names the milestone that turns it on.
    [AvaloniaFact]
    public void AKeyThatDoesNotWorkYetNamesItsMilestone()
    {
        var w = Shells.Open();
        var later = ShortcutDoc.Table(ShortcutDoc.Text(), "## Keys that later milestones turn on");
        var named = later.ToDictionary(r => r[1], r => r[2]);
        var shown = ShortcutDoc.Flatten(WindowsMenus.Build(MacCatalog.Embedded())).Where(e => e.Key is not null).Select(e => e.Key!).ToHashSet();
        var off = shown.Where(k => !w.WorkingKeys.Contains(k) && k != "Alt+F4").Order(StringComparer.Ordinal).ToList();
        Assert.Equal(off, named.Keys.Order(StringComparer.Ordinal));
        foreach (var (key, milestone) in named) Assert.True(Regex.IsMatch(milestone, @"^W\d+$") && int.Parse(milestone[1..]) > 8, $"{key}: {milestone} is not a later milestone");
        Assert.True(off.Count >= 30, $"only {off.Count} keys are off");
        w.Close();
    }

    /// Items with a later milestone are in the menus and off.
    [AvaloniaFact]
    public void CommandsThatComeLaterStayPresentAndOff()
    {
        var w = Shells.Open();
        var later = ShortcutDoc.Table(ShortcutDoc.Text(), "## Keys that later milestones turn on");
        var items = Items(w.TitleBar.Menus.Items).ToList();
        foreach (var row in later)
        {
            var item = items.SingleOrDefault(i => i.Entry.Path == row[0]);
            Assert.True(item is not null, $"{row[0]} is not in the menus");
            Assert.False(item.IsEnabled, $"{row[0]} is on");
            Assert.Equal(row[1], item.Entry.Key);
        }
        w.Close();
    }

    static IEnumerable<CatalogMenuItem> Items(IEnumerable<object?> items)
    {
        foreach (var item in items.OfType<MenuItem>())
        {
            if (item is CatalogMenuItem c) yield return c;
            foreach (var inner in Items(item.Items)) yield return inner;
        }
    }

    /// The doc's canvas table is the Mac's, row for row, and only the words change.
    [Fact]
    public void TheCanvasTableHasEveryMacRow()
    {
        var mac = Repo.Read(ShortcutDoc.MacList);
        var from = mac.IndexOf("## Canvas, panel and dialog keys", StringComparison.Ordinal);
        var rows = ShortcutDoc.Table(mac[from..mac.IndexOf("## macOS keys left alone", StringComparison.Ordinal)], "## Canvas, panel and dialog keys").Where(r => r[2] != "–").ToList();
        var doc = ShortcutDoc.Table(ShortcutDoc.Text(), "## Canvas, panel and dialog keys");
        Assert.Equal(rows.Select(r => r[0]), doc.Select(r => r[0]));
        Assert.Equal(rows.Select(r => r[2]), doc.Select(r => r[1]));
        Assert.True(doc.Count >= 30, $"only {doc.Count} rows");
        foreach (var row in doc)
        {
            // Only a row in the differences table may say more than the words below.
            var words = row[1].Replace("×", "x").Replace("⇧⌘", "Ctrl+Shift+").Replace("⌃⇧", "Ctrl+Shift+").Replace("⌘", "Ctrl+").Replace("⌃", "Ctrl+").Replace("⇧", "Shift+").Replace("↩", "Enter");
            Assert.Equal(words, row[2]);
        }
    }

    /// The test that proves each row that works: Class.Method. The doc's State column is held to this.
    static readonly Dictionary<string, string[]> RowTests = new()
    {
        ["Rectangle Select"] = [Each], ["Lasso Select"] = [Each], ["Ellipse Select"] = [Each], ["Magic Wand"] = [Each],
        ["Move Selected Pixels"] = [Each], ["Move Selection"] = [Each], ["Zoom"] = [Each], ["Pan"] = [Each],
        ["Paint Bucket"] = [Each], ["Gradient"] = [Each], ["Paintbrush"] = [Each], ["Eraser"] = [Each], ["Pencil"] = [Each],
        ["Color Picker"] = [Each], ["Clone Stamp"] = [Each], ["Recolor"] = [Each], ["Text"] = [Each],
        ["Line / Curve"] = [Each], ["Shapes"] = [Each],
        ["Brush width −1"] = [Cycle], ["Brush width +1"] = [Cycle], ["Brush width −5"] = [Cycle], ["Brush width +5"] = [Cycle],
        ["Swap primary and secondary colors"] = [Cycle],
        ["Switch the active color (the notch)"] = [Cycle],
        ["Pan while held"] = ["ViewTests.SpacePansWithAnyToolAndOnlyWhileHeld"],
        ["Finish the crop, a move or a fill; else Deselect"] = ["ShortcutTableTests.EveryRowThatIsNotAMenuItemIsHeldByItsRealKeys"],
        ["Next image"] = ["FocusRuleTests.TabLeavesTheCanvasAndCtrlTabStays"],
        ["Previous image"] = ["FocusRuleTests.TabLeavesTheCanvasAndCtrlTabStays"],
        ["Next or previous control"] = ["FocusRuleTests.TabLeavesTheCanvasAndCtrlTabStays", "FocusRuleTests.TabReachesEveryRowOfAPanel"],
        ["Pick the focused row, swatch or button"] = ["FocusRuleTests.LayerRowsTakeTheKeyboard"],
        ["Row above or below"] = ["FocusRuleTests.LayerRowsTakeTheKeyboard"],
        ["Palette swatch to the left, right, above or below"] = ["ColorsPanelTests.TheArrowKeysMoveBetweenTheSwatches"],
        ["Hue −1° / +1° (⇧: 15°); saturation −1 / +1 (⇧: 10)"] = ["ColorsPanelTests.TheWheelsArrowKeysTurnTheHueAndChangeTheSaturation"],
        ["Default button; Cancel"] = ["LayerPropertiesTests.EnterIsOk", "LayerPropertiesTests.TheCloseButtonAndEscapeAreCancel"],
    };

    const string Each = "ShortcutTableTests.EachToolRowOfTheDocPicksThatTool";
    const string Cycle = "ShortcutTableTests.TheCanvasKeysCycleToolsChangeTheWidthAndSwapColorsAsOnTheMac";

    /// A state the doc may use, and whether the row needs a test now.
    static bool? NeedsTest(string state) =>
        state == "works" || state.StartsWith("works: ", StringComparison.Ordinal) || state.StartsWith("one image until W10: ", StringComparison.Ordinal) ? true
        : Regex.IsMatch(state, @"^off until W(9|[1-9]\d)$") ? false
        : null;

    /// Names a state nobody knows, a working row with no test, or a named test that does not exist.
    public static List<string> RowStateProblems(IEnumerable<string[]> rows, IReadOnlyDictionary<string, string[]> tests, IReadOnlySet<string> known)
    {
        var problems = new List<string>();
        foreach (var row in rows)
        {
            var needs = NeedsTest(row[3]);
            if (needs is null) problems.Add($"{row[0]}: the state \"{row[3]}\" means nothing to this test");
            else if (needs == true && !tests.ContainsKey(row[0])) problems.Add($"{row[0]}: it works and has no test");
            else if (needs == false && tests.ContainsKey(row[0])) problems.Add($"{row[0]}: it says {row[3]} and has a test");
            if (tests.TryGetValue(row[0], out var names))
                foreach (var name in names.Where(n => !known.Contains(n))) problems.Add($"{row[0]}: there is no test {name}");
        }
        return problems;
    }

    /// Read from the test source, as FlowListTests does, so the AOT build gives the same answer.
    static HashSet<string> KnownTests()
    {
        var known = new HashSet<string>();
        foreach (var path in Directory.GetFiles(Repo.File("windows/OpenPainter.Tests/src"), "*.cs"))
        {
            var type = "";
            foreach (var line in File.ReadAllLines(path))
            {
                if (Regex.Match(line, @"^public sealed (?:partial )?class (\w+)") is { Success: true } c) type = c.Groups[1].Value;
                if (Regex.Match(line, @"^    public (?:static )?void (\w+)\(") is { Success: true } m) known.Add($"{type}.{m.Groups[1].Value}");
            }
        }
        return known;
    }

    [Fact]
    public void EachRowThatWorksNamesARealTestAndEachOtherRowSaysWhenItComes()
    {
        var doc = ShortcutDoc.Table(ShortcutDoc.Text(), "## Canvas, panel and dialog keys");
        Assert.Empty(RowStateProblems(doc, RowTests, KnownTests()));
    }

    [Fact]
    public void ARowThatWorksWithNoTestOrAnUnknownStateFailsByName()
    {
        var tests = new Dictionary<string, string[]> { ["Has one"] = ["Real.Test"], ["Off with one"] = ["Real.Test"], ["Bad name"] = ["Not.There"] };
        var wrong = RowStateProblems(
            [["No test", "x", "y", "works"], ["Has one", "x", "y", "works"], ["Strange", "x", "y", "maybe"], ["Off with one", "x", "y", "off until W9"], ["Bad name", "x", "y", "works"], ["Too soon", "x", "y", "off until W8"]],
            tests, new HashSet<string> { "Real.Test" });
        Assert.Equal(5, wrong.Count);
        Assert.Contains("No test: it works and has no test", wrong);
        Assert.Contains("Off with one: it says off until W9 and has a test", wrong);
        Assert.Contains("Bad name: there is no test Not.There", wrong);
        Assert.Contains(wrong, w => w.StartsWith("Strange:", StringComparison.Ordinal));
        Assert.Contains(wrong, w => w.StartsWith("Too soon:", StringComparison.Ordinal));
    }

    /// Reads each tool row's keys from the doc and presses them from another tool, each way the row gives.
    [AvaloniaFact]
    public void EachToolRowOfTheDocPicksThatTool()
    {
        var w = Shells.Open();
        w.CanvasInput.Focus();
        var tools = w.Tools;
        var doc = ShortcutDoc.Table(ShortcutDoc.Text(), "## Canvas, panel and dialog keys");
        var labels = MacCatalog.Embedded().Tools.Select(t => t.Label).ToHashSet();
        var rows = doc.Where(r => labels.Contains(r[0])).ToList();
        Assert.Equal(19, rows.Count);
        foreach (var row in rows)
        {
            foreach (var way in row[2].Split(" or "))
            {
                var parts = way.Split(" x");
                var times = parts.Length == 2 ? int.Parse(parts[1]) : 1;
                var shift = parts[0].StartsWith("Shift+", StringComparison.Ordinal);
                var key = Enum.Parse<Key>(shift ? parts[0]["Shift+".Length..] : parts[0]);
                // Start from a tool outside the row's group, as the row's count means.
                tools.Current = key == Key.H ? "paintbrush" : ToolState.Pan;
                for (var i = 0; i < times; i++) Press(w, key, shift ? RawInputModifiers.Shift : RawInputModifiers.None);
                Assert.True(row[0] == tools.CurrentTool.Label, $"{row[0]}: {way} chose {tools.CurrentTool.Label}");
            }
        }
        w.Close();
    }

    /// A row that says it comes later must do nothing until then.
    [AvaloniaFact]
    public void TheRowsThatAreOffStillDoNothing()
    {
        var w = Shells.Open();
        w.CanvasInput.Focus();
        var doc = ShortcutDoc.Table(ShortcutDoc.Text(), "## Canvas, panel and dialog keys");
        var off = doc.Where(r => NeedsTest(r[3]) == false).ToList();
        Assert.Equal(3, off.Count);
        foreach (var words in new[] { "Up", "Down", "Left", "Right", "Ctrl+Up", "Ctrl+Down", "Ctrl+Left", "Ctrl+Right", "Esc" })
            Assert.False(w.Route(words, false, KeySource.Canvas, w.CanvasInput), words);
        w.Close();
    }

    // The keys, pressed. The Mac's canvas keys come from testCanvasKeysCycleToolsChangeTheWidthAndSwapColors.

    static void Press(Avalonia.Controls.Window w, Key key, RawInputModifiers mods = RawInputModifiers.None)
    {
        w.KeyPress(key, mods, PhysicalKey.None, null);
        Avalonia.Threading.Dispatcher.UIThread.RunJobs();
    }

    [AvaloniaFact]
    public void TheCanvasKeysCycleToolsChangeTheWidthAndSwapColorsAsOnTheMac()
    {
        var w = Shells.Open();
        var tools = w.Tools;
        w.CanvasInput.Focus();
        tools.Current = "paintbrush";
        Press(w, Key.S);
        Assert.Equal("rectangleSelect", tools.Current);
        Press(w, Key.S);
        Assert.Equal("lassoSelect", tools.Current);
        Press(w, Key.S, RawInputModifiers.Shift);
        Assert.Equal("rectangleSelect", tools.Current);
        Press(w, Key.S, RawInputModifiers.Shift);
        Assert.Equal("magicWand", tools.Current);
        tools.Current = "paintbrush";
        Press(w, Key.S, RawInputModifiers.Shift);
        Assert.Equal("magicWand", tools.Current);
        Press(w, Key.M, RawInputModifiers.Shift);
        Assert.Equal("moveSelection", tools.Current);
        Press(w, Key.B);
        Assert.Equal("paintbrush", tools.Current);

        tools.Settings = tools.Settings with { BrushWidth = 10 };
        Press(w, Key.OemCloseBrackets);
        Assert.Equal(11, tools.Settings.BrushWidth);
        Press(w, Key.OemOpenBrackets);
        Press(w, Key.OemOpenBrackets);
        Assert.Equal(9, tools.Settings.BrushWidth);
        Press(w, Key.OemCloseBrackets, RawInputModifiers.Control);
        Assert.Equal(14, tools.Settings.BrushWidth);
        Press(w, Key.OemOpenBrackets, RawInputModifiers.Control);
        Assert.Equal(9, tools.Settings.BrushWidth);
        tools.Settings = tools.Settings with { BrushWidth = 1 };
        Press(w, Key.OemOpenBrackets);
        Assert.Equal(1, tools.Settings.BrushWidth);

        var (p, s) = (tools.Primary, tools.Secondary);
        Press(w, Key.X);
        Assert.Equal(s, tools.Primary);
        Assert.Equal(p, tools.Secondary);
        var slot = tools.ActiveSlot;
        Press(w, Key.C);
        Assert.Equal(ToolState.Other(slot), tools.ActiveSlot);
        Press(w, Key.D);
        Assert.Equal(new DocColor(0, 0, 0), tools.Primary);
        Assert.Equal(new DocColor(255, 255, 255), tools.Secondary);
        w.Close();
    }

    [Theory]
    [InlineData('s', 4)]
    [InlineData('m', 2)]
    [InlineData('o', 2)]
    public void RepeatedLettersWalkEachGroupBothWays(char letter, int size)
    {
        var tools = new ToolState(MacCatalog.Embedded().Tools);
        var forward = new List<string>();
        for (var i = 0; i < size; i++)
        {
            tools.Current = tools.ToolForKey(letter)!;
            forward.Add(tools.Current);
        }
        Assert.Equal(size, forward.Distinct().Count());
        tools.Current = ToolState.Pan;
        var back = new List<string>();
        for (var i = 0; i < size; i++)
        {
            tools.Current = tools.ToolForKey(letter, reverse: true)!;
            back.Add(tools.Current);
        }
        back.Reverse();
        Assert.Equal(forward, back);
    }

    [AvaloniaFact]
    public void AHeldLetterDoesNotKeepCyclingTheTools()
    {
        var tools = new ToolState(MacCatalog.Embedded().Tools);
        Assert.True(CanvasKeys.Run(tools, null, "S", repeat: false));
        var first = tools.Current;
        Assert.False(CanvasKeys.Run(tools, null, "S", repeat: true));
        Assert.Equal(first, tools.Current);
        Assert.True(CanvasKeys.Run(tools, null, "]", repeat: true));
    }

    [AvaloniaFact]
    public void CtrlAltDigitsToggleThePanelsAsTheHiddenMacItemsDo()
    {
        var w = Shells.Open();
        w.Focus();
        Press(w, Key.D2, RawInputModifiers.Control | RawInputModifiers.Alt);
        Assert.False(w.Panels.IsShown("history"));
        Press(w, Key.D2, RawInputModifiers.Control | RawInputModifiers.Alt);
        Assert.True(w.Panels.IsShown("history"));
        w.Close();
    }

    [Theory]
    [InlineData(0xBB, true, true, false, "Ctrl++")]
    [InlineData(0xBF, true, true, false, "Ctrl+?")]
    [InlineData(0xDB, false, false, false, "[")]
    [InlineData(0xDD, true, false, false, "Ctrl+]")]
    [InlineData(0x1B, false, false, false, "Esc")]
    [InlineData(0x0D, false, false, false, "Enter")]
    [InlineData(0x09, true, true, false, "Ctrl+Shift+Tab")]
    [InlineData(0x53, false, true, false, "Shift+S")]
    public void MoreWindowKeysBecomeTheirWords(int vk, bool ctrl, bool shift, bool alt, string words) =>
        Assert.Equal(words, WindowsKeys.FromVirtualKey(vk, ctrl, shift, alt));
}
