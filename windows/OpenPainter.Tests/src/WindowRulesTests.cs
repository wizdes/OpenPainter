// The window's rules that a real window also proves in the harness: the app ends with its
// main window, the window fits its monitor, and a panel cannot be lost off screen.
using Avalonia;
using Avalonia.Controls;
using Avalonia.Controls.ApplicationLifetimes;
using Avalonia.Headless.XUnit;
using Avalonia.Threading;
using OpenPainter.Shell;
using Xunit;

namespace OpenPainter.Tests;

public sealed class WindowRulesTests
{
    static MainWindow Ready()
    {
        var window = Shells.Open();
        Dispatcher.UIThread.RunJobs();
        return window;
    }

    // ---- The app ends when the main window closes.

    [AvaloniaFact]
    public void TheAppEndsWithItsMainWindowAndNotWithItsLastWindow()
    {
        using var lifetime = new ClassicDesktopStyleApplicationLifetime();
        Assert.Equal(ShutdownMode.OnLastWindowClose, lifetime.ShutdownMode);
        App.ConfigureLifetime(lifetime);
        Assert.Equal(ShutdownMode.OnMainWindowClose, lifetime.ShutdownMode);
    }

    [AvaloniaFact]
    public void HidingAPanelLeavesItWithNoOwner()
    {
        // Why the rule above exists: nothing closes a hidden panel along with its owner.
        var w = Ready();
        var panel = w.Panels.Panels[PanelLayout.Colors];
        w.Panels.SetShown(PanelLayout.Colors, false);
        Assert.Null(panel.Owner);
        w.Close();
    }

    [AvaloniaFact]
    public void ClosingTheMainWindowClosesEveryPanelWhenAllShow() => ClosesEveryPanel();

    [AvaloniaFact]
    public void ClosingTheMainWindowClosesEveryPanelWhenOneIsHidden() => ClosesEveryPanel(PanelLayout.Colors);

    [AvaloniaFact]
    public void ClosingTheMainWindowClosesEveryPanelWhenAllAreHidden() =>
        ClosesEveryPanel(PanelLayout.Tools, PanelLayout.History, PanelLayout.Layers, PanelLayout.Colors);

    static void ClosesEveryPanel(params string[] hidden)
    {
        var w = Ready();
        foreach (var id in hidden) w.Panels.SetShown(id, false);
        var closed = new List<string>();
        foreach (var (id, panel) in w.Panels.Panels) panel.Closed += (_, _) => closed.Add(id);
        w.Close();
        Dispatcher.UIThread.RunJobs();
        Assert.Equal(w.Panels.Panels.Keys.Order(), closed.Order());
    }

    [AvaloniaFact]
    public void APanelsCloseStillOnlyHidesItWhileTheMainWindowIsOpen()
    {
        var w = Ready();
        var panel = w.Panels.Panels[PanelLayout.History];
        var closed = false;
        panel.Closed += (_, _) => closed = true;
        panel.Close();
        Dispatcher.UIThread.RunJobs();
        Assert.False(closed);
        Assert.False(w.Panels.IsShown(PanelLayout.History));
        w.Close();
    }

    // ---- The window fits the monitor.

    [Theory]
    // Plenty of room: the Mac's minimum holds.
    [InlineData(1, 1920, 1040, 16, 8, 1100, 792)]
    [InlineData(1.5, 2560, 1400, 16, 8, 1100, 792)]
    [InlineData(2, 3840, 2100, 16, 8, 1100, 792)]
    // 1366 by 768 at 100%, with a 40 pixel taskbar.
    [InlineData(1, 1366, 728, 16, 8, 1100, 720)]
    // 1080p at 150%: 792 points would be 1188 pixels.
    [InlineData(1.5, 1920, 1040, 16, 8, 1100, 688)]
    // 1080p at 200%: both sides are over.
    [InlineData(2, 1920, 1040, 16, 8, 952, 516)]
    // 1366 by 768 at 150% and at 200%.
    [InlineData(1.5, 1366, 728, 16, 8, 900, 480)]
    [InlineData(2, 1366, 728, 16, 8, 675, 360)]
    public void TheMinimumIsCappedToTheWorkArea(double scale, int workWidth, int workHeight, int frameX, int frameY, double wantWidth, double wantHeight)
    {
        var work = new PixelSize(workWidth, workHeight);
        var capped = WindowLimits.Capped(MainWindow.MinimumClient, scale, work, frameX, frameY);
        Assert.Equal(new Size(wantWidth, wantHeight), capped);
        var (trackX, trackY) = WindowLimits.Track(capped, scale, frameX, frameY);
        Assert.True(trackX <= workWidth, $"{trackX} is wider than the {workWidth} pixel work area");
        Assert.True(trackY <= workHeight, $"{trackY} is taller than the {workHeight} pixel work area");
    }

