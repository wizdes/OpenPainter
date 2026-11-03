// The W8 review's findings, one test each: the strip's picture, Alt and F10, a stale thumbnail, the
// keyboard after the eye, and the opacity field. Each failed on the code the review read.
using Avalonia;
using Avalonia.Controls;
using Avalonia.Controls.Primitives;
using Avalonia.Headless;
using Avalonia.Headless.XUnit;
using Avalonia.Input;
using Avalonia.Threading;
using Avalonia.VisualTree;
using OpenPainter.Bridge;
using OpenPainter.Canvas;
using OpenPainter.Shell;
using Xunit;

namespace OpenPainter.Tests;

public sealed class StripPictureTests
{
    const uint Width = 16, Height = 12;

    /// An editor with no window, its first picture in.
    static (EngineClient Client, Editor Editor) Ready()
    {
        var client = new EngineClient();
        var editor = new Editor(client, client.ImportPixels(Width, Height, Fixture.Flat(200, 40, 10)));
        // The fatal path opens a message box, which a test run must never do.
        editor.PreviewFault = e => Dispatcher.UIThread.Post(() => editor.Report($"a fault: {e.Message}"));
        editor.WaitForPreview();
        Dispatcher.UIThread.RunJobs();
        return (client, editor);
    }

    /// Runs a command now, on the thread that asks.
    static void RunNow(EngineClient client, Editor editor, string command, byte[]? json = null)
    {
        var result = client.Wait(client.Dispatch(editor.Document, command, json ?? EngineParams.None), 60_000) ?? throw new TimeoutException(command);
        Assert.Equal("ok", result.Status);
    }

    static byte[] Crop(int width, int height) => EngineParams.Of(("x", 0), ("y", 0), ("width", width), ("height", height), ("angle", 0));

    /// Width and height from before a crop, with pixels from after it, read out of range. The
    /// task then failed where nobody saw it, and the strip stopped for good.
    [AvaloniaFact]
    public void ACropWhileThePictureIsBeingMadeReadsNothingOutOfRange()
    {
        var (client, editor) = Ready();
        using (client)
        using (editor)
        {
            var once = 0;
            editor.PreviewStage = stage =>
            {
                if (stage == "pixels" && Interlocked.Exchange(ref once, 1) == 0) RunNow(client, editor, "image.crop_rect", Crop(4, 3));
            };
            RunNow(client, editor, "image.invert_colors");
            editor.Refresh();
            editor.WaitForPreview();
            editor.Settle();
            editor.WaitForPreview();
            Assert.Null(editor.Message);
            Assert.Equal((4, 3), (editor.Info.Width, editor.Info.Height));
            Assert.Equal((4, 3), (editor.Preview!.Value.Width, editor.Preview.Value.Height));
        }
    }

    /// A rotation keeps the byte count, so only the document's own numbers can catch it. The strip
    /// must never show pixels from one shape in the numbers of the other.
    [AvaloniaTheory]
    [InlineData("pixels")]
    [InlineData("check")]
    public void ARotationWhileThePictureIsBeingMadeShowsNoPictureOfTheOldShape(string stage)
    {
        var (client, editor) = Ready();
        using (client)
        using (editor)
        {
            var sizes = new List<(int, int)>();
            editor.PreviewReady += (_, w, h) => sizes.Add((w, h));
            var once = 0;
            editor.PreviewStage = at =>
            {
                if (at == stage && Interlocked.Exchange(ref once, 1) == 0) RunNow(client, editor, "image.rotate_90_cw");
            };
            RunNow(client, editor, "image.invert_colors");
            editor.Refresh();
            editor.WaitForPreview();
            editor.Settle();
            editor.WaitForPreview();
            Assert.Null(editor.Message);
            Assert.Equal((12, 16), (editor.Info.Width, editor.Info.Height));
            Assert.Equal((12, 16), (editor.Preview!.Value.Width, editor.Preview.Value.Height));
            Assert.DoesNotContain((16, 12), sizes);
        }
    }

    [AvaloniaFact]
    public void AFailedPictureReachesTheStatusBarAndTheNextOneStillRuns()
    {
        var (client, editor) = Ready();
        using (client)
        using (editor)
        {
            var before = editor.Preview!.Value.Rgba;
            var once = 0;
            editor.PreviewStage = stage =>
            {
                if (stage == "pixels" && Interlocked.Exchange(ref once, 1) == 0) throw new EngineCallException("doc.pixels", OpStatus.OP_ERR_PANIC);
            };
            RunNow(client, editor, "image.invert_colors");
            editor.Refresh();
            editor.WaitForPreview();
            Dispatcher.UIThread.RunJobs();
            Assert.Contains("The image strip's picture failed", editor.Message);
            RunNow(client, editor, "image.invert_colors");
            editor.Refresh();
            editor.WaitForPreview();
            editor.Settle();
            editor.WaitForPreview();
            // Two inverts give the first picture back, so this is the newer one.
            Assert.Equal(before, editor.Preview!.Value.Rgba);
            Assert.False(editor.PictureUnderWay);
        }
    }

