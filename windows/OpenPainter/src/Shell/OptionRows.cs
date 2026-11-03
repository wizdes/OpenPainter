// What each tool's options row holds, as plain data, in the Mac's order and words:
// OptionsBarView.rebuild, TextOptionsControls.add, and the rows SelectCropTool.swift and
// MoveTool.swift give it. A test reads those files, so the words cannot drift.
// Widths come from W6's pictures. Each group stays whole when the row runs out of room.
namespace OpenPainter.Shell;

public abstract record OptionItem;
public sealed record OptionLabel(string Text) : OptionItem;
public sealed record OptionField(string Name, string Text) : OptionItem;
public sealed record OptionChoice(string Name, IReadOnlyList<string> Items, double Width, int Selected = 0) : OptionItem;
public sealed record OptionCheck(string Name, string Text, bool On) : OptionItem;
public sealed record OptionStep(string Name, string Icon) : OptionItem;
public sealed record OptionToggle(string Name, string Icon, bool On = false) : OptionItem;
public sealed record OptionSlide(string Name, double Min, double Max, double Value) : OptionItem;
public sealed record OptionPush(string Name, string Text, double Width) : OptionItem;

public static class OptionRows
{
    const double FieldWidth = 52, SlideWidth = 96;   // OptionsBarView's fields and tolerance slider.
    public const double PickerWidth = 170;           // OptionsBarView.setUpPicker.

    public static readonly IReadOnlyList<string> SelectionModes = ["Replace", "Add", "Subtract", "Intersect", "Invert"];
    static readonly IReadOnlyList<string> Blends = ["Normal", "Overwrite"];
    static readonly IReadOnlyList<string> Samplings = ["Layer", "Image"];

    static IReadOnlyList<OptionItem> Group(params OptionItem[] items) => items;

    static IReadOnlyList<OptionItem> Width() => Group(new OptionLabel("Brush width"),
        new OptionStep("Decrease brush width", "opt-minus"), new OptionField("Brush width", "2"),
        new OptionStep("Increase brush width", "opt-plus"), new OptionLabel("px"));

    static IReadOnlyList<OptionItem> Hardness() => Group(new OptionLabel("Hardness"), new OptionField("Hardness", "100"), new OptionLabel("%"));
    static IReadOnlyList<OptionItem> Antialias() => Group(new OptionLabel("Antialiasing"), new OptionCheck("Antialiasing", "On", true));
    static IReadOnlyList<OptionItem> Blend() => Group(new OptionLabel("Blend mode"), new OptionChoice("Blend mode", Blends, 102));
    static IReadOnlyList<OptionItem> Opacity() => Group(new OptionLabel("Opacity"), new OptionField("Opacity", "100"), new OptionLabel("%"));

