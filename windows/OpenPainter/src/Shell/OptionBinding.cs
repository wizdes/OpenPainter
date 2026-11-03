// How each control in the options row reads and writes tool state, by the control's name: the
// Mac's OptionsBarView.sync and fieldChanged, SelectCropController.fieldChanged and
// TextOptionsControls. Plain functions, so tests need no window.
using System.Globalization;

namespace OpenPainter.Shell;

public static class OptionBinding
{
    public const string BrushWidth = "Brush width", Hardness = "Hardness", Antialiasing = "Antialiasing", BlendMode = "Blend mode", Opacity = "Opacity";
    public const string PenSize = "Pen pressure changes the brush width", PenOpacity = "Pen pressure changes the opacity";
    public const string Tolerance = "Tolerance", ToleranceAlpha = "Tolerance alpha", SelectionMode = "Selection mode", Sampling = "Sampling", FloodMode = "Flood mode";
    public const string SampleSize = "Sample size", AfterClick = "After click", Resampling = "Resampling";
    public const string Style = "Selection style", RatioWidth = "Ratio width", RatioHeight = "Ratio height", SizeWidth = "Width in pixels", SizeHeight = "Height in pixels";
    public const string Font = "Font", FontSize = "Font size", Bold = "Bold", Italic = "Italic", Underline = "Underline", Strikethrough = "Strikethrough";
    public const string AlignLeft = "Align Left", AlignCenter = "Align Center", AlignRight = "Align Right";

    /// The text a field shows. An unknown name is a bug in the row's table.
    public static string FieldText(ToolSettings s, string name) => ColorText.Number(name switch
    {
        BrushWidth => s.BrushWidth,
        Hardness => s.Hardness,
        Opacity => s.Opacity,
        Tolerance => s.Tolerance,
        RatioWidth => s.RatioWidth,
        RatioHeight => s.RatioHeight,
        SizeWidth => s.SizeWidth,
        SizeHeight => s.SizeHeight,
        FontSize => s.Text.Size,
        _ => throw new ArgumentException($"OPTIONS: no field named {name}"),
    });

    /// What a typed number does. Out of range clamps, and bad input leaves the state as it was,
    /// so the field shows the old value again.
    public static ToolSettings WithField(ToolSettings s, string name, string text)
    {
        if (ColorText.Number(text) is not { } v) return s;
        return name switch
        {
            BrushWidth => s with { BrushWidth = Math.Clamp(v, ToolSettings.LeastWidth, ToolSettings.MostWidth) },
            Hardness => s with { Hardness = Math.Clamp(v, 0, 100) },
            Opacity => s with { Opacity = Math.Clamp(v, 0, 100) },
            Tolerance => s with { Tolerance = Math.Clamp(v, 0, 100) },
            // SelectCropController.fieldChanged: a number above 0. Sizes are whole pixels.
            RatioWidth => v > 0 ? s with { RatioWidth = v } : s,
            RatioHeight => v > 0 ? s with { RatioHeight = v } : s,
            SizeWidth => v > 0 ? s with { SizeWidth = Math.Round(v, MidpointRounding.AwayFromZero) } : s,
            SizeHeight => v > 0 ? s with { SizeHeight = Math.Round(v, MidpointRounding.AwayFromZero) } : s,
            FontSize => s with { Text = s.Text with { Size = Math.Clamp(v, TextToolSettings.LeastSize, TextToolSettings.MostSize) } },
            _ => throw new ArgumentException($"OPTIONS: no field named {name}"),
        };
    }

    /// The Color Picker's Sampling is its own setting, as on the Mac.
    public static int Choice(ToolSettings s, string tool, string name) => name switch
    {
        BlendMode => (int)s.Blend,
        ToleranceAlpha => (int)s.ToleranceAlpha,
        SelectionMode => (int)s.SelectionMode,
        Sampling => (int)(tool == "colorPicker" ? s.PickerSampling : s.Sampling),
        FloodMode => (int)s.FloodMode,
        SampleSize => (int)s.PickerSize,
        AfterClick => (int)s.PickerAfterClick,
        Resampling => (int)s.MoveResampling,
        Style => (int)s.SelectStyle,
        _ => throw new ArgumentException($"OPTIONS: no choice named {name}"),
    };

