// The window's layout, the caption buttons and the Tab handoff, with no real window.
using System.Text.RegularExpressions;
using Avalonia;
using Avalonia.Controls;
using Avalonia.Controls.Chrome;
using Avalonia.Headless;
using Avalonia.Headless.XUnit;
using Avalonia.Input;
using Avalonia.Media;
using Avalonia.Media.Imaging;
using Avalonia.Platform;
using Avalonia.Styling;
using Avalonia.Threading;
using Avalonia.VisualTree;
using OpenPainter.Shell;
using UiLook;
using Xunit;

namespace OpenPainter.Tests;

public sealed class ShellTests
{
    static Rect BoundsIn(Control control, Window window)
    {
        var at = control.TranslatePoint(default, window);
        Assert.True(at.HasValue, $"{control.GetType().Name} is not in the window");
        return new Rect(at.Value, control.Bounds.Size);
    }

    static void Settle(Window window)
    {
        Dispatcher.UIThread.RunJobs();
        window.UpdateLayout();
    }

    static unsafe Color Pixel(Bitmap frame, PixelPoint at)
    {
        var bytes = new byte[4];
        fixed (byte* p = bytes)
            frame.CopyPixels(new PixelRect(at, new PixelSize(1, 1)), (nint)p, 4, 4);
        return frame.Format == PixelFormats.Rgba8888
            ? Color.FromArgb(bytes[3], bytes[0], bytes[1], bytes[2])
            : Color.FromArgb(bytes[3], bytes[2], bytes[1], bytes[0]);
    }

    [AvaloniaFact]
    public void RowsSitWhereTheLookSaysAtTheMacWindowSize()
    {
        var w = Shells.Open();
        Settle(w);
        Assert.Equal(new Rect(0, 0, Tokens.WindowWidth, Tokens.TitleHeight), BoundsIn(w.TitleBar, w));
        Assert.Equal(new Rect(0, Tokens.WindowHeight - Tokens.StatusHeight, Tokens.WindowWidth, Tokens.StatusHeight), BoundsIn(w.Status, w));
        var layer = w.CanvasInput.GetVisualParent() as Control;
        Assert.NotNull(layer);
        Assert.Equal(new Rect(0, Tokens.TitleHeight, Tokens.WindowWidth, Tokens.WindowHeight - Tokens.TitleHeight - Tokens.StatusHeight), BoundsIn(layer, w));
        w.Close();
    }

    [AvaloniaFact]
    public void TheCanvasTakesAllSpareRoomWhenTheWindowGrows()
    {
        var w = Shells.Open(width: 1600, height: 1000);
        Settle(w);
        Assert.Equal(1600, w.CanvasInput.Bounds.Width);
        Assert.Equal(1000 - Tokens.TitleHeight - Tokens.StatusHeight, w.CanvasInput.Bounds.Height);
        Assert.Equal(1000 - Tokens.StatusHeight, BoundsIn(w.Status, w).Y);
        w.Close();
    }

    [AvaloniaFact]
    public void TheWindowHasTheMacMinimumSize()
    {
        var w = Shells.Open();
        var source = Repo.Read("mac/OpenPainterKit/Sources/Shell/EditorWindowController.swift");
        var match = Regex.Match(source, @"minimumSize = NSSize\(width: (\d+), height: (\d+)\)");
        Assert.True(match.Success, "EditorWindowController.minimumSize is gone");
        Assert.Equal(double.Parse(match.Groups[1].Value), w.MinWidth);
        Assert.Equal(double.Parse(match.Groups[2].Value), w.MinHeight);
        w.Close();
    }

    [AvaloniaFact]
    public void CaptionButtonsAreOnTheRightInWindowsOrderAtTheWindowsSize()
    {
        var w = Shells.Open();
        Settle(w);
        var row = w.TitleBar;
        var close = BoundsIn(row.CloseButton, w);
        var max = BoundsIn(row.MaximizeButton, w);
        var min = BoundsIn(row.MinimizeButton, w);
        Assert.Equal(Tokens.WindowWidth, close.Right);
        Assert.Equal(close.X, max.Right);
        Assert.Equal(max.X, min.Right);
        foreach (var b in new[] { close, max, min })
        {
            Assert.Equal(Tokens.CaptionButtonWidth, b.Width);
            Assert.Equal(Tokens.TitleHeight, b.Height);
            Assert.Equal(0, b.Y);
        }
        w.Close();
    }