    [Fact]
    public void AWorkAreaSmallerThanTheFrameLeavesAPositiveMinimum()
    {
        var capped = WindowLimits.Capped(MainWindow.MinimumClient, 2, new PixelSize(10, 4), 16, 8);
        Assert.True(capped.Width > 0 && capped.Height > 0, $"{capped}");
    }

    [AvaloniaFact]
    public void ATooBigWindowShrinksAndItsStatusBarStaysInside()
    {
        var w = Ready();
        Assert.Equal(UiLook.Tokens.WindowHeight, w.Height);
        Assert.True(w.FitTo(new PixelSize(1920, 1040), 1.5, 16, 8));
        Dispatcher.UIThread.RunJobs();
        Assert.Equal(new Size(1100, 688), w.LeastClient);
        Assert.Equal(688, w.MinHeight);
        Assert.Equal(688, w.Height);
        Assert.True(w.Width <= Math.Ceiling((1920 - 16) / 1.5), $"the window is {w.Width} wide");
        var status = w.Status.TranslatePoint(new Point(0, w.Status.Bounds.Height), w)!.Value;
        Assert.True(status.Y <= 688, $"the status bar ends at {status.Y}");
        Assert.True(w.Stage.Bounds.Height > 0, "no room is left for the canvas");
        w.Close();
    }

    [AvaloniaFact]
    public void AWindowThatFitsKeepsItsSizeAndTheFullMinimum()
    {
        var w = Ready();
        Assert.False(w.FitTo(new PixelSize(1920, 1040), 1, 16, 8));
        Assert.Equal(MainWindow.MinimumClient, w.LeastClient);
        Assert.Equal(UiLook.Tokens.WindowHeight, w.Height);
        w.Close();
    }

    [AvaloniaFact]
    public void AMoveOntoASmallMonitorCapsTheMinimumAndKeepsTheSize()
    {
        // A big window spread over two monitors must survive a move.
        var w = Ready();
        var width = w.Width;
        w.CapMinimum(new PixelSize(1000, 600), 1, 16, 8);
        Assert.Equal(new Size(984, 592), w.LeastClient);
        Assert.Equal(width, w.Width);
        Assert.Equal(UiLook.Tokens.WindowHeight, w.Height);
        w.Close();
    }

    [AvaloniaFact]
    public void AMonitorWithMoreRoomGivesTheFullMinimumBack()
    {
        var w = Ready();
        w.FitTo(new PixelSize(1920, 1040), 2, 16, 8);
        w.FitTo(new PixelSize(3840, 2100), 2, 16, 8);
        Assert.Equal(MainWindow.MinimumClient, w.LeastClient);
        Assert.Equal(1100, w.MinWidth);
        Assert.Equal(792, w.MinHeight);
        w.Close();
    }

    // ---- PanelLayout's on-screen math, from PanelLayout.swift, with y growing down.

    static readonly Rect Laptop = new(0, 0, 1440, 875);
    static readonly Rect External = new(1440, -200, 2560, 1415);

    [Fact]
    public void ClampMovesAFrameInsideAndShrinksOnlyWhenLarger()
    {
        Assert.Equal(new Rect(0, 0, 100, 100), PanelLayout.Clamp(new Rect(-50, -50, 100, 100), Laptop));
        Assert.Equal(new Rect(1340, 775, 100, 100), PanelLayout.Clamp(new Rect(5000, 5000, 100, 100), Laptop));
        Assert.Equal(new Rect(200, 300, 100, 100), PanelLayout.Clamp(new Rect(200, 300, 100, 100), Laptop));
        Assert.Equal(new Rect(0, 0, 1440, 875), PanelLayout.Clamp(new Rect(10, 10, 3000, 3000), Laptop));
    }

    [Theory]
    [InlineData(100, 100, true)]            // Well inside.
    [InlineData(-200, 100, true)]           // Mostly off the left, 60 points of title left.
    [InlineData(-201, 100, false)]          // 59 points left is too little.
    [InlineData(100, -4, true)]             // 20 points of the title strip still show.
    [InlineData(100, -5, false)]            // Above the screen, 19 points show.
    [InlineData(100, 855, true)]            // 20 points show at the bottom.
    [InlineData(100, 856, false)]
    [InlineData(5000, 100, false)]          // No screen at all.
    public void AFrameIsReachableWhenItsTitleStripIsMostlyOnAScreen(double x, double y, bool want) =>
        Assert.Equal(want, PanelLayout.IsReachable(new Rect(x, y, 260, 300), [Laptop]));

    [Fact]
    public void ANarrowPanelNeedsOnlyItsWholeWidthOnScreen() =>
        Assert.True(PanelLayout.IsReachable(new Rect(0, 100, 40, 300), [Laptop]));

    [Fact]
    public void AFrameOnAMissingScreenComesFullyOntoTheOneThatIsLeft()
    {
        // PanelTests.testMissingDisplayMovesPanelsBackOnScreen.
        var back = PanelLayout.OnScreen(new Rect(2000, 300, 260, 300), [Laptop]);
        Assert.True(Laptop.Contains(back), $"{back}");
        Assert.True(PanelLayout.IsReachable(back, [Laptop]));
    }