    /// The groups after the tool picker, for a tool id and the catalog's name for its options.
    public static IReadOnlyList<IReadOnlyList<OptionItem>> For(string tool, string options)
    {
        switch (tool)
        {
            case "rectangleSelect" or "ellipseSelect":
                return
                [
                    Group(new OptionLabel("Selection mode"), new OptionChoice("Selection mode", SelectionModes, 97)),
                    Group(new OptionLabel("Style"), new OptionChoice("Style", ["Normal", "Fixed Ratio", "Fixed Size"], 110)),
                ];
            case "lassoSelect":
                return [Group(new OptionLabel("Selection mode"), new OptionChoice("Selection mode", SelectionModes, 97))];
            case "moveSelectedPixels" or "moveSelection":
                return
                [
                    Group(new OptionLabel("Resampling"), new OptionChoice("Resampling", ["Nearest Neighbor", "Bilinear", "Bicubic", "Supersampling", "Best Quality"], 140, 2)),
                    Group(new OptionPush("Cancel", "Cancel", 64), new OptionPush("Finish", "Finish", 64)),
                ];
        }
        return options switch
        {
            "none" => [Group(new OptionLabel("This tool has no options."))],
            "brush" => [Width(), Hardness(), Antialias(), Blend(), Opacity(),
                Group(new OptionLabel("Pen pressure"), new OptionCheck("Pen pressure changes the brush width", "Size", false), new OptionCheck("Pen pressure changes the opacity", "Opacity", false))],
            "eraser" => [Width(), Hardness(), Antialias(), Opacity(),
                Group(new OptionLabel("Pen pressure"), new OptionCheck("Pen pressure changes the brush width", "Size", false), new OptionCheck("Pen pressure changes the opacity", "Opacity", false))],
            "pencil" => [Blend(), Opacity(), Group(new OptionLabel("Pen pressure"), new OptionCheck("Pen pressure changes the opacity", "Opacity", false))],
            "flood" =>
            [
                Group(new OptionLabel("Tolerance"), new OptionSlide("Tolerance", 0, 100, 50), new OptionField("Tolerance", "50"), new OptionLabel("%")),
                Group(new OptionLabel("Tolerance alpha"), new OptionChoice("Tolerance alpha", ["Premultiplied", "Straight"], 124)),
                tool == ToolState.MagicWand
                    ? Group(new OptionLabel("Selection mode"), new OptionChoice("Selection mode", SelectionModes, 97))
                    : Blend(),
                Group(new OptionLabel("Sampling"), new OptionChoice("Sampling", Samplings, 90)),
                Group(new OptionLabel("Flood mode"), new OptionChoice("Flood mode", ["Contiguous", "Global"], 110)),
                Antialias(),
                Group(new OptionPush("Finish fill or selection", "Finish", 64)),
            ],
            "picker" =>
            [
                Group(new OptionLabel("Sampling"), new OptionChoice("Sample size", ["Single Pixel", "3 x 3 Region", "5 x 5 Region", "7 x 7 Region", "9 x 9 Region", "11 x 11 Region", "31 x 31 Region", "51 x 51 Region"], 130)),
                Group(new OptionLabel("Layer or image"), new OptionChoice("Sampling", Samplings, 90)),
                Group(new OptionLabel("After click"), new OptionChoice("After click", ["Do not switch tool", "Switch to previous tool", "Switch to Pencil tool"], 170)),
            ],
            "text" =>
            [
                // Avenir Next and Helvetica Neue are not on Windows. W6 shows Segoe UI, and W8 lists the fonts.
                Group(new OptionLabel("Font"), new OptionChoice("Font", ["Segoe UI"], 170), new OptionChoice("Font size", ["8", "9", "10", "11", "12", "14", "16", "18", "20", "22", "24", "26", "28", "36", "48", "72", "84", "96", "108", "144", "192", "216", "288"], 64, 4), new OptionLabel("pt")),
                Group(new OptionToggle("Bold", "text-bold"), new OptionToggle("Italic", "text-italic"), new OptionToggle("Underline", "text-underline"), new OptionToggle("Strikethrough", "text-strikethrough")),
                Group(new OptionToggle("Align Left", "text-align-left", true), new OptionToggle("Align Center", "text-align-center"), new OptionToggle("Align Right", "text-align-right")),
                Antialias(),
                Blend(),
                Group(new OptionPush("Finish text", "Finish", 64)),
            ],
            _ => throw new InvalidDataException($"CATALOG: the tool {tool} has unknown options `{options}`"),
        };
    }

    public static IEnumerable<string> Words(IEnumerable<IReadOnlyList<OptionItem>> groups) => groups.SelectMany(g => g).Select(item => item switch
    {
        OptionLabel l => l.Text,
        OptionField f => f.Name,
        OptionChoice c => c.Name,
        OptionCheck c => c.Text,
        OptionStep s => s.Name,
        OptionToggle t => t.Name,
        OptionSlide s => s.Name,
        OptionPush p => p.Text,
        _ => throw new InvalidOperationException($"OPTIONS: unknown row item {item}"),
    });
}

/// Where each group of the row sits, and what that means when the row runs out of room.
/// Plain math, so tests need no window.
public static class OptionsFit
{
    /// True when the row's content does not fit in the room the row has.
    public static bool Overflows(double contentWidth, double room) => contentWidth > room + 0.5;

    /// The groups that are not whole in the visible part. These are what "more" opens.
    public static IReadOnlyList<int> Hidden(IReadOnlyList<(double Left, double Right)> groups, double offset, double viewport)
    {
        var hidden = new List<int>();
        for (var i = 0; i < groups.Count; i++)
            if (groups[i].Left < offset - 0.5 || groups[i].Right > offset + viewport + 0.5) hidden.Add(i);
        return hidden;
    }

    /// How much to cover at each end of the visible part, so no cut word or control shows.
    public static (double Left, double Right) Covers(IReadOnlyList<(double Left, double Right)> groups, double offset, double viewport)
    {
        double left = 0, right = 0;
        var end = offset + viewport;
        foreach (var (l, r) in groups)
        {
            if (l < offset - 0.5 && r > offset) left = Math.Max(left, r - offset);
            if (r > end + 0.5 && l < end) right = Math.Max(right, end - l);
        }
        return (Math.Min(left, viewport), Math.Min(right, viewport));
    }
}
