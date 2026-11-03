// The harness's judgement, with made-up inputs. A black frame must fail here, because the
// W3 test program passed on one. The real-window side runs only as root; see docs/windows/w7-shell.md.
using OpenPainter.Canvas;
using OpenPainter.Harness;
using OpenPainter.Shell;
using Xunit;

namespace OpenPainter.Tests;

public sealed class HarnessTests
{
    static readonly PixelRect Canvas = new(10, 20, 480, 320);

    static ViewMap Map(double zoom = 1, double originX = -300, double originY = -200) =>
        new(zoom, originX, originY, 1, TestImages.Width24Mp, TestImages.Height24Mp);

    /// A picture whose canvas shows the test image, as the view would.
    static Picture ShownImage(ViewMap map)
    {
        var picture = Picture.Filled(Canvas.X + Canvas.Width, Canvas.Y + Canvas.Height, new Rgb(50, 50, 50));
        for (var y = 0; y < Canvas.Height; y++)
        {
            for (var x = 0; x < Canvas.Width; x++)
            {
                var (docX, docY) = map.DocumentPoint(x + .5, y + .5);
                if (!map.OnDocument(docX, docY)) continue;
                var color = TestImages.CellColor((int)docX, (int)docY);
                var at = ((y + Canvas.Y) * picture.Width + x + Canvas.X) * 4;
                (picture.Bgra[at], picture.Bgra[at + 1], picture.Bgra[at + 2]) = (color.B, color.G, color.R);
            }
        }
        return picture;
    }

    static PixelResult Judge(Picture picture, ViewMap map, double margin = 6) =>
        PixelCheck.Judge(picture, Canvas, map, (x, y) => TestImages.CellColorInside(x, y, margin), tolerance: 1);

    [Fact]
    public void TheShownImagePasses()
    {
        var map = Map();
        var result = Judge(ShownImage(map), map);
        Assert.Null(result.Problem());
        Assert.True(result.Samples >= PixelResult.LeastSamples);
        Assert.Equal(result.Samples, result.Matched);
    }

    [Fact]
    public void ABlackFrameFailsAndSaysSo()
    {
        var map = Map();
        var result = Judge(Picture.Filled(Canvas.X + Canvas.Width, Canvas.Y + Canvas.Height, new Rgb(0, 0, 0)), map);
        Assert.Contains("BLACK FRAME", result.Problem());
        Assert.True(PixelCheck.IsBlack(Picture.Filled(Canvas.X + Canvas.Width, Canvas.Y + Canvas.Height, new Rgb(0, 0, 0)), Canvas));
    }

    [Fact]
    public void AFrameShiftedByOneCellFails()
    {
        var map = Map();
        var shifted = ShownImage(Map(originX: -300 - TestImages.CellSize));
        var problem = Judge(shifted, map).Problem();
        Assert.NotNull(problem);
        Assert.Contains("points differ", problem);
    }

    [Fact]
    public void AColorOffByTwoFailsAndOffByOnePasses()
    {
        var map = Map();
        var off = ShownImage(map);
        for (var i = 0; i < off.Bgra.Length; i += 4) off.Bgra[i + 1] = (byte)(off.Bgra[i + 1] + 1);
        Assert.Null(Judge(off, map).Problem());
        for (var i = 0; i < off.Bgra.Length; i += 4) off.Bgra[i + 1] = (byte)(off.Bgra[i + 1] + 1);
        Assert.NotNull(Judge(off, map).Problem());
    }

    [Fact]
    public void AViewWithTooFewPointsOnTheImageProvesNothing()
    {
        var off = Map(originX: 100000, originY: 100000);
        Assert.Contains("proves nothing", Judge(ShownImage(off), off).Problem());
    }

    [Fact]
    public void TestImageCellsAreNeverNearBlack()
    {
        for (var y = 0; y < TestImages.Height24Mp; y += TestImages.CellSize)
            for (var x = 0; x < TestImages.Width24Mp; x += TestImages.CellSize)
                Assert.False(TestImages.CellColor(x, y).IsBlack, $"{x},{y}");
    }

    [Fact]
    public void AStillFrameLogMeetsNoLimitAndAFullRateOneMeetsThem()
    {
        var good = Enumerable.Range(0, 1801).Select(i => i * 1000 / 60.0).ToArray();
        var stats = FrameStats.From(good);
        Assert.Null(stats.Problem(30));
        Assert.Equal(1801, stats.Frames);
        Assert.InRange(stats.AverageFps, 59.9, 60.1);

        Assert.Contains("under 59", FrameStats.From(Enumerable.Range(0, 1741).Select(i => i * 1000 / 58.0).ToArray()).Problem(30));
        Assert.Contains("cover", FrameStats.From([0, 16.6]).Problem(30));
        Assert.NotNull(FrameStats.From([]).Problem(30));
    }

    [Fact]
    public void ASingleStallOverFiftyMillisecondsFails()
    {
        var times = Enumerable.Range(0, 1801).Select(i => i * 1000 / 60.0).ToList();
        for (var i = 900; i < times.Count; i++) times[i] += 51 - 1000 / 60.0;
        var stats = FrameStats.From(times);
        Assert.InRange(stats.LongestStallMs, 50.9, 51.1);
        Assert.Contains("stall", stats.Problem(30));
    }

