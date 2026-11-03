// The real-window checks of W7, run by root as the signed-in user:
//
//   dotnet run --project windows/harness/OpenPainter.Harness.csproj -c Release -- --app <OpenPainter.exe> --out <folder>
//
// Each part prints one line: PASS, FAIL or NOT RUN with a reason. The last line is
// HARNESS: PASS or HARNESS: FAIL. Any part that did not pass fails the run.
using System.Diagnostics;
using System.Globalization;
using System.Text;
using System.Text.Json;

namespace OpenPainter.Harness;

public static class Program
{
    const string Usage = "usage: OpenPainter.Harness --app <OpenPainter.exe> --out <folder> [--parts report,pixels,color,shots,pan,exit,fatal,small-screen] [--pan-seconds <s>] [--first-frame-runs <n>] [--allow-busy] [--ignore-disk]\n       OpenPainter.Harness --export-vectors";
    static readonly string[] AllParts = ["report", "pixels", "color", "shots", "pan", "exit", "fatal", "small-screen"];

    public static int Main(string[] args)
    {
        Console.OutputEncoding = Encoding.UTF8;
        try
        {
            if (args is ["--export-vectors"]) return ExportVectors();
            return new Run(Options.Parse(args)).Go();
        }
        catch (ArgumentException e)
        {
            Console.Error.WriteLine($"{e.Message}\n{Usage}");
            return 2;
        }
    }

    public static string RepoRoot()
    {
        for (var dir = new DirectoryInfo(AppContext.BaseDirectory); dir is not null; dir = dir.Parent)
            if (Directory.Exists(Path.Combine(dir.FullName, "windows", "harness")) && Directory.Exists(Path.Combine(dir.FullName, "engine")))
                return dir.FullName;
        throw new DirectoryNotFoundException($"no repository above {AppContext.BaseDirectory}");
    }

    static int ExportVectors()
    {
        var root = RepoRoot();
        var folder = Path.Combine(root, Vectors.Folder.Replace('/', Path.DirectorySeparatorChar));
        Directory.CreateDirectory(folder);
        foreach (var file in Vectors.FromSources(root))
        {
            File.WriteAllText(Path.Combine(folder, file.FileName), Vectors.ToJson(file), new UTF8Encoding(false));
            Console.WriteLine($"{file.FileName}: {file.Vectors.Length} vectors from {file.Source}");
        }
        return 0;
    }

    public sealed record Options(string App, string Out, string[] Parts, double PanSeconds, int FirstFrameRuns, bool AllowBusy, bool IgnoreDisk)
    {
        public const double FullPanSeconds = 30, MostPanSeconds = 3600;

        public static Options Parse(string[] args)
        {
            string? app = null, output = null;
            var parts = AllParts;
            var seconds = FullPanSeconds;
            var runs = 3;
            bool allowBusy = false, ignoreDisk = false;
            for (var i = 0; i < args.Length; i++)
            {
                string Value() => ++i < args.Length ? args[i] : throw new ArgumentException($"HARNESS: {args[i - 1]} needs a value");
                switch (args[i])
                {
                    case "--app": app = Path.GetFullPath(Value()); break;
                    case "--out": output = Path.GetFullPath(Value()); break;
                    case "--parts":
                        parts = Value().Split(',', StringSplitOptions.RemoveEmptyEntries);
                        foreach (var part in parts) if (!AllParts.Contains(part)) throw new ArgumentException($"HARNESS: --parts: no part named {part}");
                        break;
                    case "--pan-seconds":
                        var text = Value();
                        if (!double.TryParse(text, NumberStyles.Float, CultureInfo.InvariantCulture, out seconds) || !double.IsFinite(seconds) || seconds <= 0 || seconds > MostPanSeconds)
                            throw new ArgumentException($"HARNESS: --pan-seconds: {text} is not a number of seconds from 0 to {MostPanSeconds}");
                        break;
                    case "--first-frame-runs":
                        var count = Value();
                        if (!int.TryParse(count, out runs) || runs <= 0) throw new ArgumentException($"HARNESS: --first-frame-runs: {count}");
                        break;
                    case "--allow-busy": allowBusy = true; break;
                    case "--ignore-disk": ignoreDisk = true; break;
                    default: throw new ArgumentException($"HARNESS: unknown argument {args[i]}");
                }
            }
            if (app is null || output is null) throw new ArgumentException("HARNESS: --app and --out are required");
            if (!File.Exists(app)) throw new ArgumentException($"HARNESS: no app at {app}");
            return new Options(app, output, parts, seconds, runs, allowBusy, ignoreDisk);
        }
    }

    sealed class Run(Options options)
    {
        const int StartLimitMs = 120_000;
        const int ColorTolerance = 1;      // tests/tolerances.toml's limit.
        const int EdgeMarginDevicePixels = 4;
        const string Png24Mp = "cells-24mp.png";
        readonly List<PartResult> _parts = [];
        int _run;

        void Report(PartResult part)
        {
            _parts.Add(part);
            Console.WriteLine(part.Line);
        }

        string NextFolder(string label) => Path.Combine(options.Out, "runs", $"{++_run:D2}-{label}");

