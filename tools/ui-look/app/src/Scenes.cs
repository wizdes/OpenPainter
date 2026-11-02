// What each scene shows. The content copies the Mac shots: same rows, labels and numbers.
namespace UiLook;

public enum Options { Brush, Text, Selection, Flood }
public enum DialogKind { None, Curves, HueSaturation }
public enum CanvasFx { None, Text, Selection, CurvesPreview, HueSaturationPreview }

public sealed record HistoryEntry(string Use, string Icon, string Title);
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
        new("history:document.open", "doc-image", "Open"),
        new("history:effect.adjust.*", "step-adjustment", "Curves"),
        new("history:effect.adjust.*", "step-adjustment", "Hue / Saturation"),
        new("history:selection.ellipse", "tool-ellipse-select", "Ellipse Select"),
        new("history:selection.deselect", "cmd-deselect", "Deselect"),
        new("history:layer.add", "layer-add", "Add New Layer"),
        new("history:tool.text.finish", "tool-text", "Text"),
    ];

    /// The Tools panel in the Mac's order: two columns, read across. Case is the Mac's
    /// PaintTool case, so a test can tie each button to its row in icons.toml.
    public static readonly (string Case, string Icon, string Label)[] Tools =
    [
        ("rectangleSelect", "tool-rectangle-select", "Rectangle Select"), ("moveSelectedPixels", "tool-move-pixels", "Move Selected Pixels"),
        ("lassoSelect", "tool-lasso", "Lasso Select"), ("moveSelection", "tool-move-selection", "Move Selection"),
        ("ellipseSelect", "tool-ellipse-select", "Ellipse Select"), ("zoom", "tool-zoom", "Zoom"),
        ("magicWand", "tool-magic-wand", "Magic Wand"), ("pan", "tool-pan", "Pan"),
        ("paintBucket", "tool-paint-bucket", "Paint Bucket"), ("gradient", "tool-gradient", "Gradient"),
        ("paintbrush", "tool-paintbrush", "Paintbrush"), ("eraser", "tool-eraser", "Eraser"),
        ("pencil", "tool-pencil", "Pencil"), ("colorPicker", "tool-color-picker", "Color Picker"),
        ("cloneStamp", "tool-clone-stamp", "Clone Stamp"), ("recolor", "tool-recolor", "Recolor"),
        ("text", "tool-text", "Text"), ("lineCurve", "tool-line", "Line / Curve"),
        ("shapes", "tool-shapes", "Shapes"),
    ];

    /// The Layers panel's action row, with the row in icons.toml for each.
    public static readonly (string Use, string Icon)[] LayerActions =
    [
        ("site:LayersPanel.swift:Add New Layer", "layer-add"),
        ("site:LayersPanel.swift:Delete Layer", "layer-delete"),
        ("site:LayersPanel.swift:Duplicate Layer", "layer-duplicate"),
        ("site:LayersPanel.swift:Merge Layer Down", "layer-merge-down"),
        ("site:LayersPanel.swift:Move Layer Up", "arrow-up"),
        ("site:LayersPanel.swift:Move Layer Down", "arrow-down"),
        ("site:LayersPanel.swift:Layer Properties", "layer-properties"),
    ];

    /// The command buttons at the start of the options row, with their rows in icons.toml.
    public static readonly (string Use, string Icon)[] BarCommands =
    [
        ("bar:new", "cmd-new"), ("bar:open", "cmd-open"), ("bar:save", "cmd-save"),
        ("bar:undo", "cmd-undo"), ("bar:redo", "cmd-redo"),
        ("bar:crop", "cmd-crop"), ("bar:wand", "tool-magic-wand"),
        ("site:CommandBar.swift:More commands", "cmd-more"),
    ];

    /// Not one of the 12. It proves the selected-tool dot and the pressed wand button draw.
    public static readonly Scene MagicWandCheck = new(
        "magic-wand-check", "tool-magic-wand", Options.Flood, "Magic Wand: click to select a region of similar color", "None",
        HistoryCount: 4, HistoryCurrent: 3, TwoLayers: false, "#CF684D", "#E9E7DC", ChosenSwatch: -1,
        CanvasFx.None, DialogKind.None, Dirty: true, CropEnabled: false, UndoEnabled: true);

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