    [Fact]
    public void ThePanPathNeverRestsAndStaysInItsReach()
    {
        var trace = PanTrace.For(6000, 4000, 1280, 700, 1, 1);
        Assert.Equal((640, 350), trace.At(0));
        var last = trace.At(0);
        var (minX, maxX, minY, maxY) = (last.X, last.X, last.Y, last.Y);
        for (var t = 1.0 / 60; t < 30; t += 1.0 / 60)
        {
            var now = trace.At(t);
            Assert.True(now != last, $"the path rests at {t} s, so a frame would have nothing to show");
            last = now;
            (minX, maxX, minY, maxY) = (Math.Min(minX, now.X), Math.Max(maxX, now.X), Math.Min(minY, now.Y), Math.Max(maxY, now.Y));
        }
        Assert.Equal(2356, trace.ReachX);
        Assert.Equal(1646, trace.ReachY);
        Assert.True(maxX - minX >= 2 * trace.ReachX - 2 && maxY - minY >= 2 * trace.ReachY - 2);
        Assert.True(maxX <= trace.StartX + trace.ReachX && minY >= trace.StartY - trace.ReachY);
    }

    [Fact]
    public void AnImageThatCannotPanStopsTheTraceByName() =>
        Assert.Contains("PAN_TRACE", Assert.Throws<InvalidOperationException>(() => PanTrace.For(800, 600, 1280, 700, 1, 1)).Message);

    [Theory]
    [InlineData("UI TESTS: NOT RUN (desktop is locked)\nPREFLIGHT: FAIL (UI tests: desktop is locked)", false, "desktop is locked")]
    [InlineData(".NET 10: PASS\nUI TESTS: PASS\nPREFLIGHT: PASS", false, null)]
    [InlineData("UI TESTS: PASS\nPREFLIGHT: FAIL (under 40 GB free)", false, "preflight: under 40 GB free")]
    [InlineData("UI TESTS: PASS\nPREFLIGHT: FAIL (under 40 GB free)", true, null)]
    [InlineData("UI TESTS: PASS\nPREFLIGHT: FAIL (cargo failed)", true, "preflight: cargo failed")]
    [InlineData("nonsense", false, "preflight printed no result")]
    public void ThePreflightsWordsDecideWhetherUiTestsRun(string output, bool ignoreDisk, string? want) =>
        Assert.Equal(want, Preflight.BlockReason(output, ignoreDisk));

    [Fact]
    public void AKnownDialogBlocksUiTestsAndAnyOtherWindowDoesNot()
    {
        Assert.Null(Preflight.DialogReason([]));
        Assert.Null(Preflight.DialogReason(["notepad", "explorer"]));
        Assert.Equal("known dialog is up: WerFault", Preflight.DialogReason(["notepad", "WerFault", "werfault"]));
        Assert.Equal("known dialog is up: consent, CredentialUIBroker", Preflight.DialogReason(["consent", "CredentialUIBroker"]));
    }

    [Fact]
    public void EachPartLineSaysPassFailOrNotRunWithItsReason()
    {
        Assert.Equal("a: PASS", PartResult.Pass("a").Line);
        Assert.Equal("a: FAIL (why)", PartResult.Fail("a", "why").Line);
        Assert.Equal("a: NOT RUN (why)", PartResult.NotRun("a", "why").Line);
    }

    [Fact]
    public void AnyPartThatDidNotPassFailsTheRunByName()
    {
        Assert.Equal("HARNESS: PASS (1 parts)", PartResult.Summary([PartResult.Pass("a")]));
        Assert.Equal("HARNESS: FAIL (b, c)", PartResult.Summary([PartResult.Pass("a"), PartResult.Fail("b", "x"), PartResult.NotRun("c", "y")]));
        Assert.Equal("HARNESS: FAIL (no part ran)", PartResult.Summary([]));
    }

    /// Every part's evidence, as a full run writes it.
    static readonly string AllChecks = string.Join(", ", ReportJudge.LeastChecks.Select(c => $"\"{c.Part}\": {c.Least}"));
    static readonly string CleanReport = $$"""{ "panels": { "colors": { "screen": "on top" } }, "checks_run": { {{AllChecks}} }, "problems": [] }""";

    [Fact]
    public void ACleanReportPassesEveryPart()
    {
        var parts = ReportJudge.Parts(CleanReport, 0, " (light)");
        Assert.Equal(6, parts.Count);
        Assert.All(parts, part => Assert.Equal(PartStatus.Pass, part.Status));
    }

    [Theory]
    [InlineData("close: Windows says 1, expected 20", "hit test")]
    [InlineData("panel colors: it is not a tool window", "panels")]
    [InlineData("icons: 2 are not on whole pixels", "icons")]
    [InlineData("menu: the first top menu did not open", "menus and options")]
    [InlineData("options more: it did not open as its own window", "menus and options")]
    [InlineData("pan: the view moved by 1, expected -40, 20", "canvas input")]
    [InlineData("window minimum: the client stops at 1100, 824, not 1100, 792", "window minimum size")]
    public void AReportProblemFailsItsOwnPart(string problem, string part)
    {
        var parts = ReportJudge.Parts($$"""{ "checks_run": { {{AllChecks}} }, "problems": ["{{problem}}"] }""", 1, "");
        Assert.Equal(PartStatus.Fail, parts.Single(p => p.Name == part).Status);
        Assert.Single(parts, p => p.Status != PartStatus.Pass);
    }

    [Fact]
    public void ACoveredPanelIsNotRunAndNotAPass()
    {
        var parts = ReportJudge.Parts("""{ "panels": { "colors": { "screen": "NOT RUN: class Chrome covers it" } }, "problems": [] }""", 0, "");
        var panels = parts.Single(p => p.Name == "panels");
        Assert.Equal(PartStatus.NotRun, panels.Status);
        Assert.Contains("class Chrome", panels.Detail);
    }

    [Fact]
    public void AFailedRunThatNamesNoProblemStillFails() =>
        Assert.Contains(ReportJudge.Parts(CleanReport, 1, ""), part => part.Status == PartStatus.Fail);

