// History, Layers and Layer Properties against the real engine, through the C bridge, with no
// stand-in. The Mac's LayersHistoryTests.swift holds the same behavior.
using System.Text.RegularExpressions;
using Avalonia;
using Avalonia.Automation;
using Avalonia.Controls.Chrome;
using Avalonia.Controls;
using Avalonia.Headless;
using Avalonia.Headless.XUnit;
using Avalonia.Input;
using Avalonia.Interactivity;
using Avalonia.Media.Imaging;
using Avalonia.Threading;
using Avalonia.VisualTree;
using OpenPainter.Bridge;
using OpenPainter.Shell;
using UiLook;
using Xunit;

namespace OpenPainter.Tests;

/// A real engine and document, with the main window's panels bound to it.
sealed class Fixture : IDisposable
{
    public EngineClient Client { get; } = new();
    public Editor Editor { get; }
    public MainWindow Window { get; }
    public HistoryPanel History => Window.Panels.History;
    public LayersPanel Layers => Window.Panels.Layers;

    public const uint Width = 16, Height = 12;

    /// A flat opaque picture, so pixel and thumbnail checks know what to expect.
    public static byte[] Flat(byte r, byte g, byte b)
    {
        var rgba = new byte[Width * Height * 4];
        for (var i = 0; i < rgba.Length; i += 4)
        {
            rgba[i] = r;
            rgba[i + 1] = g;
            rgba[i + 2] = b;
            rgba[i + 3] = 255;
        }
        return rgba;
    }

    public Fixture(string? theme = null)
    {
        Editor = new Editor(Client, Client.ImportPixels(Width, Height, Flat(200, 40, 10)));
        Window = Shells.Open(theme);
        Dispatcher.UIThread.RunJobs();
        Window.Bind(Editor);
        Settle();
    }

    /// Every queued command has run, the panels show it, and their pictures are in.
    public void Settle()
    {
        Editor.Settle();
        Editor.Thumbnails.WaitForRenders();
        foreach (var panel in Window.Panels.Panels.Values) panel.UpdateLayout();
        Window.Dialog?.UpdateLayout();
        Dispatcher.UIThread.RunJobs();
    }

    public void Run(string command, byte[]? json = null)
    {
        Assert.NotNull(Editor.Run(command, json));
        Settle();
    }

    public void AddLayer() => Run("layer.add", EngineParams.Of(("name", $"Layer {Editor.Info.Layers.Count + 1}")));

    public LayerRow Row(int layer) => Layers.LayerRows.Single(r => r.Index == layer);

    public static void Click(Button button)
    {
        Assert.True(button.IsEnabled, $"{AutomationProperties.GetName(button)} is off");
        button.RaiseEvent(new RoutedEventArgs(Button.ClickEvent));
    }

    /// A real press and release on a row, through the panel's window.
    public void Press(PanelRow row, int clicks = 1)
    {
        var window = TopLevel.GetTopLevel(row) as Window ?? throw new InvalidOperationException("the row has no window");
        window.UpdateLayout();
        var at = row.TranslatePoint(new Point(row.Bounds.Width / 2, row.Bounds.Height / 2), window) ?? throw new InvalidOperationException("the row is not in its window");
        for (var i = 0; i < clicks; i++)
        {
            window.MouseMove(at, RawInputModifiers.None);
            window.MouseDown(at, MouseButton.Left, RawInputModifiers.None);
            window.MouseUp(at, MouseButton.Left, RawInputModifiers.None);
        }
        Dispatcher.UIThread.RunJobs();
    }

    public byte[] Pixels(bool preview = false) => Client.Query(Editor.Document, "doc.pixels", EngineParams.Of(("preview", preview)));

    public void Dispose()
    {
        Window.Dialog?.Cancel();
        Window.Bind(null);
        Window.Close();
        Editor.Dispose();
        Client.Dispose();
    }
}

public sealed class HistoryPanelTests
{
    [AvaloniaFact]
    public void TheListShowsEachStepAndMarksTheCurrentAndTheUndone()
    {
        using var f = new Fixture();
        f.Run("selection.select_all");
        f.Run("layer.duplicate");
        f.Run("layer.add", EngineParams.Of(("name", "Layer 3")));
        f.Run("edit.undo");
        var rows = f.History.HistoryRows;
        Assert.Equal(["Open", "Select All", "Duplicate Layer", "Add New Layer"], rows.Select(r => r.TitleLabel.Text));
        Assert.Equal([false, false, true, false], rows.Select(r => r.IsCurrent));
        Assert.Equal([false, false, false, true], rows.Select(r => r.Undone));
        Assert.Equal(["done", "done", "current", "undone"], rows.Select(r => r.StateWord));
        Assert.Equal(["done", "done", "current", "undone"], rows.Select(AutomationProperties.GetItemStatus));
        Assert.NotNull(rows[3].RedoMark);
        Assert.Null(rows[2].RedoMark);
        Assert.Equal(Tokens.Light.Muted, ((Avalonia.Media.ISolidColorBrush)rows[3].TitleLabel.Foreground!).Color);
        Assert.Equal(Tokens.Light.Ink, ((Avalonia.Media.ISolidColorBrush)rows[0].TitleLabel.Foreground!).Color);
        Assert.Equal(Tokens.StrongWeight, rows[2].TitleLabel.FontWeight);
        Assert.Equal(27, rows[0].Bounds.Height);
    }

