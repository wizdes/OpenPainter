// What each tool's options row holds, as plain data, in the Mac's order and words:
// OptionsBarView.rebuild, TextOptionsControls.add, and the rows SelectCropTool.swift and
// MoveTool.swift give it. A test reads those files, so the words cannot drift.
// Widths come from W6's pictures. Each group stays whole when the row runs out of room.
namespace OpenPainter.Shell;

public abstract record OptionItem;
public sealed record OptionLabel(string Text) : OptionItem;
public sealed record OptionField(string Name, string Text, double Width = 52) : OptionItem;
public sealed record OptionChoice(string Name, IReadOnlyList<string> Items, double Width, int Selected = 0) : OptionItem;
public sealed record OptionCheck(string Name, string Text, bool On) : OptionItem;
public sealed record OptionStep(string Name, string Icon) : OptionItem;
public sealed record OptionToggle(string Name, string Icon, bool On = false) : OptionItem;
public sealed record OptionSlide(string Name, double Min, double Max, double Value) : OptionItem;
public sealed record OptionPush(string Name, string Text, double Width) : OptionItem;

public static class OptionRows
{
    const double RatioFieldWidth = 44;               // SelectCropController.ratioWidth.
    public const double PickerWidth = 170;           // OptionsBarView.setUpPicker.

    public static readonly IReadOnlyList<string> SelectionModes = ["Replace", "Add", "Subtract", "Intersect", "Invert"];
    public static readonly IReadOnlyList<string> SelectStyles = ["Normal", "Fixed Ratio", "Fixed Size"];
    static readonly IReadOnlyList<string> Blends = ["Normal", "Overwrite"];
    static readonly IReadOnlyList<string> Samplings = ["Layer", "Image"];

    static IReadOnlyList<OptionItem> Group(params OptionItem[] items) => items;