    [Fact]
    public void AWrittenPngHoldsItsHeaderAndEnds()
    {
        var path = Path.Combine(Path.GetTempPath(), $"op-harness-{Environment.ProcessId}-{Guid.NewGuid():N}.png");
        try
        {
            TestImages.WriteCells(path, 300, 200);
            var bytes = File.ReadAllBytes(path);
            Assert.Equal(new byte[] { 0x89, 0x50, 0x4E, 0x47 }, bytes[..4]);
            Assert.Equal(300, System.Buffers.Binary.BinaryPrimitives.ReadInt32BigEndian(bytes.AsSpan(16)));
            Assert.Equal(200, System.Buffers.Binary.BinaryPrimitives.ReadInt32BigEndian(bytes.AsSpan(20)));
            Assert.Equal("IEND", System.Text.Encoding.ASCII.GetString(bytes, bytes.Length - 8, 4));
        }
        finally
        {
            File.Delete(path);
        }
    }

    static readonly string[] BannedCalls = ["SendInput", "keybd_event", "mouse_event", "SetCursorPos", "CopyFromScreen", "BitBlt"];

    static IEnumerable<string> BannedIn(string text) => BannedCalls.Where(banned => text.Contains($"{banned}(", StringComparison.Ordinal));

    [Fact]
    public void NothingUnderWindowsHoldsABannedCall()
    {
        // PLAN 4.3: tests use messages to the app's own window, and capture one window. No
        // part of windows/ has a reason to send raw input or read the whole screen.
        static bool IsBuildOutput(string file) =>
            file.Split(Path.DirectorySeparatorChar).Any(part => part is "bin" or "obj");
        var files = Directory.GetFiles(Repo.File("windows"), "*.cs", SearchOption.AllDirectories).Where(f => !IsBuildOutput(f)).ToList();
        Assert.True(files.Count > 50, $"only {files.Count} source files under windows/");
        Assert.Contains(files, f => f.EndsWith("MainWindow.cs", StringComparison.Ordinal));
        Assert.Contains(files, f => f.EndsWith(Path.Combine("harness", "src", "Win32.cs"), StringComparison.Ordinal));
        foreach (var file in files)
            Assert.True(!BannedIn(File.ReadAllText(file)).Any(), $"{file} calls {string.Join(", ", BannedIn(File.ReadAllText(file)))}");
    }

    [Fact]
    public void ThePlantedBannedCallIsFound() =>
        Assert.Equal(["SendInput"], BannedIn("static extern uint Send" + "Input(uint count);"));

    // ---- A freeze at either end of the trace.

    static double[] Frames(double fromSeconds, double toSeconds)
    {
        var count = (int)((toSeconds - fromSeconds) * 60);
        return Enumerable.Range(0, count + 1).Select(i => (fromSeconds + i / 60.0) * 1000).ToArray();
    }

    [Theory]
    [InlineData(.6, 30)]    // Frozen for its first 0.6 s, which 98% cover let through.
    [InlineData(0, 29.4)]   // Frozen for its last 0.6 s.
    [InlineData(.06, 29.94)] // A little at each end, 120 ms together, over two allowed stalls.
    public void AFreezeAtAnEndOfTheTraceFails(double from, double to) =>
        Assert.Contains("cover", FrameStats.From(Frames(from, to)).Problem(30));

    [Theory]
    [InlineData(51.0, 0.0, "first frame")]
    [InlineData(0.0, 51.0, "last frame")]
    public void AGapOverOneStallAtEitherEndFailsByName(double start, double end, string words) =>
        Assert.Contains(words, FrameStats.From(Frames(0, 30)).Problem(30, start, end));

    [Fact]
    public void GapsOfUpToOneStallPass() => Assert.Null(FrameStats.From(Frames(0, 30)).Problem(30, 50, 50));

    // ---- The pan must really move the image.

    /// A view that follows the pointer one to one, with `lag` pixels behind it.
    static List<PanSample> Following(double lag = 20)
    {
        var trace = PanTrace.For(6000, 4000, 1280, 672, 1, 1);
        return Enumerable.Range(0, 120).Select(i =>
        {
            var at = trace.At(i * .25);
            return new PanSample(i * .25, at.X, at.Y, at.X - trace.StartX - lag, at.Y - trace.StartY - lag, i * 15 + 1);
        }).ToList();
    }

    [Fact]
    public void AViewThatFollowsThePointerPasses() => Assert.Null(PanMotion.Problem(Following(), 1, 1));

    [Fact]
    public void APanThatDoesNothingFailsThoughFramesKeepComing()
    {
        var still = Following().Select(s => s with { OriginX = -300, OriginY = -200 }).ToList();
        Assert.Contains("the view moved 0 by 0", PanMotion.Problem(still, 1, 1));
    }

    [Fact]
    public void AViewThatLeavesThePointersPathFails()
    {
        var mirrored = Following().Select(s => s with { OriginX = -s.OriginX, OriginY = -s.OriginY }).ToList();
        Assert.Contains("off the pointer", PanMotion.Problem(mirrored, 1, 1));
    }

    [Fact]
    public void AViewWithNoNewFrameFails()
    {
        var stuck = Following().Select(s => s with { Frames = 7 }).ToList();
        Assert.Contains("no frame was shown", PanMotion.Problem(stuck, 1, 1));
    }

    [Fact]
    public void ATraceWithFewSamplesProvesNothing() => Assert.Contains("only 2", PanMotion.Problem(Following().Take(2).ToList(), 1, 1));

