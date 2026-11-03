// The zoom steps, limits and labels, from mac/OpenPainterKit/Sources/ZoomPresets.swift,
// which ports paint.net 3.36.7's ScaleFactor.cs (MIT; see THIRD_PARTY.md).
using System.Globalization;

namespace OpenPainter.Canvas;

public static class ZoomPresets
{
    /// 1% to 3200%, ascending. 1.0 is always present.
    public static readonly double[] Values =
        [0.01, 0.02, 0.03, 0.04, 0.05, 0.06, 0.08, 0.12, 0.16, 0.25, 0.33, 0.50, 0.66, 1, 2, 3, 4, 5, 6, 7, 8, 12, 16, 24, 32];

    public const double Minimum = 0.01, Maximum = 32;

    public static double Clamp(double zoom) => Math.Min(Math.Max(zoom, Minimum), Maximum);

    /// paint.net keeps a scale as n/1000, rounded down.
    static double FromDouble(double value) => Clamp(Math.Floor(value * 1000) / 1000);

    /// The first preset at or above `zoom + 0.005`, or the largest.
    public static double NextLarger(double zoom)
    {
        var ratio = zoom + 0.005;
        var index = Array.FindIndex(Values, v => ratio <= v);
        return FromDouble(Values[index < 0 ? Values.Length - 1 : index]);
    }

    /// The preset before the first one at or above `zoom - 0.005`, or the smallest.
    public static double NextSmaller(double zoom)
    {
        var ratio = zoom - 0.005;
        var found = Array.FindIndex(Values, v => ratio <= v);
        return FromDouble(Values[Math.Max(found - 1, 0)]);
    }

    /// The largest scale that fits the document inside the view less 10 points on each axis, never above 100%.
    public static double Fit(double documentWidth, double documentHeight, double viewWidth, double viewHeight)
    {
        if (documentWidth <= 0 || documentHeight <= 0) return 1;
        var x = Math.Max(viewWidth - 10, 1) / documentWidth;
        var y = Math.Max(viewHeight - 10, 1) / documentHeight;
        return Clamp(Math.Min(Math.Min(x, y), 1));
    }

    /// The percent text: "33%", "100%", "1600%", with 1 decimal below 10%.
    public static string Label(double zoom)
    {
        var percent = zoom * 100;
        var whole = Math.Round(percent, MidpointRounding.AwayFromZero);
        if (percent < 10 && whole != percent) return percent.ToString("F1", CultureInfo.InvariantCulture) + "%";
        return ((long)whole).ToString(CultureInfo.InvariantCulture) + "%";
    }

    /// What the zoom field means: a percent with or without its sign. Null for anything else.
    public static double? ParseField(string? text)
    {
        var plain = (text ?? "").Replace("%", "").Trim();
        if (!double.TryParse(plain, NumberStyles.Float, CultureInfo.InvariantCulture, out var percent)) return null;
        if (!double.IsFinite(percent) || percent <= 0) return null;
        return Clamp(percent / 100);
    }

    /// The zoom a View > Zoom Presets item stands for, by its title.
    public static double? ForTitle(string title)
    {
        foreach (var value in Values)
            if (Label(value) == title) return value;
        return null;
    }
}
