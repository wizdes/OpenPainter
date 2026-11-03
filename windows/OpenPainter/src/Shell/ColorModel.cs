// Color math and data for the Colors panel and the Color Picker tool: ColorModel.swift, plus
// DocColor from ToolState.swift. HSV conversion, the wheel's modifier rules, paint.net palette
// files and the default palette. Plain classes with no window, so tests need none.
using System.Globalization;
using System.Text;

namespace OpenPainter.Shell;

/// RGBA bytes of a document color. The bytes never change with the theme.
public readonly record struct DocColor(byte R, byte G, byte B, byte A = 255)
{
    /// DocColor(hex:): "#RRGGBB", "RRGGBB" or "RRGGBBAA", with spaces around it allowed. Null
    /// for anything else.
    public static DocColor? FromHex(string hex)
    {
        var s = ColorText.TrimSpaces(hex);
        if (s.StartsWith('#')) s = s[1..];
        if (s.Length is not (6 or 8) || !s.All(char.IsAsciiHexDigit)) return null;
        var v = uint.Parse(s, NumberStyles.AllowHexSpecifier, CultureInfo.InvariantCulture);
        return s.Length == 6
            ? new DocColor((byte)(v >> 16), (byte)(v >> 8), (byte)v)
            : new DocColor((byte)(v >> 24), (byte)(v >> 16), (byte)(v >> 8), (byte)v);
    }

    /// "#CF684D". Alpha shows only when it is not 255.
    public string Hex => A == 255 ? $"#{R:X2}{G:X2}{B:X2}" : $"#{R:X2}{G:X2}{B:X2}{A:X2}";

    /// The engine's eight-digit RGBA form.
    public string EngineHex => $"{R:x2}{G:x2}{B:x2}{A:x2}";

    public Avalonia.Media.Color Exact => Avalonia.Media.Color.FromArgb(A, R, G, B);
}

public static class ColorText
{
    /// Swift's trimmingCharacters(in: .whitespaces): spaces and tabs, not line ends.
    public static string TrimSpaces(string text)
    {
        static bool Space(char c) => c == '\t' || char.GetUnicodeCategory(c) == UnicodeCategory.SpaceSeparator;
        var (from, to) = (0, text.Length);
        while (from < to && Space(text[from])) from++;
        while (to > from && Space(text[to - 1])) to--;
        return text[from..to];
    }

    /// ChannelRow.parse: a whole number in range. Anything else is refused.
    public static double? Channel(string text, double min, double max)
    {
        var t = TrimSpaces(text);
        if (t.Length == 0 || !t.All(char.IsAscii)) return null;
        if (!int.TryParse(t, NumberStyles.AllowLeadingSign, CultureInfo.InvariantCulture, out var v) || v < min || v > max) return null;
        return v;
    }

    /// OptionsBarView.number: a whole number, or one decimal.
    public static string Number(double v) =>
        Math.Round(v, MidpointRounding.AwayFromZero) == v ? ((long)v).ToString(CultureInfo.InvariantCulture) : v.ToString("F1", CultureInfo.InvariantCulture);

    /// Swift's Double(String): the whole text is a number, or null.
    public static double? Number(string text)
    {
        var t = ColorText.TrimSpaces(text);
        return double.TryParse(t, NumberStyles.Float, CultureInfo.InvariantCulture, out var v) && double.IsFinite(v) ? v : null;
    }
}

/// Hue 0 up to 360 degrees, saturation and value 0 to 100 (paint.net's HSV ranges).
public readonly record struct Hsv(double H, double S, double V)
{
    public static Hsv Of(DocColor c)
    {
        double r = c.R / 255.0, g = c.G / 255.0, b = c.B / 255.0;
        double max = Math.Max(r, Math.Max(g, b)), min = Math.Min(r, Math.Min(g, b)), d = max - min;
        var h = 0.0;
        if (d > 0)
        {
            if (max == r) h = 60 * ((g - b) / d % 6);
            else if (max == g) h = 60 * ((b - r) / d + 2);
            else h = 60 * ((r - g) / d + 4);
        }
        if (h < 0) h += 360;
        return new Hsv(h, max == 0 ? 0 : d / max * 100, max * 100);
    }

    /// The RGB bytes for this HSV, with the given alpha.
    public DocColor Color(byte alpha = 255)
    {
        double s = Math.Clamp(S, 0, 100) / 100, v = Math.Clamp(V, 0, 100) / 100;
        var h = H % 360;
        if (h < 0) h += 360;
        var c = v * s;
        var x = c * (1 - Math.Abs(h / 60 % 2 - 1));
        var m = v - c;
        var (r, g, b) = (int)(h / 60) switch
        {
            0 => (c, x, 0.0),
            1 => (x, c, 0.0),
            2 => (0.0, c, x),
            3 => (0.0, x, c),
            4 => (x, 0.0, c),
            _ => (c, 0.0, x),
        };
        byte Byte(double u) => (byte)Math.Clamp(Math.Round((u + m) * 255, MidpointRounding.AwayFromZero), 0, 255);
        return new DocColor(Byte(r), Byte(g), Byte(b), alpha);
    }
}