    [Fact]
    public void ScreenPixelsThatDoNotChangeMeanTheImageDidNotMove()
    {
        var still = Enumerable.Repeat<Rgb?>(new Rgb(9, 9, 9), 100).ToList();
        Assert.Contains("did not move", PanMotion.ScreenProblem(still, out var judged));
        Assert.True(judged);
        var moving = Enumerable.Range(0, 100).Select(i => (Rgb?)new Rgb((byte)i, 0, 0)).ToList();
        Assert.Null(PanMotion.ScreenProblem(moving, out judged));
        Assert.True(judged);
        Assert.Null(PanMotion.ScreenProblem(Enumerable.Repeat<Rgb?>(null, 100).ToList(), out judged));
        Assert.False(judged);
    }

    // ---- Report parts need evidence.

    [Theory]
    [InlineData("hit test")]
    [InlineData("panels")]
    [InlineData("canvas input")]
    [InlineData("menus and options")]
    [InlineData("icons")]
    [InlineData("window minimum size")]
    public void APartWhoseCheckDidNotRunFails(string part)
    {
        var ran = string.Join(", ", ReportJudge.LeastChecks.Where(c => c.Part != part).Select(c => $"\"{c.Part}\": {c.Least}"));
        var parts = ReportJudge.Parts($$"""{ "checks_run": { {{ran}} }, "problems": [] }""", 0, "");
        var mine = parts.Single(p => p.Name == part);
        Assert.Equal(PartStatus.Fail, mine.Status);
        Assert.Contains("ran 0 checks", mine.Detail);
        Assert.Single(parts, p => p.Status != PartStatus.Pass);
    }

    [Fact]
    public void AReportWithNoEvidenceAtAllPassesNothing() =>
        Assert.All(ReportJudge.Parts("""{ "problems": [] }""", 0, ""), part => Assert.Equal(PartStatus.Fail, part.Status));

    [Fact]
    public void APartThatRanTooFewChecksFails()
    {
        var ran = string.Join(", ", ReportJudge.LeastChecks.Select(c => $"\"{c.Part}\": {(c.Part == "icons" ? c.Least - 1 : c.Least)}"));
        var parts = ReportJudge.Parts($$"""{ "checks_run": { {{ran}} }, "problems": [] }""", 0, "");
        Assert.Equal(PartStatus.Fail, parts.Single(p => p.Name == "icons").Status);
    }

    [Fact]
    public void APassCountsItsChecks() => Assert.Contains(ReportJudge.Parts(CleanReport, 0, ""), part => part.Line == $"panels: PASS ({ReportJudge.LeastChecks.Single(c => c.Part == "panels").Least} checks)");

    [Fact]
    public void ACoveredPopupIsNotRunInTheMenusPart()
    {
        var json = $$"""{ "more_popup_screen": "NOT RUN: class Chrome covers it", "checks_run": { {{AllChecks}} }, "problems": [] }""";
        var menus = ReportJudge.Parts(json, 0, "").Single(p => p.Name == "menus and options");
        Assert.Equal(PartStatus.NotRun, menus.Status);
        Assert.Contains("class Chrome", menus.Detail);
    }

    [Fact]
    public void APopupUnderTheCanvasIsAFaultOfTheMenusPart() =>
        Assert.Equal(PartStatus.Fail, ReportJudge.Parts($$"""{ "checks_run": { {{AllChecks}} }, "problems": ["more popup: the main window or canvas answers"] }""", 1, "").Single(p => p.Name == "menus and options").Status);

    // ---- A real screen pixel must have been judged.

    [Fact]
    public void AScreenRowWithNoPointJudgedIsNotRunAndNotAPass()
    {
        var part = ScreenCheck.Part([new ScreenRead("a", 3, 3, 0, null), new ScreenRead("24 MP at Fit", 0, 0, 4, null)]);
        Assert.Equal(PartStatus.NotRun, part.Status);
        Assert.Contains("24 MP at Fit", part.Detail);
    }

    [Fact]
    public void AScreenPixelThatDiffersFails() =>
        Assert.Equal(PartStatus.Fail, ScreenCheck.Part([new ScreenRead("a", 3, 2, 0, "#000000 at 1,1")]).Status);

    [Fact]
    public void AScreenRowSaysHowManyPointsItJudged() =>
        Assert.Equal("screen pixels: PASS (5 points judged over 2 launches, all matched)", ScreenCheck.Part([new ScreenRead("a", 3, 3, 0, null), new ScreenRead("b", 2, 2, 1, null)]).Line);

    [Fact]
    public void ScreenPointsStayOffColorEdgesAtFitToo()
    {
        var fit = new ViewMap(.126, 40, 20, 1, TestImages.Width24Mp, TestImages.Height24Mp);
        var margin = 4 / .126;
        var points = PixelCheck.ScreenPoints(1280, 672, fit, (x, y) => TestImages.CellColorInside(x, y, margin));
        Assert.True(points.Count >= 4, $"only {points.Count} points");
        foreach (var (x, y, want) in points)
        {
            var (docX, docY) = fit.DocumentPoint(x + .5, y + .5);
            Assert.Equal(want, TestImages.CellColorInside(docX, docY, margin));
        }
        var distance = (int X, int Y) => Math.Abs(X - 640) + Math.Abs(Y - 336);
        Assert.True(distance(points[0].X, points[0].Y) <= distance(points[^1].X, points[^1].Y));
    }

    // ---- The black canvas at launch.

    static readonly Rgb Stage = new(30, 30, 30);

    static LaunchRead Launch(params (double Ms, Rgb Color)[] pixels) =>
        new("a", Stage, 1000, pixels.Select(p => new LaunchPixel(p.Ms, p.Color)).ToList());