        public int Go()
        {
            Directory.CreateDirectory(options.Out);
            Win32.UsePhysicalPixels();
            var block = PreflightBlock();
            if (block is not null)
            {
                Console.WriteLine($"UI TESTS: NOT RUN ({block})");
                foreach (var part in options.Parts) Report(PartResult.NotRun(part, block));
                Console.WriteLine(PartResult.Summary(_parts));
                return 1;
            }
            Console.WriteLine("UI TESTS: PASS");
            var settingsBefore = RealSettings();
            foreach (var part in options.Parts)
            {
                try
                {
                    switch (part)
                    {
                        case "report": ReportPart("light"); ReportPart("dark"); break;
                        case "pixels": PixelsPart(); break;
                        case "color": ColorPart("light"); ColorPart("dark"); break;
                        case "shots": ShotsPart(); break;
                        case "pan": PanPart(); break;
                        case "exit": ExitPart(); break;
                        case "fatal": FatalPart(); break;
                        case "small-screen": SmallScreenPart(); break;
                        default: throw new InvalidOperationException($"HARNESS: no part named {part}");
                    }
                }
                catch (Exception e) when (e is InvalidOperationException or TimeoutException or IOException or JsonException or KeyNotFoundException)
                {
                    Report(PartResult.Fail(part, e.Message));
                }
            }
            var settingsAfter = RealSettings();
            Report(settingsBefore == settingsAfter
                ? PartResult.Pass("real settings folder", "unchanged")
                : PartResult.Fail("real settings folder", $"it changed: {settingsBefore} to {settingsAfter}"));
            Console.WriteLine(PartResult.Summary(_parts));
            return _parts.All(p => p.Status == PartStatus.Pass) ? 0 : 1;
        }

        string? PreflightBlock()
        {
            var script = Path.Combine(RepoRoot(), "scripts", "windows", "preflight.ps1");
            var info = new ProcessStartInfo("powershell") { RedirectStandardOutput = true, RedirectStandardError = true, UseShellExecute = false, CreateNoWindow = true };
            foreach (var arg in new[] { "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", script, "-Level", "window" }) info.ArgumentList.Add(arg);
            using var process = Process.Start(info) ?? throw new InvalidOperationException("HARNESS: could not start powershell");
            var error = process.StandardError.ReadToEndAsync();
            var output = process.StandardOutput.ReadToEnd();
            process.WaitForExit();
            var reason = Preflight.BlockReason(output, options.IgnoreDisk) ?? Preflight.DialogReason(
                Preflight.KnownDialogs.SelectMany(Process.GetProcessesByName).Where(p => p.MainWindowHandle != 0).Select(p => p.ProcessName));
            if (reason is null && output.Contains("under 40 GB free", StringComparison.Ordinal))
                Console.WriteLine("Note: under 40 GB free on C:. --ignore-disk let this proof run go on, and the window level never passes that switch.");
            return reason is null ? null : reason + (error.Result.Length == 0 ? "" : $" {error.Result.Trim()}");
        }

        /// A fingerprint of the folders a real run would use. PLAN 4.3: a test must not touch them.
        static string RealSettings()
        {
            var text = new StringBuilder();
            foreach (var root in new[] { Environment.SpecialFolder.ApplicationData, Environment.SpecialFolder.LocalApplicationData })
            {
                var folder = Path.Combine(Environment.GetFolderPath(root), "OpenPainter");
                if (!Directory.Exists(folder))
                {
                    text.Append($"{folder}: none; ");
                    continue;
                }
                var files = Directory.GetFiles(folder, "*", SearchOption.AllDirectories);
                text.Append($"{folder}: {files.Length} files, newest {files.Select(File.GetLastWriteTimeUtc).DefaultIfEmpty().Max():O}; ");
            }
            return text.ToString();
        }

        void CheckDataRootEmpty(AppRun run, string name)
        {
            var left = Directory.GetFileSystemEntries(run.DataRoot);
            if (left.Length != 0) Console.WriteLine($"Note: {name}: the app wrote {left.Length} items to its temp data root");
        }

        // ---- The checks --test-report already does, for both themes.
        void ReportPart(string theme)
        {
            const int RunMs = 2000, LimitMs = 240_000;
            var folder = NextFolder($"report-{theme}");
            var reportPath = Path.Combine(folder, "report.json");
            var png = Path.Combine(RepoRoot(), "tests", "fixtures", "plasma_alpha_512x384.png");
            using var run = new AppRun(options.App, folder, ["--test-load-png", png, "--theme", theme, "--test-report", reportPath, "--exit-after-ms", RunMs.ToString()], selfEnd: false);
            var suffix = $" ({theme})";
            if (!run.WaitExit(LimitMs))
            {
                Report(PartResult.Fail($"report{suffix}", $"the app did not end in {LimitMs} ms"));
                return;
            }
            if (!File.Exists(reportPath))
            {
                Report(PartResult.Fail($"report{suffix}", $"the app exited {run.ExitCode} and wrote no report: {run.Output()}"));
                return;
            }
            foreach (var part in ReportJudge.Parts(File.ReadAllText(reportPath), run.ExitCode, suffix)) Report(part);
            CheckDataRootEmpty(run, "report");
        }

        // ---- Done-test 2: real pixels, and how long the first real frame takes.
        void PixelsPart()
        {
            const int FirstFrameWidth = 2400, FirstFrameHeight = 1400;
            var images = Path.Combine(options.Out, "images");
            Directory.CreateDirectory(images);
            var small = Path.Combine(images, "cells-small.png");
            TestImages.WriteCells(small, FirstFrameWidth, FirstFrameHeight);
            var big = Big24Mp();

            var problems = new List<string>();
            var lines = new List<string>();
            var screens = new List<ScreenRead>();
            var launches = new List<LaunchRead>();
            for (var i = 0; i < options.FirstFrameRuns; i++) ReadBack($"run {i + 1}", small, "1", "light", problems, lines, screens, launches);
            ReadBack("dark run", small, "1", "dark", problems, lines, screens, launches);
            ReadBack("24 MP at 100%", big, "1", "light", problems, lines, screens, launches);
            ReadBack("24 MP at Fit", big, null, "light", problems, lines, screens, launches);
            ReadBack("24 MP dark", big, "1", "dark", problems, lines, screens, launches);
            foreach (var line in lines) Console.WriteLine($"  {line}");
            Report(problems.Count == 0
                ? PartResult.Pass("pixel read-back", $"{screens.Count} runs, real pixels matched the image in every capture after the signal")
                : PartResult.Fail("pixel read-back", string.Join("; ", problems)));
            Report(ScreenCheck.Part(screens));
            Report(LaunchPaint.Part(launches));
        }

