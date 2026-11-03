// Test values read out of the Swift tests, for the Windows tests to check against. The files
// in windows/tests/vectors are the record: a test re-reads the Swift source each run and
// fails, by the vector's name, when a file and its source disagree.
using System.Globalization;
using System.Text;
using System.Text.Json;
using System.Text.RegularExpressions;

namespace OpenPainter.Harness;

public sealed record Vector(string Name, string Op, string[] In, string Out);

public sealed record VectorFile(string FileName, string Source, string[] SourceFunctions, Vector[] Vectors);

public static partial class Vectors
{
    public const string Folder = "windows/tests/vectors";
    const string ZoomTests = "mac/Tests/OpenPainterKitTests/ZoomTests.swift";
    const string ColorTests = "mac/Tests/OpenPainterKitTests/ColorsTests.swift";
    const string MenuSkeleton = "mac/Tests/OpenPainterKitTests/menu-skeleton.txt";

    static string Number(string swift)
    {
        var text = swift.Replace("_", "");
        var value = text.StartsWith("0x", StringComparison.Ordinal) ? int.Parse(text[2..], NumberStyles.HexNumber, CultureInfo.InvariantCulture) : double.Parse(text, CultureInfo.InvariantCulture);
        return value.ToString("R", CultureInfo.InvariantCulture);
    }

    /// `min(950.0 / 6000, 590.0 / 4000)` as a number, or a plain number.
    static string Expression(string swift)
    {
        var min = MinOfQuotients().Match(swift);
        if (!min.Success) return Number(swift);
        double Q(int group) => double.Parse(min.Groups[group].Value, CultureInfo.InvariantCulture);
        return Math.Min(Q(1) / Q(2), Q(3) / Q(4)).ToString("R", CultureInfo.InvariantCulture);
    }

    [GeneratedRegex(@"^min\(([\d.]+) / ([\d.]+), ([\d.]+) / ([\d.]+)\)$")] private static partial Regex MinOfQuotients();
    [GeneratedRegex(@"let presets: \[Double\] = \[([^\]]+)\]")] private static partial Regex PresetList();
    [GeneratedRegex(@"XCTAssertEqual\(ZoomPresets\.(nextLarger|nextSmaller|clamp)\(([\d.]+)\), ([\d.]+)(?:, ""[^""]*"")?\)")] private static partial Regex ZoomStep();
    [GeneratedRegex(@"XCTAssertEqual\(ZoomPresets\.label\(([\d.]+)\), ""([^""]+)""\)")] private static partial Regex ZoomLabel();
    [GeneratedRegex(@"ZoomPresets\.fit\(document: CGSize\(width: ([\d_]+), height: ([\d_]+)\), view: CGSize\(width: ([\d_]+), height: ([\d_]+)\)\)(?:, ([\d.]+)\)|\s*\n\s*XCTAssertEqual\(fit, (min\([^)]*\)), accuracy)")] private static partial Regex ZoomFit();
    [GeneratedRegex(@"^(.*) \[([^\]]+)\]( \(off\))?$")] private static partial Regex SkeletonLine();
    [GeneratedRegex(@"XCTAssertEqual\(DocColor\(hex: ""([^""]*)""\), DocColor\(r: (\w+), g: (\w+), b: (\w+)(?:, a: (\w+))?\)\)")] private static partial Regex HexGood();
    [GeneratedRegex(@"for bad in \[((?:""[^""]*"",?\s*)+)\]")] private static partial Regex HexBadList();
    [GeneratedRegex(@"""([^""]*)""")] private static partial Regex Quoted();
    [GeneratedRegex(@"XCTAssertEqual\(DocColor\(r: (\w+), g: (\w+), b: (\w+)(?:, a: (\w+))?\)\.hex, ""([^""]+)""\)")] private static partial Regex HexText();
    [GeneratedRegex(@"XCTAssertEqual\(ChannelRow\.parse\(""([^""]*)"", range: (\d+)\.\.\.(\d+)\), (\d+)\)")] private static partial Regex ChannelGood();
    [GeneratedRegex(@"XCTAssertNil\(ChannelRow\.parse\(""([^""]*)"", range: (\d+)\.\.\.(\d+)\)\)")] private static partial Regex ChannelBad();
    [GeneratedRegex(@"XCTAssertEqual\(HSV\(DocColor\(r: (\d+), g: (\d+), b: (\d+)\)\), HSV\(h: (\d+), s: (\d+), v: (\d+)\)\)")] private static partial Regex RgbToHsv();
    [GeneratedRegex(@"XCTAssertEqual\(HSV\(h: (\d+), s: (\d+), v: (\d+)\)\.color\((?:alpha: (\d+))?\), DocColor\(r: (\d+), g: (\d+), b: (\d+)(?:, a: (\d+))?\)\)")] private static partial Regex HsvToRgb();