    [AvaloniaFact]
    public void TheTitleHasTheAppNameAndSitsBetweenMenusAndButtons()
    {
        var w = Shells.Open();
        Settle(w);
        Assert.Equal("Untitled - OpenPainter", w.Title);
        Assert.Equal("Untitled - OpenPainter", w.TitleBar.TitleText);
        Assert.Equal("OpenPainter", TitleRow.TitleFor(null));
        var menus = BoundsIn(w.TitleBar.Menus, w);
        var min = BoundsIn(w.TitleBar.MinimizeButton, w);
        Assert.True(menus.Right < min.X, "the menus overlap the caption buttons");
        Assert.Equal(6, menus.X);
        w.Close();
    }

    [AvaloniaFact]
    public void EachPartOfTheTitleRowHasTheRoleWindowsAsksAbout()
    {
        var w = Shells.Open();
        var row = w.TitleBar;
        Assert.Equal(WindowDecorationsElementRole.TitleBar, WindowDecorationProperties.GetElementRole(row));
        Assert.Equal(WindowDecorationsElementRole.MinimizeButton, WindowDecorationProperties.GetElementRole(row.MinimizeButton));
        Assert.Equal(WindowDecorationsElementRole.MaximizeButton, WindowDecorationProperties.GetElementRole(row.MaximizeButton));
        Assert.Equal(WindowDecorationsElementRole.CloseButton, WindowDecorationProperties.GetElementRole(row.CloseButton));
        var tops = row.Menus.Items.OfType<MenuItem>().ToList();
        Assert.Equal(9, tops.Count);
        foreach (var top in tops) Assert.Equal(WindowDecorationsElementRole.User, WindowDecorationProperties.GetElementRole(top));
        Assert.True(w.ExtendClientAreaToDecorationsHint);
        Assert.Equal(Tokens.TitleHeight, w.ExtendClientAreaTitleBarHeightHint);
        w.Close();
    }

    [AvaloniaFact]
    public void ThePointerOnCloseDrawsTheWindowsRedAndAWhiteGlyph()
    {
        var w = Shells.Open();
        Settle(w);
        var close = w.TitleBar.CloseButton;
        var bounds = BoundsIn(close, w);
        Assert.Equal(Colors.Transparent, close.CurrentFill);
        w.MouseMove(bounds.Center, RawInputModifiers.None);
        Settle(w);
        Assert.Equal(CaptionLook.CloseHover, close.CurrentFill);
        Assert.Equal(Colors.White, close.CurrentGlyph);
        using (var frame = w.CaptureRenderedFrame())
        {
            Assert.NotNull(frame);
            Assert.Equal(CaptionLook.CloseHover, Pixel(frame, new PixelPoint((int)bounds.X + 3, 3)));
        }
        w.MouseMove(new Point(600, 400), RawInputModifiers.None);
        Settle(w);
        Assert.Equal(Colors.Transparent, close.CurrentFill);
        w.Close();
    }

    [Fact]
    public void OtherCaptionButtonsHoverInTheInkColorNotRed()
    {
        var p = Tokens.Light;
        var fill = CaptionLook.Fill(CaptionKind.Maximize, hover: true, pressed: false, p);
        Assert.Equal((p.Ink.R, p.Ink.G, p.Ink.B), (fill.R, fill.G, fill.B));
        Assert.True(fill.A is > 0 and < 64, $"alpha {fill.A}");
        Assert.Equal(CaptionLook.ClosePressed, CaptionLook.Fill(CaptionKind.Close, hover: true, pressed: true, p));
    }

    [AvaloniaFact]
    public void AnInactiveWindowTintsItsTitleAndGlyphs()
    {
        var w = Shells.Open();
        var row = w.TitleBar;
        row.SetActive(true);
        Assert.Equal(w.Palette.Muted, ((ISolidColorBrush)row.TitleBrush!).Color);
        Assert.Equal(w.Palette.Ink, row.CloseButton.CurrentGlyph);
        row.SetActive(false);
        Assert.Equal(w.Palette.Disabled, ((ISolidColorBrush)row.TitleBrush!).Color);
        Assert.Equal(w.Palette.Disabled, row.CloseButton.CurrentGlyph);
        Assert.Equal(w.Palette.Disabled, row.MinimizeButton.CurrentGlyph);
        var menu = row.Menus.Items.OfType<MenuItem>().First();
        Assert.Equal(w.Palette.Muted, ((ISolidColorBrush)menu.Foreground!).Color);
        w.Close();
    }