    [AvaloniaFact]
    public void AClickOnARowJumpsToThatStepAndAddsNoStep()
    {
        using var f = new Fixture();
        f.Run("layer.duplicate");
        f.Run("layer.duplicate");
        Assert.Equal(3, f.Editor.Info.Layers.Count);
        f.Press(f.History.HistoryRows[0]);
        f.Settle();
        Assert.Equal(0, f.Editor.Info.HistoryIndex);
        Assert.Single(f.Editor.Info.Layers);
        Assert.Equal(3, f.Editor.Info.HistoryLabels.Count);
        Assert.Equal([true, false, false], f.History.HistoryRows.Select(r => r.IsCurrent));
        f.Press(f.History.HistoryRows[2]);
        f.Settle();
        Assert.Equal(2, f.Editor.Info.HistoryIndex);
        Assert.Equal(3, f.Editor.Info.Layers.Count);
        // A click on the current step sends nothing.
        var revision = f.Editor.Info.Revision;
        f.Press(f.History.HistoryRows[2]);
        f.Settle();
        Assert.Equal(revision, f.Editor.Info.Revision);
    }

    [AvaloniaFact]
    public void UndoAndRedoRunAndFollowThePosition()
    {
        using var f = new Fixture();
        Assert.False(f.History.UndoButton.IsEnabled);
        Assert.False(f.History.RedoButton.IsEnabled);
        f.Run("layer.duplicate");
        Assert.True(f.History.UndoButton.IsEnabled);
        Assert.False(f.History.RedoButton.IsEnabled);
        Fixture.Click(f.History.UndoButton);
        f.Settle();
        Assert.Equal(0, f.Editor.Info.HistoryIndex);
        Assert.Single(f.Editor.Info.Layers);
        Assert.False(f.History.UndoButton.IsEnabled);
        Assert.True(f.History.RedoButton.IsEnabled);
        Fixture.Click(f.History.RedoButton);
        f.Settle();
        Assert.Equal(1, f.Editor.Info.HistoryIndex);
        Assert.Equal(2, f.Editor.Info.Layers.Count);
        Assert.Equal(24, f.History.UndoButton.Bounds.Height);
    }

    [AvaloniaFact]
    public void ThePanelFollowsAChangeMadeElsewhere()
    {
        using var f = new Fixture();
        Assert.Single(f.History.HistoryRows);
        // No panel asked for this, so only the engine's own event can bring it.
        var ticket = f.Client.Dispatch(f.Editor.Document, "layer.duplicate", EngineParams.None);
        Assert.NotNull(f.Client.Wait(ticket, 60_000));
        var clock = System.Diagnostics.Stopwatch.StartNew();
        while (f.History.HistoryRows.Count != 2 && clock.ElapsedMilliseconds < 60_000)
        {
            Dispatcher.UIThread.RunJobs();
            Thread.Sleep(5);
        }
        Assert.Equal(["Open", "Duplicate Layer"], f.History.HistoryRows.Select(r => r.TitleLabel.Text));
        Assert.Equal(2, f.Layers.LayerRows.Count);
    }

    [Fact]
    public void EveryStepIconIsTheFileIconsTomlNames()
    {
        var toml = Repo.Read("docs/windows/icons.toml");
        var rows = Regex.Matches(toml, @"use = ""history:([^""]+)""[^\[]*?file = ""windows/assets/icons/([^""]+)\.svg""");
        Assert.True(rows.Count >= 40, $"only {rows.Count} history rows");
        foreach (Match row in rows)
        {
            var use = row.Groups[1].Value;
            var command = use switch { "default" => "something.else", "effect.*" => "effect.blur", "effect.adjust.*" => "effect.adjust.curves", "move*" => "move.pixels", _ => use };
            Assert.True(HistoryStep.Icon(command) == row.Groups[2].Value, $"{use}: {HistoryStep.Icon(command)}, and icons.toml says {row.Groups[2].Value}");
        }
        Assert.Equal("step-adjustment", HistoryStep.Icon("image.invert_colors"));
        Assert.Equal("tool-move-pixels", HistoryStep.Icon("Move Selected Pixels"));
    }

    [Fact]
    public void TheStepNamesAreTheMacs()
    {
        Assert.Equal("Add New Layer", HistoryStep.Title("layer.add"));
        Assert.Equal("Delete Layer", HistoryStep.Title("layer.delete"));
        Assert.Equal("Curves", HistoryStep.Title("Curves"));
        var source = Repo.Read("mac/OpenPainterKit/Sources/Shell/HistoryPanel.swift");
        Assert.Contains("\"layer.add\": return \"Add New Layer\"", source);
        Assert.Contains("\"layer.delete\": return \"Delete Layer\"", source);
    }
}

public sealed class LayersPanelTests
{
    [AvaloniaFact]
    public void TheListShowsTheTopLayerFirstAndMarksTheActiveOne()
    {
        using var f = new Fixture();
        f.AddLayer();
        f.AddLayer();
        Assert.Equal(["Layer 3", "Layer 2", "Background"], f.Layers.LayerRows.Select(r => r.TitleLabel.Text));
        Assert.Equal([2, 1, 0], f.Layers.LayerRows.Select(r => r.Index));
        Assert.Equal([true, false, false], f.Layers.LayerRows.Select(r => r.IsCurrent));
        Assert.Equal(49, f.Layers.LayerRows[0].Bounds.Height);
    }

    [AvaloniaFact]
    public void AClickOnARowSelectsItAndAddsNoStep()
    {
        using var f = new Fixture();
        f.AddLayer();
        var steps = f.Editor.Info.HistoryLabels.Count;
        var dirty = f.Editor.Info.Dirty;
        f.Press(f.Row(0));
        f.Settle();
        Assert.Equal(0, f.Editor.Info.ActiveLayer);
        Assert.True(f.Row(0).IsCurrent);
        Assert.Equal(steps, f.Editor.Info.HistoryLabels.Count);
        Assert.Equal(dirty, f.Editor.Info.Dirty);
    }