    /// The body of one `func test...` in Swift, up to the next test function.
    static string Function(string swift, string name, string file)
    {
        var start = swift.IndexOf($"func {name}(", StringComparison.Ordinal);
        if (start < 0) throw new InvalidDataException($"VECTORS: {file} has no func {name}");
        var end = swift.IndexOf("\n    func ", start + 1, StringComparison.Ordinal);
        return end < 0 ? swift[start..] : swift[start..end];
    }

    static string[] Items(string list) => list.Split(',', StringSplitOptions.TrimEntries | StringSplitOptions.RemoveEmptyEntries);

    static Vector[] Need(Vector[] vectors, string op, string file)
    {
        // A rewritten Swift test must not leave a vector file checking nothing.
        if (vectors.All(v => v.Op != op)) throw new InvalidDataException($"VECTORS: {file} gave no {op} values; its form changed");
        return vectors;
    }

    /// Zoom steps, from ZoomPresetTests.
    public static VectorFile ZoomSteps(string swift)
    {
        var list = new List<Vector>();
        var presets = PresetList().Match(swift);
        if (!presets.Success) throw new InvalidDataException($"VECTORS: {ZoomTests} has no presets list");
        var values = Items(presets.Groups[1].Value).Select(Number).ToArray();
        list.Add(new Vector("presets", "presets", [], string.Join(" ", values)));
        foreach (Match m in ZoomStep().Matches(swift))
        {
            var op = m.Groups[1].Value;
            var input = Number(m.Groups[2].Value);
            list.Add(new Vector($"{op}({input})", op, [input], Number(m.Groups[3].Value)));
        }
        foreach (Match m in ZoomLabel().Matches(swift))
        {
            var input = Number(m.Groups[1].Value);
            list.Add(new Vector($"label({input})", "label", [input], m.Groups[2].Value));
        }
        var all = list.DistinctBy(v => v.Name).ToArray();
        foreach (var op in new[] { "presets", "nextLarger", "nextSmaller", "clamp", "label" }) Need(all, op, ZoomTests);
        return new VectorFile("zoom-steps.json", ZoomTests, ["testPresetListIsPaintNets", "testStepsFromValuesBetweenPresetsFollowTheHalfPercentRule", "testLabels"], all);
    }

    /// Fit math, from one Swift test.
    public static VectorFile FitMath(string swift)
    {
        var body = Function(swift, "testFitIsTheSmallerAxisLessTenPointsAndNeverAboveActualSize", ZoomTests);
        var list = new List<Vector>();
        foreach (Match m in ZoomFit().Matches(body))
        {
            string[] input = [Number(m.Groups[1].Value), Number(m.Groups[2].Value), Number(m.Groups[3].Value), Number(m.Groups[4].Value)];
            var output = Expression(m.Groups[5].Success ? m.Groups[5].Value : m.Groups[6].Value);
            list.Add(new Vector($"fit({string.Join(",", input)})", "fit", input, output));
        }
        var all = Need([.. list.DistinctBy(v => v.Name)], "fit", ZoomTests);
        return new VectorFile("fit-math.json", ZoomTests, ["testFitIsTheSmallerAxisLessTenPointsAndNeverAboveActualSize"], all);
    }

    /// Each menu item's Mac key. The Windows words are worked out by the app's FromMac, and
    /// the Mac's key is the record here, because the Swift tests hold no Windows words.
    public static VectorFile ShortcutWords(string skeleton)
    {
        var list = new List<Vector>();
        foreach (var line in skeleton.Split('\n').Select(l => l.TrimEnd('\r')).Where(l => !l.StartsWith('#')))
        {
            var m = SkeletonLine().Match(line);
            if (!m.Success) continue;
            list.Add(new Vector(m.Groups[1].Value, "macKey", [m.Groups[2].Value], WindowsWords.FromMac(m.Groups[2].Value)));
        }
        return new VectorFile("shortcut-words.json", MenuSkeleton, [], Need([.. list.DistinctBy(v => v.Name)], "macKey", MenuSkeleton));
    }