    [Fact]
    public void AStageColoredCanvasBeforeTheFirstFramePasses()
    {
        var part = LaunchPaint.Part([Launch((300, new Rgb(12, 12, 12)), (340, Stage), (500, Stage), (820, Stage), (1100, new Rgb(0, 200, 0)))]);
        Assert.Equal(PartStatus.Pass, part.Status);
        Assert.Contains("3 real screen pixels", part.Detail);
    }

    [Fact]
    public void ABlackCanvasBeforeTheFirstFrameFails()
    {
        var part = LaunchPaint.Part([Launch((300, new Rgb(0, 0, 0)), (500, new Rgb(0, 0, 0)), (800, new Rgb(0, 0, 0)))]);
        Assert.Equal(PartStatus.Fail, part.Status);
        Assert.Contains("3 were black", part.Detail);
    }

    [Fact]
    public void BlackAfterTheStageShowedFails()
    {
        var part = LaunchPaint.Part([Launch((300, Stage), (500, new Rgb(0, 0, 0)), (800, Stage))]);
        Assert.Equal(PartStatus.Fail, part.Status);
        Assert.Contains("1 of them black", part.Detail);
    }

    [Fact]
    public void AWindowThatTakesLongToPaintFails() =>
        Assert.Contains("took 400 ms", LaunchPaint.Part([Launch((300, new Rgb(12, 12, 12)), (700, Stage))]).Detail);

    [Fact]
    public void NoPixelReadBeforeAFirstFrameIsNotRun() =>
        Assert.Equal(PartStatus.NotRun, LaunchPaint.Part([Launch((950, Stage))]).Status);

    // ---- W8: the app says when it has painted. A color alone cannot, because a window behind
    // the app can have any color, and Windows says the app owns the pixel before it paints.

    static LaunchRead Signaled(double painted, params (double Ms, Rgb Color, int Point)[] pixels) =>
        new("a", new Rgb(30, 30, 30), 1000, pixels.Select(p => new LaunchPixel(p.Ms, p.Color, p.Point)).ToList(), painted, 250);

    /// The real read from a 24 MP dark launch: a window behind read #1F1F1F at one point and
    /// #0C0C0C at the next. The old rule took the first for the stage and failed on the second.
    [Fact]
    public void AWindowBehindTheAppThatLooksLikeTheStageDoesNotCountAsPainted()
    {
        var behind = new Rgb(31, 31, 31);
        var terminal = new Rgb(12, 12, 12);
        var read = Signaled(330, (269, behind, 0), (285, terminal, 1), (301, Stage, 2), (318, behind, 0), (335, Stage, 1), (400, Stage, 2), (500, Stage, 0));
        Assert.Equal(PartStatus.Fail, LaunchPaint.Part([read with { PaintedMs = null }]).Status);
        var part = LaunchPaint.Part([read]);
        Assert.Equal(PartStatus.Pass, part.Status);
        Assert.Contains("3 real screen pixels", part.Detail);
    }

    [Fact]
    public void NothingReadBeforeTheAppPaintedIsJudged()
    {
        var part = LaunchPaint.Part([Signaled(300, (280, new Rgb(0, 0, 0), 0), (290, new Rgb(200, 0, 0), 1), (350, Stage, 0))]);
        Assert.Equal(PartStatus.Pass, part.Status);
        Assert.Contains("1 real screen pixels", part.Detail);
    }

    [Fact]
    public void BlackAfterTheAppPaintedFails()
    {
        var part = LaunchPaint.Part([Signaled(330, (400, Stage, 0), (500, new Rgb(0, 0, 0), 1), (600, Stage, 2))]);
        Assert.Equal(PartStatus.Fail, part.Status);
        Assert.Contains("1 of them black", part.Detail);
    }

    [Fact]
    public void AWrongColorAfterTheAppPaintedFailsByItsColorAndTime()
    {
        var part = LaunchPaint.Part([Signaled(330, (400, Stage, 0), (500, new Rgb(200, 0, 0), 1))]);
        Assert.Equal(PartStatus.Fail, part.Status);
        Assert.Contains("#C80000 at 500 ms", part.Detail);
    }

    [Fact]
    public void AnAppThatPaintsLateFailsByItsTime()
    {
        var part = LaunchPaint.Part([Signaled(620, (600, new Rgb(5, 5, 5), 0), (650, Stage, 0))]);
        Assert.Equal(PartStatus.Fail, part.Status);
        Assert.Contains("took 350 ms", part.Detail);
    }

    [Fact]
    public void ALatePaintOnABusyPcIsNamedAndNotJudgedAndItsColorsStillAre()
    {
        var read = Signaled(620, (600, new Rgb(5, 5, 5), 0), (650, Stage, 0)) with { OthersCpu = 40 };
        var part = LaunchPaint.Part([read]);
        Assert.Equal(PartStatus.Pass, part.Status);
        Assert.Contains("not judged on a busy PC: a took 350 ms with other programs at 40% of the CPU", part.Detail);
        Assert.Equal(PartStatus.Fail, LaunchPaint.Part([read with { OthersCpu = 5 }]).Status);
        Assert.Equal(PartStatus.Fail, LaunchPaint.Part([read with { Pixels = [new LaunchPixel(600, new Rgb(5, 5, 5)), new LaunchPixel(650, new Rgb(0, 0, 0))] }]).Status);
    }

    /// The signal is late by the UI thread's work and the harness's poll, and that is no fault.
    [Fact]
    public void ASignalThatComesAfterTheStageShowedDoesNotCountAsALatePaint()
    {
        var part = LaunchPaint.Part([Signaled(600, (260, new Rgb(5, 5, 5), 0), (300, Stage, 1), (400, Stage, 2), (650, Stage, 0))]);
        Assert.Equal(PartStatus.Pass, part.Status);
        Assert.Contains("at most 10 ms", part.Detail);
    }