    [AvaloniaFact]
    public void TheButtonsAreTheMacsInItsOrderAndFollowTheEngine()
    {
        using var f = new Fixture();
        var row = f.Layers.Actions.Children.OfType<StackPanel>().Single().Children.OfType<IconCommand>().ToList();
        Assert.Equal(["Add New Layer", "Delete Layer", "Duplicate Layer", "Merge Layer Down", "Move Layer Up", "Move Layer Down", "Layer Properties"], row.Select(b => AutomationProperties.GetName(b)));
        Assert.Equal(7, f.Layers.Buttons.Count);
        foreach (var b in row) Assert.Equal(24, b.Width);
        // One layer: nothing to merge, move or delete.
        foreach (var id in new[] { "layer.delete", "layer.merge_down", "layer.move_up", "layer.move_down" }) Assert.False(f.Layers.Buttons[id].IsEnabled, id);
        foreach (var id in new[] { "layer.add", "layer.duplicate", "layer.properties" }) Assert.True(f.Layers.Buttons[id].IsEnabled, id);
        f.Run("layer.duplicate");
        Assert.True(f.Layers.Buttons["layer.merge_down"].IsEnabled);
        Assert.False(f.Layers.Buttons["layer.move_up"].IsEnabled);
        Assert.True(f.Layers.Buttons["layer.move_down"].IsEnabled);
        Assert.True(f.Layers.Buttons["layer.delete"].IsEnabled);
    }

    [AvaloniaFact]
    public void TheButtonsTipsNameTheirKeysWithCtrlForCommand()
    {
        using var f = new Fixture();
        string? Tip(string id) => ToolTip.GetTip(f.Layers.Buttons[id]) as string;
        Assert.Equal("Add New Layer (Ctrl+Shift+N)", Tip("layer.add"));
        Assert.Equal("Duplicate Layer (Ctrl+Shift+D)", Tip("layer.duplicate"));
        Assert.Equal("Delete Layer (Ctrl+Shift+Delete)", Tip("layer.delete"));
        Assert.Equal("Layer Properties (F4)", Tip("layer.properties"));
        Assert.Equal("Merge Layer Down", Tip("layer.merge_down"));
        Assert.Equal("Undo (Ctrl+Z)", ToolTip.GetTip(f.History.UndoButton));
        Assert.Equal("Redo (Ctrl+Shift+Z)", ToolTip.GetTip(f.History.RedoButton));
    }

    [AvaloniaFact]
    public void AddPutsANamedLayerAboveTheActiveOne()
    {
        using var f = new Fixture();
        f.AddLayer();
        f.Press(f.Row(0));
        f.Settle();
        Fixture.Click(f.Layers.Buttons["layer.add"]);
        f.Settle();
        Assert.Equal(["Layer 2", "Layer 3", "Background"], f.Layers.LayerRows.Select(r => r.TitleLabel.Text));
        Assert.Equal(1, f.Editor.Info.ActiveLayer);
        Assert.Equal("Add New Layer", f.History.HistoryRows[^1].TitleLabel.Text);
    }

    [AvaloniaFact]
    public void DeleteRemovesTheActiveLayerAndUndoBringsItBack()
    {
        using var f = new Fixture();
        f.AddLayer();
        Fixture.Click(f.Layers.Buttons["layer.delete"]);
        f.Settle();
        Assert.Equal(["Background"], f.Layers.LayerRows.Select(r => r.TitleLabel.Text));
        Assert.False(f.Layers.Buttons["layer.delete"].IsEnabled);
        Assert.Equal("Delete Layer", f.History.HistoryRows[^1].TitleLabel.Text);
        Fixture.Click(f.History.UndoButton);
        f.Settle();
        Assert.Equal(2, f.Layers.LayerRows.Count);
    }

    [AvaloniaFact]
    public void DuplicatePutsACopyAboveAndMakesItActive()
    {
        using var f = new Fixture();
        Fixture.Click(f.Layers.Buttons["layer.duplicate"]);
        f.Settle();
        Assert.Equal(2, f.Layers.LayerRows.Count);
        Assert.Equal(1, f.Editor.Info.ActiveLayer);
        Assert.Equal("Duplicate Layer", f.History.HistoryRows[^1].TitleLabel.Text);
    }

    [AvaloniaFact]
    public void MergeDownJoinsTheLayerIntoTheOneBelow()
    {
        using var f = new Fixture();
        f.Run("layer.duplicate");
        Fixture.Click(f.Layers.Buttons["layer.merge_down"]);
        f.Settle();
        Assert.Single(f.Layers.LayerRows);
        Assert.Equal(0, f.Editor.Info.ActiveLayer);
        Assert.Equal("Merge Layer Down", f.History.HistoryRows[^1].TitleLabel.Text);
    }

    [AvaloniaFact]
    public void MoveUpAndMoveDownSwapTheActiveLayer()
    {
        using var f = new Fixture();
        f.AddLayer();
        Fixture.Click(f.Layers.Buttons["layer.move_down"]);
        f.Settle();
        Assert.Equal(["Background", "Layer 2"], f.Layers.LayerRows.Select(r => r.TitleLabel.Text));
        Assert.Equal(0, f.Editor.Info.ActiveLayer);
        Assert.Equal("Move Layer Down", f.History.HistoryRows[^1].TitleLabel.Text);
        Assert.False(f.Layers.Buttons["layer.move_down"].IsEnabled);
        Fixture.Click(f.Layers.Buttons["layer.move_up"]);
        f.Settle();
        Assert.Equal(["Layer 2", "Background"], f.Layers.LayerRows.Select(r => r.TitleLabel.Text));
        Assert.Equal("Move Layer Up", f.History.HistoryRows[^1].TitleLabel.Text);
    }

