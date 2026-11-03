// The menus against the catalog: order, words and shortcuts, with Ctrl for Command.
using System.Text;
using Avalonia.Controls;
using Avalonia.Headless.XUnit;
using OpenPainter.Catalog;
using OpenPainter.Shell;
using Xunit;

namespace OpenPainter.Tests;

public sealed class MenuTests
{
    const string CatalogPath = "windows/assets/catalog/mac-catalog.json";

    static MacCatalog FromText(string json) => MacCatalog.Read(new MemoryStream(Encoding.UTF8.GetBytes(json)));
    static MacCatalog Catalog() => FromText(Repo.Read(CatalogPath));

    /// One line per entry from the real controls, in the shape `WindowsMenus.Lines` gives.
    static IEnumerable<string> Walk(IEnumerable<object?> items, string parent)
    {
        foreach (var item in items)
        {
            if (item is Separator)
            {
                yield return $"{parent} > ---";
                continue;
            }
            var menu = Assert.IsAssignableFrom<MenuItem>(item);
            var path = parent.Length == 0 ? (string)menu.Header! : $"{parent} > {menu.Header}";
            Assert.Equal(menu is CatalogMenuItem { Entry.Key: not null }, menu.InputGesture is not null);
            yield return menu is CatalogMenuItem { Entry.Key: { } key } ? $"{path} [{key}]" : path;
            foreach (var line in Walk(menu.Items, path)) yield return line;
        }
    }

    [AvaloniaFact]
    public void TheRealMenuControlsMatchTheWindowsMenusLineByLine()
    {
        var window = Shells.Open();
        var want = WindowsMenus.Lines(WindowsMenus.Build(MacCatalog.Embedded())).ToList();
        var got = Walk(window.TitleBar.Menus.Items, "").ToList();
        Assert.True(want.Count > 200, $"only {want.Count} lines");
        Assert.Equal(want, got);
        window.Close();
    }

    [Fact]
    public void TheEmbeddedCatalogIsTheCheckedInFile()
    {
        var want = WindowsMenus.Lines(WindowsMenus.Build(Catalog())).ToList();
        var got = WindowsMenus.Lines(WindowsMenus.Build(MacCatalog.Embedded())).ToList();
        Assert.Equal(want, got);
    }

    [Fact]
    public void TheTopMenusAreTheMacOrderWithoutTheAppMenu()
    {
        var titles = WindowsMenus.Build(Catalog()).Select(m => m.Title).ToArray();
        Assert.Equal(["File", "Edit", "View", "Image", "Layers", "Adjustments", "Effects", "Window", "Help"], titles);
    }

    /// Every Mac menu reads the same on Windows, except for the items the rule tables name.
    [Fact]
    public void EveryMacItemKeepsItsPlaceAndWordsUnlessARuleNamesIt()
    {
        var catalog = Catalog();
        var windows = WindowsMenus.Build(catalog);
        var moved = WindowsMenus.Moved.Select(m => $"{m.Menu} > {m.Title}").ToHashSet();

        static IEnumerable<(string Path, string? Key)> Flatten(IEnumerable<CatalogEntry> items)
        {
            foreach (var item in items)
            {
                if (item.Kind == EntryKind.Separator) continue;
                if (WindowsMenus.Removed.ContainsKey(item.Path)) continue;
                yield return (item.Path, item.Shortcut);
                foreach (var inner in Flatten(item.Items)) yield return inner;
            }
        }
        static IEnumerable<MenuEntry> FlattenWindows(IEnumerable<MenuEntry> items)
        {
            foreach (var item in items)
            {
                if (item.Kind == EntryKind.Separator) continue;
                yield return item;
                foreach (var inner in FlattenWindows(item.Items)) yield return inner;
            }
        }

        var macRows = Flatten(catalog.Menus.Where(m => m.Title != WindowsMenus.AppMenu)).ToList();
        var windowsRows = FlattenWindows(windows).Where(e => !moved.Contains(e.Path)).ToList();
        Assert.Equal(macRows.Select(r => r.Path), windowsRows.Select(e => e.Path));
        foreach (var (row, entry) in macRows.Zip(windowsRows))
            Assert.True(entry.Key == (row.Key is null ? null : WindowsKeys.FromMac(row.Key)), $"{row.Path}: {row.Key} became {entry.Key}");
    }