        string Big24Mp()
        {
            var images = Path.Combine(options.Out, "images");
            Directory.CreateDirectory(images);
            var path = Path.Combine(images, Png24Mp);
            if (!File.Exists(path)) TestImages.WriteCells(path, TestImages.Width24Mp, TestImages.Height24Mp);
            return path;
        }

        /// Reads the real screen in the canvas's area from launch, so anything but the stage
        /// color before the first frame is measured. Any capture after the signal must show the image.
        void ReadBack(string label, string png, string? zoom, string theme, List<string> problems, List<string> lines, List<ScreenRead> screens, List<LaunchRead> launches)
        {
            const int PollMs = 4;
            const int AfterSignalCaptures = 5;
            const int WindowLimitMs = 60_000;
            const int ScreenPointsWanted = 8;
            var folder = NextFolder($"pixels-{label.Replace(' ', '-').Replace("%", "")}");
            var args = new List<string> { "--test-load-png", png, "--theme", theme };
            if (zoom is not null) args.AddRange(["--test-zoom", zoom]);
            var firstFramePath = Path.Combine(folder, "first-frame.json");
            args.AddRange(["--test-first-frame", firstFramePath]);
            using var run = new AppRun(options.App, folder, args);
            var main = run.WaitMainWindow(WindowLimitMs);
            var windowMs = run.MsSinceStart;
            Win32.RaiseWithoutFocus(main);
            double? firstCanvas = null;
            var canvasHandles = new HashSet<nint>();
            var pixels = new List<LaunchPixel>();
            while (!File.Exists(firstFramePath))
            {
                if (run.Exited) throw new InvalidOperationException($"{label}: the app exited {run.ExitCode} before its first frame: {run.Output()}");
                if (run.MsSinceStart > StartLimitMs) throw new TimeoutException($"{label}: no first-frame signal in {StartLimitMs} ms");
                var canvas = Win32.CanvasOf(main);
                if (canvas != 0)
                {
                    canvasHandles.Add(canvas);
                    firstCanvas ??= run.MsSinceStart;
                }
                // Points the stage holds whatever the panels do, read from the window's own place.
                if (Win32.GetWindowRect(main, out var mainRect))
                {
                    foreach (var (x, y) in new[] { (1, 2), (2, 2), (1, 3) })
                    {
                        if (Win32.ScreenPixelInApp(main, mainRect.Left + mainRect.Width * x / 3 + 40, mainRect.Top + mainRect.Height * y / 4, out _) is { } seen)
                            pixels.Add(new LaunchPixel(run.MsSinceStart, seen));
                    }
                }
                Thread.Sleep(PollMs);
            }
            var frame = FirstFrame.Read(firstFramePath);
            if (frame.FrameErrors != 0) problems.Add($"{label}: {frame.FrameErrors} frame errors, last {frame.LastError}");
            if (frame.ValidationErrors != 0) problems.Add($"{label}: {frame.ValidationErrors} validation errors");
            launches.Add(new LaunchRead(label, frame.Stage, frame.FirstFrameMs, pixels));
            File.WriteAllLines(Path.Combine(folder, "launch-pixels.txt"), pixels.Select(p => $"{p.Ms:F0} {p.Color}"));

            Win32.WaitForDisplay();
            var margin = EdgeMarginDevicePixels / (frame.Map.Zoom * frame.Map.Scale);
            var failedAfterSignal = 0;
            string? firstProblem = null;
            for (var i = 0; i < AfterSignalCaptures; i++)
            {
                var picture = Win32.Capture(main);
                var rect = Win32.CanvasRect(main, frame.Canvas);
                var result = PixelCheck.Judge(picture, rect, frame.Map, (x, y) => TestImages.CellColorInside(x, y, margin), ColorTolerance, columns: 64, rows: 40);
                if (result.Problem() is { } problem)
                {
                    failedAfterSignal++;
                    firstProblem ??= problem;
                    Png.Write(Path.Combine(folder, $"bad-capture-{i}.png"), picture);
                }
                if (i == 0 && firstProblem is null) lines.Add($"{label}: {result.Matched} of {result.Samples} points match at zoom {frame.Map.Zoom:0.###}");
                Thread.Sleep(PollMs);
            }
            // The same image on the real screen, at points away from color edges. A point another
            // program covers does not count. One more round after raising the app gets past a
            // window that came up on top of it.
            if (!Win32.GetWindowRect(frame.Canvas, out var shown)) throw new InvalidOperationException($"{label}: GetWindowRect of the canvas failed");
            var candidates = PixelCheck.ScreenPoints(shown.Width, shown.Height, frame.Map, (x, y) => TestImages.CellColorInside(x, y, margin));
            int judged = 0, matched = 0, covered = 0;
            string? miss = null;
            string? coverName = null;
            for (var round = 0; round < 2 && judged == 0; round++)
            {
                Win32.RaiseWithoutFocus(main);
                Win32.WaitForDisplay();
                covered = 0;
                foreach (var (x, y, want) in candidates)
                {
                    if (judged >= ScreenPointsWanted) break;
                    if (Win32.ScreenPixelOf(frame.Canvas, shown.Left + x, shown.Top + y, out var by) is not { } seen)
                    {
                        covered++;
                        coverName = by;
                        continue;
                    }
                    judged++;
                    if (seen.Distance(want) <= ColorTolerance) matched++;
                    else miss ??= $"{seen} at {x},{y}, expected {want}";
                }
            }
            screens.Add(new ScreenRead(label, judged, matched, covered, miss));
            var screenWords = $"{judged} points judged of {candidates.Count} away from color edges, {matched} matched, {covered} covered{(coverName is null ? "" : $" by class {coverName}")}";
            var early = pixels.Count(p => p.Ms < frame.FirstFrameMs - LaunchPaint.FrameClockSlackMs);
            lines.Add($"{label}: window {windowMs:F0} ms, canvas window {firstCanvas?.ToString("F0") ?? "before the first poll"} ms, view attached {frame.AttachedMs:F0} ms, first frame {frame.FirstFrameMs:F0} ms, signal {frame.SignalMs:F0} ms; before the signal {pixels.Count} screen pixels, {early} before the first frame, {pixels.Count(p => p.Color.IsBlack)} black, {pixels.Count(p => p.Color.Distance(frame.Stage) <= 1)} the stage color; {canvasHandles.Count} canvas handles; screen after the signal: {screenWords}");
            if (failedAfterSignal != 0) problems.Add($"{label}: {failedAfterSignal} of {AfterSignalCaptures} captures after the signal failed: {firstProblem}");
            run.Close(main);
            if (run.ExitCode != 0) problems.Add($"{label}: the app exited {run.ExitCode}: {run.Output()}");
            CheckDataRootEmpty(run, label);
        }

