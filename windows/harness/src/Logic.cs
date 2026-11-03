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

    /// Why this run fails done-test 1, or null when it meets it. Gaps between frames see no
    /// freeze before the first frame or after the last, so those two times are stalls too.
    /// Each may be up to one allowed stall. The frame log alone gives only their sum, and the
    /// app's hook measures each one.
    public string? Problem(double askedSeconds, double? startGapMs = null, double? endGapMs = null)
    {
        if ((askedSeconds - Seconds) * 1000 > 2 * MostStallMs) return $"the frames cover {Seconds:F2} s of the {askedSeconds:F2} s asked";
        if (startGapMs > MostStallMs) return $"the first frame came {startGapMs:F1} ms after the trace began, over {MostStallMs} ms";
        if (endGapMs > MostStallMs) return $"the last frame came {endGapMs:F1} ms before the trace ended, over {MostStallMs} ms";
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

    /// Points on the canvas where a screen pixel can be judged: on the image and away from
    /// any color edge, in window pixels from the canvas's corner, from the middle outward.
    public static IReadOnlyList<(int X, int Y, Rgb Want)> ScreenPoints(int width, int height, ViewMap map, Func<double, double, Rgb?> expected, int columns = 16, int rows = 10)
    {
        var points = new List<(int X, int Y, Rgb Want)>();
        for (var row = 0; row < rows; row++)
        {
            for (var column = 0; column < columns; column++)
            {
                var x = (int)((column + .5) * width / columns);
                var y = (int)((row + .5) * height / rows);
                var (docX, docY) = map.DocumentPoint(x + .5, y + .5);
                if (map.OnDocument(docX, docY) && expected(docX, docY) is { } want) points.Add((x, y, want));
            }
        }
        return points.OrderBy(p => Math.Abs(p.X - width / 2.0) + Math.Abs(p.Y - height / 2.0)).ToList();
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

    /// The fewest checks each part must have run, from a full run of the report. A part that
    /// names no problem passes only with this evidence, so a check taken out of the report fails.
    public static readonly IReadOnlyList<(string Part, int Least)> LeastChecks =
    [
        ("hit test", 15),
        ("panels", 46),
        ("canvas input", 36),
        ("menus and options", 9),
        ("icons", 10),
        ("window minimum size", 1),
    ];

    static string PartOf(string problem)
    {
        if (problem.StartsWith("panel", StringComparison.Ordinal)) return "panels";
        if (problem.StartsWith("icons", StringComparison.Ordinal)) return "icons";
        if (problem.StartsWith("window minimum", StringComparison.Ordinal)) return "window minimum size";
        if (problem.StartsWith("menu", StringComparison.Ordinal) || problem.StartsWith("options", StringComparison.Ordinal) || problem.StartsWith("more popup", StringComparison.Ordinal)) return "menus and options";
        if (HitTestWords.Any(word => problem.StartsWith(word, StringComparison.Ordinal))) return "hit test";
        return "canvas input";
    }

    public static IReadOnlyList<PartResult> Parts(string json, int exitCode, string suffix)
    {
        using var report = JsonDocument.Parse(json);
        var root = report.RootElement;
        var problems = root.GetProperty("problems").EnumerateArray().Select(p => p.GetString() ?? "").ToList();
        var parts = new List<PartResult>();
        foreach (var (name, least) in LeastChecks)
        {
            var mine = problems.Where(p => PartOf(p) == name).ToList();
            var label = $"{name}{suffix}";
            if (mine.Count != 0)
            {
                parts.Add(PartResult.Fail(label, string.Join("; ", mine)));
                continue;
            }
            var covered = Covered(root, name);
            if (covered.Count != 0)
            {
                parts.Add(PartResult.NotRun(label, $"another program covers: {string.Join("; ", covered)}"));
                continue;
            }
            var ran = ChecksRun(root, name);
            parts.Add(ran >= least
                ? PartResult.Pass(label, $"{ran} checks")
                : PartResult.Fail(label, $"the report ran {ran} checks here and this part needs at least {least}, so a pass would have no evidence"));
        }
        if (exitCode != 0 && problems.Count == 0) parts.Add(PartResult.Fail($"report{suffix}", $"the app exited {exitCode} with no problem named"));
        return parts;
    }

    static int ChecksRun(JsonElement root, string part) =>
        root.TryGetProperty("checks_run", out var ran) && ran.TryGetProperty(part, out var count) ? count.GetInt32() : 0;

    /// What a part could not see because another program sat on top of it.
    static List<string> Covered(JsonElement root, string part)
    {
        var covered = new List<string>();
        if (part == "panels" && root.TryGetProperty("panels", out var panels))
        {
            foreach (var panel in panels.EnumerateObject())
                if (panel.Value.GetProperty("screen").GetString() is { } screen && screen.StartsWith("NOT RUN", StringComparison.Ordinal))
                    covered.Add($"{panel.Name} ({screen})");
        }
        if (part == "menus and options" && root.TryGetProperty("more_popup_screen", out var popup)
            && popup.GetString() is { } shown && shown.StartsWith("NOT RUN", StringComparison.Ordinal))
            covered.Add($"the more popup ({shown})");
        return covered;
    }
}

/// One launch's real screen pixels after the first-frame signal.
public sealed record ScreenRead(string Label, int Judged, int Matched, int Covered, string? FirstMiss);

public static class ScreenCheck
{
    /// A real screen pixel proves something only when one was judged. A launch where other
    /// programs covered every point, or every point was near a color edge, is NOT RUN.
    public static PartResult Part(IReadOnlyList<ScreenRead> reads)
    {
        const string Name = "screen pixels";
        if (reads.Count == 0) return PartResult.NotRun(Name, "no launch read the screen");
        var missed = reads.Where(r => r.Matched != r.Judged).ToList();
        if (missed.Count != 0) return PartResult.Fail(Name, string.Join("; ", missed.Select(r => $"{r.Label}: {r.Judged - r.Matched} of {r.Judged} differ, first {r.FirstMiss}")));
        var blind = reads.Where(r => r.Judged == 0).ToList();
        if (blind.Count != 0) return PartResult.NotRun(Name, $"no screen pixel was judged in {string.Join(", ", blind.Select(r => $"{r.Label} ({r.Covered} points covered)"))}");
        return PartResult.Pass(Name, $"{reads.Sum(r => r.Judged)} points judged over {reads.Count} launches, all matched");
    }
}

/// A real screen pixel in the canvas's area before the first-frame signal.
public readonly record struct LaunchPixel(double Ms, Rgb Color);

/// What the canvas's area showed while the image was loading.
public sealed record LaunchRead(string Label, Rgb Stage, double FirstFrameMs, IReadOnlyList<LaunchPixel> Pixels);

public static class LaunchPaint
{
    /// The two clocks start a little apart, and a frame shows before the app can say so.
    public const double FrameClockSlackMs = 150;

    /// How long a new window may show what is behind it before its first paint.
    public const double MostOpeningMs = 250;

    /// The window first shows what is behind it, for a moment, until Avalonia paints. From
    /// that paint until the first frame only the stage color may show, and black never may.
    /// After the first frame the image shows, so only black means anything there.
    public static PartResult Part(IReadOnlyList<LaunchRead> reads)
    {
        const string Name = "launch paint";
        const int Tolerance = 1;
        if (reads.Count == 0) return PartResult.NotRun(Name, "no launch was measured");
        var problems = new List<string>();
        int judged = 0, launches = 0;
        double longestOpening = 0;
        foreach (var read in reads)
        {
            bool IsStage(LaunchPixel p) => p.Color.Distance(read.Stage) <= Tolerance;
            var early = read.Pixels.Where(p => p.Ms < read.FirstFrameMs - FrameClockSlackMs).ToList();
            if (early.Count == 0) continue;
            launches++;
            var painted = early.FindIndex(IsStage);
            if (painted < 0)
            {
                var black = early.Count(p => p.Color.IsBlack);
                problems.Add($"{read.Label}: none of {early.Count} pixels before the first frame at {read.FirstFrameMs:F0} ms was the stage color {read.Stage}, {black} were black");
                continue;
            }
            var opening = early[painted].Ms - early[0].Ms;
            longestOpening = Math.Max(longestOpening, opening);
            if (opening > MostOpeningMs) problems.Add($"{read.Label}: the stage color took {opening:F0} ms to show, over {MostOpeningMs} ms");
            var after = early.Skip(painted).ToList();
            judged += after.Count;
            var wrong = after.Where(p => !IsStage(p)).ToList();
            if (wrong.Count != 0) problems.Add($"{read.Label}: {wrong.Count} of {after.Count} pixels after the first paint were not the stage color {read.Stage}, {wrong.Count(p => p.Color.IsBlack)} of them black, first {wrong[0].Color} at {wrong[0].Ms:F0} ms");
        }
        if (problems.Count != 0) return PartResult.Fail(Name, string.Join("; ", problems));
        if (judged == 0) return PartResult.NotRun(Name, $"no pixel was read before a first frame in any of {reads.Count} launches");
        return PartResult.Pass(Name, $"{judged} real screen pixels before the first frame over {launches} of {reads.Count} launches, all the stage color, none black; the window showed what is behind it for at most {longestOpening:F0} ms");
    }
}

/// One moment of the pan trace, read from the engine by the app's hook.
public readonly record struct PanSample(double Seconds, int PointerX, int PointerY, double OriginX, double OriginY, long Frames);

public static class PanMotion
{
    /// In window pixels: about 0.2 s of the trace's fastest speed, the lag a slow UI may show.
    public const double MostLagPixels = 150;
    const double LeastSweep = .9;
    const int LeastSamples = 4;

    /// Why the engine's view did not follow the pointer, or null when it did. A pan that does
    /// nothing can still get frames from the engine, so the frame numbers alone cannot see one.
    public static string? Problem(IReadOnlyList<PanSample> samples, double zoom, double scale)
    {
        if (samples.Count < LeastSamples) return $"the trace has only {samples.Count} view samples";
        var first = samples[0];
        var sweepX = samples.Max(s => s.PointerX) - samples.Min(s => s.PointerX);
        var sweepY = samples.Max(s => s.PointerY) - samples.Min(s => s.PointerY);
        if (sweepX == 0 || sweepY == 0) return $"the pointer did not move on both axes: {sweepX} by {sweepY} pixels";
        var movedX = (samples.Max(s => s.OriginX) - samples.Min(s => s.OriginX)) * zoom * scale;
        var movedY = (samples.Max(s => s.OriginY) - samples.Min(s => s.OriginY)) * zoom * scale;
        if (movedX < sweepX * LeastSweep || movedY < sweepY * LeastSweep)
            return $"the view moved {movedX:F0} by {movedY:F0} pixels while the pointer moved {sweepX} by {sweepY}";
        for (var i = 0; i < samples.Count; i++)
        {
            var s = samples[i];
            var offX = (s.OriginX - first.OriginX) * zoom * scale - (s.PointerX - first.PointerX);
            var offY = (s.OriginY - first.OriginY) * zoom * scale - (s.PointerY - first.PointerY);
            if (Math.Abs(offX) > MostLagPixels || Math.Abs(offY) > MostLagPixels)
                return $"at {s.Seconds:F2} s the view is {offX:F0} by {offY:F0} pixels off the pointer's path";
            if (i != 0 && s.Frames <= samples[i - 1].Frames) return $"no frame was shown between {samples[i - 1].Seconds:F2} s and {s.Seconds:F2} s";
        }
        return null;
    }

    /// Real pixels at one screen point during the trace. Flat cells pass under it, so the
    /// color must keep changing. `judged` is false when too few were read to say.
    public static string? ScreenProblem(IReadOnlyList<Rgb?> seen, out bool judged)
    {
        const int LeastRead = 8;
        const double LeastChangeShare = .25;
        var read = seen.Where(c => c is not null).Select(c => c!.Value).ToList();
        judged = read.Count >= LeastRead;
        if (!judged) return null;
        var changes = 0;
        for (var i = 1; i < read.Count; i++) if (read[i] != read[i - 1]) changes++;
        return changes < (read.Count - 1) * LeastChangeShare
            ? $"the screen's color changed only {changes} times in {read.Count} reads, so the image did not move"
            : null;
    }
}

/// PLAN 4.3: a busy run is reported and thrown out.
public static class BusyJudge
{
    public const double MostCpuPercent = 15;        // The whole PC less the app, on average.
    public const double MostGpuBeforePercent = 15;
    public const double MostGpuDuringPercent = 50;  // The app's own pan sits far under this on the RTX.

    /// Why a run must not start. GPU use that cannot be read counts, because then nobody knows.
    public static List<string> Before(double cpu, double? gpu, IEnumerable<string> busyPrograms)
    {
        var reasons = new List<string>();
        if (cpu >= MostCpuPercent) reasons.Add($"other programs kept the PC {cpu:F0}% busy before the trace");
        if (gpu is null) reasons.Add("GPU use before the trace could not be read");
        else if (gpu >= MostGpuBeforePercent) reasons.Add($"the GPU was {gpu:F0}% busy before the trace");
        reasons.AddRange(busyPrograms);
        return reasons;
    }

    /// Why a finished run does not count.
    public static List<string> During(IReadOnlyList<double> cpuOthers, IReadOnlyList<double?> gpu)
    {
        var reasons = new List<string>();
        if (cpuOthers.Count == 0) reasons.Add("CPU use during the trace could not be read");
        else if (cpuOthers.Average() >= MostCpuPercent) reasons.Add($"other programs took {cpuOthers.Average():F0}% of the CPU during the trace");
        var read = gpu.Where(g => g is not null).Select(g => g!.Value).ToList();
        if (read.Count == 0) reasons.Add("GPU use during the trace could not be read");
        else if (read.Average() >= MostGpuDuringPercent) reasons.Add($"the GPU was {read.Average():F0}% busy during the trace");
        return reasons;
    }

    /// The words for one program that preflight.ps1 watches. Running with nothing in use is
    /// only named. A null count means it is running and nobody could tell.
    public static (string? Note, string? Busy) Program(string name, bool running, int? inUse, string unit)
    {
        if (!running) return (null, null);
        if (inUse is null) return ($"{name} is running", $"{name} is running, and its {unit} could not be read");
        if (inUse == 0) return ($"{name} is running, with no {unit}", null);
        return ($"{name} is running, with {inUse} {unit}", $"{name} is running, with {inUse} {unit}");
    }
}

/// One program's share of the whole PC's CPU over one read, from 0 to 100.
public readonly record struct ProgramUse(string Name, double Percent)
{
    /// Adds up CPU time by program name between two lists of running processes. A process
    /// that started between them counts in full. One that ended between them is left out,
    /// and only the whole-PC number sees it.
    public static List<ProgramUse> Top(IReadOnlyDictionary<int, (string Name, TimeSpan Cpu)> before, IReadOnlyDictionary<int, (string Name, TimeSpan Cpu)> after, double coreSeconds, int count = 5, double leastPercent = .5)
    {
        var used = new Dictionary<string, double>();
        foreach (var (pid, now) in after)
        {
            var then = before.TryGetValue(pid, out var old) && old.Name == now.Name ? old.Cpu : TimeSpan.Zero;
            used[now.Name] = used.GetValueOrDefault(now.Name) + (now.Cpu - then).TotalSeconds;
        }
        return used.Select(u => new ProgramUse(u.Key, coreSeconds <= 0 ? 0 : 100 * u.Value / coreSeconds))
            .Where(p => p.Percent >= leastPercent)
            .OrderByDescending(p => p.Percent)
            .Take(count)
            .ToList();
    }

    public override string ToString() => $"{Name} {Percent:F0}%";
}

/// One read of how busy the PC is: other programs' share of the CPU, which is the whole PC
/// less the harness. `Unread` counts programs that would not give their CPU time. The
/// whole-PC number still holds their use, and only the names leave them out.
public sealed record QuietRead(double OthersCpu, double? Gpu, IReadOnlyList<string> BusyPrograms, IReadOnlyList<ProgramUse> Top, int Unread = 0);

public sealed record SettleResult(bool Quiet, double WaitedSeconds, IReadOnlyList<QuietRead> Reads, IReadOnlyList<string> Reasons)
{
    public string Words
    {
        get
        {
            var last = Reads[^1];
            var top = last.Top.Count == 0 ? "no program at 1% or more" : string.Join(", ", last.Top);
            var gpu = string.Join(", ", Reads.Select(r => r.Gpu is { } g ? $"{g:F0}%" : "not read"));
            return $"waited {WaitedSeconds:F0} s for the PC to settle, {Reads.Count} reads; other programs' CPU {string.Join(", ", Reads.Select(r => $"{r.OthersCpu:F0}%"))}; GPU {gpu}; top programs at the last read: {top}{(last.Unread == 0 ? "" : $"; {last.Unread} programs could not be read")}";
        }
    }
}

/// The harness's own launches, build and shut-downs leave the PC busy for a while. So it reads
/// the PC until the busy limits hold, and the limits do not change. Waiting gives a busy PC
/// more than one chance, so it takes more than one quiet read in a row to go on.
public static class Settle
{
    public const double MostWaitSeconds = 60;
    public const int QuietReadsNeeded = 2;

    public static SettleResult Wait(Func<QuietRead> read, Func<double> seconds, double mostWaitSeconds = MostWaitSeconds, int readsNeeded = QuietReadsNeeded)
    {
        var start = seconds();
        var reads = new List<QuietRead>();
        var quiet = 0;
        while (true)
        {
            var now = read();
            reads.Add(now);
            var reasons = BusyJudge.Before(now.OthersCpu, now.Gpu, now.BusyPrograms);
            quiet = reasons.Count == 0 ? quiet + 1 : 0;
            var waited = seconds() - start;
            if (quiet >= readsNeeded) return new SettleResult(true, waited, reads, []);
            if (waited < mostWaitSeconds) continue;
            if (reasons.Count == 0) reasons.Add($"the PC had only one read under the limits, at the end of the {mostWaitSeconds:F0} s wait");
            return new SettleResult(false, waited, reads, reasons);
        }
    }
}

/// The canvas's window must stay the same one for the whole run, because the engine draws into
/// it. A change here is a fault in the app, so it is named and never read past.
public static class CanvasHandles
{
    /// `before` holds the handles seen before the first-frame signal. `now` is the one seen later.
    public static string? Problem(string label, IReadOnlyCollection<nint> before, nint signal, nint now)
    {
        if (before.Count > 1) return $"{label}: {before.Count} canvas handles were seen before the signal, so the app replaced the window the engine draws into: {string.Join(", ", before)}";
        if (before.Count == 1 && !before.Contains(signal)) return $"{label}: the canvas handle was {before.First()} before the signal, and the signal names {signal}";
        if (now != signal) return $"{label}: the signal names canvas handle {signal}, and after the signal the window has {now}";
        return null;
    }
}

/// `first-frame.json`, `report.json` and `pan.json` are how the app tells the harness it is done.
/// One left by an earlier harness run looks just like one from this run, so each harness
/// run starts with none. The numbered folders are the harness's own, and nothing else goes.
public static class RunFolders
{
    public static void Clear(string runs)
    {
        if (!Directory.Exists(runs)) return;
        foreach (var folder in Directory.GetDirectories(runs))
            if (IsRunFolder(Path.GetFileName(folder))) Directory.Delete(folder, recursive: true);
    }

    static bool IsRunFolder(string name)
    {
        var dash = name.IndexOf('-');
        return dash >= 2 && name[..dash].All(char.IsAsciiDigit);
    }
}

/// What nvidia-smi says about the GPU.
public sealed record GpuFacts(string Name, double? UsePercent, int? EncoderSessions)
{
    /// One line of `--query-gpu=name,utilization.gpu,encoder.stats.sessionCount --format=csv,noheader,nounits`.
    public static GpuFacts Parse(string output)
    {
        var line = output.Split('\n').Select(l => l.Trim()).FirstOrDefault(l => l.Length != 0) ?? "";
        var fields = line.Split(',').Select(f => f.Trim()).ToArray();
        if (fields.Length != 3) return new GpuFacts($"nvidia-smi printed: {line}", null, null);
        double? use = double.TryParse(fields[1], System.Globalization.NumberStyles.Float, System.Globalization.CultureInfo.InvariantCulture, out var value) && double.IsFinite(value) ? value : null;
        int? sessions = int.TryParse(fields[2], out var count) ? count : null;
        return new GpuFacts(fields[0], use, sessions);
    }

    public override string ToString() => UsePercent is { } use ? $"{Name} {use:F0}%" : Name;
}

public static class ShotScales
{
    public static readonly double[] Wanted = [1, 2];

    /// One row per display scale the shots must cover. A scale this display was not at is NOT
    /// RUN and says so, because the harness does not change the user's display settings.
    public static IEnumerable<PartResult> Rows(IReadOnlyCollection<double> seen)
    {
        foreach (var scale in Wanted)
        {
            var name = $"window shots at {scale * 100:0}%";
            yield return seen.Contains(scale)
                ? PartResult.Pass(name)
                : PartResult.NotRun(name, $"the display scale is {string.Join(", ", seen.Select(s => $"{s * 100:0}%"))}, and the harness does not change the user's display settings");
        }
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