    /// Any other failure must end the wait too, so nothing can hang on the flag.
    [AvaloniaFact]
    public void AnUnexpectedFailureOfThePictureEndsTheWaitAndIsFatal()
    {
        var (client, editor) = Ready();
        using (client)
        using (editor)
        {
            var faults = new List<Exception>();
            editor.PreviewFault = faults.Add;
            editor.PreviewStage = stage =>
            {
                if (stage == "pixels") throw new ArithmeticException("planted");
            };
            RunNow(client, editor, "image.invert_colors");
            editor.Refresh();
            editor.WaitForPreview();
            Assert.False(editor.PictureUnderWay);
            Assert.Contains(faults, e => e is ArithmeticException);
        }
    }

    /// The old wait turned forever here: the engine closed, so the flag never came down.
    [AvaloniaFact]
    public void WaitingForThePictureEndsWhenTheEngineHasClosed()
    {
        var (client, editor) = Ready();
        using (editor)
        {
            var once = 0;
            editor.PreviewStage = stage =>
            {
                if (stage == "pixels" && Interlocked.Exchange(ref once, 1) == 0) client.Dispose();
            };
            RunNow(client, editor, "image.invert_colors");
            editor.Refresh();
            editor.WaitForPreview();
            Assert.False(editor.PictureUnderWay);
            Assert.Null(editor.Message);
        }
    }

    [Fact]
    public void ShrinkRefusesPixelsThatDoNotMatchTheirSize()
    {
        var e = Assert.Throws<InvalidDataException>(() => CanvasSession.Shrink(new byte[10], 16, 12, 8, 8));
        Assert.Contains("10 bytes", e.Message);
        Assert.Contains("16 by 12", e.Message);
    }
}

public sealed class AltAndF10Tests
{
    static void Press(TopLevel w, Key key, RawInputModifiers mods = RawInputModifiers.None)
    {
        w.KeyPress(key, mods, PhysicalKey.None, null);
        Dispatcher.UIThread.RunJobs();
    }

    static void AltDown(TopLevel w) => w.KeyPressQwerty(PhysicalKey.AltLeft, RawInputModifiers.Alt);

    static void AltUp(TopLevel w)
    {
        w.KeyReleaseQwerty(PhysicalKey.AltLeft, RawInputModifiers.None);
        Dispatcher.UIThread.RunJobs();
    }

    static void AltClick(MainWindow w)
    {
        AltDown(w);
        var at = w.Options.ToolPicker.TranslatePoint(new Point(4, 4), w)!.Value;
        w.MouseDown(at, MouseButton.Left, RawInputModifiers.Alt);
        w.MouseUp(at, MouseButton.Left, RawInputModifiers.Alt);
        // The tests look at the release, with the press and the click's own popup out of the way.
        w.KeyReleaseQwerty(PhysicalKey.AltLeft, RawInputModifiers.None);
        Assert.False(w.TitleBar.Menus.IsOpen);
        Dispatcher.UIThread.RunJobs();
        foreach (var box in w.GetVisualDescendants().OfType<ComboBox>()) box.IsDropDownOpen = false;
        Dispatcher.UIThread.RunJobs();
    }

    /// A headless window loses the keyboard when it hides, and gets it back when it shows.
    static void LoseWindow(Window w)
    {
        w.Hide();
        Dispatcher.UIThread.RunJobs();
    }

    static void GetWindow(Window w)
    {
        w.Show();
        Dispatcher.UIThread.RunJobs();
        w.Focus();
    }

    static MenuItem Menu(MainWindow w, string title) => w.TitleBar.Menus.Items.OfType<MenuItem>().Single(m => (string?)m.Header == title);

    [AvaloniaFact]
    public void F10OpensAndClosesTheMenuBarFromTheMainWindow()
    {
        using var f = new Fixture();
        var w = f.Window;
        w.Focus();
        Press(w, Key.F10);
        Assert.True(w.TitleBar.Menus.IsOpen);
        Press(w, Key.F10);
        Assert.False(w.TitleBar.Menus.IsOpen);
    }