    /// The groups after the tool picker, for a tool id and the catalog's name for its options,
    /// showing these settings. Ratio and Size join when the Style asks for them.
    public static IReadOnlyList<IReadOnlyList<OptionItem>> For(string tool, string options, ToolSettings? settings = null, IReadOnlyList<string>? fonts = null)
    {
        var s = settings ?? new ToolSettings();
        OptionField Field(string name, double width = 52) => new(name, OptionBinding.FieldText(s, name), width);
        OptionChoice Choice(string name, IReadOnlyList<string> items, double width) => new(name, items, width, OptionBinding.Choice(s, tool, name));
        OptionCheck Check(string name, string text) => new(name, text, OptionBinding.Check(s, name));
        OptionToggle Toggle(string name, string icon) => new(name, icon, OptionBinding.Toggle(s, name));
        IReadOnlyList<OptionItem> Width() => Group(new OptionLabel("Brush width"), new OptionStep("Decrease brush width", "opt-minus"), Field(OptionBinding.BrushWidth), new OptionStep("Increase brush width", "opt-plus"), new OptionLabel("px"));
        IReadOnlyList<OptionItem> Hardness() => Group(new OptionLabel("Hardness"), Field(OptionBinding.Hardness), new OptionLabel("%"));
        IReadOnlyList<OptionItem> Antialias() => Group(new OptionLabel("Antialiasing"), Check(OptionBinding.Antialiasing, "On"));
        IReadOnlyList<OptionItem> Blend() => Group(new OptionLabel("Blend mode"), Choice(OptionBinding.BlendMode, Blends, 102));
        IReadOnlyList<OptionItem> Opacity() => Group(new OptionLabel("Opacity"), Field(OptionBinding.Opacity), new OptionLabel("%"));
        IReadOnlyList<OptionItem> Pen(bool size) => size
            ? Group(new OptionLabel("Pen pressure"), Check(OptionBinding.PenSize, "Size"), Check(OptionBinding.PenOpacity, "Opacity"))
            : Group(new OptionLabel("Pen pressure"), Check(OptionBinding.PenOpacity, "Opacity"));
        IReadOnlyList<OptionItem> Mode() => Group(new OptionLabel("Selection mode"), Choice(OptionBinding.SelectionMode, SelectionModes, 97));

        switch (tool)
        {
            case "rectangleSelect" or "ellipseSelect":
                var select = new List<IReadOnlyList<OptionItem>> { Mode(), Group(new OptionLabel("Style"), Choice(OptionBinding.Style, SelectStyles, 110)) };
                if (StyleGroup(s) is { } extra) select.Add(extra);
                return select;
            case "lassoSelect":
                return [Mode()];
            case "moveSelectedPixels" or "moveSelection":
                return
                [
                    Group(new OptionLabel("Resampling"), Choice(OptionBinding.Resampling, ["Nearest Neighbor", "Bilinear", "Bicubic", "Supersampling", "Best Quality"], 140)),
                    Group(new OptionPush("Cancel move (Escape)", "Cancel", 64), new OptionPush("Finish move (Enter)", "Finish", 64)),
                ];
        }
        return options switch
        {
            "none" => [Group(new OptionLabel("This tool has no options."))],
            "brush" => [Width(), Hardness(), Antialias(), Blend(), Opacity(), Pen(size: true)],
            "eraser" => [Width(), Hardness(), Antialias(), Opacity(), Pen(size: true)],
            "pencil" => [Blend(), Opacity(), Pen(size: false)],
            "flood" =>
            [
                Group(new OptionLabel("Tolerance"), new OptionSlide(OptionBinding.Tolerance, 0, 100, s.Tolerance), Field(OptionBinding.Tolerance), new OptionLabel("%")),
                Group(new OptionLabel("Tolerance alpha"), Choice(OptionBinding.ToleranceAlpha, ["Premultiplied", "Straight"], 124)),
                tool == ToolState.MagicWand ? Mode() : Blend(),
                Group(new OptionLabel("Sampling"), Choice(OptionBinding.Sampling, Samplings, 90)),
                Group(new OptionLabel("Flood mode"), Choice(OptionBinding.FloodMode, ["Contiguous", "Global"], 110)),
                Antialias(),
                Group(new OptionPush("Finish fill or selection", "Finish", 64)),
            ],
            "picker" =>
            [
                Group(new OptionLabel("Sampling"), Choice(OptionBinding.SampleSize, ["Single Pixel", "3 x 3 Region", "5 x 5 Region", "7 x 7 Region", "9 x 9 Region", "11 x 11 Region", "31 x 31 Region", "51 x 51 Region"], 130)),
                Group(new OptionLabel("Layer or image"), Choice(OptionBinding.Sampling, Samplings, 90)),
                Group(new OptionLabel("After click"), Choice(OptionBinding.AfterClick, ["Do not switch tool", "Switch to previous tool", "Switch to Pencil tool"], 170)),
            ],
            "text" =>
            [
                // Avenir Next and Helvetica Neue are not on Windows. The list is what Windows has.
                Group(new OptionLabel("Font"), new OptionChoice(OptionBinding.Font, fonts ?? [s.Text.Family], 170, fonts is null ? 0 : FontIndex(fonts, s.Text.Family)),
                    new OptionChoice(OptionBinding.FontSize, TextToolSettings.Sizes.Select(ColorText.Number).ToList(), 64, Array.IndexOf(TextToolSettings.Sizes, s.Text.Size)), new OptionLabel("pt")),
                Group(Toggle(OptionBinding.Bold, "text-bold"), Toggle(OptionBinding.Italic, "text-italic"), Toggle(OptionBinding.Underline, "text-underline"), Toggle(OptionBinding.Strikethrough, "text-strikethrough")),
                Group(Toggle(OptionBinding.AlignLeft, "text-align-left"), Toggle(OptionBinding.AlignCenter, "text-align-center"), Toggle(OptionBinding.AlignRight, "text-align-right")),
                Antialias(),
                Blend(),
                Group(new OptionPush("Finish text", "Finish", 64)),
            ],
            _ => throw new InvalidDataException($"CATALOG: the tool {tool} has unknown options `{options}`"),
        };
    }

    public static int FontIndex(IReadOnlyList<string> fonts, string family)
    {
        for (var i = 0; i < fonts.Count; i++)
            if (string.Equals(fonts[i], family, StringComparison.OrdinalIgnoreCase)) return i;
        throw new InvalidOperationException($"OPTIONS: {family} is not in the font list");
    }

    /// SelectCropController.ratioViews and sizeViews: the group the Style shows, if any.
    public static IReadOnlyList<OptionItem>? StyleGroup(ToolSettings s) => s.SelectStyle switch
    {
        SelectStyleChoice.FixedRatio => Group(new OptionLabel("Ratio"), new OptionField(OptionBinding.RatioWidth, OptionBinding.FieldText(s, OptionBinding.RatioWidth), RatioFieldWidth), new OptionLabel(":"), new OptionField(OptionBinding.RatioHeight, OptionBinding.FieldText(s, OptionBinding.RatioHeight), RatioFieldWidth)),
        SelectStyleChoice.FixedSize => Group(new OptionLabel("Size"), new OptionField(OptionBinding.SizeWidth, OptionBinding.FieldText(s, OptionBinding.SizeWidth)), new OptionLabel("×"), new OptionField(OptionBinding.SizeHeight, OptionBinding.FieldText(s, OptionBinding.SizeHeight)), new OptionLabel("px")),
        _ => null,
    };

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