    /// A busy app can hold the harness past its first reads, so they say nothing either way.
    [Fact]
    public void ALaunchWithNoReadBeforeTheSignalSaysNothingOfThePaintTime()
    {
        var part = LaunchPaint.Part([Signaled(600, (650, Stage, 0))]);
        Assert.Equal(PartStatus.Pass, part.Status);
        Assert.Contains("at most 0 ms", part.Detail);
    }

    [Fact]
    public void ALaunchThatReachedItsFirstFrameBeforeAnyReadIsNotJudged()
    {
        var part = LaunchPaint.Part([Signaled(260, (280, Stage, 0)), new LaunchRead("quick", Stage, 400, [new LaunchPixel(300, new Rgb(5, 5, 5))], 280, 250)]);
        Assert.Equal(PartStatus.Pass, part.Status);
        Assert.Contains("1 of 2 launches", part.Detail);
    }

    [Fact]
    public void NoPixelReadAfterThePaintIsNotRun() =>
        Assert.Equal(PartStatus.NotRun, LaunchPaint.Part([Signaled(300, (280, new Rgb(5, 5, 5), 0))]).Status);

    [Fact]
    public void AReadJustAfterAnotherProgramsWindowSaysNothing()
    {
        Assert.False(LaunchPaint.TooSoonAfterCover(300, null));
        Assert.True(LaunchPaint.TooSoonAfterCover(300 + LaunchPaint.AfterCoverMs - 1, 300));
        Assert.False(LaunchPaint.TooSoonAfterCover(300 + LaunchPaint.AfterCoverMs, 300));
    }

    /// The left panels end near a fifth of the default width and the right ones begin near four
    /// fifths, and the rows above the stage end near a sixth of its height.
    [Fact]
    public void TheReadPointsAreOnTheStageWhereverAPanelStartsOut()
    {
        Assert.True(LaunchPaint.ReadPoints.Length >= 15);
        Assert.Equal(LaunchPaint.ReadPoints.Length, LaunchPaint.ReadPoints.Distinct().Count());
        foreach (var (x, y) in LaunchPaint.ReadPoints)
        {
            Assert.InRange(x * 1296, 256 + 64, 1296 - 256 - 64);
            Assert.InRange(y * 840, 170 + 64, 840 - 40 - 64);
        }
    }

    [Fact]
    public void TheFaultProofPassesOnlyWhenThePlantFails()
    {
        Assert.Equal(PartStatus.Pass, LaunchPaint.Proof("black canvas", LaunchPaint.Part([Signaled(330, (400, new Rgb(0, 0, 0), 0))])).Status);
        Assert.Equal(PartStatus.Fail, LaunchPaint.Proof("black canvas", LaunchPaint.Part([Signaled(330, (400, Stage, 0))])).Status);
        Assert.Equal(PartStatus.Fail, LaunchPaint.Proof("black canvas", LaunchPaint.Part([Signaled(300, (280, Stage, 0))])).Status);
    }

    [Fact]
    public void TheFaultProofNamesBothPlantsAndAFullRunHoldsIt()
    {
        Assert.Equal(LaunchPaint.Plants, OpenPainter.Shell.StartOptions.PaintFaults);
        Assert.Contains("paint-fault", OpenPainter.Harness.Program.AllParts);
    }

    // ---- Busy runs are thrown out, and GPU use counts.

    [Fact]
    public void AQuietRunHasNoBusyReason()
    {
        Assert.Empty(BusyJudge.Before(3, 5, []));
        Assert.Empty(BusyJudge.During([4, 6], [20, 25]));
    }

    [Fact]
    public void ABusyCpuOrGpuBeforeOrDuringTheTraceIsAReason()
    {
        Assert.Contains("15% busy", BusyJudge.Before(15, 5, []).Single());
        Assert.Contains("GPU was 40%", BusyJudge.Before(3, 40, []).Single());
        Assert.Contains("15% of the CPU", BusyJudge.During([10, 20], [20]).Single());
        Assert.Contains("GPU was 60%", BusyJudge.During([1], [50, 70]).Single());
    }

    [Fact]
    public void GpuUseThatCannotBeReadIsAReason()
    {
        Assert.Contains("could not be read", BusyJudge.Before(3, null, []).Single());
        Assert.Contains("could not be read", BusyJudge.During([1], [null, null]).Single());
        Assert.Contains("could not be read", BusyJudge.During([], [10]).Single());
    }

    [Fact]
    public void SunshineAndLmStudioAreNamedAndBusyOnlyWhenInUse()
    {
        Assert.Equal((null, null), BusyJudge.Program("Sunshine", false, null, "encoder sessions"));
        Assert.Equal(("Sunshine is running, with no encoder sessions", null), BusyJudge.Program("Sunshine", true, 0, "encoder sessions"));
        var streaming = BusyJudge.Program("Sunshine", true, 1, "encoder sessions");
        Assert.Contains("Sunshine", streaming.Busy);
        var unknown = BusyJudge.Program("LM Studio", true, null, "models loaded");
        Assert.Contains("LM Studio", unknown.Busy);
        Assert.Contains("LM Studio is running, with 2 models loaded", BusyJudge.Before(1, 1, [BusyJudge.Program("LM Studio", true, 2, "models loaded").Busy!]));
    }

    // ---- Before the trace the PC gets time to settle, and the limits stay the same.

    static QuietRead Read(double others, double? gpu = 2, string[]? busy = null, ProgramUse[]? top = null) => new(others, gpu, busy ?? [], top ?? []);