    /// Every item in the Mac's app menu is moved or removed, each named in a table.
    [Fact]
    public void TheAppMenusItemsGoWhereTheRulesSay()
    {
        var windows = WindowsMenus.Build(Catalog());
        var file = windows.Single(m => m.Title == "File").Items;
        var help = windows.Single(m => m.Title == "Help").Items;
        Assert.Equal("Exit", file[^1].Title);
        Assert.Equal("Alt+F4", file[^1].Key);
        Assert.Equal(WindowsMenus.ExitAction, file[^1].Action);
        Assert.Equal(EntryKind.Separator, file[^2].Kind);
        Assert.Equal("Settings…", file[^3].Title);
        Assert.Equal("Ctrl+,", file[^3].Key);
        Assert.Equal("About OpenPainter", help[^1].Title);
        Assert.Equal("app-exit", file[^1].Icon);
    }

    [Theory]
    [InlineData("⌘N", "Ctrl+N")]
    [InlineData("⇧⌘S", "Ctrl+Shift+S")]
    [InlineData("⌥⌘S", "Ctrl+Alt+S")]
    [InlineData("⌥⇧⌘C", "Ctrl+Shift+Alt+C")]
    [InlineData("⌃`", "Ctrl+`")]
    [InlineData("⇧⌃`", "Ctrl+Shift+`")]
    [InlineData("⌫", "Delete")]
    [InlineData("⇧⌫", "Shift+Delete")]
    [InlineData("⇧⌘⌫", "Ctrl+Shift+Delete")]
    [InlineData("⌘=", "Ctrl++")]
    [InlineData("F5", "F5")]
    [InlineData("⌥PageUp", "Alt+PageUp")]
    public void MacKeysBecomeWindowsWords(string mac, string windows) => Assert.Equal(windows, WindowsKeys.FromMac(mac));

    [Fact]
    public void AKeyTheMenuCannotShowFailsByName()
    {
        var error = Assert.Throws<FormatException>(() => WindowsKeys.Gesture("Ctrl+§"));
        Assert.Contains("§", error.Message);
        Assert.Throws<FormatException>(() => WindowsKeys.FromMac("⌘"));
    }

    [Fact]
    public void EveryMenuShortcutHasAGesture()
    {
        foreach (var line in WindowsMenus.Lines(WindowsMenus.Build(Catalog())))
        {
            var open = line.LastIndexOf(" [", StringComparison.Ordinal);
            if (open >= 0) Assert.NotNull(WindowsKeys.Gesture(line[(open + 2)..^1]));
        }
    }

    [Fact]
    public void ACatalogWithSwappedItemsChangesTheLines()
    {
        var text = Repo.Read(CatalogPath);
        var swapped = text.Replace("Zoom In", "Swap").Replace("Zoom Out", "Zoom In").Replace("Swap", "Zoom Out");
        Assert.NotEqual(
            WindowsMenus.Lines(WindowsMenus.Build(FromText(text))),
            WindowsMenus.Lines(WindowsMenus.Build(FromText(swapped))));
    }

    [Fact]
    public void ANewItemInTheMacAppMenuFailsByName()
    {
        var text = Repo.Read(CatalogPath).Replace("OpenPainter > Show All", "OpenPainter > Brand New");
        var error = Assert.Throws<InvalidDataException>(() => WindowsMenus.Build(FromText(text)));
        Assert.Contains("OpenPainter > Brand New", error.Message);
    }

    [Fact]
    public void ARemovedRuleForAnItemTheMacDroppedFailsByName()
    {
        var text = Repo.Read(CatalogPath).Replace("Window > Bring All to Front", "Window > Other");
        var error = Assert.Throws<InvalidDataException>(() => WindowsMenus.Build(FromText(text)));
        Assert.Contains("Window > Bring All to Front", error.Message);
    }

    [Fact]
    public void AnUnknownRowKindOrSchemaFailsByName()
    {
        var kind = Assert.Throws<InvalidDataException>(() => FromText("""{"schema":1,"menus":[{"kind":"odd","path":"A"}]}"""));
        Assert.Contains("odd", kind.Message);
        var schema = Assert.Throws<InvalidDataException>(() => FromText("""{"schema":2,"menus":[]}"""));
        Assert.Contains("2", schema.Message);
    }

