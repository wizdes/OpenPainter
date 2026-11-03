// The current tool, from the catalog's list, its options, and the two colors. ToolState.swift
// is the source. Tools that change pixels come in W9, so here choosing one changes the options
// row, the hint and the canvas cursor. The options wait for W9 to read them.
using OpenPainter.Canvas;
using OpenPainter.Catalog;

namespace OpenPainter.Shell;

// Each enum's order is its choice list's order in the options row.
public enum BlendChoice { Normal, Overwrite }
public enum FloodModeChoice { Contiguous, Global }
public enum SamplingChoice { Layer, Image }
public enum ToleranceAlphaChoice { Premultiplied, Straight }
public enum SelectionModeChoice { Replace, Add, Subtract, Intersect, Invert }
public enum PickerSizeChoice { One, Three, Five, Seven, Nine, Eleven, ThirtyOne, FiftyOne }
public enum PickerAfterClickChoice { DoNotSwitch, PreviousTool, Pencil }
public enum MoveResamplingChoice { Nearest, Bilinear, Bicubic, SuperSampling, BestQuality }
public enum SelectStyleChoice { Normal, FixedRatio, FixedSize }
public enum TextAlignChoice { Left, Center, Right }

/// TextToolSettings.swift.
public sealed record TextToolSettings
{
    public const string DefaultFamily = "Segoe UI";   // The Mac has Helvetica Neue, which Windows does not.
    public const double LeastSize = 1, MostSize = 2000;   // TextStyleSpec.sizeRange.

    public string Family { get; init; } = DefaultFamily;
    public double Size { get; init; } = 12;
    public bool Bold { get; init; }
    public bool Italic { get; init; }
    public bool Underline { get; init; }
    public bool Strikethrough { get; init; }
    public TextAlignChoice Alignment { get; init; } = TextAlignChoice.Left;

    /// The size menu's steps: paint.net's list.
    public static readonly double[] Sizes = [8, 9, 10, 11, 12, 14, 16, 18, 20, 22, 24, 26, 28, 36, 48, 72, 84, 96, 108, 144, 192, 216, 288];
}

/// Options for every tool: ToolSettings. The Style, Ratio and Size of Rectangle and Ellipse
/// Select sit here too, where the Mac has them in SelectCropController.
public sealed record ToolSettings
{
    public const double LeastWidth = 1, MostWidth = 2000;

    /// paint.net's brush-width steps for the - and + buttons.
    public static readonly double[] WidthSteps =
    [
        1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 20, 25, 30, 35, 40, 45, 50, 55, 60, 65, 70, 75, 80,
        85, 90, 95, 100, 125, 150, 175, 200, 225, 250, 275, 300, 325, 350, 375, 400, 425, 450, 475, 500, 1000,
        1500, 2000,
    ];

    public double BrushWidth { get; init; } = 2;
    public bool Antialias { get; init; } = true;
    public BlendChoice Blend { get; init; }
    public double Opacity { get; init; } = 100;
    public double Hardness { get; init; } = 100;
    public bool PressureSize { get; init; }
    public bool PressureOpacity { get; init; }
    public double Tolerance { get; init; } = 50;
    public FloodModeChoice FloodMode { get; init; }
    public SamplingChoice Sampling { get; init; }
    public ToleranceAlphaChoice ToleranceAlpha { get; init; }
    public SelectionModeChoice SelectionMode { get; init; }
    public PickerSizeChoice PickerSize { get; init; }
    public SamplingChoice PickerSampling { get; init; }
    public PickerAfterClickChoice PickerAfterClick { get; init; }
    public TextToolSettings Text { get; init; } = new();
    public MoveResamplingChoice MoveResampling { get; init; } = MoveResamplingChoice.Bicubic;
    public SelectStyleChoice SelectStyle { get; init; }
    public double RatioWidth { get; init; } = 1;
    public double RatioHeight { get; init; } = 1;
    public double SizeWidth { get; init; } = 100;
    public double SizeHeight { get; init; } = 100;

    /// stepWidth: the next step up or down.
    public ToolSettings StepWidth(bool up)
    {
        var width = BrushWidth;
        return this with
        {
            BrushWidth = up
                ? WidthSteps.Where(s => s > width).DefaultIfEmpty(MostWidth).First()
                : WidthSteps.Where(s => s < width).DefaultIfEmpty(LeastWidth).Last(),
        };
    }
}

public sealed class ToolState
{
    public const string Default = "paintbrush";   // ToolState.tool's first value.
    public const string Zoom = "zoom", Pan = "pan", MagicWand = "magicWand";

    string _current = Default;
    ToolSettings _settings = new();
    DocColor _primary = new(0xCF, 0x68, 0x4D), _secondary = new(0xE9, 0xE7, 0xDC);
    ColorSlot _activeSlot = ColorSlot.Primary;
    ColorPalette _palette = ColorPalette.Standard;

    public IReadOnlyList<CatalogTool> Tools { get; }

    /// The tool changed.
    public event Action? Changed;

    /// The options, colors, active slot or palette changed. The Mac has one observer list, but
    /// here a tool change also clears the status bar's message, which these must not do.
    public event Action? ValuesChanged;

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
            PreviousTool = _current;
            _current = value;
            Changed?.Invoke();
        }
    }

    /// The tool before the current one: Color Picker's "Switch to previous tool".
    public string? PreviousTool { get; private set; }

    public CatalogTool CurrentTool => Tools.First(t => t.Id == _current);

    public ToolSettings Settings { get => _settings; set => Set(ref _settings, value); }
    public DocColor Primary { get => _primary; set => Set(ref _primary, value); }
    public DocColor Secondary { get => _secondary; set => Set(ref _secondary, value); }

    /// The slot with the notch: Colors panel edits and left-clicks go here.
    public ColorSlot ActiveSlot { get => _activeSlot; set => Set(ref _activeSlot, value); }
    public ColorPalette Palette { get => _palette; set => Set(ref _palette, value); }

    void Set<T>(ref T field, T value) where T : notnull
    {
        if (field.Equals(value)) return;
        field = value;
        ValuesChanged?.Invoke();
    }

    public static ColorSlot Other(ColorSlot slot) => slot == ColorSlot.Primary ? ColorSlot.Secondary : ColorSlot.Primary;

    public DocColor Color(ColorSlot slot) => slot == ColorSlot.Primary ? _primary : _secondary;

    public void SetColor(ColorSlot slot, DocColor color)
    {
        if (slot == ColorSlot.Primary) Primary = color;
        else Secondary = color;
    }

    public void SwapColors() => (Primary, Secondary) = (Secondary, Primary);

    /// The black and white button: primary black, secondary white.
    public void ResetColors()
    {
        Primary = new DocColor(0, 0, 0);
        Secondary = new DocColor(255, 255, 255);
    }

    /// The C key: move the notch to the other slot.
    public void ToggleActiveSlot() => ActiveSlot = Other(_activeSlot);

    /// What the canvas does with a drag for this tool. Only Zoom and Pan act until W9.
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
