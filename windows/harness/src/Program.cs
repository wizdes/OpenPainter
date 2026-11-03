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
    const string Usage = "usage: OpenPainter.Harness --app <OpenPainter.exe> --out <folder> [--parts report,pixels,color,shots,pan] [--pan-seconds <s>] [--first-frame-runs <n>] [--allow-busy] [--ignore-disk]\n       OpenPainter.Harness --export-vectors";
    static readonly string[] AllParts = ["report", "pixels", "color", "shots", "pan"];

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
        public const double FullPanSeconds = 30;

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
                        if (!double.TryParse(text, NumberStyles.Float, CultureInfo.InvariantCulture, out seconds) || seconds <= 0) throw new ArgumentException($"HARNESS: --pan-seconds: {text}");
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
            for (var i = 0; i < options.FirstFrameRuns; i++) ReadBack($"run {i + 1}", small, "1", problems, lines);
            ReadBack("24 MP at 100%", big, "1", problems, lines);
            ReadBack("24 MP at Fit", big, null, problems, lines);
            foreach (var line in lines) Console.WriteLine($"  {line}");
            Report(problems.Count == 0
                ? PartResult.Pass("pixel read-back", $"{options.FirstFrameRuns + 2} runs, real pixels matched the image in every capture after the signal")
                : PartResult.Fail("pixel read-back", string.Join("; ", problems)));
        }

        string Big24Mp()
        {
            var images = Path.Combine(options.Out, "images");
            Directory.CreateDirectory(images);
            var path = Path.Combine(images, Png24Mp);
            if (!File.Exists(path)) TestImages.WriteCells(path, TestImages.Width24Mp, TestImages.Height24Mp);
            return path;
        }

        /// Captures the canvas over and over from launch, so a black window before the signal
        /// is measured, and any capture after the signal must show the image.
        void ReadBack(string label, string png, string? zoom, List<string> problems, List<string> lines)
        {
            const int PollMs = 40;
            const int AfterSignalCaptures = 5;
            const int WindowLimitMs = 60_000;
            var folder = NextFolder($"pixels-{label.Replace(' ', '-').Replace("%", "")}");
            var args = new List<string> { "--test-load-png", png, "--theme", "light" };
            if (zoom is not null) args.AddRange(["--test-zoom", zoom]);
            var firstFramePath = Path.Combine(folder, "first-frame.json");
            args.AddRange(["--test-first-frame", firstFramePath]);
            using var run = new AppRun(options.App, folder, args);
            var main = run.WaitMainWindow(WindowLimitMs);
            var windowMs = run.MsSinceStart;
            Win32.RaiseWithoutFocus(main);
            double? firstCanvas = null, lastBlack = null;
            int blackCaptures = 0, captures = 0, screenBlack = 0, screenSeen = 0;
            var canvasHandles = new HashSet<nint>();
            while (!File.Exists(firstFramePath))
            {
                if (run.Exited) throw new InvalidOperationException($"{label}: the app exited {run.ExitCode} before its first frame: {run.Output()}");
                if (run.MsSinceStart > StartLimitMs) throw new TimeoutException($"{label}: no first-frame signal in {StartLimitMs} ms");
                var canvas = Win32.CanvasOf(main);
                if (canvas != 0)
                {
                    canvasHandles.Add(canvas);
                    firstCanvas ??= run.MsSinceStart;
                    Win32.RaiseWithoutFocus(main);
                    var picture = Win32.Capture(main);
                    captures++;
                    if (!Win32.TryCanvasRect(main, canvas, out var rect)) continue;
                    if (rect.Width > 0 && rect.Height > 0 && PixelCheck.IsBlack(picture, rect))
                    {
                        blackCaptures++;
                        lastBlack = run.MsSinceStart;
                    }
                    if (Win32.GetWindowRect(canvas, out var onScreen) && Win32.ScreenPixelOf(canvas, onScreen.Left + onScreen.Width / 2, onScreen.Top + onScreen.Height / 2, out _) is { } middle)
                    {
                        screenSeen++;
                        if (middle.IsBlack) screenBlack++;
                    }
                }
                Thread.Sleep(PollMs);
            }
            var frame = FirstFrame.Read(firstFramePath);
            if (frame.FrameErrors != 0) problems.Add($"{label}: {frame.FrameErrors} frame errors, last {frame.LastError}");
            if (frame.ValidationErrors != 0) problems.Add($"{label}: {frame.ValidationErrors} validation errors");

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
            // The same image on the real screen, at the canvas's middle, when nothing covers it.
            if (!Win32.GetWindowRect(frame.Canvas, out var shown)) throw new InvalidOperationException($"{label}: GetWindowRect of the canvas failed");
            Win32.RaiseWithoutFocus(main);
            Win32.WaitForDisplay();
            var seen = Win32.ScreenPixelOf(frame.Canvas, shown.Left + shown.Width / 2, shown.Top + shown.Height / 2, out var coveredBy);
            var screenWords = $"not read, class {coveredBy} covers it";
            if (seen is { } onScreenPixel)
            {
                var (docX, docY) = frame.Map.DocumentPoint(shown.Width / 2 + .5, shown.Height / 2 + .5);
                var wanted = frame.Map.OnDocument(docX, docY) ? TestImages.CellColorInside(docX, docY, margin) : null;
                screenWords = wanted is null ? $"{onScreenPixel}, near a color edge so not judged" : $"{onScreenPixel}, expected {wanted}";
                if (wanted is { } want && onScreenPixel.Distance(want) > ColorTolerance) problems.Add($"{label}: the screen shows {onScreenPixel} at the canvas's middle, expected {want}");
            }
            lines.Add($"{label}: window {windowMs:F0} ms, canvas window {firstCanvas?.ToString("F0") ?? "before the first poll"} ms, view attached {frame.AttachedMs:F0} ms, first frame {frame.FirstFrameMs:F0} ms, signal {frame.SignalMs:F0} ms; before the signal {blackCaptures} of {captures} captures and {screenBlack} of {screenSeen} screen pixels were black, last capture at {lastBlack?.ToString("F0") ?? "none"} ms, {canvasHandles.Count} canvas handles; screen after the signal: {screenWords}");
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
                ? PartResult.Pass("window shots", $"{saved.Count} saved in {shots}; root must look at each, and 200% is not in this set")
                : PartResult.Fail("window shots", string.Join("; ", failures)));
        }

        // ---- Done-test 1: the 24 MP pan trace.
        void PanPart()
        {
            const int BusyCpuPercent = 15, BusySampleMs = 3000, SpareMs = 180_000, WindowLimitMs = 60_000;
            const string Name = "pan trace";
            var cpu = Win32.CpuBusyPercent(BusySampleMs);
            var gpu = GpuFacts();
            var busy = cpu >= BusyCpuPercent;
            var proofReasons = new List<string>();
            if (Math.Abs(options.PanSeconds - Options.FullPanSeconds) > .001) proofReasons.Add($"{options.PanSeconds:0.#} s, not {Options.FullPanSeconds:0} s");
            if (busy) proofReasons.Add($"the PC was {cpu:F0}% busy before it started");
            if (busy && !options.AllowBusy)
            {
                Report(PartResult.NotRun(Name, $"the PC is {cpu:F0}% busy, and a busy run does not count (PLAN 4.3); pass --allow-busy for a proof run"));
                return;
            }
            if (!gpu.Contains("RTX", StringComparison.Ordinal)) proofReasons.Add($"nvidia-smi names no RTX: {gpu}");

            var big = Big24Mp();
            var folder = NextFolder("pan");
            var path = Path.Combine(folder, "pan.json");
            using var run = new AppRun(options.App, folder, ["--test-load-png", big, "--theme", "light", "--test-zoom", "1", "--test-pan-trace", path, "--test-pan-seconds", options.PanSeconds.ToString(CultureInfo.InvariantCulture)]);
            var main = run.WaitMainWindow(WindowLimitMs);
            var refresh = Win32.RefreshRate(main);
            var limit = (int)(options.PanSeconds * 1000) + SpareMs;
            if (!run.WaitExit(limit))
            {
                Report(PartResult.Fail(Name, $"the app did not end in {limit} ms"));
                return;
            }
            if (!File.Exists(path))
            {
                Report(PartResult.Fail(Name, $"the app exited {run.ExitCode} and wrote no frame times: {run.Output()}"));
                return;
            }
            using var file = JsonDocument.Parse(File.ReadAllText(path));
            var root = file.RootElement;
            var times = root.GetProperty("frame_times_ms").EnumerateArray().Select(t => t.GetDouble()).ToArray();
            var stats = FrameStats.From(times);
            var moves = root.GetProperty("moves_posted").GetInt64();
            var changes = root.GetProperty("view_changes").GetInt64();
            var errors = root.GetProperty("frame_errors_during").GetInt64();
            var validation = root.GetProperty("validation_errors").GetInt64();
            var drawable = $"{root.GetProperty("drawable_width").GetInt64()}x{root.GetProperty("drawable_height").GetInt64()}";
            var zoom = root.GetProperty("zoom").GetDouble();
            var facts = $"{stats}; {moves} moves posted, {changes} view changes, canvas {drawable} at zoom {zoom:0.###}, display {refresh} Hz, {gpu}, PC {cpu:F0}% busy before";
            var problem = stats.Problem(options.PanSeconds);
            if (errors != 0) problem = $"{errors} frame errors; {problem}";
            if (validation != 0) problem = $"{validation} validation errors; {problem}";
            if (zoom != 1) problem = $"zoom is {zoom}, not 1; {problem}";
            if (changes < moves / 2) problem = $"the UI thread handled only {changes} of {moves} moves; {problem}";
            if (run.ExitCode != 0) problem = $"the app exited {run.ExitCode}: {run.Output()}; {problem}";
            if (refresh < 59 && problem is not null) proofReasons.Add($"the display runs at {refresh} Hz, under the 59 fps asked");
            CheckDataRootEmpty(run, Name);
            if (problem is not null && proofReasons.Count == 0) Report(PartResult.Fail(Name, $"{problem}; {facts}"));
            else if (proofReasons.Count != 0) Report(PartResult.NotRun(Name, $"proof run only, these numbers do not count ({string.Join("; ", proofReasons)}): {(problem is null ? "met the limits" : problem)}; {facts}"));
            else Report(PartResult.Pass(Name, facts));
        }

        static string GpuFacts()
        {
            try
            {
                var info = new ProcessStartInfo("nvidia-smi", "--query-gpu=name,utilization.gpu --format=csv,noheader") { RedirectStandardOutput = true, UseShellExecute = false, CreateNoWindow = true };
                using var process = Process.Start(info) ?? throw new InvalidOperationException("no process");
                var text = process.StandardOutput.ReadToEnd().Trim();
                process.WaitForExit();
                return process.ExitCode == 0 ? text : $"nvidia-smi exited {process.ExitCode}";
            }
            catch (System.ComponentModel.Win32Exception e)
            {
                return $"nvidia-smi could not start: {e.Message}";
            }
        }
    }
}