    [AvaloniaFact]
    public void TheEyeHidesAndShowsALayerAndTheDetailLineSaysSo()
    {
        using var f = new Fixture();
        f.AddLayer();
        var eye = f.Row(1).Eye;
        Assert.Equal("Hide Layer 2", AutomationProperties.GetName(eye));
        Assert.Equal("layer-visible", eye.Icon);
        Assert.Equal("visible", AutomationProperties.GetItemStatus(eye));
        Fixture.Click(eye);
        f.Settle();
        Assert.False(f.Editor.Info.Layers[1].Visible);
        Assert.True(f.Editor.Info.Layers[0].Visible);
        var hidden = f.Row(1);
        Assert.Equal("Show Layer 2", AutomationProperties.GetName(hidden.Eye));
        Assert.Equal("layer-hidden", hidden.Eye.Icon);
        Assert.Equal("hidden", AutomationProperties.GetItemStatus(hidden.Eye));
        Assert.Equal("Hidden", hidden.DetailLabel?.Text);
        Assert.Equal("Hide Layer", f.History.HistoryRows[^1].TitleLabel.Text);
        Fixture.Click(hidden.Eye);
        f.Settle();
        Assert.True(f.Editor.Info.Layers[1].Visible);
        Assert.Null(f.Row(1).DetailLabel);
        Assert.Equal("Show Layer", f.History.HistoryRows[^1].TitleLabel.Text);
    }

    [AvaloniaFact]
    public void ARealClickOnTheEyeDoesNotAlsoSelectTheRow()
    {
        using var f = new Fixture();
        f.AddLayer();
        var window = f.Window.Panels.Panels[PanelLayout.Layers];
        var eye = f.Row(0).Eye;
        var at = eye.TranslatePoint(new Point(12, 12), window)!.Value;
        window.MouseMove(at, RawInputModifiers.None);
        window.MouseDown(at, MouseButton.Left, RawInputModifiers.None);
        window.MouseUp(at, MouseButton.Left, RawInputModifiers.None);
        f.Settle();
        Assert.False(f.Editor.Info.Layers[0].Visible);
        Assert.Equal(1, f.Editor.Info.ActiveLayer);
    }

    /// The first click selects the row, and that rebuilds the list under the second click.
    [AvaloniaFact]
    public void ADoubleClickCountsWhenTheFirstClickRebuiltTheList()
    {
        using var f = new Fixture();
        f.AddLayer();
        f.Press(f.Row(0));
        f.Settle();
        Assert.Equal(0, f.Editor.Info.ActiveLayer);
        f.Press(f.Row(0));
        var d = f.Window.Dialog;
        Assert.NotNull(d);
        Assert.Equal(0, d.LayerIndex);
    }

    /// The Mac's AppKit delivers a release to a row that left its list, and Avalonia does not.
    [AvaloniaFact]
    public void AListThatChangesDuringAPressKeepsTheClick()
    {
        using var f = new Fixture();
        f.AddLayer();
        var window = f.Window.Panels.Panels[PanelLayout.Layers];
        var row = f.Row(0);
        var at = row.TranslatePoint(new Point(100, 24), window)!.Value;
        window.MouseMove(at, RawInputModifiers.None);
        window.MouseDown(at, MouseButton.Left, RawInputModifiers.None);
        f.Layers.Reload();
        Assert.Same(row, f.Row(0));
        window.MouseUp(at, MouseButton.Left, RawInputModifiers.None);
        f.Settle();
        Assert.Equal(0, f.Editor.Info.ActiveLayer);
        Assert.True(f.Row(0).IsCurrent);
    }

    [AvaloniaFact]
    public void ADragThatLosesThePointerEndsWithNoMove()
    {
        using var f = new Fixture();
        f.AddLayer();
        var window = f.Window.Panels.Panels[PanelLayout.Layers];
        var row = f.Row(1);
        var at = row.TranslatePoint(new Point(100, 24), window)!.Value;
        window.MouseMove(at, RawInputModifiers.None);
        window.MouseDown(at, MouseButton.Left, RawInputModifiers.None);
        window.MouseMove(at + new Point(0, 60), RawInputModifiers.LeftMouseButton);
        Assert.True(f.Layers.DropLine.IsVisible);
        var steps = f.Editor.Info.HistoryLabels.Count;
        row.RaiseEvent(new PointerCaptureLostEventArgs(row, new Pointer(99, PointerType.Mouse, true)));
        Assert.False(f.Layers.DropLine.IsVisible);
        Assert.False(row.IsDragging);
        f.Settle();
        Assert.Equal(steps, f.Editor.Info.HistoryLabels.Count);
    }

    [Fact]
    public void TheDetailLineIsTheMacs()
    {
        LayerInfo L(bool visible, string blend, int opacity) => new("a", visible, 1, opacity, blend, 0);
        Assert.Null(LayerRow.Detail(L(true, "normal", 255)));
        Assert.Equal("Hidden", LayerRow.Detail(L(false, "normal", 255)));
        Assert.Equal("Color Burn", LayerRow.Detail(L(true, "color_burn", 255)));
        Assert.Equal("50%", LayerRow.Detail(L(true, "normal", 128)));
        Assert.Equal("Hidden · Xor · 0%", LayerRow.Detail(L(false, "xor", 0)));
    }

    [AvaloniaFact]
    public void AThumbnailIsTheEnginesPictureOfThatLayerAndFollowsItsPixels()
    {
        using var f = new Fixture();
        var image = f.Row(0).Thumb.Image;
        Assert.NotNull(image);
        Assert.Equal(new PixelSize(16, 12), image.PixelSize);
        Assert.Equal((200, 40, 10), Middle(image));
        f.Run("image.invert_colors");
        var inverted = f.Row(0).Thumb.Image;
        Assert.NotNull(inverted);
        Assert.NotSame(image, inverted);
        Assert.Equal((55, 215, 245), Middle(inverted));
    }

    [AvaloniaFact]
    public void AThumbnailKeepsTheImagesShapeInItsBox()
    {
        using var f = new Fixture();
        f.Row(0).Thumb.Measure(new Size(40, 40));
        f.Row(0).Thumb.Arrange(new Rect(0, 0, 40, 40));
        Assert.Equal(new Rect(0, 5, 40, 30), f.Row(0).Thumb.ImageRect);
    }

