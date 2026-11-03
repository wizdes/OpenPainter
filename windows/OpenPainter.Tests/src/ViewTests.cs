// Zoom and Pan math with no window: the steps and limits against the Mac's source, the
// placement on open, and what each input does.
using System.Text.RegularExpressions;
using Avalonia;
using OpenPainter.Canvas;
using OpenPainter.Shell;
using UiLook;
using Xunit;

namespace OpenPainter.Tests;

public sealed class ViewTests
{
    const string Swift = "mac/OpenPainterKit/Sources/ZoomPresets.swift";

    [Fact]
    public void TheStepsAndLimitsAreTheMacs()
    {
        var source = Repo.Read(Swift);
        var list = Regex.Match(source, @"values: \[Double\] = \[(.*?)\]", RegexOptions.Singleline);
        Assert.True(list.Success, "ZoomPresets.swift changed its list");
        var want = list.Groups[1].Value.Split(',', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries).Select(v => double.Parse(v, System.Globalization.CultureInfo.InvariantCulture)).ToArray();
        Assert.Equal(want, ZoomPresets.Values);
        Assert.Contains("minimum = 0.01", source);
        Assert.Contains("maximum = 32.0", source);
        Assert.Equal(0.01, ZoomPresets.Minimum);
        Assert.Equal(32, ZoomPresets.Maximum);
    }

    [Fact]
    public void ZoomInWalksEveryPresetAndStopsAtTheTop()
    {
        var zoom = 0.01;
        var seen = new List<double> { zoom };
        for (var i = 0; i < 30; i++)
        {
            zoom = ZoomPresets.NextLarger(zoom);
            seen.Add(zoom);
        }
        Assert.Equal(ZoomPresets.Values, seen.Distinct().ToArray());
        Assert.Equal(32, ZoomPresets.NextLarger(32));
    }

    [Fact]
    public void ZoomOutWalksEveryPresetAndStopsAtTheBottom()
    {
        var zoom = 32.0;
        var seen = new List<double> { zoom };
        for (var i = 0; i < 30; i++)
        {
            zoom = ZoomPresets.NextSmaller(zoom);
            seen.Add(zoom);
        }
        Assert.Equal(ZoomPresets.Values.Reverse(), seen.Distinct().ToArray());
        Assert.Equal(0.01, ZoomPresets.NextSmaller(0.01));
    }

    [Theory]
    [InlineData(0.184, 0.25, 0.16)]   // Between presets.
    [InlineData(1.0, 2.0, 0.66)]
    [InlineData(1.003, 2.0, 0.66)]    // Within 0.005 of a preset counts as on it.
    public void AStepFromBetweenPresetsGoesToTheNeighbors(double zoom, double larger, double smaller)
    {
        Assert.Equal(larger, ZoomPresets.NextLarger(zoom));
        Assert.Equal(smaller, ZoomPresets.NextSmaller(zoom));
    }

    [Theory]
    [InlineData(1.0, "100%")]
    [InlineData(16.0, "1600%")]
    [InlineData(0.33, "33%")]
    [InlineData(0.184, "18%")]
    [InlineData(0.05, "5%")]
    [InlineData(0.012, "1.2%")]
    public void TheLabelIsPaintNetsText(double zoom, string label) => Assert.Equal(label, ZoomPresets.Label(zoom));

    [Theory]
    [InlineData("250", 2.5)]
    [InlineData(" 50% ", 0.5)]
    [InlineData("99999", 32.0)]
    [InlineData("0.5", 0.01)]
    public void TheFieldReadsPercents(string text, double zoom) => Assert.Equal(zoom, ZoomPresets.ParseField(text));

    [Theory]
    [InlineData("")]
    [InlineData("abc")]
    [InlineData("0")]
    [InlineData("-5")]
    [InlineData("NaN")]
    public void TheFieldRefusesAnythingElse(string text) => Assert.Null(ZoomPresets.ParseField(text));

    [Fact]
    public void FitLeavesTenPointsAndNeverEnlarges()
    {
        Assert.Equal(1, ZoomPresets.Fit(100, 100, 1000, 1000));
        Assert.Equal(0.5, ZoomPresets.Fit(1000, 400, 510, 510));
        Assert.Equal(1, ZoomPresets.Fit(0, 0, 100, 100));
        Assert.Equal(0.01, ZoomPresets.Fit(100000, 100000, 20, 20));
    }