        // ---- Done-test 3: the color check on the live window.
        void ColorPart(string theme)
        {
            const int LimitMs = 60_000;
            var images = Path.Combine(options.Out, "images");
            Directory.CreateDirectory(images);
            var png = Path.Combine(images, "patches.png");
            if (!File.Exists(png)) TestImages.WritePatches(png);
            var name = $"color check ({theme})";
            var folder = NextFolder($"color-{theme}");
            using var real = new AppRun(options.App, folder, ["--test-load-png", png, "--theme", theme, "--test-zoom", "1", "--test-first-frame", Path.Combine(folder, "first-frame.json")]);
            var frame = real.WaitFirstFrame(LimitMs);
            var profile = Win32.DisplayProfile(frame.Canvas);
            var profileName = Path.GetFileName(profile);
            var isSrgb = profileName.Contains("sRGB", StringComparison.OrdinalIgnoreCase);
            Win32.WaitForDisplay();
            var picture = Win32.Capture(frame.Main);
            var rect = Win32.CanvasRect(frame.Main, frame.Canvas);
            var misses = new List<string>();
            var checkedPatches = 0;
            for (var i = 0; i < TestImages.Patches.Length; i++)
            {
                var (patch, color) = TestImages.Patches[i];
                var docX = i % TestImages.PatchColumns * TestImages.PatchSize + TestImages.PatchSize / 2.0;
                var docY = i / TestImages.PatchColumns * TestImages.PatchSize + TestImages.PatchSize / 2.0;
                var (x, y) = frame.Map.DevicePoint(docX, docY);
                var px = rect.X + (int)x;
                var py = rect.Y + (int)y;
                if (px < rect.X || py < rect.Y || px >= rect.X + rect.Width || py >= rect.Y + rect.Height)
                {
                    misses.Add($"{patch} is outside the canvas");
                    continue;
                }
                checkedPatches++;
                var got = picture.At(px, py);
                if (got.Distance(color) > ColorTolerance) misses.Add($"{patch} shows {got}, expected {color}");
            }
            // The stage around the image. Avalonia paints the same color next to the canvas, so a seam would show.
            var stageShown = new Rgb();
            var stageFound = false;
            for (var x = 2; x < rect.Width - 2 && !stageFound; x += 8)
            {
                var (docX, docY) = frame.Map.DocumentPoint(x, 2);
                if (frame.Map.OnDocument(docX, docY)) continue;
                stageShown = picture.At(rect.X + x, rect.Y + 2);
                stageFound = true;
            }
            if (!stageFound) misses.Add("no stage showed around the image");
            else if (stageShown.Distance(frame.Stage) > ColorTolerance) misses.Add($"the stage shows {stageShown}, expected {frame.Stage}");
            var facts = $"{checkedPatches} patches, display profile {profileName}, table in use {frame.HasDisplayTable}";
            if (!isSrgb && misses.Count != 0)
                Report(PartResult.NotRun(name, $"the display profile {profileName} is not sRGB, and this check has expected values only for sRGB; {string.Join("; ", misses.Take(3))}"));
            else if (misses.Count != 0)
            {
                Png.Write(Path.Combine(options.Out, $"color-{theme}-bad.png"), picture);
                Report(PartResult.Fail(name, $"{facts}: {string.Join("; ", misses)}"));
            }
            else Report(PartResult.Pass(name, facts));
            real.Close(frame.Main);
            if (real.ExitCode != 0) Report(PartResult.Fail($"{name} exit", $"the app exited {real.ExitCode}: {real.Output()}"));
            CheckDataRootEmpty(real, name);
        }

