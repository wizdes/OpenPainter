// The current tool, from the catalog's list, and the words that go with it.
// ToolState.swift is the source. Tools that change pixels come in W8, so here choosing
// one changes the options row and the hint only.
using OpenPainter.Canvas;
using OpenPainter.Catalog;

namespace OpenPainter.Shell;

public sealed class ToolState
{
    public const string Default = "paintbrush";   // ToolState.tool's first value.
    public const string Zoom = "zoom", Pan = "pan", MagicWand = "magicWand";

    string _current = Default;

    public IReadOnlyList<CatalogTool> Tools { get; }
    public event Action? Changed;

    public ToolState(IReadOnlyList<CatalogTool> tools)
    {
        Tools = tools;
        if (tools.All(t => t.Id != Default)) throw new InvalidDataException($"CATALOG: no tool named {Default}");
    }

    public string Current
    {
        get => _current;
        set
        {
            if (value == _current) return;
            if (Tools.All(t => t.Id != value)) throw new ArgumentException($"TOOL: no tool named {value}");
            _current = value;
            Changed?.Invoke();
        }
    }

    public CatalogTool CurrentTool => Tools.First(t => t.Id == _current);

    /// What the canvas does with a drag for this tool. Only Zoom and Pan act in W7.
    public CanvasTool CanvasMode => _current switch { Zoom => CanvasTool.Zoom, Pan => CanvasTool.Pan, _ => CanvasTool.Inactive };

    /// PaintTool.tool(forKey:): the next tool for a key press, cycling among tools that share the key.
    public string? ToolForKey(char key)
    {
        var letter = char.ToLowerInvariant(key).ToString();
        var matches = Tools.Where(t => t.Key == letter).ToList();
        if (matches.Count == 0) return null;
        var at = matches.FindIndex(t => t.Id == _current);
        return at >= 0 ? matches[(at + 1) % matches.Count].Id : matches[0].Id;
    }

    /// The Mac's words with the Windows names for its keys.
    public static string WindowsWords(string mac) => mac
        .Replace("Option-click", "Alt-click", StringComparison.Ordinal)
        .Replace("Command-click", "Ctrl-click", StringComparison.Ordinal)
        .Replace("Command ", "Ctrl ", StringComparison.Ordinal)
        .Replace("Option ", "Alt ", StringComparison.Ordinal);

    /// PaintTool.hint, in Windows words.
    public static string Hint(CatalogTool tool)
    {
        if (!tool.Functional) return $"{tool.Label} is coming soon";
        return WindowsWords(tool.Id switch
        {
            Zoom => "Zoom: click to zoom in, Option-click to zoom out, drag a rectangle",
            Pan => "Pan: drag to move the view",
            MagicWand => "Magic Wand: click to select a region of similar color",
            "rectangleSelect" => "Rectangle Select: drag a rectangle; Command adds, Option subtracts, Shift makes a square",
            "ellipseSelect" => "Ellipse Select: drag an ellipse; Command adds, Option subtracts, Shift makes a circle",
            "lassoSelect" => "Lasso Select: drag around an area; Command adds, Option subtracts",
            "paintBucket" => "Paint Bucket: click to fill with the primary color, right-click for the secondary",
            "colorPicker" => "Color Picker: click sets the primary color, right-click the secondary; Command-click samples the image",
            "text" => "Text: click to type; Escape, Finish, a click outside or another tool commits",
            "moveSelectedPixels" => "Move Selected Pixels: drag to move, nubs scale, drag outside to rotate; Enter finishes, Escape cancels",
            "moveSelection" => "Move Selection: drag to move the outline, nubs scale, drag outside to rotate; Enter finishes",
            _ => $"{tool.Label}: drag to paint",
        });
    }
}