    /// W6's picture: 4096 by 3072 between the panels, in the 1280 by 672 stage.
    [Fact]
    public void APictureOpensWhereTheLookPutsIt()
    {
        var stage = new Rect(0, 132, Tokens.WindowWidth, Tokens.WindowHeight - 132 - Tokens.StatusHeight);
        var clear = PanelLayout.ClearRect(stage, PanelLayout.Defaults(stage).Values);
        var c = new ViewController();
        c.SetDocumentSize(new Size(4096, 3072));
        c.Resize(stage.Size);
        c.SetClearRect(new Rect(clear.X - stage.X, clear.Y - stage.Y, clear.Width, clear.Height));
        Assert.True(c.IsFitted);
        var image = new Rect(c.View.Origin.X, c.View.Origin.Y + stage.Y, c.View.ContentSize.Width, c.View.ContentSize.Height);
        Assert.Equal(Shell_CanvasRect.X, image.X, 0.01);
        Assert.Equal(Shell_CanvasRect.Y, image.Y, 0.5);
        Assert.Equal(Shell_CanvasRect.Width, image.Width, 0.01);
        Assert.Equal(Shell_CanvasRect.Height, image.Height, 0.01);
    }

    // Shell.CanvasRect in tools/ui-look/app/src/Shell.cs. That file is not part of the app.
    static readonly Rect Shell_CanvasRect = new(272, 185.25, 754, 565.5);

    [Fact]
    public void TheNumbersAboveAreStillTheLooks() =>
        Assert.Contains("CanvasRect = new(272, 185.25, 754, 565.5)", Repo.Read("tools/ui-look/app/src/Shell.cs"));

    static ViewController Big(double zoom = 1)
    {
        var c = new ViewController();
        c.SetDocumentSize(new Size(4000, 3000));
        c.Resize(new Size(1000, 600));
        c.SetZoom(zoom);
        return c;
    }

    [Fact]
    public void ASmallImageIsCenteredAndCannotScroll()
    {
        var c = new ViewController();
        c.SetDocumentSize(new Size(100, 50));
        c.Resize(new Size(1000, 600));
        Assert.Equal(1, c.View.Zoom);
        Assert.Equal(new Point(450, 275), c.View.Origin);
        c.View.Pan(300, 300);
        Assert.Equal(new Point(450, 275), c.View.Origin);
    }

    [Fact]
    public void ALargeImageCannotScrollPastItsEdges()
    {
        var c = Big();
        c.View.Pan(100000, 100000);
        Assert.Equal(new Point(0, 0), c.View.Origin);
        c.View.Pan(-100000, -100000);
        Assert.Equal(new Point(-3000, -2400), c.View.Origin);
    }

    [Fact]
    public void AnImageSmallerThanTheViewCentersOnTheClearRect()
    {
        var c = new ViewController();
        c.SetDocumentSize(new Size(100, 100));
        c.Resize(new Size(1000, 600));
        c.SetClearRect(new Rect(300, 16, 200, 568));
        Assert.Equal(350, c.View.Origin.X);
        Assert.Equal(250, c.View.Origin.Y);
    }

    [Fact]
    public void FitHoldsThroughResizesUntilTheZoomChanges()
    {
        var c = Big(zoom: 1);
        Assert.False(c.IsFitted);
        c.Fit();
        Assert.True(c.IsFitted);
        var first = c.View.Zoom;
        c.Resize(new Size(2000, 1200));
        Assert.True(c.IsFitted);
        Assert.True(c.View.Zoom > first);
        c.ZoomIn();
        Assert.False(c.IsFitted);
        var chosen = c.View.Zoom;
        c.Resize(new Size(1000, 600));
        Assert.Equal(chosen, c.View.Zoom);
    }

    [Fact]
    public void AClearRectFitsAFittedImageAndKeepsAChosenZoom()
    {
        var c = Big(zoom: 1);
        c.SetClearRect(new Rect(100, 16, 400, 568));
        Assert.Equal(1, c.View.Zoom);
        c.Fit();
        Assert.Equal(400 / 4000.0, c.View.Zoom, 9);
        c.SetClearRect(new Rect(100, 16, 800, 568));
        Assert.Equal(568.0 / 3000, c.View.Zoom, 9);
    }

