// The Mac app's menus, read from windows/assets/catalog/mac-catalog.json.
// `opcli dump-catalog` writes that file from the Swift source, and a test in
// engine/op-cli holds the two equal.
using System.Reflection;
using System.Text.Json;

namespace OpenPainter.Catalog;

public enum EntryKind { Menu, Item, Separator }

public sealed record CatalogEntry(
    EntryKind Kind,
    string Path,
    string Title,
    string? Shortcut,
    string Action,
    bool Off,
    string? Icon,
    IReadOnlyList<CatalogEntry> Items);

/// One tool in the Mac's order. `Options` names its options row, and `Key` is its letter.
public sealed record CatalogTool(string Id, string Label, string Key, string Options, string? Icon, bool Functional);

/// One floating panel in the Mac's order.
public sealed record CatalogPanel(string Id, string Title, string Shortcut, string? Icon);

public sealed record MacCatalog(IReadOnlyList<CatalogEntry> Menus)
{
    public IReadOnlyList<CatalogTool> Tools { get; init; } = [];
    public IReadOnlyList<CatalogPanel> Panels { get; init; } = [];

    const string ResourceName = "mac-catalog.json";

    /// The copy built into the program.
    public static MacCatalog Embedded()
    {
        using var stream = Assembly.GetExecutingAssembly().GetManifestResourceStream(ResourceName)
            ?? throw new InvalidOperationException($"CATALOG: resource {ResourceName} is missing");
        return Read(stream);
    }

    public static MacCatalog Read(Stream json)
    {
        using var document = JsonDocument.Parse(json);
        var root = document.RootElement;
        var schema = root.GetProperty("schema").GetInt32();
        if (schema != 1) throw new InvalidDataException($"CATALOG: unknown schema {schema}");
        return new MacCatalog(root.GetProperty("menus").EnumerateArray().Select(Entry).ToList())
        {
            Tools = root.TryGetProperty("tools", out var tools) ? tools.EnumerateArray().Select(Tool).ToList() : [],
            Panels = root.TryGetProperty("panels", out var panels) ? panels.EnumerateArray().Select(Panel).ToList() : [],
        };
    }

    static CatalogEntry Entry(JsonElement e)
    {
        var path = e.GetProperty("path").GetString() ?? throw new InvalidDataException("CATALOG: a row has an empty path");
        var kind = e.GetProperty("kind").GetString() switch
        {
            "menu" => EntryKind.Menu,
            "item" => EntryKind.Item,
            "separator" => EntryKind.Separator,
            var other => throw new InvalidDataException($"CATALOG: {path}: unknown kind {other}"),
        };
        return new CatalogEntry(
            kind,
            path,
            Text(e, "title") ?? "",
            Text(e, "shortcut"),
            Text(e, "action") ?? "",
            e.TryGetProperty("off", out var off) && off.GetBoolean(),
            Text(e, "icon"),
            e.TryGetProperty("items", out var items) ? items.EnumerateArray().Select(Entry).ToList() : []);
    }

    static CatalogTool Tool(JsonElement e) => new(
        Need(e, "id", "tool"), Need(e, "label", "tool"), Need(e, "key", "tool"), Need(e, "options", "tool"), Text(e, "icon"),
        e.GetProperty("functional").GetBoolean());

    static CatalogPanel Panel(JsonElement e) =>
        new(Need(e, "id", "panel"), Need(e, "title", "panel"), Need(e, "shortcut", "panel"), Text(e, "icon"));

    static string Need(JsonElement e, string name, string what) =>
        Text(e, name) ?? throw new InvalidDataException($"CATALOG: a {what} row has no {name}");

    static string? Text(JsonElement e, string name) => e.TryGetProperty(name, out var v) ? v.GetString() : null;
}