    /// Hands out the reads in order, each taking the 3 s the real one takes. The last repeats.
    static SettleResult Settled(double mostWait, params QuietRead[] reads)
    {
        var (now, next) = (0.0, 0);
        return Settle.Wait(() =>
        {
            now += 3;
            return reads[Math.Min(next++, reads.Length - 1)];
        }, () => now, mostWait);
    }

    [Fact]
    public void AQuietPcIsReadTwiceAndGoesOn()
    {
        var result = Settled(60, Read(3), Read(4));
        Assert.True(result.Quiet);
        Assert.Empty(result.Reasons);
        Assert.Equal(2, result.Reads.Count);
        Assert.Equal(6, result.WaitedSeconds);
        Assert.Contains("waited 6 s", result.Words);
        Assert.Contains("3%, 4%", result.Words);
    }

    [Fact]
    public void ThePcGetsTimeToSettleAfterTheHarnesssOwnLaunches()
    {
        var result = Settled(60, Read(24), Read(19), Read(8), Read(5));
        Assert.True(result.Quiet);
        Assert.Equal(12, result.WaitedSeconds);
        Assert.Contains("24%, 19%, 8%, 5%", result.Words);
    }

    [Fact]
    public void OneQuietReadBetweenBusyOnesDoesNotCount()
    {
        var result = Settled(60, Read(24), Read(5), Read(24), Read(5), Read(6));
        Assert.True(result.Quiet);
        Assert.Equal(5, result.Reads.Count);
    }

    [Fact]
    public void APcStillBusyAfterTheWaitNamesTheTopPrograms()
    {
        var result = Settled(60, Read(40, top: [new("MsMpEng", 22), new("SearchIndexer", 9.6)]));
        Assert.False(result.Quiet);
        Assert.Equal(60, result.WaitedSeconds);
        Assert.Contains("40% busy", result.Reasons.Single());
        Assert.Contains("MsMpEng 22%", result.Words);
        Assert.Contains("SearchIndexer 10%", result.Words);
        Assert.Contains("waited 60 s", result.Words);
    }

    [Fact]
    public void AQuietReadAtTheVeryEndOfTheWaitIsNotSettled()
    {
        var result = Settled(6, Read(24), Read(5));
        Assert.False(result.Quiet);
        Assert.Contains("one read", result.Reasons.Single());
    }

    [Fact]
    public void TheSettleLimitsAreTheBusyLimits()
    {
        Assert.False(Settled(9, Read(BusyJudge.MostCpuPercent)).Quiet);
        Assert.True(Settled(9, Read(BusyJudge.MostCpuPercent - .1)).Quiet);
        Assert.False(Settled(9, Read(3, BusyJudge.MostGpuBeforePercent)).Quiet);
        Assert.Contains("could not be read", Settled(9, Read(3, null)).Reasons.Single());
        Assert.Contains("LM Studio", Settled(9, Read(3, busy: ["LM Studio is running, with 2 models loaded"])).Reasons.Single());
        Assert.Equal(15, BusyJudge.MostCpuPercent);
        Assert.Equal(15, BusyJudge.MostGpuBeforePercent);
    }

    [Fact]
    public void AProofRunReadsOnceAndDoesNotWait()
    {
        var result = Settle.Wait(() => Read(40), () => 0, mostWaitSeconds: 0, readsNeeded: 1);
        Assert.False(result.Quiet);
        Assert.Single(result.Reads);
        Assert.Contains("40% busy", result.Reasons.Single());
    }

    [Fact]
    public void TopProgramsAddUpByNameAndLeaveOutTheIdleOnes()
    {
        Dictionary<int, (string, TimeSpan)> before = new()
        {
            [1] = ("MsMpEng", TimeSpan.FromSeconds(100)),
            [2] = ("dotnet", TimeSpan.FromSeconds(5)),
            [3] = ("idle-one", TimeSpan.FromSeconds(9)),
            [4] = ("gone", TimeSpan.FromSeconds(9)),
        };
        Dictionary<int, (string, TimeSpan)> after = new()
        {
            [1] = ("MsMpEng", TimeSpan.FromSeconds(102)),
            [2] = ("dotnet", TimeSpan.FromSeconds(6)),
            [3] = ("idle-one", TimeSpan.FromSeconds(9)),
            [5] = ("dotnet", TimeSpan.FromSeconds(1)), // Started during the read, so all of its time counts.
        };
        // 2 s of 10 cores is 20 core-seconds.
        var top = ProgramUse.Top(before, after, 20);
        Assert.Equal([new ProgramUse("MsMpEng", 10), new ProgramUse("dotnet", 10)], top.OrderBy(p => p.Name, StringComparer.Ordinal));
        Assert.DoesNotContain(top, p => p.Name == "idle-one" || p.Name == "gone");
    }

    // ---- A signal file left by an earlier harness run must never pass for this run's.

    static string TempFolder()
    {
        var folder = Path.Combine(Path.GetTempPath(), $"op-harness-{Environment.ProcessId}-{Guid.NewGuid():N}");
        Directory.CreateDirectory(folder);
        return folder;
    }

    [Fact]
    public void AnEarlierRunsFoldersAreClearedAndNothingElse()
    {
        var runs = TempFolder();
        try
        {
            var old = Path.Combine(runs, "03-pixels-run-1");
            Directory.CreateDirectory(Path.Combine(old, "data"));
            File.WriteAllText(Path.Combine(old, "first-frame.json"), "{}");
            var pan = Path.Combine(runs, "27-pan");
            Directory.CreateDirectory(pan);
            File.WriteAllText(Path.Combine(pan, "pan.json"), "{}");
            var other = Path.Combine(runs, "keep-me");
            Directory.CreateDirectory(other);
            var file = Path.Combine(runs, "01-a-file.txt");
            File.WriteAllText(file, "");

            RunFolders.Clear(runs);

            Assert.False(Directory.Exists(old));
            Assert.False(Directory.Exists(pan));
            Assert.True(Directory.Exists(other));
            Assert.True(File.Exists(file));
        }
        finally
        {
            Directory.Delete(runs, recursive: true);
        }
    }