    /// Color math for W8: hex fields, channel fields and HSV.
    public static VectorFile ColorMath(string swift)
    {
        var hexBody = Function(swift, "testHexFieldValidation", ColorTests);
        var hsvBody = Function(swift, "testHSVConversionRoundTripsEveryByteTriple", ColorTests);
        var list = new List<Vector>();
        string Byte(string text) => Number(text);
        foreach (Match m in HexGood().Matches(hexBody))
        {
            var alpha = m.Groups[5].Success ? Byte(m.Groups[5].Value) : "255";
            list.Add(new Vector($"hex({m.Groups[1].Value})", "hex", [m.Groups[1].Value], $"{Byte(m.Groups[2].Value)} {Byte(m.Groups[3].Value)} {Byte(m.Groups[4].Value)} {alpha}"));
        }
        var bad = HexBadList().Match(hexBody);
        if (!bad.Success) throw new InvalidDataException($"VECTORS: {ColorTests} has no bad hex list");
        foreach (Match m in Quoted().Matches(bad.Groups[1].Value)) list.Add(new Vector($"hex({m.Groups[1].Value})", "hexRejected", [m.Groups[1].Value], "nil"));
        foreach (Match m in HexText().Matches(hexBody))
        {
            var alpha = m.Groups[4].Success ? Byte(m.Groups[4].Value) : "255";
            string[] input = [Byte(m.Groups[1].Value), Byte(m.Groups[2].Value), Byte(m.Groups[3].Value), alpha];
            list.Add(new Vector($"hexText({string.Join(" ", input)})", "hexText", input, m.Groups[5].Value));
        }
        foreach (Match m in ChannelGood().Matches(hexBody))
            list.Add(new Vector($"channel({m.Groups[1].Value},{m.Groups[2].Value}...{m.Groups[3].Value})", "channel", [m.Groups[1].Value, m.Groups[2].Value, m.Groups[3].Value], m.Groups[4].Value));
        foreach (Match m in ChannelBad().Matches(hexBody))
            list.Add(new Vector($"channel({m.Groups[1].Value},{m.Groups[2].Value}...{m.Groups[3].Value})", "channelRejected", [m.Groups[1].Value, m.Groups[2].Value, m.Groups[3].Value], "nil"));
        foreach (Match m in RgbToHsv().Matches(hsvBody))
        {
            string[] input = [m.Groups[1].Value, m.Groups[2].Value, m.Groups[3].Value];
            list.Add(new Vector($"rgbToHsv({string.Join(" ", input)})", "rgbToHsv", input, $"{m.Groups[4].Value} {m.Groups[5].Value} {m.Groups[6].Value}"));
        }
        foreach (Match m in HsvToRgb().Matches(hsvBody))
        {
            var alpha = m.Groups[4].Success ? m.Groups[4].Value : "255";
            var outAlpha = m.Groups[8].Success ? m.Groups[8].Value : "255";
            string[] input = [m.Groups[1].Value, m.Groups[2].Value, m.Groups[3].Value, alpha];
            list.Add(new Vector($"hsvToRgb({string.Join(" ", input)})", "hsvToRgb", input, $"{m.Groups[5].Value} {m.Groups[6].Value} {m.Groups[7].Value} {outAlpha}"));
        }
        var all = list.DistinctBy(v => v.Name).ToArray();
        foreach (var op in new[] { "hex", "hexRejected", "hexText", "channel", "channelRejected", "rgbToHsv", "hsvToRgb" }) Need(all, op, ColorTests);
        return new VectorFile("color-math.json", ColorTests, ["testHexFieldValidation", "testHSVConversionRoundTripsEveryByteTriple"], all);
    }

    /// Every vector file, built from the sources under `root`.
    public static IReadOnlyList<VectorFile> FromSources(string root)
    {
        string Read(string relative) => File.ReadAllText(Path.Combine(root, relative.Replace('/', Path.DirectorySeparatorChar))).Replace("\r\n", "\n");
        var zoom = Read(ZoomTests);
        return [ZoomSteps(zoom), FitMath(zoom), ShortcutWords(Read(MenuSkeleton)), ColorMath(Read(ColorTests))];
    }

