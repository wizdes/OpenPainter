// What each scene shows. The content copies the Mac shots: same rows, labels and numbers.
namespace UiLook;

public enum Options { Brush, Text, Selection }
public enum DialogKind { None, Curves, HueSaturation }
public enum CanvasFx { None, Text, Selection, CurvesPreview, HueSaturationPreview }

public sealed record HistoryEntry(string Icon, string Title);
public sealed record LayerEntry(string Name, bool HasImage);

public sealed record Scene(
    string Name,
    string ToolIcon,
    Options Options,
    string StatusHint,
    string SelectionText,
    int HistoryCount,          // How many rows of the shared list show.
    int HistoryCurrent,
    bool TwoLayers,
    string PrimaryHex,
    string SecondaryHex,
    int ChosenSwatch,          // -1 when no swatch equals the active color.
    CanvasFx Canvas,
    DialogKind Dialog,
    bool Dirty,
    bool CropEnabled,
    bool UndoEnabled);

public static class Scenes
{
    public const string Title = "plasma_4096x3072.png";
    public const string StripTitle = "plasma_4096...";

    /// The Mac shots' seven steps, in order.
    public static readonly HistoryEntry[] History =
    [
        new("doc-image", "Open"),
        new("colors-contrast", "Curves"),
        new("colors-contrast", "Hue / Saturation"),
        new("tool-ellipse-select", "Ellipse Select"),
        new("tool-rectangle-select", "Deselect"),
        new("layer-add", "Add New Layer"),
        new("tool-text", "Text"),
    ];

    /// The Tools panel in the Mac's order: two columns, read across.
    public static readonly (string Icon, string Label)[] Tools =
    [
        ("tool-rectangle-select", "Rectangle Select"), ("tool-move-pixels", "Move Selected Pixels"),
        ("tool-lasso", "Lasso Select"), ("tool-move-selection", "Move Selection"),
        ("tool-ellipse-select", "Ellipse Select"), ("tool-zoom", "Zoom"),
        ("tool-magic-wand", "Magic Wand"), ("tool-pan", "Pan"),
        ("tool-paint-bucket", "Paint Bucket"), ("tool-gradient", "Gradient"),
        ("tool-paintbrush", "Paintbrush"), ("tool-eraser", "Eraser"),
        ("tool-pencil", "Pencil"), ("tool-color-picker", "Color Picker"),
        ("tool-clone-stamp", "Clone Stamp"), ("tool-recolor", "Recolor"),
        ("tool-text", "Text"), ("tool-line", "Line / Curve"),
        ("tool-shapes", "Shapes"),
    ];

    public static readonly Scene Main = new(
        "main", "tool-paintbrush", Options.Brush, "Paintbrush: drag to paint", "None",
        HistoryCount: 7, HistoryCurrent: 6, TwoLayers: true, "#D41E3A", "#FFFFFF", ChosenSwatch: -1,
        CanvasFx.None, DialogKind.None, Dirty: true, CropEnabled: false, UndoEnabled: true);

    public static readonly Scene Text = new(
        "text", "tool-text", Options.Text, "Text: click to type; Escape, Finish, a click outside or another tool commits", "None",
        HistoryCount: 6, HistoryCurrent: 5, TwoLayers: true, "#FFFFFF", "#E9E7DC", ChosenSwatch: 7,
        CanvasFx.Text, DialogKind.None, Dirty: true, CropEnabled: false, UndoEnabled: true);

    public static readonly Scene Selection = new(
        "selection", "tool-ellipse-select", Options.Selection,
        "Ellipse Select: drag an ellipse; Ctrl adds, Alt subtracts, Shift makes a circle", "1200 × 900 px",
        HistoryCount: 4, HistoryCurrent: 3, TwoLayers: false, "#CF684D", "#E9E7DC", ChosenSwatch: -1,
        CanvasFx.Selection, DialogKind.None, Dirty: true, CropEnabled: true, UndoEnabled: true);

    public static readonly Scene Curves = new(
        "curves", "tool-paintbrush", Options.Brush, "Paintbrush: drag to paint", "None",
        HistoryCount: 1, HistoryCurrent: 0, TwoLayers: false, "#CF684D", "#E9E7DC", ChosenSwatch: -1,
        CanvasFx.CurvesPreview, DialogKind.Curves, Dirty: false, CropEnabled: false, UndoEnabled: false);

    public static readonly Scene HueSaturation = new(
        "hue-saturation", "tool-paintbrush", Options.Brush, "Paintbrush: drag to paint", "None",
        HistoryCount: 2, HistoryCurrent: 1, TwoLayers: false, "#CF684D", "#E9E7DC", ChosenSwatch: -1,
        CanvasFx.HueSaturationPreview, DialogKind.HueSaturation, Dirty: true, CropEnabled: false, UndoEnabled: true);

    public static readonly Scene[] WindowScenes = [Main, Text, Selection, Curves, HueSaturation];

    public const string ColorsExpanded = "colors-expanded";

    /// Layer rows for a scene, top row first.
    public static LayerEntry[] Layers(Scene s) => s.TwoLayers
        ? [new("Layer 2", false), new("Background", true)]
        : [new("Background", true)];
}