/// paint.net's wheel modifiers: Ctrl keeps the radius, Alt keeps the spoke, Shift snaps.
[Flags]
public enum WheelModifiers { None = 0, KeepRadius = 1, KeepSpoke = 2, SnapSpokes = 4 }

/// The wheel's point math. Hue is the angle (0 degrees at the right, counterclockwise, red at
/// 0), saturation the distance from the center (0 at the center, 100 at the rim).
public static class ColorWheelMath
{
    /// The spacing of the Shift spokes.
    public const double SpokeStep = 15;

    /// Hue and saturation for a point relative to the center in units of the radius, y up.
    public static (double H, double S) Pick(double dx, double dy, Hsv current, WheelModifiers mods)
    {
        var angle = Math.Atan2(dy, dx) * 180 / Math.PI;
        if (angle < 0) angle += 360;
        var dist = Math.Min(Math.Sqrt(dx * dx + dy * dy), 1) * 100;
        if (mods.HasFlag(WheelModifiers.KeepSpoke))
        {
            // Project the point onto the current spoke. The far side is 0.
            var a = current.H * Math.PI / 180;
            var along = (dx * Math.Cos(a) + dy * Math.Sin(a)) * 100;
            return (current.H, Math.Clamp(along, 0, 100));
        }
        var h = angle;
        if (mods.HasFlag(WheelModifiers.SnapSpokes))
        {
            h = Math.Round(angle / SpokeStep, MidpointRounding.AwayFromZero) * SpokeStep;
            if (h >= 360) h -= 360;
        }
        return (h, mods.HasFlag(WheelModifiers.KeepRadius) ? current.S : dist);
    }

    /// The marker's place for a color, relative to the center in radius units, y up.
    public static (double X, double Y) Point(Hsv hsv)
    {
        double a = hsv.H * Math.PI / 180, r = Math.Clamp(hsv.S, 0, 100) / 100;
        return (Math.Cos(a) * r, Math.Sin(a) * r);
    }
}

/// A line the palette parser skipped, for the report.
public sealed record PaletteIssue(int Line, string Text, string Reason);

public sealed record PaletteParse(ColorPalette Palette, int ColorCount, IReadOnlyList<PaletteIssue> Issues)
{
    /// One line for the status bar.
    public string Summary
    {
        get
        {
            var parts = new List<string> { $"Loaded {ColorCount} of {ColorPalette.Size} colors" };
            var bad = Issues.Where(i => i.Reason != ColorPalette.OverflowReason).ToList();
            if (bad.Count > 0)
            {
                var lines = string.Join(", ", bad.Take(5).Select(i => i.Line.ToString(CultureInfo.InvariantCulture)));
                parts.Add($"skipped {bad.Count} malformed line{(bad.Count == 1 ? "" : "s")} ({lines}{(bad.Count > 5 ? ", ..." : "")})");
            }
            var extra = Issues.Where(i => i.Reason == ColorPalette.OverflowReason).ToList();
            if (extra.Count > 0)
                parts.Add($"ignored {extra.Count} color{(extra.Count == 1 ? "" : "s")} after slot {ColorPalette.Size} (from line {extra[0].Line})");
            return string.Join("; ", parts) + ".";
        }
    }
}

/// A 96-color palette: Palette in ColorModel.swift. Loads and saves paint.net palette text
/// files: one `aarrggbb` hex color per line, `;` starts a comment, at most 96 colors, and
/// missing slots are white.
public sealed class ColorPalette : IEquatable<ColorPalette>
{
    public const int Size = 96, CompactCount = 32;
    public const string OverflowReason = "more than 96 colors";
    public static readonly DocColor Padding = new(255, 255, 255);

    public IReadOnlyList<DocColor> Colors { get; }

    public ColorPalette(IEnumerable<DocColor> colors)
    {
        var c = colors.Take(Size).ToList();
        while (c.Count < Size) c.Add(Padding);
        Colors = c;
    }