    [AvaloniaFact]
    public void AHistoryJumpReusesAPictureItMadeBefore()
    {
        using var f = new Fixture();
        var first = f.Row(0).Thumb.Image;
        f.Run("image.invert_colors");
        var renders = f.Editor.Thumbnails.Renders;
        Fixture.Click(f.History.UndoButton);
        f.Settle();
        Assert.Same(first, f.Row(0).Thumb.Image);
        Assert.Equal(renders, f.Editor.Thumbnails.Renders);
    }

    static unsafe (int, int, int) Middle(Bitmap image)
    {
        var size = image.PixelSize;
        var bytes = new byte[size.Width * size.Height * 4];
        fixed (byte* to = bytes) image.CopyPixels(new PixelRect(size), (nint)to, bytes.Length, size.Width * 4);
        var at = (size.Height / 2 * size.Width + size.Width / 2) * 4;
        return (bytes[at], bytes[at + 1], bytes[at + 2]);
    }

    [Theory]
    [InlineData(0, 0, 3, 2)]    // Bottom to the top.
    [InlineData(2, 3, 3, 0)]    // Top to the bottom.
    [InlineData(1, 1, 3, null)] // Onto its own gap.
    [InlineData(1, 2, 3, null)] // Just below itself.
    [InlineData(1, 0, 3, 2)]
    [InlineData(2, 2, 3, 1)]
    public void TheDropIndexIsTheMacs(int from, int gap, int count, int? want) => Assert.Equal(want, LayersPanel.DropIndex(from, gap, count));

    [AvaloniaFact]
    public void DraggingARowMovesTheLayerAndShowsTheLine()
    {
        using var f = new Fixture();
        f.AddLayer();
        f.AddLayer();
        var window = f.Window.Panels.Panels[PanelLayout.Layers];
        var row = f.Row(2);
        var at = row.TranslatePoint(new Point(100, 24), window)!.Value;
        window.MouseMove(at, RawInputModifiers.None);
        window.MouseDown(at, MouseButton.Left, RawInputModifiers.None);
        Assert.False(f.Layers.DropLine.IsVisible);
        var below = at + new Point(0, f.Layers.RowHeight * 2 + 4);
        window.MouseMove(below, RawInputModifiers.LeftMouseButton);
        Assert.True(row.IsDragging);
        Assert.True(f.Layers.DropLine.IsVisible);
        Assert.Equal(3, f.Layers.DropGap);
        window.MouseUp(below, MouseButton.Left, RawInputModifiers.None);
        Assert.False(f.Layers.DropLine.IsVisible);
        f.Settle();
        Assert.Equal(["Layer 2", "Background", "Layer 3"], f.Layers.LayerRows.Select(r => r.TitleLabel.Text));
        Assert.Equal("Move Layer", f.History.HistoryRows[^1].TitleLabel.Text);
    }

    [AvaloniaFact]
    public void AListThatChangesDuringADragWaitsForTheDrop()
    {
        using var f = new Fixture();
        f.AddLayer();
        var window = f.Window.Panels.Panels[PanelLayout.Layers];
        var row = f.Row(1);
        var at = row.TranslatePoint(new Point(100, 24), window)!.Value;
        window.MouseMove(at, RawInputModifiers.None);
        window.MouseDown(at, MouseButton.Left, RawInputModifiers.None);
        window.MouseMove(at + new Point(0, 10), RawInputModifiers.LeftMouseButton);
        f.Layers.Reload();
        Assert.Same(row, f.Row(1));
        window.MouseUp(at + new Point(0, 10), MouseButton.Left, RawInputModifiers.None);
        Dispatcher.UIThread.RunJobs();
        Assert.NotSame(row, f.Row(1));
    }

    [Fact]
    public void TheBlendModesAreTheMacsAndTheEnginesInOrder()
    {
        var swift = Regex.Matches(Repo.Read("mac/OpenPainterKit/Sources/Shell/LayersPanel.swift"), @"\(""(\w+)"", ""([\w ]+)""\)[,\s]").Select(m => (m.Groups[1].Value, m.Groups[2].Value)).ToArray();
        Assert.Equal(14, swift.Length);
        Assert.Equal(swift, LayerBlend.Modes.Select(m => (m.Id, m.Title)));
        var rust = Regex.Match(Repo.Read("engine/op-engine/src/layer_commands.rs"), @"BLEND_NAMES: \[&str; 14\] = \[(.*?)\];", RegexOptions.Singleline);
        Assert.Equal(LayerBlend.Modes.Select(m => m.Id), Regex.Matches(rust.Groups[1].Value, @"""(\w+)""").Select(m => m.Groups[1].Value));
    }

    [Fact]
    public void EveryButtonsIconIsTheFileIconsTomlNames()
    {
        var toml = Repo.Read("docs/windows/icons.toml");
        foreach (var (_, icon, label, _) in LayersPanel.Items)
            Assert.Contains($"use = \"site:LayersPanel.swift:{label}\"", toml);
        foreach (var (_, icon, label, _) in LayersPanel.Items)
        {
            var row = Regex.Match(toml, $@"use = ""site:LayersPanel\.swift:{label}""[^\[]*?file = ""windows/assets/icons/([^""]+)\.svg""");
            Assert.True(row.Groups[1].Value == icon, $"{label}: {icon}, and icons.toml says {row.Groups[1].Value}");
        }
    }

    [Fact]
    public void TheButtonsAreInTheOrderOfTheMacsList()
    {
        var swift = Regex.Matches(Repo.Read("mac/OpenPainterKit/Sources/Shell/LayersPanel.swift"), @"\(""(layer\.\w+)"", ""[\w.]+"", ""([\w ]+)""\)").Select(m => (m.Groups[1].Value, m.Groups[2].Value)).ToArray();
        Assert.Equal(swift, LayersPanel.Items.Select(i => (i.Id, i.Label)));
    }
}