    [Fact]
    public void CtrlWheelZoomsByTheMacFactorAroundThePointer()
    {
        var c = Big();
        var p = new Point(310, 170);
        var before = c.View.DocumentPoint(p);
        c.Wheel(p, 0, 1, 3, ctrl: true, shift: false);
        Assert.Equal(1.12, c.View.Zoom, 9);
        var after = c.View.DocumentPoint(p);
        Assert.Equal(before.X, after.X, 6);
        Assert.Equal(before.Y, after.Y, 6);
        c.Wheel(p, 0, -2, 3, ctrl: true, shift: false);
        Assert.Equal(1.12 / (1.12 * 1.12), c.View.Zoom, 9);
    }

    [Fact]
    public void WheelZoomStopsAtTheLimits()
    {
        var c = Big();
        c.Wheel(default, 0, 500, 3, ctrl: true, shift: false);
        Assert.Equal(32, c.View.Zoom);
        c.Wheel(default, 0, -5000, 3, ctrl: true, shift: false);
        Assert.Equal(0.01, c.View.Zoom);
    }

    [Fact]
    public void APlainWheelScrollsAndShiftTurnsItSideways()
    {
        var c = Big();
        var start = c.View.Origin;
        c.Wheel(default, 0, -1, 3, ctrl: false, shift: false);
        Assert.Equal(start + new Point(0, -48), c.View.Origin);
        var next = c.View.Origin;
        c.Wheel(default, 0, -1, 3, ctrl: false, shift: true);
        Assert.Equal(next + new Point(-48, 0), c.View.Origin);
        var last = c.View.Origin;
        c.Wheel(default, -1, 0, 3, ctrl: false, shift: false);
        Assert.Equal(last + new Point(-48, 0), c.View.Origin);
    }

    [Fact]
    public void ThePanToolDragsTheView()
    {
        var c = Big();
        c.Tool = CanvasTool.Pan;
        var changes = 0;
        c.Changed += () => changes++;
        var start = c.View.Origin;
        c.PointerDown(new Point(100, 100), CanvasButton.Left, alt: false);
        Assert.True(c.Dragging);
        c.PointerMove(new Point(90, 110));
        c.PointerMove(new Point(70, 130));
        c.PointerUp(new Point(70, 130));
        Assert.False(c.Dragging);
        Assert.Equal(start + new Point(-30, 30), c.View.Origin);
        Assert.Equal(2, changes);
    }

    [Fact]
    public void AMoveWithNoPressOnlyReportsThePointer()
    {
        var c = Big();
        c.Tool = CanvasTool.Pan;
        Point? seen = null;
        c.PointerAt += p => seen = p;
        var start = c.View.Origin;
        c.PointerMove(new Point(500, 300));
        Assert.Equal(start, c.View.Origin);
        Assert.Equal(c.View.DocumentPoint(new Point(500, 300)), seen);
        c.PointerLeft();
        Assert.Null(seen);
    }

    [Fact]
    public void SpacePansWithAnyToolAndOnlyWhileHeld()
    {
        var c = Big();
        Assert.Equal(CanvasCursor.Arrow, c.Cursor);
        c.SpaceDown();
        Assert.Equal(CanvasTool.Pan, c.EffectiveTool);
        Assert.Equal(CanvasCursor.Move, c.Cursor);
        var start = c.View.Origin;
        c.PointerDown(new Point(100, 100), CanvasButton.Left, alt: false);
        c.PointerMove(new Point(120, 100));
        c.PointerUp(new Point(120, 100));
        Assert.Equal(start + new Point(20, 0), c.View.Origin);
        c.SpaceUp();
        Assert.Equal(CanvasTool.Inactive, c.EffectiveTool);
        var after = c.View.Origin;
        c.PointerDown(new Point(100, 100), CanvasButton.Left, alt: false);
        c.PointerMove(new Point(160, 100));
        c.PointerUp(new Point(160, 100));
        Assert.Equal(after, c.View.Origin);
    }

    [Fact]
    public void LosingFocusLetsGoOfSpaceAndTheDrag()
    {
        var c = Big();
        c.SpaceDown();
        c.PointerDown(new Point(100, 100), CanvasButton.Left, alt: false);
        c.FocusLost();
        Assert.False(c.Dragging);
        Assert.Equal(CanvasTool.Inactive, c.EffectiveTool);
    }

    [Fact]
    public void AZoomClickStepsInAndCentersOnThePoint()
    {
        var c = Big();
        c.Tool = CanvasTool.Zoom;
        var p = new Point(600, 320);
        var doc = c.View.DocumentPoint(p);
        c.PointerDown(p, CanvasButton.Left, alt: false);
        c.PointerUp(p);
        Assert.Equal(2, c.View.Zoom);
        var center = c.View.DocumentPoint(c.View.ViewCenter);
        Assert.Equal(doc.X, center.X, 6);
        Assert.Equal(doc.Y, center.Y, 6);
    }