        // ---- One window-only shot per screen, for root to look at.
        void ShotsPart()
        {
            const int LimitMs = 60_000, PopupLimitMs = 8000, SettleMs = 500;
            var png = Path.Combine(RepoRoot(), "tests", "fixtures", "plasma_alpha_512x384.png");
            var saved = new List<string>();
            var failures = new List<string>();
            var scales = new HashSet<double>();
            var shots = Path.Combine(options.Out, "shots");
            Directory.CreateDirectory(shots);
            (string Name, string[] Args, bool PopupWindow)[] screens =
            [
                ("main", [], false),
                ("menu-file", ["--test-open-menu", "File"], true),
                ("more", ["--test-open-more"], true),
            ];
            foreach (var theme in new[] { "light", "dark" })
            {
                foreach (var (screen, extra, popup) in screens)
                {
                    var folder = NextFolder($"shot-{screen}-{theme}");
                    var firstFrame = Path.Combine(folder, "first-frame.json");
                    using var run = new AppRun(options.App, folder, new[] { "--test-load-png", png, "--theme", theme, "--test-first-frame", firstFrame }.Concat(extra));
                    var frame = run.WaitFirstFrame(LimitMs);
                    scales.Add(frame.Map.Scale);
                    Win32.WaitForDisplay();
                    // The panels appear with the window. A menu or popup adds one more of the app's windows.
                    var clock = Stopwatch.StartNew();
                    var baseWindows = 1 + 4;
                    while (popup && Win32.WindowsOf(run.Pid).Count <= baseWindows && clock.ElapsedMilliseconds < PopupLimitMs) Thread.Sleep(20);
                    if (popup && Win32.WindowsOf(run.Pid).Count <= baseWindows) failures.Add($"{screen} {theme}: no popup window opened in {PopupLimitMs} ms");
                    Thread.Sleep(SettleMs);
                    var picture = Win32.CaptureApp(run.Pid);
                    var path = Path.Combine(shots, $"{screen}-{theme}.png");
                    Png.Write(path, picture);
                    saved.Add(path);
                    var canvas = Win32.Capture(frame.Main);
                    if (PixelCheck.IsBlack(canvas, Win32.CanvasRect(frame.Main, frame.Canvas))) failures.Add($"{screen} {theme}: the canvas is black");
                    run.Close(frame.Main);
                    if (run.ExitCode != 0) failures.Add($"{screen} {theme}: the app exited {run.ExitCode}: {run.Output()}");
                    CheckDataRootEmpty(run, $"{screen} {theme}");
                }
            }
            Report(failures.Count == 0
                ? PartResult.Pass("window shots", $"{saved.Count} saved in {shots}; root must look at each")
                : PartResult.Fail("window shots", string.Join("; ", failures)));
            foreach (var row in ShotScales.Rows(scales)) Report(row);
        }