public sealed class PanelStateTests
{
    [AvaloniaFact]
    public void WithNoDocumentThePanelsShowTheMacsEmptyState()
    {
        var w = Shells.Open();
        Dispatcher.UIThread.RunJobs();
        Assert.Null(w.Editor);
        Assert.Empty(w.Panels.History.Rows);
        Assert.Empty(w.Panels.Layers.Rows);
        Assert.False(w.Panels.History.UndoButton.IsEnabled);
        Assert.False(w.Panels.History.RedoButton.IsEnabled);
        Assert.Equal(7, w.Panels.Layers.Buttons.Count);
        Assert.All(w.Panels.Layers.Buttons.Values, b => Assert.False(b.IsEnabled));
        Assert.Null(w.ShowLayerProperties(0));
        w.Close();
    }

    [AvaloniaFact]
    public void ThePanelsFollowTheOpenDocumentAndGoEmptyWhenItCloses()
    {
        using var f = new Fixture();
        Assert.Single(f.Layers.Rows);
        Assert.Single(f.History.Rows);
        f.Window.Bind(null);
        Assert.Empty(f.Layers.Rows);
        Assert.Empty(f.History.Rows);
        Assert.All(f.Layers.Buttons.Values, b => Assert.False(b.IsEnabled));
        f.Window.Bind(f.Editor);
        Assert.Single(f.Layers.Rows);
    }

    [AvaloniaFact]
    public void AThemeChangeKeepsTheListsAndThePanelsLook()
    {
        using var f = new Fixture("light");
        f.AddLayer();
        f.Window.RequestedThemeVariant = Avalonia.Styling.ThemeVariant.Dark;
        Dispatcher.UIThread.RunJobs();
        f.Window.Panels.ReloadPanels();
        f.Settle();
        Assert.Equal(2, f.Layers.LayerRows.Count);
        Assert.Equal(2, f.History.HistoryRows.Count);
        Assert.Equal(Tokens.Dark.Panel, ((Avalonia.Media.ISolidColorBrush)f.Layers.Background!).Color);
    }

    [AvaloniaFact]
    public void AnEngineErrorReachesTheStatusBar()
    {
        using var f = new Fixture();
        // One layer, so the engine refuses to delete it, whatever the panel's button says.
        f.Editor.Run("layer.delete");
        f.Settle();
        Assert.Equal("layer.delete failed: bad_argument", f.Editor.Message);
        Assert.Equal("layer.delete failed: bad_argument", f.Window.Status.Hint.Text);
        // An id the engine does not know fails at the call.
        Assert.Null(f.Editor.Run("layer.no_such_thing"));
        Assert.StartsWith("layer.no_such_thing failed", f.Window.Status.Hint.Text);
        // A tool change clears it.
        f.Window.Tools.Current = ToolState.Pan;
        Assert.Null(f.Editor.Message);
        Assert.Equal(ToolState.Hint(f.Window.Tools.CurrentTool), f.Window.Status.Hint.Text);
    }

    [Fact]
    public void ALimitShowsTheEnginesOwnWords()
    {
        Assert.Equal("Too many layers.", Editor.FailureText("layer.add", "limit", "too many layers"));
        Assert.Equal("layer.add failed: read_only", Editor.FailureText("layer.add", "read_only", "x"));
    }

    [AvaloniaFact]
    public void AnEmptyPanelHasACloseButtonAndItsCommandsOff()
    {
        var w = Shells.Open();
        Dispatcher.UIThread.RunJobs();
        var history = w.Panels.Panels[PanelLayout.History];
        history.UpdateLayout();
        Assert.Equal(3, history.GetVisualDescendants().OfType<IconView>().Count());
        Assert.Same(w.Panels.History, history.Body);
        Assert.Null(w.Panels.Panels[PanelLayout.Tools].Body);
        Assert.Null(w.Panels.Panels[PanelLayout.Colors].Body);
        w.Close();
    }
}

public sealed class LayerPropertiesTests
{
    static LayerPropertiesDialog Open(Fixture f, int index = 0)
    {
        var dialog = f.Window.ShowLayerProperties(index);
        Assert.NotNull(dialog);
        Dispatcher.UIThread.RunJobs();
        dialog.UpdateLayout();
        return dialog;
    }

    [AvaloniaFact]
    public void TheDialogHasTheMacsFieldsAndWords()
    {
        using var f = new Fixture();
        var d = Open(f);
        Assert.Equal("Layer Properties", d.Title);
        Assert.Equal("Background", d.NameField.Text);
        Assert.Equal(true, d.VisibleBox.IsChecked);
        Assert.Equal("Visible", d.VisibleBox.Content);
        Assert.Equal(LayerBlend.Modes.Select(m => m.Title), d.BlendPopup.Items.Cast<string>());
        Assert.Equal(0, d.BlendPopup.SelectedIndex);
        Assert.Equal(255, d.OpacitySlider.Value);
        Assert.Equal("255", d.OpacityField.Text);
        Assert.Equal("OK", d.OkButton.Content);
        Assert.Equal("Cancel", d.CancelButton.Content);
        var words = d.GetVisualDescendants().OfType<TextBlock>().Select(t => t.Text).ToList();
        foreach (var word in new[] { "Name:", "Blend mode:", "Opacity:", "Layer Properties" }) Assert.Contains(word, words);
        Assert.Equal(LayerPropertiesDialog.ClientWidth, d.ClientSize.Width);
        Assert.Equal(LayerPropertiesDialog.BodyHeight + Tokens.DialogTitleHeight, d.ClientSize.Height);
    }

