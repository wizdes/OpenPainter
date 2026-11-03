// The harness's judgement, with no window: frame numbers, pixel checks and the words each
// part prints. The app tests drive this code with made-up inputs, so a black frame is proven
// to fail without needing one on a screen.
using System.Text.Json;

namespace OpenPainter.Harness;

public enum PartStatus { Pass, Fail, NotRun }

public sealed record PartResult(string Name, PartStatus Status, string Detail)
{
    public string Line => Status switch
    {
        PartStatus.Pass => Detail.Length == 0 ? $"{Name}: PASS" : $"{Name}: PASS ({Detail})",
        PartStatus.Fail => $"{Name}: FAIL ({Detail})",
        _ => $"{Name}: NOT RUN ({Detail})",
    };

    public static PartResult Pass(string name, string detail = "") => new(name, PartStatus.Pass, detail);
    public static PartResult Fail(string name, string detail) => new(name, PartStatus.Fail, detail);
    public static PartResult NotRun(string name, string detail) => new(name, PartStatus.NotRun, detail);

    /// The last line of a run. The `window` level reads it, and any part that did not pass fails the run.
    public static string Summary(IReadOnlyCollection<PartResult> parts)
    {
        var bad = parts.Where(p => p.Status != PartStatus.Pass).Select(p => p.Name).ToList();
        if (parts.Count == 0) return "HARNESS: FAIL (no part ran)";
        return bad.Count == 0 ? $"HARNESS: PASS ({parts.Count} parts)" : $"HARNESS: FAIL ({string.Join(", ", bad)})";
    }
}

/// Numbers from the engine's own frame times, in milliseconds.
public sealed record FrameStats(int Frames, double Seconds, double AverageFps, double LongestStallMs)
{
    public const double LeastFps = 59, MostStallMs = 50;

    public static FrameStats From(IReadOnlyList<double> times)
    {
        if (times.Count < 2) return new FrameStats(times.Count, 0, 0, 0);
        var longest = 0.0;
        for (var i = 1; i < times.Count; i++) longest = Math.Max(longest, times[i] - times[i - 1]);
        var seconds = (times[^1] - times[0]) / 1000;
        return new FrameStats(times.Count, seconds, seconds > 0 ? (times.Count - 1) / seconds : 0, longest);
    }

    /// Why this run fails done-test 1, or null when it meets it. A frame log that stops early
    /// must fail, so the frames must cover the asked time too.
    public string? Problem(double askedSeconds)
    {
        const double LeastCover = .98;
        if (Seconds < askedSeconds * LeastCover) return $"the frames cover {Seconds:F1} s of the {askedSeconds:F1} s asked";
        if (AverageFps < LeastFps) return $"average {AverageFps:F1} fps is under {LeastFps}";
        if (LongestStallMs > MostStallMs) return $"the longest stall, {LongestStallMs:F1} ms, is over {MostStallMs} ms";
        return null;
    }

    public override string ToString() => $"average {AverageFps:F1} fps, longest stall {LongestStallMs:F1} ms, {Frames} frames in {Seconds:F1} s";
}

public readonly record struct Rgb(byte R, byte G, byte B)
{
    public int Distance(Rgb other) => Math.Max(Math.Abs(R - other.R), Math.Max(Math.Abs(G - other.G), Math.Abs(B - other.B)));
    public bool IsBlack => R <= 2 && G <= 2 && B <= 2;
    public override string ToString() => $"#{R:X2}{G:X2}{B:X2}";
}

public readonly record struct PixelRect(int X, int Y, int Width, int Height);

/// A captured picture: 4 bytes per pixel in blue, green, red, unused order.
public sealed record Picture(int Width, int Height, byte[] Bgra)
{
    public Rgb At(int x, int y)
    {
        var i = (y * Width + x) * 4;
        return new Rgb(Bgra[i + 2], Bgra[i + 1], Bgra[i]);
    }

    public static Picture Filled(int width, int height, Rgb color)
    {
        var bytes = new byte[width * height * 4];
        for (var i = 0; i < bytes.Length; i += 4) (bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]) = (color.B, color.G, color.R, 255);
        return new Picture(width, height, bytes);
    }
}

/// Where a document pixel lands in the canvas's window pixels.
public sealed record ViewMap(double Zoom, double OriginX, double OriginY, double Scale, double DocumentWidth, double DocumentHeight)
{
    public (double X, double Y) DocumentPoint(double deviceX, double deviceY) =>
        ((deviceX / Scale - OriginX) / Zoom, (deviceY / Scale - OriginY) / Zoom);

    public (double X, double Y) DevicePoint(double docX, double docY) =>
        ((docX * Zoom + OriginX) * Scale, (docY * Zoom + OriginY) * Scale);

    public bool OnDocument(double docX, double docY) => docX >= 0 && docY >= 0 && docX < DocumentWidth && docY < DocumentHeight;
}

public sealed record PixelResult(int Samples, int Matched, int Black, string? FirstMiss)
{
    public const int LeastSamples = 40;

    public string? Problem()
    {
        if (Samples < LeastSamples) return $"only {Samples} sample points fell on the image, so this proves nothing";
        if (Black == Samples) return $"BLACK FRAME: all {Samples} points are black";
        if (Matched != Samples) return $"{Samples - Matched} of {Samples} points differ, {Black} of them black; first: {FirstMiss}";
        return null;
    }
}