        // ---- Done-test 1: the 24 MP pan trace.
        void PanPart()
        {
            const int BusySampleMs = 3000, SpareMs = 180_000, WindowLimitMs = 60_000, SettleMs = 1000;
            const int ScreenEveryMs = 250, CpuEveryMs = 1000, GpuEveryMs = 5000, PollMs = 20;
            const string Name = "pan trace";
            var cpuBefore = Win32.CpuBusyPercent(BusySampleMs);
            var gpuBefore = ReadGpu();
            var (programNotes, busyPrograms) = WatchedPrograms(gpuBefore);
            var reasons = BusyJudge.Before(cpuBefore, gpuBefore.UsePercent, busyPrograms);
            var proofReasons = new List<string>();
            if (Math.Abs(options.PanSeconds - Options.FullPanSeconds) > .001) proofReasons.Add($"{options.PanSeconds:0.#} s, not {Options.FullPanSeconds:0} s");
            if (reasons.Count != 0 && !options.AllowBusy)
            {
                Report(PartResult.NotRun(Name, $"a busy run does not count (PLAN 4.3), and this PC is busy: {string.Join("; ", reasons)}; pass --allow-busy for a proof run"));
                return;
            }
            proofReasons.AddRange(reasons);
            if (!gpuBefore.Name.Contains("RTX", StringComparison.Ordinal)) proofReasons.Add($"nvidia-smi names no RTX: {gpuBefore}");

            var big = Big24Mp();
            var folder = NextFolder("pan");
            var path = Path.Combine(folder, "pan.json");
            var signal = Path.Combine(folder, "first-frame.json");
            using var run = new AppRun(options.App, folder, ["--test-load-png", big, "--theme", "light", "--test-zoom", "1", "--test-first-frame", signal, "--test-pan-trace", path, "--test-pan-seconds", options.PanSeconds.ToString(CultureInfo.InvariantCulture)]);
            var main = run.WaitMainWindow(WindowLimitMs);
            var refresh = Win32.RefreshRate(main);
            var limit = (int)(options.PanSeconds * 1000) + SpareMs;
            var clock = Stopwatch.StartNew();
            while (!File.Exists(signal))
            {
                if (run.Exited || clock.ElapsedMilliseconds > limit)
                {
                    Report(PartResult.Fail(Name, $"no first-frame signal: exited {run.Exited}: {run.Output()}"));
                    return;
                }
                Thread.Sleep(PollMs);
            }
            Thread.Sleep(SettleMs);

            // The trace runs now. These reads cost the app nothing: they only look from outside.
            var canvas = Win32.CanvasOf(main);
            if (canvas == 0 || !Win32.GetWindowRect(canvas, out var shown)) throw new InvalidOperationException("the canvas window is gone");
            var cpuOthers = new List<double>();
            var gpuDuring = new List<double?>();
            var seen = new List<Rgb?>();
            var lastCpu = Win32.CpuTimes();
            var lastApp = run.CpuTime;
            var lastCpuAt = clock.ElapsedMilliseconds;
            long lastGpuAt = clock.ElapsedMilliseconds, lastScreenAt = 0;
            while (!File.Exists(path))
            {
                if (run.Exited || clock.ElapsedMilliseconds > limit)
                {
                    Report(PartResult.Fail(Name, run.Exited ? $"the app exited {run.ExitCode} and wrote no frame times: {run.Output()}" : $"the app wrote no frame times in {limit} ms"));
                    return;
                }
                var now = clock.ElapsedMilliseconds;
                if (now - lastScreenAt >= ScreenEveryMs)
                {
                    lastScreenAt = now;
                    seen.Add(Win32.ScreenPixelOf(canvas, shown.Left + shown.Width / 2, shown.Top + shown.Height / 2, out _));
                }
                if (now - lastCpuAt >= CpuEveryMs)
                {
                    var cpu = Win32.CpuTimes();
                    var app = run.CpuTime;
                    var machineShare = cpu.Total <= lastCpu.Total ? 0 : 100.0 * (cpu.Busy - lastCpu.Busy) / (cpu.Total - lastCpu.Total);
                    var appShare = 100.0 * (app - lastApp).TotalMilliseconds / ((now - lastCpuAt) * Environment.ProcessorCount);
                    cpuOthers.Add(Math.Max(0, machineShare - appShare));
                    (lastCpu, lastApp, lastCpuAt) = (cpu, app, now);
                }
                if (now - lastGpuAt >= GpuEveryMs)
                {
                    lastGpuAt = now;
                    gpuDuring.Add(ReadGpu().UsePercent);
                }
                Thread.Sleep(PollMs);
            }
            proofReasons.AddRange(BusyJudge.During(cpuOthers, gpuDuring));

            using var file = JsonDocument.Parse(File.ReadAllText(path));
            var root = file.RootElement;
            var frame = FirstFrame.From(root);
            var times = root.GetProperty("frame_times_ms").EnumerateArray().Select(t => t.GetDouble()).ToArray();
            var stats = FrameStats.From(times);
            var moves = root.GetProperty("moves_posted").GetInt64();
            var changes = root.GetProperty("view_changes").GetInt64();
            var errors = root.GetProperty("frame_errors_during").GetInt64();
            var drawable = $"{frame.DrawableWidth}x{frame.DrawableHeight}";
            var zoom = frame.Map.Zoom;
            var startGap = root.GetProperty("start_gap_ms").GetDouble();
            var endGap = root.GetProperty("end_gap_ms").GetDouble();
            var samples = root.GetProperty("view_samples").EnumerateArray().Select(a =>
            {
                var v = a.EnumerateArray().Select(n => n.GetDouble()).ToArray();
                return new PanSample(v[0], (int)v[1], (int)v[2], v[3], v[4], (long)v[5]);
            }).ToList();
            var gpuRead = gpuDuring.Where(g => g is not null).Select(g => g!.Value).ToList();
            var busyWords = $"PC {cpuBefore:F0}% busy before, others {(cpuOthers.Count == 0 ? "not read" : $"{cpuOthers.Average():F0}%")} during; GPU {gpuBefore} before, {(gpuRead.Count == 0 ? "not read" : $"{gpuRead.Average():F0}%")} during{(programNotes.Count == 0 ? "" : $"; {string.Join("; ", programNotes)}")}";
            var facts = $"{stats}, first frame {startGap:F1} ms after the start, last frame {endGap:F1} ms before the end; {moves} moves posted, {changes} view changes, {samples.Count} view samples, canvas {drawable} at zoom {zoom:0.###}, display {refresh} Hz; {busyWords}";

            // A fault in what the trace did is a fault on any PC, so these never become NOT RUN.
            var faults = new List<string>();
            if (errors != 0) faults.Add($"{errors} frame errors");
            if (frame.ValidationErrors != 0) faults.Add($"{frame.ValidationErrors} validation errors");
            if (zoom != 1) faults.Add($"zoom is {zoom}, not 1");
            if (PanMotion.Problem(samples, zoom, frame.Map.Scale) is { } motion) faults.Add(motion);
            if (PanMotion.ScreenProblem(seen, out var screenJudged) is { } still) faults.Add(still);
            if (!screenJudged) proofReasons.Add($"another program covered the canvas's middle, so only {seen.Count(c => c is not null)} of {seen.Count} screen reads during the trace counted");
            // The last view must be what the canvas shows: real pixels, read after the trace.
            Win32.WaitForDisplay();
            var margin = EdgeMarginDevicePixels / (frame.Map.Zoom * frame.Map.Scale);
            var picture = Win32.Capture(main);
            var endPixels = PixelCheck.Judge(picture, Win32.CanvasRect(main, frame.Canvas), frame.Map, (x, y) => TestImages.CellColorInside(x, y, margin), ColorTolerance, columns: 64, rows: 40);
            if (endPixels.Problem() is { } pixelsProblem)
            {
                faults.Add($"after the trace the canvas does not show the view: {pixelsProblem}");
                Png.Write(Path.Combine(folder, "bad-end-capture.png"), picture);
            }
            run.Close(main);
            if (run.ExitCode != 0) faults.Add($"the app exited {run.ExitCode}: {run.Output()}");
            CheckDataRootEmpty(run, Name);
            facts = $"{facts}; {endPixels.Matched} of {endPixels.Samples} real pixels matched after the trace; screen color read {seen.Count(c => c is not null)} times during it";

            var speed = stats.Problem(options.PanSeconds, startGap, endGap);
            if (changes < moves / 2) speed = $"the UI thread handled only {changes} of {moves} moves; {speed}";
            if (refresh < 59 && speed is not null) proofReasons.Add($"the display runs at {refresh} Hz, under the 59 fps asked");
            if (faults.Count != 0) Report(PartResult.Fail(Name, $"{string.Join("; ", faults)}; {facts}"));
            else if (proofReasons.Count != 0) Report(PartResult.NotRun(Name, $"proof run only, these numbers do not count ({string.Join("; ", proofReasons)}): {speed ?? "met the limits"}; {facts}"));
            else if (speed is not null) Report(PartResult.Fail(Name, $"{speed}; {facts}"));
            else Report(PartResult.Pass(Name, facts));
        }