    /// shortcuts-windows.md: F10 does what the Alt tap does, and the Alt tap works in a panel.
    [AvaloniaFact]
    public void F10InAPanelOpensTheMainWindowsMenuBar()
    {
        using var f = new Fixture();
        var panel = f.Window.Panels.Panels[PanelLayout.Colors];
        Press(panel, Key.F10);
        Assert.True(f.Window.TitleBar.Menus.IsOpen);
    }

    /// The spoiled tap stayed spoiled until the next Alt press, and F10 was refused until then.
    [AvaloniaFact]
    public void F10WorksAfterAnAltClick()
    {
        using var f = new Fixture();
        var w = f.Window;
        w.Focus();
        AltClick(w);
        Assert.False(w.TitleBar.Menus.IsOpen);
        Press(w, Key.F10);
        Assert.True(w.TitleBar.Menus.IsOpen);
    }

    [AvaloniaFact]
    public void AltWithALetterOpensTheBarSoItsKeysWorkAndF10WorksAfterEsc()
    {
        using var f = new Fixture();
        var w = f.Window;
        w.Focus();
        AltDown(w);
        Press(w, Key.E, RawInputModifiers.Alt);
        Assert.True(w.TitleBar.Menus.IsOpen);
        Assert.True(Menu(w, "Edit").IsSubMenuOpen);
        AltUp(w);
        Assert.True(w.TitleBar.Menus.IsOpen);
        Press(w, Key.Escape);
        Press(w, Key.Escape);
        Assert.False(w.TitleBar.Menus.IsOpen);
        Press(w, Key.F10);
        Assert.True(w.TitleBar.Menus.IsOpen);
    }

    /// Alt held through Alt-Tab: the release goes to another program, so this window never sees it.
    [AvaloniaFact]
    public void AltHeldThroughAltTabLeavesNothingStuck()
    {
        using var f = new Fixture();
        var w = f.Window;
        w.AltIsDown = () => false;
        w.Focus();
        AltDown(w);
        Assert.True(w.GetValue(AccessText.ShowAccessKeyProperty));
        LoseWindow(w);
        Dispatcher.UIThread.RunJobs();
        // The letters stop being underlined with Alt gone.
        Assert.False(w.GetValue(AccessText.ShowAccessKeyProperty));
        GetWindow(w);
        Dispatcher.UIThread.RunJobs();
        Assert.False(w.TitleBar.Menus.IsOpen);
        Press(w, Key.F10);
        Assert.True(w.TitleBar.Menus.IsOpen);
        Press(w, Key.F10);
        Assert.False(w.TitleBar.Menus.IsOpen);
        // A clean tap still works afterwards.
        AltDown(w);
        AltUp(w);
        Assert.True(w.TitleBar.Menus.IsOpen);
    }

    /// Alt-Tab away and back with Alt still down: the release comes here, and it is no tap.
    [AvaloniaFact]
    public void AltStillDownWhenTheWindowComesBackOpensNothingOnRelease()
    {
        using var f = new Fixture();
        var w = f.Window;
        w.AltIsDown = () => true;
        w.Focus();
        AltDown(w);
        LoseWindow(w);
        GetWindow(w);
        Dispatcher.UIThread.RunJobs();
        AltUp(w);
        Assert.False(w.TitleBar.Menus.IsOpen);
        Press(w, Key.F10);
        Assert.True(w.TitleBar.Menus.IsOpen);
    }

    /// shortcuts-windows.md: Esc closes it and the keyboard goes back to where it was.
    [AvaloniaFact]
    public void ClosingTheMenusGivesTheKeyboardBackToThePanelItCameFrom()
    {
        using var f = new Fixture();
        var w = f.Window;
        var panel = w.Panels.Panels[PanelLayout.Layers];
        Press(panel, Key.F10);
        Assert.True(w.TitleBar.Menus.IsOpen);
        var given = 0;
        panel.Activated += (_, _) => given++;
        w.TitleBar.Menus.Close();
        Dispatcher.UIThread.RunJobs();
        Assert.Equal(1, given);
    }

    [AvaloniaFact]
    public void ClosingTheMenusGivesTheKeyboardBackToTheCanvas()
    {
        using var f = new Fixture();
        var w = f.Window;
        w.CanvasInput.Focus();
        AltDown(w);
        AltUp(w);
        Assert.True(w.TitleBar.Menus.IsOpen);
        Press(w, Key.Escape);
        Assert.False(w.TitleBar.Menus.IsOpen);
        Assert.True(w.CanvasInput.IsFocused);
        Press(w, Key.E);
        Assert.Equal("eraser", w.Tools.Current);
    }
}

