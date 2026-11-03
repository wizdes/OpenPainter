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

public sealed record MacCatalog(IReadOnlyList<CatalogEntry> Menus)
{
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
        return new MacCatalog(root.GetProperty("menus").EnumerateArray().Select(Entry).ToList());
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

    static string? Text(JsonElement e, string name) => e.TryGetProperty(name, out var v) ? v.GetString() : null;
}