    /// W6 asks this of dialogs, and W7's NoDialogExistsYetToDrag waits for this first one.
    [AvaloniaFact]
    public void TheDialogDragsByItsTitleRowAndStaysAboveTheCanvasAsAnOwnedWindow()
    {
        using var f = new Fixture();
        var d = Open(f);
        Assert.Same(f.Window, d.Owner);
        Assert.Contains(d, f.Window.OwnedWindows);
        Assert.False(d.ShowInTaskbar);
        Assert.Equal(WindowDecorationsElementRole.TitleBar, WindowDecorationProperties.GetElementRole(d.TitleRow));
        Assert.Equal(WindowDecorationsElementRole.User, WindowDecorationProperties.GetElementRole(d.CloseButton));
        Assert.Equal(Tokens.DialogTitleHeight, d.TitleRow.Bounds.Height);
    }

    [AvaloniaFact]
    public void TheTypedOpacityAndTheSliderFollowEachOther()
    {
        using var f = new Fixture();
        var d = Open(f);
        d.OpacitySlider.Value = 100;
        Assert.Equal(100, d.Current.Opacity);
        Assert.Equal("100", d.OpacityField.Text);
        d.OpacityField.Text = "60";
        Dispatcher.UIThread.RunJobs();
        Assert.Equal(60, d.Current.Opacity);
        Assert.Equal(60, d.OpacitySlider.Value);
        d.OpacityField.Text = "not a number";
        Dispatcher.UIThread.RunJobs();
        Assert.Equal(60, d.Current.Opacity);
        d.OpacityField.Text = "900";
        Dispatcher.UIThread.RunJobs();
        Assert.Equal(255, d.Current.Opacity);
    }

    [AvaloniaFact]
    public void EveryControlChangesThePreviewAndTheDocumentStaysAsItWas()
    {
        using var f = new Fixture();
        f.Run("layer.duplicate");
        var before = f.Pixels();
        var steps = f.Editor.Info.HistoryLabels.Count;
        var d = Open(f, 1);
        Assert.False(d.SessionStarted);
        d.BlendPopup.SelectedIndex = 8; // Difference over its copy is black.
        f.Settle();
        Assert.True(d.SessionStarted);
        Assert.Equal("difference", d.Current.Blend);
        Assert.NotEqual(before, f.Pixels(preview: true));
        Assert.Equal(before, f.Pixels());
        d.VisibleBox.IsChecked = false;
        f.Settle();
        Assert.False(d.Current.Visible);
        Assert.Equal(before, f.Pixels(preview: true));
        d.VisibleBox.IsChecked = true;
        d.OpacitySlider.Value = 128;
        d.NameField.Text = "Renamed";
        Dispatcher.UIThread.RunJobs();
        f.Settle();
        Assert.Equal("Renamed", d.Current.Name);
        Assert.Equal(128, d.Current.Opacity);
        Assert.Equal("Background", f.Editor.Info.Layers[0].Name);
        Assert.Equal(steps, f.Editor.Info.HistoryLabels.Count);
        d.Cancel();
        f.Settle();
    }

    [AvaloniaFact]
    public void CancelLeavesLayersHistoryAndPixelsAsTheyWere()
    {
        using var f = new Fixture();
        f.Run("layer.duplicate");
        var pixels = f.Pixels();
        var info = f.Editor.Info;
        var d = Open(f, 1);
        d.Set(new("Changed", true, "difference", 200));
        f.Settle();
        Assert.NotEqual(pixels, f.Pixels(preview: true));
        Fixture.Click(d.CancelButton);
        f.Settle();
        Assert.Null(f.Window.Dialog);
        Assert.Equal(pixels, f.Pixels(preview: true));
        Assert.Equal(pixels, f.Pixels());
        Assert.Equal(info.HistoryLabels, f.Editor.Info.HistoryLabels);
        Assert.Equal(info.Layers, f.Editor.Info.Layers);
        Assert.Equal(info.Dirty, f.Editor.Info.Dirty);
    }

    [AvaloniaFact]
    public void TheCloseButtonAndEscapeAreCancel()
    {
        using var f = new Fixture();
        f.Run("layer.duplicate");
        var layers = f.Editor.Info.Layers;
        var d = Open(f, 1);
        d.Set(d.Current with { Opacity = 9 });
        Fixture.Click(d.CloseButton);
        f.Settle();
        Assert.Null(f.Window.Dialog);
        Assert.Equal(layers, f.Editor.Info.Layers);
        d = Open(f, 1);
        d.Set(d.Current with { Opacity = 9 });
        d.KeyPress(Key.Escape, RawInputModifiers.None, PhysicalKey.Escape, null);
        f.Settle();
        Assert.Null(f.Window.Dialog);
        Assert.Equal(layers, f.Editor.Info.Layers);
        d = Open(f, 1);
        d.Set(d.Current with { Opacity = 9 });
        d.Close();
        f.Settle();
        Assert.Null(f.Window.Dialog);
        Assert.Equal(layers, f.Editor.Info.Layers);
    }

    [AvaloniaFact]
    public void OkAfterManyChangesIsOneStepWithTheLastValues()
    {
        using var f = new Fixture();
        f.Run("layer.duplicate");
        var d = Open(f, 1);
        for (var o = 0; o < 100; o++) d.Set(new("Fast", true, "overlay", 255 - o));
        Fixture.Click(d.OkButton);
        f.Settle();
        Assert.Null(f.Window.Dialog);
        Assert.Equal(3, f.Editor.Info.HistoryLabels.Count);
        Assert.Equal("Layer Properties", f.Editor.Info.HistoryLabels[^1]);
        var l = f.Editor.Info.Layers[1];
        Assert.Equal("Fast", l.Name);
        Assert.Equal("overlay", l.Blend);
        Assert.Equal(156, l.Opacity);
        Assert.Equal("Overlay · 61%", f.Row(1).DetailLabel?.Text);
        Assert.Equal("Fast", f.Row(1).TitleLabel.Text);
    }