public sealed class LayerThumbnailFixTests
{
    /// Add a layer and press Ctrl+Z quickly: the query names a layer that is gone. That is no error.
    [AvaloniaFact]
    public void AQueryForALayerThatIsGoneShowsNoError()
    {
        using var f = new Fixture();
        f.AddLayer();
        var gone = f.Editor.Info.Layers[^1];
        f.Run("edit.undo");
        Assert.Null(f.Editor.Thumbnails.Image(gone with { Revision = gone.Revision + 1_000 }));
        f.Settle();
        Assert.Null(f.Editor.Message);
    }

    [AvaloniaFact]
    public void AFailureForALayerThatIsThereStillShows()
    {
        using var f = new Fixture();
        var live = f.Editor.Info.Layers[0];
        f.Editor.Thumbnails.RequestSize = 0; // The engine refuses this size.
        Assert.Null(f.Editor.Thumbnails.Image(live with { Revision = live.Revision + 1_000 }));
        f.Settle();
        Assert.Contains("Layer thumbnail failed", f.Editor.Message);
    }

    [AvaloniaFact]
    public void FinishedRendersAreNotHeldForever()
    {
        using var f = new Fixture();
        var live = f.Editor.Info.Layers[0];
        var t = f.Editor.Thumbnails;
        for (var i = 1; i <= 30; i++)
        {
            var before = t.Renders;
            Assert.Null(t.Image(live with { Revision = live.Revision + 1_000 + i }));
            var end = Environment.TickCount64 + 30_000;
            while (t.Renders == before)
            {
                Assert.True(Environment.TickCount64 < end, "a render did not end in 30 s");
                Dispatcher.UIThread.RunJobs();
                Thread.Yield();
            }
        }
        Assert.InRange(t.HeldTasks, 0, 2);
    }

    /// The oldest pictures leave and free their memory, except a picture a row still shows.
    [AvaloniaFact]
    public void AnEvictedPictureIsFreedAndOneOnScreenIsKept()
    {
        using var f = new Fixture();
        var t = f.Editor.Thumbnails;
        t.Limit = 4;
        // The Background's row keeps its picture. Each invert gives the copy above it new pixels.
        var below = f.Row(0).Thumb.Image!;
        Assert.NotNull(below);
        f.Run("layer.duplicate");
        var first = f.Row(1).Thumb.Image!;
        Assert.NotNull(first);
        for (var i = 0; i < 12; i++)
        {
            f.Run("effect.adjust.invert_colors");
            Assert.InRange(t.Kept, 0, t.Limit + 1);
        }
        // Layer 0 did not change, so its row still shows the very first picture.
        Assert.Same(below, f.Row(0).Thumb.Image);
        Assert.Equal((int)Fixture.Width, below.PixelSize.Width);
        Assert.Throws<ObjectDisposedException>(() => first.PixelSize);
    }
}

public sealed class LayerRowKeyboardTests
{
    static void SpaceTap(Window w)
    {
        w.KeyPress(Key.Space, RawInputModifiers.None, PhysicalKey.Space, " ");
        w.KeyRelease(Key.Space, RawInputModifiers.None, PhysicalKey.Space, " ");
        Dispatcher.UIThread.RunJobs();
    }

    /// Space on the eye rebuilt the rows, and the keyboard fell off the list.
    [AvaloniaFact]
    public void SpaceOnTheEyeKeepsTheKeyboardOnThatEye()
    {
        using var f = new Fixture();
        f.AddLayer();
        var panel = f.Window.Panels.Panels[PanelLayout.Layers];
        f.Layers.LayerRows[1].Eye.Focus(NavigationMethod.Tab);
        SpaceTap(panel);
        f.Settle();
        Assert.False(f.Editor.Info.Layers[0].Visible);
        Assert.Same(f.Layers.LayerRows[1].Eye, panel.FocusManager?.GetFocusedElement());
        // And it works a second time, on the new row.
        SpaceTap(panel);
        f.Settle();
        Assert.True(f.Editor.Info.Layers[0].Visible);
        Assert.Same(f.Layers.LayerRows[1].Eye, panel.FocusManager?.GetFocusedElement());
    }

    [AvaloniaFact]
    public void ARowWithTheKeyboardKeepsItWhenTheListIsRebuilt()
    {
        using var f = new Fixture();
        f.AddLayer();
        var panel = f.Window.Panels.Panels[PanelLayout.Layers];
        f.Layers.LayerRows[1].Focus(NavigationMethod.Tab);
        f.Run("layer.set_visible", EngineParams.Of(("index", 0), ("visible", false)));
        Assert.Same(f.Layers.LayerRows[1], panel.FocusManager?.GetFocusedElement());
    }
}