        static GpuFacts ReadGpu()
        {
            try
            {
                var info = new ProcessStartInfo("nvidia-smi", "--query-gpu=name,utilization.gpu,encoder.stats.sessionCount --format=csv,noheader,nounits") { RedirectStandardOutput = true, UseShellExecute = false, CreateNoWindow = true };
                using var process = Process.Start(info) ?? throw new InvalidOperationException("no process");
                var text = process.StandardOutput.ReadToEnd();
                process.WaitForExit();
                return process.ExitCode == 0 ? GpuFacts.Parse(text) : new GpuFacts($"nvidia-smi exited {process.ExitCode}", null, null);
            }
            catch (System.ComponentModel.Win32Exception e)
            {
                return new GpuFacts($"nvidia-smi could not start: {e.Message}", null, null);
            }
        }

        /// Sunshine and LM Studio, as scripts/windows/preflight.ps1 looks at them: a stream
        /// holds an encoder session, and a model loaded in LM Studio holds the GPU's memory.
        static (List<string> Notes, List<string> Busy) WatchedPrograms(GpuFacts gpu)
        {
            var notes = new List<string>();
            var busy = new List<string>();
            void Add((string? Note, string? Busy) words)
            {
                if (words.Note is not null) notes.Add(words.Note);
                if (words.Busy is not null) busy.Add(words.Busy);
            }
            static bool Running(params string[] names) => names.SelectMany(Process.GetProcessesByName).Any();
            Add(BusyJudge.Program("Sunshine", Running("sunshine"), gpu.EncoderSessions, "encoder sessions"));
            var lmStudio = Running("LM Studio", "LMStudio");
            Add(BusyJudge.Program("LM Studio", lmStudio, lmStudio ? LoadedModels() : null, "models loaded"));
            return (notes, busy);
        }

        /// Asks only LM Studio's own local server, because its `lms` command would wake it.
        static int? LoadedModels()
        {
            const int LimitSeconds = 3;
            try
            {
                using var client = new HttpClient { Timeout = TimeSpan.FromSeconds(LimitSeconds) };
                using var body = JsonDocument.Parse(client.GetStringAsync("http://127.0.0.1:1234/api/v0/models").GetAwaiter().GetResult());
                return body.RootElement.GetProperty("data").EnumerateArray().Count(m => m.GetProperty("state").GetString() == "loaded");
            }
            catch (Exception e) when (e is HttpRequestException or TaskCanceledException or JsonException or KeyNotFoundException)
            {
                Console.WriteLine($"Note: LM Studio's server did not answer: {e.Message}");
                return null;
            }
        }

        // ---- The app ends when its main window closes, whatever the panels are doing.
        void ExitPart()
        {
            const string Name = "app exit";
            const int LimitMs = 60_000, ChangeLimitMs = 8000, ExitLimitMs = 15_000, VkF8 = 0x77;
            var png = Path.Combine(RepoRoot(), "tests", "fixtures", "plasma_alpha_512x384.png");
            var problems = new List<string>();
            var words = new List<string>();

            (string State, Action<AppRun, FirstFrame, List<nint>> Arrange)[] states =
            [
                ("all panels shown", (_, _, _) => { }),
                ("one panel hidden", (run, frame, panels) => { Win32.AskToClose(panels[0]); Wait(run, frame, 3); }),
                ("all four hidden", (run, frame, panels) => { foreach (var panel in panels) Win32.AskToClose(panel); Wait(run, frame, 0); }),
                ("one hidden, then shown again", (run, frame, _) =>
                {
                    Win32.PostKey(frame.Canvas, VkF8);
                    Wait(run, frame, 3);
                    Win32.PostKey(frame.Canvas, VkF8);
                    Wait(run, frame, 4);
                }),
                ("one hidden, then minimized", (_, frame, panels) => { Win32.AskToClose(panels[0]); Thread.Sleep(500); Win32.Minimize(frame.Main); Thread.Sleep(500); }),
            ];
            void Wait(AppRun run, FirstFrame frame, int panels)
            {
                var clock = Stopwatch.StartNew();
                while (Win32.WindowsOf(run.Pid).Count(h => h != frame.Main) != panels)
                {
                    if (clock.ElapsedMilliseconds > ChangeLimitMs) throw new TimeoutException($"{Win32.WindowsOf(run.Pid).Count - 1} panels show, expected {panels}");
                    Thread.Sleep(20);
                }
            }
            foreach (var (state, arrange) in states)
            {
                var folder = NextFolder($"exit-{state.Replace(' ', '-').Replace(",", "")}");
                using var run = new AppRun(options.App, folder, ["--test-load-png", png, "--theme", "light", "--test-first-frame", Path.Combine(folder, "first-frame.json")]);
                try
                {
                    var frame = run.WaitFirstFrame(LimitMs);
                    Wait(run, frame, 4);
                    var panels = Win32.WindowsOf(run.Pid).Where(h => h != frame.Main).ToList();
                    arrange(run, frame, panels);
                    var clock = Stopwatch.StartNew();
                    Win32.AskToClose(frame.Main);
                    if (!run.WaitExit(ExitLimitMs))
                    {
                        problems.Add($"{state}: the process was still running {ExitLimitMs} ms after the main window closed, with {Win32.WindowsOf(run.Pid).Count} visible windows");
                        continue;
                    }
                    if (run.ExitCode != 0) problems.Add($"{state}: the app exited {run.ExitCode}: {run.Output()}");
                    else words.Add($"{state} {clock.ElapsedMilliseconds} ms");
                }
                catch (TimeoutException e)
                {
                    problems.Add($"{state}: {e.Message}");
                }
                CheckDataRootEmpty(run, Name);
            }
            Report(problems.Count == 0
                ? PartResult.Pass(Name, $"{states.Length} states, each exit code 0: {string.Join(", ", words)}")
                : PartResult.Fail(Name, string.Join("; ", problems)));
        }

