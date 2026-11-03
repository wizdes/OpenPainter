// Turns the Mac menus into the Windows menus. The Mac decides the order and the
// words. This file holds every place Windows differs, so a test can check that
// nothing else changed.
using System.Text;
using Avalonia.Input;

namespace OpenPainter.Catalog;

public sealed record MenuEntry(
    EntryKind Kind,
    string Path,
    string Title,
    string? Key,
    string Action,
    string? Icon,
    IReadOnlyList<MenuEntry> Items)
{
    /// One line in the notation the tests compare: `File > Save [Ctrl+S]`.
    public string Line => Key is null ? Path : $"{Path} [{Key}]";
}

public static class WindowsMenus
{
    public const string ExitAction = "selector:terminate:";

    /// Mac items that Windows does not show, each with the reason.
    public static readonly IReadOnlyDictionary<string, string> Removed = new Dictionary<string, string>
    {
        ["OpenPainter > Hide OpenPainter"] = "Windows apps have no hide command.",
        ["OpenPainter > Hide Others"] = "Windows apps have no hide command.",
        ["OpenPainter > Show All"] = "Windows apps have no hide command.",
        ["Window > Minimize"] = "The minimize button in the title row does this.",
        ["Window > Zoom"] = "The maximize button in the title row does this, and here Zoom means the image.",
        ["Window > Bring All to Front"] = "Windows apps have no such command.",
    };

    /// Mac items that move, because Windows has no app menu. Each says where and under what words.
    public static readonly IReadOnlyList<(string MacPath, string Menu, string Title, string? Key)> Moved =
    [
        ("OpenPainter > Settings…", "File", "Settings…", null),
        ("OpenPainter > Quit OpenPainter", "File", "Exit", "Alt+F4"),
        ("OpenPainter > About OpenPainter", "Help", "About OpenPainter", null),
    ];

    /// The Mac's app menu. Its items are moved or removed, and the menu goes.
    public const string AppMenu = "OpenPainter";

    public static IReadOnlyList<MenuEntry> Build(MacCatalog catalog)
    {
        var app = catalog.Menus.FirstOrDefault(m => m.Title == AppMenu)
            ?? throw new InvalidDataException($"CATALOG: no {AppMenu} menu");
        foreach (var item in app.Items.Where(i => i.Kind != EntryKind.Separator))
            if (!Removed.ContainsKey(item.Path) && Moved.All(m => m.MacPath != item.Path))
                throw new InvalidDataException($"CATALOG: {item.Path} is new in the Mac's app menu. Say where it goes in WindowsMenus.cs");

        var used = new HashSet<string>();
        var result = new List<MenuEntry>();
        foreach (var menu in catalog.Menus)
        {
            if (menu.Title == AppMenu) continue;
            var items = Convert(menu.Items, used).ToList();
            foreach (var move in Moved.Where(m => m.Menu == menu.Title))
            {
                var source = app.Items.FirstOrDefault(i => i.Path == move.MacPath)
                    ?? throw new InvalidDataException($"CATALOG: {move.MacPath} is gone from the Mac's app menu");
                used.Add(source.Path);
                items.Add(new MenuEntry(EntryKind.Separator, $"{menu.Title} > ---", "", null, "", null, []));
                var key = move.Key ?? (source.Shortcut is null ? null : WindowsKeys.FromMac(source.Shortcut));
                items.Add(new MenuEntry(EntryKind.Item, $"{menu.Title} > {move.Title}", move.Title, key, source.Action, source.Icon, []));
            }
            result.Add(new MenuEntry(EntryKind.Menu, menu.Path, menu.Title, null, "", null, Tidy(items)));
        }
        foreach (var removed in Removed.Keys)
            if (!used.Contains(removed) && !removed.StartsWith(AppMenu + " > ", StringComparison.Ordinal))
                throw new InvalidDataException($"CATALOG: {removed} is listed as removed but the Mac no longer has it");
        foreach (var move in Moved)
            if (!used.Contains(move.MacPath))
                throw new InvalidDataException($"CATALOG: no Windows menu named {move.Menu} for {move.MacPath}");
        return result;
    }