    public static string ToJson(VectorFile file)
    {
        var options = new JsonSerializerOptions { WriteIndented = true, Encoder = System.Text.Encodings.Web.JavaScriptEncoder.UnsafeRelaxedJsonEscaping };
        using var stream = new MemoryStream();
        using (var w = new Utf8JsonWriter(stream, new JsonWriterOptions { Indented = true, Encoder = options.Encoder }))
        {
            w.WriteStartObject();
            w.WriteString("note", "Written by `OpenPainter.Harness --export-vectors` from the Swift tests. Do not edit by hand.");
            w.WriteString("source", file.Source);
            w.WriteStartArray("functions");
            foreach (var f in file.SourceFunctions) w.WriteStringValue(f);
            w.WriteEndArray();
            w.WriteStartArray("vectors");
            foreach (var v in file.Vectors)
            {
                w.WriteStartObject();
                w.WriteString("name", v.Name);
                w.WriteString("op", v.Op);
                w.WriteStartArray("in");
                foreach (var i in v.In) w.WriteStringValue(i);
                w.WriteEndArray();
                w.WriteString("out", v.Out);
                w.WriteEndObject();
            }
            w.WriteEndArray();
            w.WriteEndObject();
        }
        return Encoding.UTF8.GetString(stream.ToArray()).Replace("\r\n", "\n") + "\n";
    }

    public static VectorFile FromJson(string fileName, string json)
    {
        using var document = JsonDocument.Parse(json);
        var root = document.RootElement;
        var vectors = root.GetProperty("vectors").EnumerateArray().Select(v => new Vector(
            v.GetProperty("name").GetString() ?? "",
            v.GetProperty("op").GetString() ?? "",
            v.GetProperty("in").EnumerateArray().Select(i => i.GetString() ?? "").ToArray(),
            v.GetProperty("out").GetString() ?? "")).ToArray();
        return new VectorFile(fileName, root.GetProperty("source").GetString() ?? "",
            root.GetProperty("functions").EnumerateArray().Select(f => f.GetString() ?? "").ToArray(), vectors);
    }

    /// One line per disagreement, each naming the file and the vector.
    public static List<string> Differences(VectorFile recorded, VectorFile fromSource)
    {
        var differences = new List<string>();
        var ours = recorded.Vectors.ToDictionary(v => v.Name);
        var theirs = fromSource.Vectors.ToDictionary(v => v.Name);
        foreach (var (name, vector) in theirs)
        {
            if (!ours.TryGetValue(name, out var recordedOne)) differences.Add($"{recorded.FileName}: {name} is in {fromSource.Source} and not in the file");
            else if (recordedOne.Op != vector.Op || recordedOne.Out != vector.Out || !recordedOne.In.SequenceEqual(vector.In))
                differences.Add($"{recorded.FileName}: {name} is {recordedOne.Op} {string.Join(" ", recordedOne.In)} = {recordedOne.Out} in the file, {vector.Op} {string.Join(" ", vector.In)} = {vector.Out} in {fromSource.Source}");
        }
        foreach (var name in ours.Keys.Where(n => !theirs.ContainsKey(n))) differences.Add($"{recorded.FileName}: {name} is in the file and not in {fromSource.Source}");
        return differences;
    }
}

/// The Mac key to Windows words rule, so the vector file records the words in plain text.
/// The app test checks that the app's `WindowsKeys.FromMac` gives the same words.
public static class WindowsWords
{
    public static string FromMac(string mac)
    {
        bool ctrl = false, shift = false, alt = false;
        var at = 0;
        for (; at < mac.Length; at++)
        {
            var c = mac[at];
            if (c is '⌘' or '⌃') ctrl = true;
            else if (c == '⌥') alt = true;
            else if (c == '⇧') shift = true;
            else break;
        }
        var text = new StringBuilder();
        if (ctrl) text.Append("Ctrl+");
        if (shift) text.Append("Shift+");
        if (alt) text.Append("Alt+");
        text.Append(mac[at..] switch { "⌫" => "Delete", "=" => "+", var key => key });
        return text.ToString();
    }
}