        // ---- An exception on the way through a window message must reach the fatal path.
        void FatalPart()
        {
            const string Name = "fatal path";
            const int LimitMs = 60_000, ExitLimitMs = 20_000, Asked = 900, MoveBy = 10;
            var png = Path.Combine(RepoRoot(), "tests", "fixtures", "plasma_alpha_512x384.png");
            var problems = new List<string>();
            var words = new List<string>();
            foreach (var place in new[] { "closed", "size-limit", "panel-size-limit", "moved", "panel-moved" })
            {
                var folder = NextFolder($"fatal-{place}");
                using var run = new AppRun(options.App, folder, ["--test-load-png", png, "--theme", "light", "--test-first-frame", Path.Combine(folder, "first-frame.json"), "--test-throw", place]);
                // A size limit and a move both run at launch, so the throw may come before the first frame.
                var clock = Stopwatch.StartNew();
                while (!run.Exited && !File.Exists(run.FirstFramePath) && clock.ElapsedMilliseconds < LimitMs) Thread.Sleep(10);
                if (!run.Exited && File.Exists(run.FirstFramePath))
                {
                    var frame = FirstFrame.Read(run.FirstFramePath);
                    var panel = Win32.WindowsOf(run.Pid).First(h => h != frame.Main);
                    switch (place)
                    {
                        case "closed": Win32.AskToClose(frame.Main); break;
                        case "size-limit": Win32.Resize(frame.Main, Asked + 400, Asked); break;
                        case "panel-size-limit": Win32.Resize(panel, Asked / 2, Asked / 2); break;
                        case "moved": Win32.MoveBy(frame.Main, MoveBy); break;
                        default: Win32.MoveBy(panel, MoveBy); break;
                    }
                }
                var exited = run.WaitExit(ExitLimitMs);
                if (!exited) problems.Add($"{place}: the app was still running {ExitLimitMs} ms later");
                else if (run.ExitCode != 1) problems.Add($"{place}: the app exited {run.ExitCode}, not 1: {run.Output()}");
                else if (!run.Output().Contains("TEST_THROW", StringComparison.Ordinal)) problems.Add($"{place}: exit code 1 with no message: {run.Output()}");
                else words.Add(place);
            }
            Report(problems.Count == 0
                ? PartResult.Pass(Name, $"exit code 1 and the message, from: {string.Join(", ", words)}")
                : PartResult.Fail(Name, string.Join("; ", problems)));
        }

        // ---- A work area smaller than the Mac's minimum: the window must still fit.
        void SmallScreenPart()
        {
            const string Name = "small screen";
            const int LimitMs = 60_000, Tiny = 200, SettleMs = 500;
            const int WorkWidth = 1000, WorkHeight = 620;
            var png = Path.Combine(RepoRoot(), "tests", "fixtures", "plasma_alpha_512x384.png");
            var problems = new List<string>();
            var folder = NextFolder("small-screen");
            var path = Path.Combine(folder, "first-frame.json");
            using var run = new AppRun(options.App, folder, ["--test-load-png", png, "--theme", "light", "--test-first-frame", path, "--test-work-area", $"{WorkWidth}x{WorkHeight}"]);
            var frame = run.WaitFirstFrame(LimitMs);
            Win32.WaitForDisplay();
            void Measure(string when)
            {
                if (!Win32.GetWindowRect(frame.Main, out var r)) throw new InvalidOperationException("GetWindowRect of the main window failed");
                if (r.Width > WorkWidth || r.Height > WorkHeight) problems.Add($"{when}: the window is {r.Width} by {r.Height}, over the {WorkWidth} by {WorkHeight} work area");
            }
            Measure("at launch");
            double statusBottom, clientHeight;
            using (var facts = JsonDocument.Parse(File.ReadAllText(path)))
            {
                statusBottom = facts.RootElement.GetProperty("status_bottom").GetDouble();
                clientHeight = facts.RootElement.GetProperty("client_height").GetDouble();
            }
            if (statusBottom > clientHeight + .5) problems.Add($"at launch: the status bar ends at {statusBottom:F0}, under the client's {clientHeight:F0}");
            Win32.Resize(frame.Main, Tiny, Tiny);
            Thread.Sleep(SettleMs);
            Measure("asked for a tiny window");
            if (Win32.GetWindowRect(frame.Main, out var least) && (least.Width <= Tiny || least.Height <= Tiny)) problems.Add($"asked for a tiny window: it became {least.Width} by {least.Height}, so no limit held");
            var shots = Path.Combine(options.Out, "shots");
            Directory.CreateDirectory(shots);
            Png.Write(Path.Combine(shots, "small-screen-light.png"), Win32.CaptureApp(run.Pid));
            run.Close(frame.Main);
            if (run.ExitCode != 0) problems.Add($"the app exited {run.ExitCode}: {run.Output()}");
            CheckDataRootEmpty(run, Name);
            Report(problems.Count == 0
                ? PartResult.Pass(Name, $"window fits a {WorkWidth} by {WorkHeight} work area at launch and when asked for less; the status bar ends at {statusBottom:F0} of {clientHeight:F0}")
                : PartResult.Fail(Name, string.Join("; ", problems)));
        }
    }
}