public sealed class LayerPropertiesFixTests
{
    static LayerPropertiesDialog Open(Fixture f, int index = 0)
    {
        var dialog = f.Window.ShowLayerProperties(index);
        Assert.NotNull(dialog);
        Dispatcher.UIThread.RunJobs();
        dialog.UpdateLayout();
        return dialog;
    }

    /// LayerProperties.swift sliderMoved writes the field whatever has focus.
    [AvaloniaFact]
    public void ASliderDragWithTheFieldFocusedStillShowsTheNumber()
    {
        using var f = new Fixture();
        var d = Open(f);
        d.OpacityField.Focus();
        Assert.True(d.OpacityField.IsFocused);
        d.OpacitySlider.Value = 77;
        Dispatcher.UIThread.RunJobs();
        Assert.Equal("77", d.OpacityField.Text);
        Assert.Equal(77, d.Current.Opacity);
    }

    /// What a person types stays as typed: only the slider writes the field while it has focus.
    [AvaloniaFact]
    public void TypingInTheFieldIsNotOverwritten()
    {
        using var f = new Fixture();
        var d = Open(f);
        d.OpacityField.Focus();
        d.OpacityField.Text = "007";
        Dispatcher.UIThread.RunJobs();
        Assert.Equal(7, d.Current.Opacity);
        Assert.Equal("007", d.OpacityField.Text);
    }

    /// The engine's pool has 2 threads, and these previews of big images on other documents hold
    /// them, so a render asked for now waits its turn. That makes the race certain.
    static void FillThePool(Fixture f)
    {
        const uint Side = 2000;
        var pixels = new byte[Side * Side * 4];
        Array.Fill(pixels, (byte)128);
        var job = EngineParams.Of(("command", "effect.adjust.invert_colors"), ("params", EngineParams.None));
        for (var i = 0; i < 8; i++)
            f.Client.Dispatch(f.Client.ImportPixels(Side, Side, pixels), "preview.begin", job);
    }

    /// doc.sync says nothing about a render on the pool, so a Settle that did not wait for it
    /// fails here each time, as the old one did about 7 runs in 24 on a busy PC.
    [AvaloniaFact]
    public void SettleWaitsForThePreviewOfABigImage()
    {
        using var f = new Fixture();
        f.Run("layer.duplicate");
        var before = f.Pixels();
        var d = Open(f, 1);
        FillThePool(f);
        d.Set(d.Current with { Blend = "difference" });
        f.Settle();
        Assert.NotEqual(before, f.Pixels(preview: true));
        d.Cancel();
        f.Settle();
    }

    /// The claim "at once": a render still running when the values change back comes in later,
    /// and it must never show.
    [AvaloniaFact]
    public void ARenderThatEndsAfterChangingBackNeverShows()
    {
        using var f = new Fixture();
        f.Run("layer.duplicate");
        var before = f.Pixels();
        var d = Open(f, 1);
        FillThePool(f);
        d.Set(d.Current with { Blend = "difference" });
        d.Set(d.Original);
        // Right after the engine took the change, and again once the late render has come back.
        var client = f.Editor.Client;
        Assert.Equal("ok", client.Wait(client.Dispatch(f.Editor.Document, "doc.sync", EngineParams.None), 60_000)?.Status);
        Assert.Equal(before, f.Pixels(preview: true));
        f.Settle();
        Assert.Equal(before, f.Pixels(preview: true));
        Assert.False(d.SessionStarted);
        Assert.Null(f.Editor.Message);
    }

    [AvaloniaFact]
    public void AWindowClosedDuringTheLoadIgnoresTheLoadedImage()
    {
        var w = Shells.Open();
        w.Close();
        Dispatcher.UIThread.RunJobs();
        w.CanvasLoaded(16, 12);
        Assert.Null(w.Editor);
    }
}

/// A plain test that touches a control, such as LayerRow.Detail, starts Avalonia's static setup on a
/// test thread. Run beside the first UI test, that setup throws "An item with the same key has
/// already been added" from RoutedEventRegistry, and every UI test in the run then fails. One whole
/// run in 32 did this on a busy PC. So xunit.runner.json runs one test class at a time, TestStart does
/// that setup first, and these two classes fail if the setting is lost.
public sealed class OneAtATimeTests
{
    static int s_running;

    public static void Hold()
    {
        Assert.Equal(1, Interlocked.Increment(ref s_running));
        Thread.Sleep(200);
        Interlocked.Decrement(ref s_running);
    }

    [Fact]
    public void NoOtherTestClassRunsBesideThisOne() => Hold();
}

public sealed class OneAtATimeTestsToo
{
    [Fact]
    public void NoOtherTestClassRunsBesideThisOne() => OneAtATimeTests.Hold();
}