    [Fact]
    public void ClearingANewOutputFolderIsFine() => RunFolders.Clear(Path.Combine(Path.GetTempPath(), $"op-harness-none-{Guid.NewGuid():N}"));

    [Fact]
    public void ARunRefusesAFolderThatHoldsAnOldSignal()
    {
        var folder = TempFolder();
        try
        {
            File.WriteAllText(Path.Combine(folder, "first-frame.json"), "{}");
            // The check comes before the launch, so no program starts here.
            var e = Assert.Throws<InvalidOperationException>(() => new AppRun(Path.Combine(folder, "no-such-app.exe"), folder, []));
            Assert.Contains("first-frame.json", e.Message);
        }
        finally
        {
            Directory.Delete(folder, recursive: true);
        }
    }

    [Fact]
    public void ACanvasHandleThatChangesInARunIsAProblem()
    {
        Assert.Null(CanvasHandles.Problem("run 1", [7], signal: 7, now: 7));
        Assert.Null(CanvasHandles.Problem("run 1", [], signal: 7, now: 7));
        Assert.Contains("2 canvas handles", CanvasHandles.Problem("run 1", [6, 7], signal: 7, now: 7));
        Assert.Contains("before the signal", CanvasHandles.Problem("run 1", [6], signal: 7, now: 7));
        Assert.Contains("after the signal", CanvasHandles.Problem("run 1", [7], signal: 7, now: 8));
        Assert.Contains("after the signal", CanvasHandles.Problem("run 1", [7], signal: 7, now: 0));
    }

    [Fact]
    public void NvidiaSmisWordsAreRead()
    {
        var gpu = GpuFacts.Parse("NVIDIA GeForce RTX 3060 Ti, 12, 0\r\n");
        Assert.Equal("NVIDIA GeForce RTX 3060 Ti", gpu.Name);
        Assert.Equal(12, gpu.UsePercent);
        Assert.Equal(0, gpu.EncoderSessions);
        Assert.Null(GpuFacts.Parse("NVIDIA GeForce RTX 3060 Ti, [N/A], [N/A]").UsePercent);
        Assert.Null(GpuFacts.Parse("garbage").UsePercent);
    }

    // ---- 200% is its own row.

    [Fact]
    public void AScaleTheDisplayWasNotAtIsItsOwnNotRunRow()
    {
        var rows = ShotScales.Rows([1.0]).ToList();
        Assert.Equal("window shots at 100%: PASS", rows[0].Line);
        Assert.Equal(PartStatus.NotRun, rows[1].Status);
        Assert.Equal("window shots at 200%", rows[1].Name);
    }

    // ---- The harness's own arguments.

    [Theory]
    [InlineData("NaN")]
    [InlineData("Infinity")]
    [InlineData("-Infinity")]
    [InlineData("0")]
    [InlineData("-3")]
    [InlineData("1e300")]
    [InlineData("soon")]
    public void ABadPanLengthStopsTheHarnessByName(string value)
    {
        var error = Assert.Throws<ArgumentException>(() => OpenPainter.Harness.Program.Options.Parse(["--app", Environment.ProcessPath!, "--out", ".", "--pan-seconds", value]));
        Assert.Contains("--pan-seconds", error.Message);
    }

    [Fact]
    public void AGoodPanLengthIsRead() =>
        Assert.Equal(5.5, OpenPainter.Harness.Program.Options.Parse(["--app", Environment.ProcessPath!, "--out", ".", "--pan-seconds", "5.5"]).PanSeconds);

    [Theory]
    [InlineData(1, 0, 0, 1100, 792)]
    [InlineData(1, 16, 8, 1116, 800)]
    [InlineData(2, 16, 8, 2216, 1592)]
    [InlineData(1.25, 0, 0, 1375, 990)]
    public void TheMinimumTrackIsTheClientMinimumPlusTheFrame(double scale, int frameX, int frameY, int wantX, int wantY) =>
        Assert.Equal((wantX, wantY), WindowLimits.Track(MainWindow.MinimumClient, scale, frameX, frameY));

    [Fact]
    public void StartOptionsReadTheHarnessHooks()
    {
        var folder = Path.GetTempPath();
        var o = StartOptions.Parse(["--data-root", folder, "--test-zoom", "1.5", "--test-first-frame", "f.json", "--test-pan-trace", "p.json", "--test-pan-seconds", "5"]);
        Assert.Equal(folder, o.DataRoot);
        Assert.Equal(1.5, o.TestZoom);
        Assert.Equal("f.json", o.FirstFramePath);
        Assert.Equal("p.json", o.PanTracePath);
        Assert.Equal(5, o.PanSeconds);
        Assert.Equal(30, StartOptions.Parse([]).PanSeconds);
    }

    [Theory]
    [InlineData("--data-root", "no-such-folder-here")]
    [InlineData("--test-zoom", "big")]
    [InlineData("--test-zoom", "0")]
    [InlineData("--test-pan-seconds", "soon")]
    [InlineData("--test-pan-seconds", "-1")]
    public void BadHarnessHookValuesFailAndNameTheArgument(string argument, string value) =>
        Assert.Contains(argument, Assert.Throws<ArgumentException>(() => StartOptions.Parse([argument, value])).Message);
}
