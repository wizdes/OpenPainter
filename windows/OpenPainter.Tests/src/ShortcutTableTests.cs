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

    /// Each difference this build really has, by where.
    public static Dictionary<string, string> MenuDifferences(IReadOnlyCollection<string> working)
    {
        var menus = WindowsMenus.Build(MacCatalog.Embedded());
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
        return found;
    }

    [GeneratedRegex(@"^Ctrl\+Alt\+[1-4]$")] public static partial Regex HiddenTwin();
}

public sealed class ShortcutTableTests
{
    static List<string[]> Differences() => ShortcutDoc.Table(ShortcutDoc.Text(), "## Differences from the Mac");

    /// The rows the table has, by where, with Windows keys that a test can compare.
    static Dictionary<string, string> TableRows() => Differences().Where(r => r[1] != "none").ToDictionary(r => r[0], r => r[3]);

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
        var table = TableRows().Where(r => r.Key.Contains(" > ", StringComparison.Ordinal)).ToDictionary(r => r.Key, r => r.Value);
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

    // The keys, pressed. The Mac's canvas keys come from testCanvasKeysCycleToolsChangeTheWidthAndSwapColors.

    static void Press(MainWindow w, Key key, RawInputModifiers mods = RawInputModifiers.None)
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