    [AvaloniaFact]
    public void CaptionClicksChangeTheWindowState()
    {
        var w = Shells.Open();
        Settle(w);
        void Click(Control c)
        {
            var at = BoundsIn(c, w).Center;
            w.MouseMove(at, RawInputModifiers.None);
            w.MouseDown(at, MouseButton.Left);
            w.MouseUp(at, MouseButton.Left);
            Settle(w);
        }
        Click(w.TitleBar.MaximizeButton);
        Assert.Equal(WindowState.Maximized, w.WindowState);
        Assert.True(w.TitleBar.MaximizeButton.Maximized);
        Assert.Equal("Restore", Avalonia.Automation.AutomationProperties.GetName(w.TitleBar.MaximizeButton));
        Click(w.TitleBar.MaximizeButton);
        Assert.Equal(WindowState.Normal, w.WindowState);
        Assert.False(w.TitleBar.MaximizeButton.Maximized);
        var closed = false;
        w.Closed += (_, _) => closed = true;
        Click(w.TitleBar.CloseButton);
        Assert.True(closed);
    }

    [AvaloniaFact]
    public void TheMinimizeButtonMinimizes()
    {
        var w = Shells.Open();
        w.TitleBar.MinimizeButton.RaiseEvent(new Avalonia.Interactivity.RoutedEventArgs(Button.ClickEvent));
        Assert.Equal(WindowState.Minimized, w.WindowState);
        w.Close();
    }

    [AvaloniaFact]
    public void RowsUseTheTokenColorsInBothThemes()
    {
        foreach (var (theme, palette) in new[] { ("light", Tokens.Light), ("dark", Tokens.Dark) })
        {
            var w = Shells.Open(theme);
            Assert.Equal(palette, w.Palette);
            Assert.Equal(palette.TitleBar, ((ISolidColorBrush)w.TitleBar.Background!).Color);
            Assert.Equal(palette.Chrome, ((ISolidColorBrush)w.Status.Background!).Color);
            Assert.Equal(palette.Stage, ((ISolidColorBrush)w.Background!).Color);
            w.Close();
        }
    }

    [AvaloniaFact]
    public void AThemeChangeRebuildsTheRowsAndKeepsTheCanvasStop()
    {
        var w = Shells.Open("light");
        var canvas = w.CanvasInput;
        w.Status.ImageValue.Text = "4096 × 3072 px";
        w.RequestedThemeVariant = ThemeVariant.Dark;
        Settle(w);
        Assert.Equal(Tokens.Dark, w.Palette);
        Assert.Equal(Tokens.Dark.Chrome, ((ISolidColorBrush)w.Status.Background!).Color);
        Assert.Same(canvas, w.CanvasInput);
        Assert.Equal("4096 × 3072 px", w.Status.ImageValue.Text);
        Assert.True(w.CanvasInput.GetVisualParent() is not null);
        w.Close();
    }

    /// The labels in the Mac's order, read from its source so the words cannot drift.
    [AvaloniaFact]
    public void TheStatusBarHasTheMacWordsInTheMacOrder()
    {
        var source = Repo.Read("mac/OpenPainterKit/Sources/Shell/StatusBarView.swift");
        var list = Regex.Match(source, @"for \(label, value\) in \[(.*?)\] \{");
        Assert.True(list.Success, "StatusBarView.swift changed its label list");
        var want = Regex.Matches(list.Groups[1].Value, "\"(\\w+)\"").Select(m => m.Groups[1].Value).ToList();
        want.Add("Units");
        Assert.Equal(["Selection", "Cursor", "Image", "Profile", "Units"], want);
        Assert.Contains("textButton(\"Fit\"", source);
        Assert.Contains("textButton(\"100%\"", source);

        var w = Shells.Open();
        Settle(w);
        var labels = w.Status.GetVisualDescendants().OfType<TextBlock>()
            .Where(t => want.Contains(t.Text ?? ""))
            .OrderBy(t => BoundsIn(t, w).X).Select(t => t.Text!).ToList();
        Assert.Equal(want, labels);
        var s = w.Status;
        Assert.Equal("None", s.SelectionValue.Text);
        Assert.Equal("–", s.CursorValue.Text);
        Assert.Equal("sRGB", s.ProfileValue.Text);
        Assert.Equal("100%", s.ZoomField.Text);
        Assert.Equal("Fit", s.FitButton.Content);
        Assert.Equal("100%", s.ActualButton.Content);
        Assert.Equal(["px", "in", "cm"], s.Units.Items.Cast<object>().Select(i => i.ToString()!).ToArray());
        double[] order = [BoundsIn(s.Units, w).X, BoundsIn(s.ZoomField, w).X, BoundsIn(s.FitButton, w).X, BoundsIn(s.ActualButton, w).X, BoundsIn(s.ZoomSlider, w).X];
        Assert.Equal(order.OrderBy(x => x), order);
        Assert.Equal(96, s.ZoomSlider.Bounds.Width);
        Assert.Equal(Math.Log(0.01), s.ZoomSlider.Minimum, 6);
        Assert.Equal(Math.Log(32), s.ZoomSlider.Maximum, 6);
        w.Close();
    }