    [Fact]
    public void AltClickAndRightClickStepOut()
    {
        var c = Big();
        c.Tool = CanvasTool.Zoom;
        c.PointerDown(new Point(50, 50), CanvasButton.Left, alt: true);
        c.PointerUp(new Point(50, 50));
        Assert.Equal(0.66, c.View.Zoom);
        c.PointerDown(new Point(50, 50), CanvasButton.Right, alt: false);
        Assert.Equal(0.5, c.View.Zoom);
    }

    [Fact]
    public void ARightClickDoesNothingWithOtherTools()
    {
        var c = Big();
        c.Tool = CanvasTool.Pan;
        c.PointerDown(new Point(50, 50), CanvasButton.Right, alt: false);
        Assert.Equal(1, c.View.Zoom);
        Assert.False(c.Dragging);
    }

    [Fact]
    public void AZoomDragShowsARectangleThenFitsIt()
    {
        var c = Big();
        c.Tool = CanvasTool.Zoom;
        var rectangles = new List<Rect?>();
        c.ZoomRectangle += r => rectangles.Add(r);
        var a = c.View.DocumentPoint(new Point(300, 100));
        var b = c.View.DocumentPoint(new Point(100, 200));
        c.PointerDown(new Point(300, 100), CanvasButton.Left, alt: false);
        c.PointerMove(new Point(100, 200));
        c.PointerUp(new Point(100, 200));
        Assert.Equal(new Rect(100, 100, 200, 100), rectangles[0]);
        Assert.Null(rectangles[^1]);
        var want = Math.Min(1000 / (Math.Abs(a.X - b.X) + 2), 600 / (Math.Abs(a.Y - b.Y) + 2));
        Assert.Equal(want, c.View.Zoom, 9);
    }

    [Fact]
    public void ACancelledDragDoesNothing()
    {
        var c = Big();
        c.Tool = CanvasTool.Zoom;
        Rect? last = new Rect(1, 1, 1, 1);
        c.ZoomRectangle += r => last = r;
        c.PointerDown(new Point(300, 100), CanvasButton.Left, alt: false);
        c.PointerMove(new Point(100, 200));
        c.CancelDrag();
        c.PointerUp(new Point(100, 200));
        Assert.Equal(1, c.View.Zoom);
        Assert.Null(last);
    }

    /// W5's rule: nothing that needs full rate may use Avalonia's UI frame callback.
    [Theory]
    [InlineData("windows/OpenPainter/src/Canvas/CanvasHost.cs")]
    [InlineData("windows/OpenPainter/src/Canvas/CanvasSession.cs")]
    [InlineData("windows/OpenPainter/src/Canvas/ViewController.cs")]
    [InlineData("windows/OpenPainter/src/Canvas/Viewport.cs")]
    [InlineData("windows/OpenPainter/src/Shell/MainWindow.cs")]
    public void NothingOnTheCanvasPathWaitsForAUiFrame(string file)
    {
        var source = Repo.Read(file);
        foreach (var name in new[] { "RequestAnimationFrame", "CompositionTarget", "DispatcherTimer", "RequestCompositionUpdate" })
            Assert.DoesNotContain(name, source);
    }

    [Fact]
    public void APreviewAveragesAndKeepsItsShape()
    {
        var rgba = new byte[8 * 4 * 4];
        for (var i = 0; i < rgba.Length; i += 4)
        {
            rgba[i] = 200;
            rgba[i + 3] = 255;
        }
        var (w, h, small) = CanvasSession.Shrink(rgba, 8, 4, 4, 4);
        Assert.Equal((4, 2), (w, h));
        Assert.Equal(4 * 2 * 4, small.Length);
        Assert.Equal(200, small[0]);
        Assert.Equal(255, small[3]);
        var (bw, bh, _) = CanvasSession.Shrink(rgba, 8, 4, 100, 100);
        Assert.Equal((8, 4), (bw, bh));
    }

    [Fact]
    public void APreviewIgnoresTheColorOfClearPixels()
    {
        var rgba = new byte[] { 255, 0, 0, 0, 10, 20, 30, 255 };
        var (w, h, small) = CanvasSession.Shrink(rgba, 2, 1, 1, 1);
        Assert.Equal((1, 1), (w, h));
        Assert.Equal(new byte[] { 10, 20, 30 }, small[..3]);
        Assert.InRange(small[3], 100, 150);
    }
}