    [Fact]
    public void EveryIconTheMenusNameIsOnDisk()
    {
        static IEnumerable<string> Icons(IEnumerable<MenuEntry> items) =>
            items.SelectMany(i => (i.Icon is null ? [] : new[] { i.Icon }).Concat(Icons(i.Items)));
        var icons = Icons(WindowsMenus.Build(Catalog())).ToList();
        Assert.True(icons.Count > 60, $"only {icons.Count} menu icons");
        foreach (var icon in icons) Assert.True(File.Exists(Repo.File($"windows/assets/icons/{icon}.svg")), $"{icon}.svg is missing");
    }

    [AvaloniaFact]
    public void OnlyWorkingItemsAreOnAndExitClosesTheWindow()
    {
        var window = Shells.Open();
        var closed = false;
        window.Closed += (_, _) => closed = true;
        var file = window.TitleBar.Menus.Items.OfType<MenuItem>().First();
        var items = file.Items.OfType<MenuItem>().ToList();
        Assert.False(items.Single(i => (string?)i.Header == "Open…").IsEnabled);
        var exit = items.Single(i => (string?)i.Header == "Exit");
        Assert.True(exit.IsEnabled);
        exit.RaiseEvent(new Avalonia.Interactivity.RoutedEventArgs(MenuItem.ClickEvent));
        Assert.True(closed);
    }

    /// The words a person reads in the open menu, for a key whose Avalonia text is wrong.
    [AvaloniaFact]
    public void TheOpenMenuShowsTheCatalogsWordsForTheShortcut()
    {
        var window = Shells.Open();
        var view = window.TitleBar.Menus.Items.OfType<MenuItem>().Single(m => (string?)m.Header == "View");
        var grid = view.Items.OfType<CatalogMenuItem>().Single(i => (string?)i.Header == "Pixel Grid");
        Assert.Equal("Ctrl+'", grid.Entry.Key);
        Assert.NotEqual("Ctrl+'", grid.InputGesture!.ToString("p", null));
        view.Open();
        Avalonia.Threading.Dispatcher.UIThread.RunJobs();
        Assert.True(view.IsSubMenuOpen);
        grid.ApplyTemplate();
        var shown = Avalonia.VisualTree.VisualExtensions.GetVisualDescendants(grid).OfType<TextBlock>().Select(t => t.Text).ToList();
        Assert.Contains("Ctrl+'", shown);
        view.Close();
        window.Close();
    }

    [AvaloniaFact]
    public void IconsLoadWhenAMenuOpens()
    {
        UiLook.Icons.Folder = Repo.File("windows/assets/icons");
        var window = Shells.Open();
        var file = window.TitleBar.Menus.Items.OfType<MenuItem>().First();
        var open = file.Items.OfType<MenuItem>().Single(i => (string?)i.Header == "Open…");
        Assert.Null(open.Icon);
        MenuBuilder.LoadIcons(file, window.Palette);
        Assert.IsType<UiLook.IconView>(open.Icon);
        window.Close();
    }

    /// Held apart from WindowsMenus.Removed, so a row added to that table fails here and not
    /// only hides an item from the check above, which reads the table it checks.
    static readonly string[] PinnedRemoved =
    [
        "OpenPainter > Hide OpenPainter",
        "OpenPainter > Hide Others",
        "OpenPainter > Show All",
        "Window > Minimize",
        "Window > Zoom",
        "Window > Bring All to Front",
    ];

    [Fact]
    public void TheRemovedTableIsThePinnedList() =>
        Assert.Equal(PinnedRemoved.Order(StringComparer.Ordinal), WindowsMenus.Removed.Keys.Order(StringComparer.Ordinal));

    /// The rows the Mac has and Windows must show too, separators included, by the rules
    /// written out here and not by the code under test.
    static IEnumerable<string> MacLines(IEnumerable<CatalogEntry> items, string parent)
    {
        var kept = new List<CatalogEntry>();
        foreach (var item in items)
        {
            if (PinnedRemoved.Contains(item.Path)) continue;
            if (item.Kind == EntryKind.Separator && (kept.Count == 0 || kept[^1].Kind == EntryKind.Separator)) continue;
            kept.Add(item);
        }
        if (kept.Count != 0 && kept[^1].Kind == EntryKind.Separator) kept.RemoveAt(kept.Count - 1);
        foreach (var item in kept)
        {
            if (item.Kind == EntryKind.Separator)
            {
                yield return $"{parent} > ---";
                continue;
            }
            yield return item.Path;
            foreach (var line in MacLines(item.Items, item.Path)) yield return line;
        }
    }