    [Fact]
    public void TheHintHidesBeforeAnyValueIsSqueezed()
    {
        Assert.True(StatusBar.HintFits(700, 1280));
        Assert.True(StatusBar.HintFits(700, 740));
        Assert.False(StatusBar.HintFits(700, 739));
    }

    [AvaloniaFact]
    public void TabMovesBetweenTheCanvasAndTheStatusBar()
    {
        var w = Shells.Open();
        Settle(w);
        w.CanvasInput.Focus(NavigationMethod.Tab);
        Assert.True(w.CanvasInput.IsFocused);
        w.KeyPress(Key.Tab, RawInputModifiers.None, PhysicalKey.Tab, null);
        Settle(w);
        var afterTab = w.FocusManager?.GetFocusedElement();
        Assert.NotNull(afterTab);
        Assert.False(w.CanvasInput.IsFocused);
        Assert.True(w.Status.IsVisualAncestorOf((Visual)afterTab), $"focus went to {afterTab}");
        w.Close();
    }

    /// The native canvas calls this on Tab. It must land where Avalonia's own Tab lands.
    [AvaloniaFact]
    public void LeavingTheNativeCanvasGoesWhereTabGoes()
    {
        var w = Shells.Open();
        Settle(w);
        w.CanvasInput.Focus(NavigationMethod.Tab);
        w.KeyPress(Key.Tab, RawInputModifiers.None, PhysicalKey.Tab, null);
        Settle(w);
        var byKey = w.FocusManager?.GetFocusedElement();

        w.CanvasInput.Focus(NavigationMethod.Tab);
        w.LeaveCanvas(back: false);
        Settle(w);
        Assert.Same(byKey, w.FocusManager?.GetFocusedElement());

        w.CanvasInput.Focus(NavigationMethod.Tab);
        w.LeaveCanvas(back: true);
        Settle(w);
        var previous = w.FocusManager?.GetFocusedElement();
        Assert.NotNull(previous);
        Assert.NotSame(w.CanvasInput, previous);
        w.Close();
    }

    [AvaloniaFact]
    public void TheMenusAreNotATabStop()
    {
        var w = Shells.Open();
        Assert.False(KeyboardNavigation.GetIsTabStop(w.TitleBar.Menus));
        foreach (var top in w.TitleBar.Menus.Items.OfType<MenuItem>()) Assert.False(KeyboardNavigation.GetIsTabStop(top));
        w.Close();
    }

    /// Tree order is Tab order, so the rows must be in the order the eye reads them.
    [AvaloniaFact]
    public void TheRowsAreInReadingOrder()
    {
        var w = Shells.Open();
        var root = Assert.IsType<Grid>(w.Content);
        Assert.Equal(3, root.Children.Count);
        Assert.Same(w.TitleBar, root.Children[0]);
        Assert.Same(w.CanvasInput.GetVisualParent(), root.Children[1]);
        Assert.Same(w.Status, root.Children[2]);
        w.Close();
    }

    [AvaloniaFact]
    public void TheCanvasStopIsNamedAndTakesFocus()
    {
        var w = Shells.Open();
        Assert.True(w.CanvasInput.Focusable);
        Assert.Equal("Canvas", Avalonia.Automation.AutomationProperties.GetName(w.CanvasInput));
        Assert.Null(w.Native);
        w.Close();
    }

    [AvaloniaFact]
    public void NoDrawnTitleBarIsLeftOverInTheWindowTheme()
    {
        var w = Shells.Open();
        var theme = w.WindowDecorationsTheme;
        Assert.NotNull(theme);
        Assert.Equal(typeof(WindowDrawnDecorations), theme.TargetType);
        var setter = Assert.Single(theme.Setters.OfType<Setter>());
        var template = Assert.IsAssignableFrom<IWindowDrawnDecorationsTemplate>(setter.Value);
        var content = template.Build().Result;
        Assert.NotNull(content);
        Assert.Null(content.Overlay);
        Assert.Null(content.Underlay);
        w.Close();
    }
}