    [Fact]
    public void AReachableFrameStaysWhereItIs()
    {
        var frame = new Rect(100, 100, 260, 300);
        Assert.Equal(frame, PanelLayout.OnScreen(frame, [Laptop, External]));
    }

    [Fact]
    public void AFrameWithNoScreensStaysWhereItIs()
    {
        var frame = new Rect(9000, 9000, 260, 300);
        Assert.Equal(frame, PanelLayout.OnScreen(frame, []));
        Assert.Equal(frame, PanelLayout.Reachable(frame, []));
    }

    [Fact]
    public void AFrameOverlappingNoScreenGoesToTheNearest()
    {
        var back = PanelLayout.OnScreen(new Rect(5000, 300, 260, 300), [Laptop, External]);
        Assert.True(External.Contains(back), $"{back}");
    }

    [Theory]
    [InlineData(-30000, -30000)]
    [InlineData(30000, 30000)]
    [InlineData(-30000, 200)]
    [InlineData(200, 30000)]
    [InlineData(200, -30000)]
    [InlineData(1439, 900)]
    public void ADragStopsWhereTheTitleStripIsStillReachable(double x, double y)
    {
        var screens = new[] { Laptop, External };
        var kept = PanelLayout.Reachable(new Rect(x, y, 260, 300), screens);
        Assert.True(PanelLayout.IsReachable(kept, screens), $"{kept}");
        Assert.Equal(new Size(260, 300), kept.Size);
    }

    [Fact]
    public void ADragMovesThePanelNoFurtherThanItMust()
    {
        // Held past the bottom: it stays at the edge, and does not jump back up the screen.
        var kept = PanelLayout.Reachable(new Rect(300, 2000, 260, 300), [Laptop]);
        Assert.Equal(new Rect(300, 855, 260, 300), kept);
    }

    // ---- The panel manager, with the screens given, as the Mac's tests give them.

    static readonly IReadOnlyList<Rect> LaptopOnly = [Laptop];

    [AvaloniaFact]
    public void WhenAMonitorGoesAwayEveryPanelComesBackShownOrHidden()
    {
        var w = Ready();
        var panels = w.Panels;
        panels.Screens = () => [Laptop, External];
        panels.SetShown(PanelLayout.Colors, false);
        foreach (var panel in panels.Panels.Values)
            panels.SetFrame(panel, new Rect(2000, 300, panel.ClientSize.Width, panel.ClientSize.Height));
        Dispatcher.UIThread.RunJobs();
        foreach (var panel in panels.Panels.Values) Assert.True(External.Intersects(panels.FrameOf(panel)), panel.Panel.Id);

        panels.Screens = () => LaptopOnly;
        var changes = 0;
        panels.Changed += () => changes++;
        panels.ScreensChanged();
        Dispatcher.UIThread.RunJobs();
        Assert.True(changes > 0);
        foreach (var panel in panels.Panels.Values)
        {
            var frame = panels.FrameOf(panel);
            Assert.True(Laptop.Contains(frame), $"{panel.Panel.Id} is at {frame}");
        }
        w.Close();
    }

    [AvaloniaFact]
    public void APanelOnAScreenThatStaysDoesNotMoveWhenAnotherGoesAway()
    {
        var w = Ready();
        var panels = w.Panels;
        panels.Screens = () => [Laptop, External];
        var panel = panels.Panels[PanelLayout.Tools];
        panels.SetFrame(panel, new Rect(200, 100, panel.ClientSize.Width, panel.ClientSize.Height));
        var before = panels.FrameOf(panel);
        panels.Screens = () => LaptopOnly;
        panels.ScreensChanged();
        Assert.Equal(before, panels.FrameOf(panel));
        w.Close();
    }

    [AvaloniaFact]
    public void APanelMovedOffEveryScreenIsKeptReachable()
    {
        var w = Ready();
        var panels = w.Panels;
        panels.Screens = () => LaptopOnly;
        var panel = panels.Panels[PanelLayout.Layers];
        panel.Position = new PixelPoint(30000, 30000);
        Dispatcher.UIThread.RunJobs();
        var frame = panels.FrameOf(panel);
        Assert.True(PanelLayout.IsReachable(frame, LaptopOnly), $"{frame}");
        w.Close();
    }

    [AvaloniaFact]
    public void APanelMovedWithinAScreenStaysWhereItWasPut()
    {
        var w = Ready();
        var panels = w.Panels;
        panels.Screens = () => LaptopOnly;
        var panel = panels.Panels[PanelLayout.Layers];
        panel.Position = new PixelPoint(400, 300);
        Dispatcher.UIThread.RunJobs();
        Assert.Equal(new PixelPoint(400, 300), panel.Position);
        w.Close();
    }
}