    [AvaloniaFact]
    public void EnterIsOk()
    {
        using var f = new Fixture();
        var d = Open(f);
        d.Set(d.Current with { Opacity = 9 });
        d.KeyPress(Key.Enter, RawInputModifiers.None, PhysicalKey.Enter, null);
        f.Settle();
        Assert.Null(f.Window.Dialog);
        Assert.Equal(9, f.Editor.Info.Layers[0].Opacity);
    }

    [AvaloniaFact]
    public void OkWithNothingChangedOrChangedBackAddsNoStep()
    {
        using var f = new Fixture();
        var steps = f.Editor.Info.HistoryLabels.Count;
        Fixture.Click(Open(f).OkButton);
        f.Settle();
        Assert.Equal(steps, f.Editor.Info.HistoryLabels.Count);
        var d = Open(f);
        d.Set(d.Current with { Opacity = 3 });
        d.Set(d.Original);
        Fixture.Click(d.OkButton);
        f.Settle();
        Assert.Equal(steps, f.Editor.Info.HistoryLabels.Count);
        Assert.Equal(f.Pixels(), f.Pixels(preview: true));
        Assert.Null(f.Editor.Message);
    }

    /// The Mac sends this change to the engine, which refuses it, so its status bar shows a
    /// failure and its canvas keeps the last preview.
    [AvaloniaFact]
    public void ChangingBackShowsTheOriginalPixelsAtOnceWithNoError()
    {
        using var f = new Fixture();
        f.Run("layer.duplicate");
        var pixels = f.Pixels();
        var d = Open(f, 1);
        d.Set(d.Current with { Blend = "difference" });
        f.Settle();
        Assert.NotEqual(pixels, f.Pixels(preview: true));
        d.Set(d.Original);
        f.Settle();
        Assert.Equal(pixels, f.Pixels(preview: true));
        Assert.False(d.SessionStarted);
        Assert.Null(f.Editor.Message);
        // A later change starts again, and OK keeps it.
        d.Set(d.Current with { Blend = "difference" });
        f.Settle();
        Assert.NotEqual(pixels, f.Pixels(preview: true));
        Fixture.Click(d.OkButton);
        f.Settle();
        Assert.Equal("difference", f.Editor.Info.Layers[1].Blend);
        Assert.Equal("Layer Properties", f.Editor.Info.HistoryLabels[^1]);
        Assert.Null(f.Editor.Message);
    }

    [AvaloniaFact]
    public void ThePanelCommandsAreOffWhileItIsUpAndComeBack()
    {
        using var f = new Fixture();
        f.Run("layer.duplicate");
        var d = Open(f, 1);
        Assert.True(f.Window.Panels.Context.Busy);
        Assert.All(f.Layers.Buttons.Values, b => Assert.False(b.IsEnabled));
        Assert.All(f.Layers.LayerRows, r => Assert.False(r.Eye.IsEnabled));
        Assert.False(f.History.UndoButton.IsEnabled);
        // Rows do nothing: no jump, no selection.
        var index = f.Editor.Info.HistoryIndex;
        f.Press(f.History.HistoryRows[0]);
        f.Press(f.Row(0));
        f.Settle();
        Assert.Equal(index, f.Editor.Info.HistoryIndex);
        Assert.Equal(1, f.Editor.Info.ActiveLayer);
        // A second dialog does not open, and window keys stay off.
        Assert.Null(f.Window.ShowLayerProperties(0));
        var zoom = f.Window.View.View.Zoom;
        f.Window.KeyPress(Key.OemPlus, RawInputModifiers.Control, PhysicalKey.Equal, null);
        Assert.Equal(zoom, f.Window.View.View.Zoom);
        d.Cancel();
        f.Settle();
        Assert.False(f.Window.Panels.Context.Busy);
        Assert.True(f.Layers.Buttons["layer.add"].IsEnabled);
        Assert.True(f.History.UndoButton.IsEnabled);
    }

    [AvaloniaFact]
    public void ThePropertiesButtonOpensItForTheActiveLayer()
    {
        using var f = new Fixture();
        f.Run("layer.duplicate");
        Fixture.Click(f.Layers.Buttons["layer.properties"]);
        var d = f.Window.Dialog;
        Assert.NotNull(d);
        Assert.Equal(1, d.LayerIndex);
        d.Cancel();
    }

    [AvaloniaFact]
    public void ADoubleClickOnARowSelectsItAndOpensItsProperties()
    {
        using var f = new Fixture();
        f.AddLayer();
        f.Press(f.Row(0), clicks: 2);
        var d = f.Window.Dialog;
        Assert.NotNull(d);
        Assert.Equal(0, d.LayerIndex);
        Assert.Equal("Background", d.NameField.Text);
        d.Cancel();
        f.Settle();
        Assert.Equal(0, f.Editor.Info.ActiveLayer);
    }

    [AvaloniaFact]
    public void AThemeChangeKeepsTheDialogsValues()
    {
        using var f = new Fixture("light");
        var d = Open(f);
        d.Set(d.Current with { Name = "Kept", Opacity = 77 });
        f.Window.RequestedThemeVariant = Avalonia.Styling.ThemeVariant.Dark;
        Dispatcher.UIThread.RunJobs();
        Assert.Same(d, f.Window.Dialog);
        Assert.Equal("Kept", d.NameField.Text);
        Assert.Equal("77", d.OpacityField.Text);
        Assert.Equal(Tokens.Dark.Panel, ((Avalonia.Media.ISolidColorBrush)d.Background!).Color);
        d.Cancel();
    }

    [AvaloniaFact]
    public void ClosingTheMainWindowCancelsTheDialog()
    {
        using var f = new Fixture();
        var d = Open(f);
        d.Set(d.Current with { Opacity = 9 });
        var layers = f.Editor.Info.Layers;
        f.Window.Close();
        Dispatcher.UIThread.RunJobs();
        Assert.Null(f.Window.Dialog);
        f.Editor.Settle();
        Assert.Equal(layers, f.Editor.Info.Layers);
    }
}
