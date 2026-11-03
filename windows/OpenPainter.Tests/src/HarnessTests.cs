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

    const string CleanReport = """{ "panels": { "colors": { "screen": "on top" } }, "problems": [] }""";

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
        var parts = ReportJudge.Parts($$"""{ "problems": ["{{problem}}"] }""", 1, "");
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

    [Fact]
    public void TheHarnessHoldsNoBannedCall()
    {
        // PLAN 4.3: tests use messages to the app's own window, and capture one window.
        foreach (var file in Directory.GetFiles(Repo.File("windows/harness/src"), "*.cs").Concat([Repo.File("windows/OpenPainter/src/Shell/TestProbe.cs")]))
        {
            var text = File.ReadAllText(file);
            foreach (var banned in new[] { "SendInput", "keybd_event", "mouse_event", "SetCursorPos", "CopyFromScreen", "BitBlt" })
                Assert.False(text.Contains($"{banned}(", StringComparison.Ordinal), $"{Path.GetFileName(file)} calls {banned}");
        }
    }

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