    public static ToolSettings WithChoice(ToolSettings s, string tool, string name, int index) => name switch
    {
        BlendMode => s with { Blend = (BlendChoice)index },
        ToleranceAlpha => s with { ToleranceAlpha = (ToleranceAlphaChoice)index },
        SelectionMode => s with { SelectionMode = (SelectionModeChoice)index },
        Sampling => tool == "colorPicker" ? s with { PickerSampling = (SamplingChoice)index } : s with { Sampling = (SamplingChoice)index },
        FloodMode => s with { FloodMode = (FloodModeChoice)index },
        SampleSize => s with { PickerSize = (PickerSizeChoice)index },
        AfterClick => s with { PickerAfterClick = (PickerAfterClickChoice)index },
        Resampling => s with { MoveResampling = (MoveResamplingChoice)index },
        Style => s with { SelectStyle = (SelectStyleChoice)index },
        _ => throw new ArgumentException($"OPTIONS: no choice named {name}"),
    };

    public static bool Check(ToolSettings s, string name) => name switch
    {
        Antialiasing => s.Antialias,
        PenSize => s.PressureSize,
        PenOpacity => s.PressureOpacity,
        _ => throw new ArgumentException($"OPTIONS: no check box named {name}"),
    };

    public static ToolSettings WithCheck(ToolSettings s, string name, bool on) => name switch
    {
        Antialiasing => s with { Antialias = on },
        PenSize => s with { PressureSize = on },
        PenOpacity => s with { PressureOpacity = on },
        _ => throw new ArgumentException($"OPTIONS: no check box named {name}"),
    };

    public static bool Toggle(ToolSettings s, string name) => name switch
    {
        Bold => s.Text.Bold,
        Italic => s.Text.Italic,
        Underline => s.Text.Underline,
        Strikethrough => s.Text.Strikethrough,
        AlignLeft => s.Text.Alignment == TextAlignChoice.Left,
        AlignCenter => s.Text.Alignment == TextAlignChoice.Center,
        AlignRight => s.Text.Alignment == TextAlignChoice.Right,
        _ => throw new ArgumentException($"OPTIONS: no toggle named {name}"),
    };

    /// A click. The style buttons flip, and the alignment buttons pick one.
    public static ToolSettings Clicked(ToolSettings s, string name)
    {
        var t = s.Text;
        return s with
        {
            Text = name switch
            {
                Bold => t with { Bold = !t.Bold },
                Italic => t with { Italic = !t.Italic },
                Underline => t with { Underline = !t.Underline },
                Strikethrough => t with { Strikethrough = !t.Strikethrough },
                AlignLeft => t with { Alignment = TextAlignChoice.Left },
                AlignCenter => t with { Alignment = TextAlignChoice.Center },
                AlignRight => t with { Alignment = TextAlignChoice.Right },
                _ => throw new ArgumentException($"OPTIONS: no toggle named {name}"),
            },
        };
    }
}

/// The fonts Windows has, as Skia's font manager lists them: TextLayoutEngine.availableFamilies.
/// They come once, the first time the Text options show. Fonts installed while the app runs
/// show after a restart.
public static class FontFamilies
{
    static IReadOnlyList<string>? _installed;

    public static IReadOnlyList<string> Installed => _installed ??= Avalonia.Media.FontManager.Current.SystemFonts
        .Select(f => f.Name)
        .Where(n => !n.StartsWith('.') && !n.StartsWith('@'))
        .Distinct(StringComparer.OrdinalIgnoreCase)
        .OrderBy(n => n, StringComparer.Create(CultureInfo.InvariantCulture, ignoreCase: true))
        .ToList();

    /// The list a tool shows. A family that is not installed still shows, so the user sees what
    /// was asked for.
    public static IReadOnlyList<string> ForChoice(string family) =>
        Installed.Contains(family, StringComparer.OrdinalIgnoreCase) ? Installed : [.. Installed, family];
}