    [Fact]
    public void SeparatorsSitWhereTheMacHasThem()
    {
        var catalog = Catalog();
        var windows = WindowsMenus.Build(catalog);
        // Windows adds a separator and an item for each row the Mac's app menu moves here.
        var moved = WindowsMenus.Moved.ToDictionary(m => $"{m.Menu} > {m.Title}", m => m.Menu);
        var separators = 0;
        foreach (var menu in catalog.Menus.Where(m => m.Title != WindowsMenus.AppMenu))
        {
            var want = MacLines(menu.Items, menu.Title).ToList();
            var got = Rows(windows.Single(m => m.Title == menu.Title).Items).ToList();
            for (var i = got.Count - 1; i >= 0; i--)
            {
                if (!moved.ContainsKey(got[i])) continue;
                got.RemoveAt(i);
                Assert.Equal($"{menu.Title} > ---", got[i - 1]);
                got.RemoveAt(i - 1);
                i--;
            }
            Assert.Equal(want, got);
            separators += want.Count(line => line.EndsWith("> ---", StringComparison.Ordinal));
        }
        Assert.True(separators >= 19, $"only {separators} separators were compared");

        static IEnumerable<string> Rows(IEnumerable<MenuEntry> entries)
        {
            foreach (var entry in entries)
            {
                yield return entry.Path;
                foreach (var row in Rows(entry.Items)) yield return row;
            }
        }
    }

    [Fact]
    public void AMissingSeparatorInTheWindowsMenusIsSeen()
    {
        var catalog = Catalog();
        var withSeparators = MacLines(catalog.Menus.Single(m => m.Title == "File").Items, "File").ToList();
        Assert.Contains("File > ---", withSeparators);
        Assert.NotEqual(withSeparators, withSeparators.Where(line => line != "File > ---"));
    }

    /// Each tool's hint against the Swift source, read again each run.
    static Dictionary<string, string> SwiftHints()
    {
        var swift = Repo.Read("mac/OpenPainterKit/Sources/Shell/ToolState.swift");
        var from = swift.IndexOf("public var hint: String", StringComparison.Ordinal);
        Assert.True(from >= 0, "ToolState.swift has no hint");
        var body = swift[from..swift.IndexOf("/// The next tool for a key press", from, StringComparison.Ordinal)];
        var hints = new Dictionary<string, string>();
        foreach (System.Text.RegularExpressions.Match match in System.Text.RegularExpressions.Regex.Matches(body, @"case \.(\w+):\s*return ""([^""]*)"""))
            hints[match.Groups[1].Value] = match.Groups[2].Value;
        return hints;
    }

    /// The Windows names for the Mac's keys, written out, because the code under test does the same.
    static string InWindowsWords(string mac) => mac
        .Replace("Option-click", "Alt-click").Replace("Command-click", "Ctrl-click")
        .Replace("Command adds", "Ctrl adds").Replace("Option subtracts", "Alt subtracts");

    [Fact]
    public void EveryToolHintIsTheSwiftOneInWindowsWords()
    {
        var hints = SwiftHints();
        Assert.Equal(11, hints.Count);
        var tools = Catalog().Tools;
        foreach (var id in hints.Keys) Assert.Contains(tools, tool => tool.Id == id);
        Assert.True(tools.Count >= 19, $"only {tools.Count} tools");
        foreach (var tool in tools)
        {
            var want = !tool.Functional ? $"{tool.Label} is coming soon"
                : hints.TryGetValue(tool.Id, out var hint) ? InWindowsWords(hint)
                : $"{tool.Label}: drag to paint";
            Assert.Equal(want, ToolState.Hint(tool));
        }
        var moved = ToolState.Hint(tools.Single(t => t.Id == "rectangleSelect"));
        Assert.DoesNotContain("Command", moved);
        Assert.DoesNotContain("Option", moved);
    }
}