    /// A copy with one slot changed.
    public ColorPalette With(int index, DocColor color)
    {
        var c = Colors.ToList();
        c[index] = color;
        return new ColorPalette(c);
    }

    public bool Equals(ColorPalette? other) => other is not null && Colors.SequenceEqual(other.Colors);
    public override bool Equals(object? obj) => Equals(obj as ColorPalette);
    public override int GetHashCode() => Colors.Aggregate(0, HashCode.Combine);

    static bool IsLineEnd(char c) => c is (char)0x0A or (char)0x0B or (char)0x0C or (char)0x0D or (char)0x85 or (char)0x2028 or (char)0x2029;

    /// Swift splits lines on every newline character, and "\r\n" counts as one.
    static IEnumerable<string> Lines(string text)
    {
        var start = 0;
        for (var i = 0; i < text.Length; i++)
        {
            if (!IsLineEnd(text[i])) continue;
            yield return text[start..i];
            if (text[i] == '\r' && i + 1 < text.Length && text[i + 1] == '\n') i++;
            start = i + 1;
        }
        yield return text[start..];
    }

    /// Comment and blank lines are skipped quietly. Malformed lines and colors past slot 96 are
    /// skipped and reported.
    public static PaletteParse Parse(string text)
    {
        var colors = new List<DocColor>();
        var issues = new List<PaletteIssue>();
        var number = 0;
        foreach (var raw in Lines(text))
        {
            number++;
            var semi = raw.IndexOf(';');
            var t = ColorText.TrimSpaces(semi >= 0 ? raw[..semi] : raw);
            if (t.Length == 0) continue;
            if (Argb(t) is not { } c)
            {
                issues.Add(new PaletteIssue(number, raw, "not an 8-digit aarrggbb hex color"));
                continue;
            }
            if (colors.Count >= Size)
            {
                issues.Add(new PaletteIssue(number, raw, OverflowReason));
                continue;
            }
            colors.Add(c);
        }
        return new PaletteParse(new ColorPalette(colors), colors.Count, issues);
    }

    /// Exactly eight hex digits as `aarrggbb`.
    public static DocColor? Argb(string s)
    {
        if (s.Length != 8 || !s.All(char.IsAsciiHexDigit)) return null;
        var v = uint.Parse(s, NumberStyles.AllowHexSpecifier, CultureInfo.InvariantCulture);
        return new DocColor((byte)(v >> 16), (byte)(v >> 8), (byte)v, (byte)(v >> 24));
    }

    public static string ArgbString(DocColor c) => $"{c.A:X2}{c.R:X2}{c.G:X2}{c.B:X2}";

    /// The file text: a short comment header and 96 `AARRGGBB` lines.
    public string FileText
    {
        get
        {
            var out_ = new StringBuilder();
            out_.Append("; OpenPainter palette (paint.net palette format)\n");
            out_.Append("; One color per line as 8 hex digits: aarrggbb. aa is alpha: FF is opaque, 00 is clear.\n");
            out_.Append("; Lines that start with a semicolon are comments. Up to 96 colors; missing slots are white.\n");
            foreach (var c in Colors) out_.Append(ArgbString(c)).Append('\n');
            return out_.ToString();
        }
    }

    /// OpenPainter's default palette: 96 colors in 12 rows of 8. Greys, then 24 strong hues,
    /// the same darker and lighter, then 8 very dark and 8 very light.
    public static readonly ColorPalette Standard = MakeStandard();

    static ColorPalette MakeStandard()
    {
        var c = new List<DocColor>();
        for (var i = 0; i < 8; i++)
        {
            var v = (byte)Math.Round(i * 255.0 / 7, MidpointRounding.AwayFromZero);
            c.Add(new DocColor(v, v, v));
        }
        for (var i = 0; i < 24; i++) c.Add(new Hsv(i * 15, 100, 100).Color());
        for (var i = 0; i < 24; i++) c.Add(new Hsv(i * 15, 100, 60).Color());
        for (var i = 0; i < 24; i++) c.Add(new Hsv(i * 15, 40, 100).Color());
        for (var i = 0; i < 8; i++) c.Add(new Hsv(i * 45, 100, 35).Color());
        for (var i = 0; i < 8; i++) c.Add(new Hsv(i * 45, 15, 100).Color());
        return new ColorPalette(c);
    }
}

/// Which color slot is active, and has the notch. Palette and wheel clicks go to the active
/// slot, and right-clicks go to the other one.
public enum ColorSlot { Primary, Secondary }