public static class PixelCheck
{
    /// Reads a grid of real pixels in the canvas and compares each with what the document
    /// holds there. `expected` returns null for a point too close to a color edge to judge.
    public static PixelResult Judge(Picture picture, PixelRect canvas, ViewMap map, Func<double, double, Rgb?> expected, int tolerance, int columns = 24, int rows = 16)
    {
        int samples = 0, matched = 0, black = 0;
        string? miss = null;
        for (var row = 0; row < rows; row++)
        {
            for (var column = 0; column < columns; column++)
            {
                var deviceX = (int)((column + .5) * canvas.Width / columns);
                var deviceY = (int)((row + .5) * canvas.Height / rows);
                var (docX, docY) = map.DocumentPoint(deviceX + .5, deviceY + .5);
                if (!map.OnDocument(docX, docY) || expected(docX, docY) is not { } want) continue;
                var x = canvas.X + deviceX;
                var y = canvas.Y + deviceY;
                if (x < 0 || y < 0 || x >= picture.Width || y >= picture.Height) continue;
                var got = picture.At(x, y);
                samples++;
                if (got.IsBlack) black++;
                if (got.Distance(want) <= tolerance) matched++;
                else miss ??= $"document {docX:F0},{docY:F0} shows {got}, expected {want}";
            }
        }
        return new PixelResult(samples, matched, black, miss);
    }

    /// True when nearly all of a grid of pixels in the rectangle is black: what a canvas
    /// with no frame yet shows.
    public static bool IsBlack(Picture picture, PixelRect canvas)
    {
        const int Columns = 24, Rows = 16;
        var black = 0;
        for (var row = 0; row < Rows; row++)
            for (var column = 0; column < Columns; column++)
                if (picture.At(canvas.X + (int)((column + .5) * canvas.Width / Columns), canvas.Y + (int)((row + .5) * canvas.Height / Rows)).IsBlack) black++;
        return black >= Columns * Rows * 95 / 100;
    }
}

/// What `--test-report` wrote, split into the parts the harness names.
public static class ReportJudge
{
    static readonly string[] HitTestWords = ["top menu", "title text", "gap beside", "minimize", "maximize", "maximized", "restore", "close", "top resize"];

    static string PartOf(string problem)
    {
        if (problem.StartsWith("panel", StringComparison.Ordinal)) return "panels";
        if (problem.StartsWith("icons", StringComparison.Ordinal)) return "icons";
        if (problem.StartsWith("window minimum", StringComparison.Ordinal)) return "window minimum size";
        if (problem.StartsWith("menu", StringComparison.Ordinal) || problem.StartsWith("options", StringComparison.Ordinal)) return "menus and options";
        if (HitTestWords.Any(word => problem.StartsWith(word, StringComparison.Ordinal))) return "hit test";
        return "canvas input";
    }

    public static IReadOnlyList<PartResult> Parts(string json, int exitCode, string suffix)
    {
        using var report = JsonDocument.Parse(json);
        var root = report.RootElement;
        var problems = root.GetProperty("problems").EnumerateArray().Select(p => p.GetString() ?? "").ToList();
        var parts = new List<PartResult>();
        foreach (var name in new[] { "hit test", "panels", "canvas input", "menus and options", "icons", "window minimum size" })
        {
            var mine = problems.Where(p => PartOf(p) == name).ToList();
            var label = $"{name}{suffix}";
            if (mine.Count != 0)
            {
                parts.Add(PartResult.Fail(label, string.Join("; ", mine)));
                continue;
            }
            var covered = name == "panels" ? CoveredPanels(root) : [];
            parts.Add(covered.Count != 0
                ? PartResult.NotRun(label, $"another program covers: {string.Join("; ", covered)}")
                : PartResult.Pass(label));
        }
        if (exitCode != 0 && problems.Count == 0) parts.Add(PartResult.Fail($"report{suffix}", $"the app exited {exitCode} with no problem named"));
        return parts;
    }

    static List<string> CoveredPanels(JsonElement root)
    {
        var covered = new List<string>();
        if (!root.TryGetProperty("panels", out var panels)) return covered;
        foreach (var panel in panels.EnumerateObject())
            if (panel.Value.GetProperty("screen").GetString() is { } screen && screen.StartsWith("NOT RUN", StringComparison.Ordinal))
                covered.Add($"{panel.Name} ({screen})");
        return covered;
    }
}

/// Reads preflight.ps1's words: null means the harness may go on.
public static class Preflight
{
    /// System prompts that broke later tests in QuickEdit. Each shows a window only when it wants an answer.
    public static readonly string[] KnownDialogs = ["consent", "CredentialUIBroker", "WerFault", "WerFaultSecure"];

    /// The block reason for the known-dialog processes that have a window, or null.
    public static string? DialogReason(IEnumerable<string> withWindow)
    {
        var up = withWindow.Where(name => KnownDialogs.Contains(name, StringComparer.OrdinalIgnoreCase)).Distinct(StringComparer.OrdinalIgnoreCase).ToList();
        return up.Count == 0 ? null : $"known dialog is up: {string.Join(", ", up)}";
    }

    public static string? BlockReason(string output, bool ignoreDisk)
    {
        foreach (var line in output.Split('\n').Select(l => l.Trim()))
        {
            if (line.StartsWith("UI TESTS: NOT RUN", StringComparison.Ordinal)) return line["UI TESTS: NOT RUN".Length..].Trim(' ', '(', ')');
        }
        foreach (var line in output.Split('\n').Select(l => l.Trim()))
        {
            if (!line.StartsWith("PREFLIGHT: FAIL", StringComparison.Ordinal)) continue;
            var reason = line["PREFLIGHT: FAIL".Length..].Trim(' ', '(', ')');
            return ignoreDisk && reason == "under 40 GB free" ? null : $"preflight: {reason}";
        }
        return output.Contains("PREFLIGHT: PASS", StringComparison.Ordinal) ? null : "preflight printed no result";
    }
}