    static IEnumerable<MenuEntry> Convert(IEnumerable<CatalogEntry> items, HashSet<string> used)
    {
        foreach (var item in items)
        {
            if (Removed.ContainsKey(item.Path))
            {
                used.Add(item.Path);
                continue;
            }
            yield return new MenuEntry(
                item.Kind,
                item.Path,
                item.Title,
                item.Shortcut is null ? null : WindowsKeys.FromMac(item.Shortcut),
                item.Action,
                item.Icon,
                item.Kind == EntryKind.Menu ? Tidy(Convert(item.Items, used)) : []);
        }
    }

    /// Removing items can leave two separators in a row, or one at the end. Neither shows.
    static List<MenuEntry> Tidy(IEnumerable<MenuEntry> items)
    {
        var result = new List<MenuEntry>();
        foreach (var item in items)
        {
            if (item.Kind == EntryKind.Separator && (result.Count == 0 || result[^1].Kind == EntryKind.Separator)) continue;
            result.Add(item);
        }
        if (result.Count != 0 && result[^1].Kind == EntryKind.Separator) result.RemoveAt(result.Count - 1);
        return result;
    }

    /// One line per visible entry, in order, for comparing.
    public static IEnumerable<string> Lines(IEnumerable<MenuEntry> menus)
    {
        foreach (var entry in menus)
        {
            yield return entry.Line;
            foreach (var line in Lines(entry.Items)) yield return line;
        }
    }
}

/// The Mac's shortcut notation to Windows words: Ctrl for Command, Alt for Option.
public static class WindowsKeys
{
    public static string FromMac(string mac)
    {
        bool ctrl = false, shift = false, alt = false;
        var at = 0;
        for (; at < mac.Length; at++)
        {
            var c = mac[at];
            if (c is '⌘' or '⌃') ctrl = true;
            else if (c == '⌥') alt = true;
            else if (c == '⇧') shift = true;
            else break;
        }
        var key = mac[at..];
        if (key.Length == 0) throw new FormatException($"SHORTCUT: {mac} has no key");
        var text = new StringBuilder();
        if (ctrl) text.Append("Ctrl+");
        if (shift) text.Append("Shift+");
        if (alt) text.Append("Alt+");
        // The Mac's Delete key is in the Windows Backspace place, and its menus mean what Windows calls Delete.
        // Windows names the = key by its plus sign in menus, as paint.net's Ctrl++ does.
        text.Append(key switch { "⌫" => "Delete", "=" => "+", _ => key });
        return text.ToString();
    }

    /// The gesture a menu shows, from the Windows words.
    public static KeyGesture Gesture(string text)
    {
        var modifiers = KeyModifiers.None;
        var rest = text;
        foreach (var (name, flag) in new[] { ("Ctrl+", KeyModifiers.Control), ("Shift+", KeyModifiers.Shift), ("Alt+", KeyModifiers.Alt) })
        {
            if (rest.StartsWith(name, StringComparison.Ordinal))
            {
                modifiers |= flag;
                rest = rest[name.Length..];
            }
        }
        return new KeyGesture(KeyFor(rest, text), modifiers);
    }

    static Key KeyFor(string name, string whole)
    {
        if (name.Length == 1 && name[0] is >= 'A' and <= 'Z') return Key.A + (name[0] - 'A');
        if (name.Length == 1 && name[0] is >= '0' and <= '9') return Key.D0 + (name[0] - '0');
        if (name.Length >= 2 && name[0] == 'F' && int.TryParse(name[1..], out var f) && f is >= 1 and <= 24) return Key.F1 + (f - 1);
        return name switch
        {
            "Delete" => Key.Delete,
            "PageUp" => Key.PageUp,
            "PageDown" => Key.PageDown,
            "+" => Key.OemPlus,
            "-" => Key.OemMinus,
            "'" => Key.OemQuotes,
            "," => Key.OemComma,
            "`" => Key.OemTilde,
            "?" => Key.OemQuestion,
            _ => throw new FormatException($"SHORTCUT: no key for `{name}` in {whole}"),
        };
    }
}
