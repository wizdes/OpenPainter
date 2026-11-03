// The Tools and Colors panels and their math: the Mac's ColorsTests.swift holds the same values
// and behavior. Color math has no window. The panels run in the main window with a real engine.
using System.Text;
using Avalonia;
using Avalonia.Automation;
using Avalonia.Controls;
using Avalonia.Headless;
using Avalonia.Headless.XUnit;
using Avalonia.Input;
using Avalonia.Interactivity;
using Avalonia.Threading;
using OpenPainter.Canvas;
using OpenPainter.Shell;
using UiLook;
using Xunit;

namespace OpenPainter.Tests;

public sealed class ColorMathTests
{
    [Fact]
    public void TheWheelsModifierMathIsTheMacs()
    {
        var current = new Hsv(90, 40, 80);
        static void Near(double want, double got) => Assert.Equal(want, got, 9);
        // No keys: the angle is the hue, the distance the saturation.
        var p = ColorWheelMath.Pick(.5, 0, current, WheelModifiers.None);
        Near(0, p.H);
        Near(50, p.S);
        p = ColorWheelMath.Pick(0, 2, current, WheelModifiers.None);
        Near(90, p.H);
        Near(100, p.S);
        Near(225, ColorWheelMath.Pick(-.3, -.3, current, WheelModifiers.None).H);
        // Ctrl: same radius, only the hue changes.
        p = ColorWheelMath.Pick(-.9, 0, current, WheelModifiers.KeepRadius);
        Near(180, p.H);
        Assert.Equal(40, p.S);
        // Alt: same spoke, saturation is the projection onto it.
        p = ColorWheelMath.Pick(.3, .6, current, WheelModifiers.KeepSpoke);
        Assert.Equal(90, p.H);
        Near(60, p.S);
        Assert.Equal(0, ColorWheelMath.Pick(.3, -.6, current, WheelModifiers.KeepSpoke).S);
        // Shift: the hue snaps to 15 degree spokes, and saturation is free.
        static double R(double degrees) => degrees * Math.PI / 180;
        p = ColorWheelMath.Pick(Math.Cos(R(37)) * .7, Math.Sin(R(37)) * .7, current, WheelModifiers.SnapSpokes);
        Near(30, p.H);
        Near(70, p.S);
        Near(0, ColorWheelMath.Pick(Math.Cos(R(-5)), Math.Sin(R(-5)), current, WheelModifiers.SnapSpokes).H);
        // Ctrl+Shift: 15 degree steps on the same radius.
        p = ColorWheelMath.Pick(Math.Cos(R(52)) * .2, Math.Sin(R(52)) * .2, current, WheelModifiers.KeepRadius | WheelModifiers.SnapSpokes);
        Near(45, p.H);
        Assert.Equal(40, p.S);
        // The marker sits where a plain click on it would pick the same color.
        var m = ColorWheelMath.Point(current);
        p = ColorWheelMath.Pick(m.X, m.Y, new Hsv(0, 0, 0), WheelModifiers.None);
        Near(90, p.H);
        Near(40, p.S);
    }

    [Fact]
    public void TheWheelsKeysAreCtrlAltAndShift()
    {
        Assert.Equal(WheelModifiers.KeepRadius, ColorWheel.Modifiers(KeyModifiers.Control));
        Assert.Equal(WheelModifiers.KeepSpoke, ColorWheel.Modifiers(KeyModifiers.Alt));
        Assert.Equal(WheelModifiers.SnapSpokes | WheelModifiers.KeepRadius, ColorWheel.Modifiers(KeyModifiers.Shift | KeyModifiers.Control));
        Assert.Equal(WheelModifiers.None, ColorWheel.Modifiers(KeyModifiers.None));
    }

    [Fact]
    public void HsvRoundTripsTheMacsGridOfColorsExactly()
    {
        for (var r = 0; r <= 255; r += 15)
            for (var g = 0; g <= 255; g += 17)
                for (var b = 0; b <= 255; b += 51)
                {
                    var c = new DocColor((byte)r, (byte)g, (byte)b);
                    Assert.Equal(c, Hsv.Of(c).Color());
                }
    }

    [Fact]
    public void TheDiscIsRedAtTheRightAndCyanAtTheLeftAndClearOutsideItsRim()
    {
        const int size = 104;
        var bytes = ColorWheel.DiscBytes(size);
        byte[] Pixel(int x, int y) => bytes[((y * size + x) * 4)..((y * size + x) * 4 + 4)];
        // BGRA. The point sits half a pixel off the center line, so a little of the neighbor hue shows.
        var red = Pixel(size - 3, size / 2 - 1);
        Assert.True(red[2] == 255 && red[1] < 25 && red[0] < 25 && red[3] == 255, string.Join(",", red));
        var cyan = Pixel(2, size / 2 - 1);
        Assert.True(cyan[2] < 25 && cyan[1] > 230 && cyan[0] > 230 && cyan[3] == 255, string.Join(",", cyan));
        Assert.Equal([0, 0, 0, 0], Pixel(0, 0));
    }

    [Fact]
    public void ThePaletteParserKeepsTheGoodLinesAndReportsTheBad()
    {
        var text = "; Paint.NET Palette File\nFF000000\n80FF0000 ; half-transparent red, with a trailing comment\n  ff00ff00\t\n00FFFFFF\n\nFF12345\nGG000000\nFF0000FF00\n#FF0000FF\n;FFFFFFFF commented out\nFFABCDEF\r\n";
        var r = ColorPalette.Parse(text);
        Assert.Equal(5, r.ColorCount);
        Assert.Equal(96, r.Palette.Colors.Count);
        Assert.Equal(new DocColor(0, 0, 0), r.Palette.Colors[0]);
        Assert.Equal(new DocColor(255, 0, 0, 0x80), r.Palette.Colors[1]);
        Assert.Equal(new DocColor(0, 255, 0), r.Palette.Colors[2]);
        Assert.Equal(new DocColor(255, 255, 255, 0), r.Palette.Colors[3]);
        Assert.Equal(new DocColor(0xAB, 0xCD, 0xEF), r.Palette.Colors[4]);
        Assert.Equal(ColorPalette.Padding, r.Palette.Colors[5]);
        Assert.Equal([7, 8, 9, 10], r.Issues.Select(i => i.Line));
        Assert.All(r.Issues, i => Assert.Contains("aarrggbb", i.Reason));
        Assert.Contains("Loaded 5 of 96", r.Summary);
        Assert.Contains("skipped 4 malformed lines (7, 8, 9, 10)", r.Summary);

        var many = ColorPalette.Parse(string.Join("\n", Enumerable.Range(0, 100).Select(i => $"FF{i:X6}")));
        Assert.Equal(96, many.ColorCount);
        Assert.Equal(new DocColor(0, 0, 95), many.Palette.Colors[95]);
        Assert.Equal(4, many.Issues.Count);
        Assert.Equal(97, many.Issues[0].Line);
        Assert.Contains("ignored 4 colors after slot 96 (from line 97)", many.Summary);
        Assert.Equal(0, ColorPalette.Parse("; only a comment\n\nnope").ColorCount);
    }

    [Fact]
    public void TheDefaultPaletteIsTheMacsAndItsFileTextParsesBack()
    {
        var p = ColorPalette.Standard;
        Assert.Equal(96, p.Colors.Count);
        Assert.Equal(new DocColor(0, 0, 0), p.Colors[0]);
        Assert.Equal(new DocColor(255, 255, 255), p.Colors[7]);
        Assert.Equal(new DocColor(255, 0, 0), p.Colors[8]);
        Assert.Equal(96, p.Colors.Select(c => c.Hex).Distinct().Count());
        Assert.All(p.Colors, c => Assert.Equal(255, c.A));
        var text = p.FileText;
        Assert.StartsWith(";", text);
        Assert.Contains("\nFFFF0000\n", text);
        var back = ColorPalette.Parse(text);
        Assert.Equal(p, back.Palette);
        Assert.Empty(back.Issues);
        Assert.Equal(96, back.ColorCount);
    }

    [Fact]
    public void MoreAndLessGiveToolsA12PointGap()
    {
        // A 1280 by 800 window's stage, with y down here where the Mac's goes up.
        var stage = new Rect(0, 0, 1280, 574);
        var d = PanelLayout.Defaults(stage);
        var r = PanelLayout.ColorsFrames(d[PanelLayout.Colors], d[PanelLayout.Tools], true, stage);
        Assert.Equal(d[PanelLayout.Colors].Bottom, r.Colors.Bottom);
        Assert.Equal(94, r.Tools!.Value.Height);
        Assert.Equal(d[PanelLayout.Tools].Y, r.Tools.Value.Y);
        Assert.Equal(12, r.Colors.Y - r.Tools.Value.Bottom);
        Assert.True(r.Colors.Height is <= 470 and > 246);

        // A tall stage: Colors reaches 470 points and Tools shortens only as needed.
        var tall = new Rect(0, 0, 1280, 1000);
        var t = PanelLayout.Defaults(tall);
        var r2 = PanelLayout.ColorsFrames(t[PanelLayout.Colors], t[PanelLayout.Tools], true, tall);
        Assert.Equal(470, r2.Colors.Height);
        Assert.True(r2.Colors.Y - r2.Tools!.Value.Bottom >= 12);

        // Less goes back to 246 points and restores Tools.
        var back = PanelLayout.ColorsFrames(r.Colors, r.Tools, false, stage, d[PanelLayout.Tools]);
        Assert.Equal(d[PanelLayout.Colors], back.Colors);
        Assert.Equal(d[PanelLayout.Tools], back.Tools);
    }
}

/// Real pointer events through a panel's own window.
static class Gesture
{
    public static void Click(Control control, Point at, MouseButton button = MouseButton.Left, RawInputModifiers keys = RawInputModifiers.None)
    {
        var window = TopLevel.GetTopLevel(control) as Window ?? throw new InvalidOperationException("the control has no window");
        window.UpdateLayout();
        var point = control.TranslatePoint(at, window) ?? throw new InvalidOperationException("the control is not in its window");
        window.MouseMove(point, keys);
        window.MouseDown(point, button, keys);
        window.MouseUp(point, button, keys);
        Dispatcher.UIThread.RunJobs();
    }

    public static void Enter(Control control) =>
        control.RaiseEvent(new KeyEventArgs { RoutedEvent = InputElement.KeyDownEvent, Key = Key.Enter });
}

public sealed class ToolsPanelTests
{
    static MainWindow Ready()
    {
        var w = Shells.Open();
        Dispatcher.UIThread.RunJobs();
        w.Panels.Panels[PanelLayout.Tools].UpdateLayout();
        return w;
    }

    [AvaloniaFact]
    public void TheCellsAreTheCatalogsToolsInTwoColumnsWithTheirKeys()
    {
        var w = Ready();
        var cells = w.Panels.Tools.Cells.Values.ToList();
        Assert.Equal(19, cells.Count);
        Assert.Equal(w.Tools.Tools.Select(t => t.Id), cells.Select(c => c.Tool.Id));
        Assert.Equal("rectangleSelect", cells[0].Tool.Id);
        Assert.Equal("moveSelectedPixels", cells[1].Tool.Id);
        Assert.Equal("shapes", cells[18].Tool.Id);
        foreach (var cell in cells)
        {
            Assert.Equal($"{cell.Tool.Label} ({cell.Tool.Key.ToUpperInvariant()})", ToolTip.GetTip(cell));
            Assert.Equal(cell.Tool.Label, AutomationProperties.GetName(cell));
        }
        Assert.Equal("Rectangle Select (S)", ToolTip.GetTip(cells[0]));
        // 24 pixel targets, 2 pixel gaps, 5 from the top, centered in the 84 pixel window.
        var window = w.Panels.Panels[PanelLayout.Tools];
        Point At(int i) => cells[i].TranslatePoint(default, window)!.Value;
        Assert.Equal(new Point(17, 29), At(0));
        Assert.Equal(new Point(43, 29), At(1));
        Assert.Equal(new Point(17, 55), At(2));
        Assert.Equal(new Point(17, 29 + 9 * 26), At(18));
        Assert.Equal(24, cells[0].Bounds.Width);
        w.Close();
    }

    [AvaloniaFact]
    public void TheDefaultPanelShowsEveryToolWithNothingToScroll()
    {
        var w = Ready();
        var scroller = w.Panels.Tools.Scroller;
        Assert.Equal(scroller.Viewport.Height, scroller.Extent.Height);
        Assert.Equal(292, w.Panels.Panels[PanelLayout.Tools].ClientSize.Height);
        w.Close();
    }

    [AvaloniaFact]
    public void AShortPanelScrollsToReachTheLastTools()
    {
        var w = Ready();
        var window = w.Panels.Panels[PanelLayout.Tools];
        window.Height = 94;
        window.UpdateLayout();
        Dispatcher.UIThread.RunJobs();
        var scroller = w.Panels.Tools.Scroller;
        Assert.True(scroller.Extent.Height > scroller.Viewport.Height, $"{scroller.Extent} in {scroller.Viewport}");
        w.Close();
    }

    [AvaloniaFact]
    public void AClickOnAToolCellSelectsItAndTheRowHintAndCursorFollow()
    {
        var w = Ready();
        var cells = w.Panels.Tools.Cells;
        Assert.True(cells["paintbrush"].IsOn);
        Gesture.Click(cells["zoom"], new Point(12, 12));
        Assert.Equal("zoom", w.Tools.Current);
        Assert.True(cells["zoom"].IsOn);
        Assert.False(cells["paintbrush"].IsOn);
        Assert.Equal("selected", AutomationProperties.GetItemStatus(cells["zoom"]));
        Assert.Equal("Zoom", ((OpenPainter.Catalog.CatalogTool)w.Options.ToolPicker.SelectedItem!).Label);
        Assert.Equal("Zoom: click to zoom in, Alt-click to zoom out, drag a rectangle", w.Status.Hint.Text);
        Assert.Equal(CanvasCursor.Cross, w.View.Cursor);
        Gesture.Click(cells["pan"], new Point(12, 12));
        Assert.Equal(CanvasCursor.Move, w.View.Cursor);
        w.Close();
    }

    [AvaloniaFact]
    public void AToolThatCannotRunYetIsStillSelectableAndSaysSo()
    {
        var w = Ready();
        Fixture.Click(w.Panels.Tools.Cells["gradient"]);
        Assert.Equal("gradient", w.Tools.Current);
        Assert.Equal("Gradient is coming soon", w.Status.Hint.Text);
        Assert.Equal(CanvasTool.Inactive, w.View.Tool);
        w.Close();
    }

    [AvaloniaFact]
    public void TheMarkFollowsAToolChosenAnywhereElseAndKeepsAcrossATheme()
    {
        var w = Ready();
        w.UseToolKey('e');
        Assert.True(w.Panels.Tools.Cells["eraser"].IsOn);
        w.Options.ToolPicker.SelectedItem = w.Tools.Tools.Single(t => t.Id == "pencil");
        Dispatcher.UIThread.RunJobs();
        Assert.True(w.Panels.Tools.Cells["pencil"].IsOn);
        w.RequestedThemeVariant = Avalonia.Styling.ThemeVariant.Dark;
        Dispatcher.UIThread.RunJobs();
        w.Panels.Panels[PanelLayout.Tools].UpdateLayout();
        Assert.True(w.Panels.Tools.Cells["pencil"].IsOn);
        Assert.Single(w.Panels.Tools.Cells.Values, c => c.IsOn);
        // The old panel stopped listening.
        w.Tools.Current = "zoom";
        Assert.True(w.Panels.Tools.Cells["zoom"].IsOn);
        w.Close();
    }
}

public sealed class ColorsPanelTests
{
    static (MainWindow Window, ColorsPanel View, ToolState Tools) Ready()
    {
        var w = Shells.Open();
        Dispatcher.UIThread.RunJobs();
        w.Panels.Panels[PanelLayout.Colors].UpdateLayout();
        return (w, w.Panels.Colors, w.Tools);
    }

    static void Settle(MainWindow w)
    {
        foreach (var panel in w.Panels.Panels.Values) panel.UpdateLayout();
        Dispatcher.UIThread.RunJobs();
    }

    static void Type(TextBox box, string text)
    {
        box.Text = text;
        Gesture.Enter(box);
    }

    static readonly DocColor Red = new(255, 0, 0), Blue = new(0, 0, 255);

    [AvaloniaFact]
    public void TheSwatchesSwapResetAndMoveTheNotch()
    {
        var (w, view, tools) = Ready();
        tools.Primary = Red;
        tools.Secondary = Blue;
        Fixture.Click(view.SwapButton);
        Assert.Equal(Blue, tools.Primary);
        Assert.Equal(Red, tools.Secondary);
        tools.SwapColors();
        Assert.Equal(Red, tools.Primary);
        Fixture.Click(view.ResetButton);
        Assert.Equal(new DocColor(0, 0, 0), tools.Primary);
        Assert.Equal(new DocColor(255, 255, 255), tools.Secondary);
        // Clicking the other swatch moves the notch, and edits then go to that slot.
        Assert.Equal(ColorSlot.Primary, tools.ActiveSlot);
        Assert.Equal(ColorSlot.Secondary, SwatchPair.Slot(new Point(50, 50)));
        Assert.Equal(ColorSlot.Primary, SwatchPair.Slot(new Point(5, 5)));
        Assert.Null(SwatchPair.Slot(new Point(50, 2)));
        Gesture.Click(view.Swatches, new Point(50, 50));
        Assert.Equal(ColorSlot.Secondary, tools.ActiveSlot);
        Type(view.HexField, "123456");
        Assert.Equal(new DocColor(0x12, 0x34, 0x56), tools.Secondary);
        Assert.Equal(new DocColor(0, 0, 0), tools.Primary);
        Assert.Equal("Secondary color hex", AutomationProperties.GetName(view.HexField));
        Gesture.Click(view.Swatches, new Point(5, 5));
        Assert.Equal(ColorSlot.Primary, tools.ActiveSlot);
        Assert.Equal("Primary color hex", AutomationProperties.GetName(view.HexField));
        tools.ToggleActiveSlot();
        Assert.Equal(ColorSlot.Secondary, tools.ActiveSlot);
        w.Close();
    }

    [AvaloniaFact]
    public void TheButtonsHaveTheMacsWordsAndPlaces()
    {
        var (w, view, _) = Ready();
        Assert.Equal("Swap colors (X)", ToolTip.GetTip(view.SwapButton));
        Assert.Equal("Black and white", ToolTip.GetTip(view.ResetButton));
        Assert.Equal("RRGGBB, or RRGGBBAA with alpha", ToolTip.GetTip(view.HexField));
        Assert.Equal("Load, save or reset the palette", ToolTip.GetTip(view.PaletteButton));
        Assert.Equal("More »", view.MoreButton.Content);
        var window = w.Panels.Panels[PanelLayout.Colors];
        Point At(Control c) => c.TranslatePoint(default, window)!.Value;
        const double title = Tokens.PanelTitleHeight;
        Assert.Equal(new Point(10, title + 10), At(view.Swatches));
        Assert.Equal(new Point(68, title + 10), At(view.SwapButton));
        Assert.Equal(new Point(68, title + 40), At(view.ResetButton));
        Assert.Equal(new Point(182, title + 10), At(view.Wheel));
        Assert.Equal(new Point(8, title + 72), At(view.MoreButton));
        Assert.Equal(new Point(182, title + 72), At(view.AddColorButton));
        Assert.Equal(new Point(210, title + 72), At(view.PaletteButton));
        w.Close();
    }

    [AvaloniaFact]
    public void TheHexFieldRefusesBadInputAndKeepsAlpha()
    {
        var (w, view, tools) = Ready();
        tools.Primary = new DocColor(0xCF, 0x68, 0x4D);
        Settle(w);
        var refused = w.RefusedInputs;
        foreach (var bad in new[] { "zzz", "+12345", "12345", "#1234567" })
        {
            Type(view.HexField, bad);
            Assert.Equal(new DocColor(0xCF, 0x68, 0x4D), tools.Primary);
            Assert.Equal("#CF684D", view.HexField.Text);
        }
        Assert.Equal(refused + 4, w.RefusedInputs);
        Type(view.HexField, "#00FF0080");
        Assert.Equal(new DocColor(0, 255, 0, 0x80), tools.Primary);
        Assert.Equal("#00FF0080", view.HexField.Text);
        w.Close();
    }

    [AvaloniaFact]
    public void EndingAnUnchangedFieldEditKeepsAColorSetElsewhere()
    {
        var (w, view, tools) = Ready();
        view.SetExpanded(true);
        Settle(w);
        tools.Primary = new DocColor(0xCF, 0x68, 0x4D);
        view.HexField.Focus();
        Dispatcher.UIThread.RunJobs();
        Assert.True(view.HexField.IsFocused);
        tools.Primary = new DocColor(1, 2, 3);
        Assert.Equal("#CF684D", view.HexField.Text);   // The field being typed in keeps its text.
        Gesture.Enter(view.HexField);
        Assert.Equal(new DocColor(1, 2, 3), tools.Primary);
        Assert.Equal("#010203", view.HexField.Text);
        var red = view.Channels[ColorChannel.Red].Field;
        red.Focus();
        Dispatcher.UIThread.RunJobs();
        tools.Primary = new DocColor(9, 2, 3);
        Gesture.Enter(red);
        Assert.Equal(new DocColor(9, 2, 3), tools.Primary);
        Assert.Equal("9", red.Text);
        w.Close();
    }

    [AvaloniaFact]
    public void TheWheelEditsTheActiveSlotAndARightClickTheOther()
    {
        var (w, view, tools) = Ready();
        tools.Primary = new DocColor(255, 0, 0);
        tools.Secondary = new DocColor(255, 255, 255);
        // The top of the wheel is hue 90.
        view.WheelPicked(0, 1, WheelModifiers.None, false);
        Assert.Equal(90, view.Hsv.H, 6);
        Assert.Equal(100, view.Hsv.S, 6);
        Assert.Equal(new Hsv(90, 100, 100).Color(), tools.Primary);
        // Ctrl keeps the saturation, and Alt keeps the hue.
        view.WheelPicked(-.5, 0, WheelModifiers.KeepRadius, false);
        Assert.Equal(180, view.Hsv.H, 6);
        Assert.Equal(100, view.Hsv.S, 6);
        view.WheelPicked(-.5, .4, WheelModifiers.KeepSpoke, false);
        Assert.Equal(180, view.Hsv.H, 6);
        Assert.Equal(50, view.Hsv.S, 6);

        // Real pointer events: a left click at the right edge is red at full strength on the
        // active slot, and a right click at the bottom sets the other slot.
        tools.Primary = new DocColor(1, 1, 1);
        tools.Secondary = new DocColor(128, 128, 128);
        Settle(w);
        var primary = tools.Primary;
        Gesture.Click(view.Wheel, new Point(26, 52), MouseButton.Right);
        Assert.Equal(primary, tools.Primary);
        Assert.Equal(new Hsv(270, 100, Hsv.Of(new DocColor(128, 128, 128)).V).Color(), tools.Secondary);
        Gesture.Click(view.Wheel, new Point(52, 26));
        Assert.Equal(0, view.Hsv.H, 6);
        Assert.Equal(100, view.Hsv.S, 6);
        w.Close();
    }

    [AvaloniaFact]
    public void BlackDoesNotHideAWheelPickAndGreyKeepsItsHue()
    {
        var (w, view, tools) = Ready();
        tools.Primary = new DocColor(0, 0, 0);
        Settle(w);
        view.WheelPicked(1, 0, WheelModifiers.None, false);
        Assert.Equal(new DocColor(255, 0, 0), tools.Primary);
        view.ChannelChanged(ColorChannel.Hue, 120);
        Assert.Equal(new DocColor(0, 255, 0), tools.Primary);
        tools.Primary = new DocColor(90, 90, 90);
        Settle(w);
        Assert.Equal(120, view.Hsv.H, 6);
        tools.Primary = new DocColor(0, 0, 0);
        Assert.Equal(120, view.Hsv.H, 6);
        w.Close();
    }

    [AvaloniaFact]
    public void TheWheelsArrowKeysTurnTheHueAndChangeTheSaturation()
    {
        var (w, view, tools) = Ready();
        view.Wheel.Focus();
        tools.Primary = new DocColor(255, 0, 0);
        Settle(w);
        var before = view.Hsv;
        void Press(Key key, RawInputModifiers keys = RawInputModifiers.None) =>
            view.Wheel.RaiseEvent(new KeyEventArgs { RoutedEvent = InputElement.KeyDownEvent, Key = key, KeyModifiers = (KeyModifiers)keys });
        Press(Key.Right);
        Assert.Equal(before.H + 1, view.Hsv.H, 6);
        Press(Key.Right, RawInputModifiers.Shift);
        Assert.Equal(before.H + 16, view.Hsv.H, 6);
        Press(Key.Down, RawInputModifiers.Shift);
        Assert.Equal(90, view.Hsv.S, 6);
        Press(Key.Up);
        Assert.Equal(91, view.Hsv.S, 6);
        w.Close();
    }

    [AvaloniaFact]
    public void MoreShowsSlidersFieldsAndAll96AndResizesThePanels()
    {
        var (w, view, tools) = Ready();
        var panels = w.Panels;
        var colorsWindow = panels.Panels[PanelLayout.Colors];
        var toolsWindow = panels.Panels[PanelLayout.Tools];
        panels.ShowAll();
        Settle(w);
        var colorsBefore = panels.FrameOf(colorsWindow);
        var toolsBefore = panels.FrameOf(toolsWindow);
        Assert.Equal(32, view.Swatchboard.Cells.Count);
        Assert.All(view.Channels.Values, row => Assert.False(row.IsVisible));

        Fixture.Click(view.MoreButton);
        Settle(w);
        Assert.True(view.IsExpanded);
        Assert.Equal("« Less", view.MoreButton.Content);
        Assert.Equal("Less", AutomationProperties.GetName(view.MoreButton));
        Assert.Equal(96, view.Swatchboard.Cells.Count);
        Assert.All(view.Channels.Values, row => Assert.True(row.IsVisible));
        var c = panels.FrameOf(colorsWindow);
        var t = panels.FrameOf(toolsWindow);
        Assert.True(c.Height > colorsBefore.Height && c.Height <= 470, $"{c}");
        Assert.Equal(colorsBefore.Bottom, c.Bottom, 3);
        Assert.True(c.Y - t.Bottom >= 11.5, $"Tools {t} and Colors {c}");
        Assert.True(t.Height >= 94, $"{t}");

        // The palette scrolls inside the expanded panel.
        Assert.True(view.Swatchboard.Scroller.Viewport.Height < PaletteGrid.HeightOf(12), $"{view.Swatchboard.Scroller.Viewport}");
        Assert.Equal(PaletteGrid.HeightOf(12), view.Swatchboard.Scroller.Extent.Height);

        // Fields and sliders edit the active color.
        tools.Primary = new DocColor(0, 0, 0);
        Settle(w);
        var red = view.Channels[ColorChannel.Red];
        Type(red.Field, "200");
        Assert.Equal(new DocColor(200, 0, 0), tools.Primary);
        var refused = w.RefusedInputs;
        Type(red.Field, "300");
        Assert.Equal(new DocColor(200, 0, 0), tools.Primary);
        Assert.Equal("200", red.Field.Text);
        Assert.Equal(refused + 1, w.RefusedInputs);
        view.ChannelChanged(ColorChannel.Alpha, 128);
        Assert.Equal(new DocColor(200, 0, 0, 128), tools.Primary);
        view.ChannelChanged(ColorChannel.Hue, 120);
        Assert.Equal(new DocColor(0, 200, 0, 128), tools.Primary);
        // Value 0 and back keeps hue and saturation.
        view.ChannelChanged(ColorChannel.Value, 0);
        Assert.Equal(new DocColor(0, 0, 0, 128), tools.Primary);
        view.ChannelChanged(ColorChannel.Value, 100);
        Assert.Equal(new DocColor(0, 255, 0, 128), tools.Primary);
        Assert.Equal("100", view.Channels[ColorChannel.Saturation].Field.Text);
        var green = view.Channels[ColorChannel.Green].Slider;
        green.RaiseEvent(new KeyEventArgs { RoutedEvent = InputElement.KeyDownEvent, Key = Key.Left });
        Assert.Equal(254, tools.Primary.G);
        var blue = view.Channels[ColorChannel.Blue].Slider;
        Assert.Equal(255, blue.ValueAt(blue.Track.Right + 10));
        Assert.Equal(0, blue.ValueAt(blue.Track.X - 10));
        // A real drag on a slider sets the value.
        Gesture.Click(blue, new Point(blue.Track.Right, 12));
        Assert.Equal(255, tools.Primary.B);

        // Less restores both panels.
        Fixture.Click(view.MoreButton);
        Settle(w);
        Assert.False(view.IsExpanded);
        Assert.Equal(32, view.Swatchboard.Cells.Count);
        Assert.Equal(colorsBefore, panels.FrameOf(colorsWindow));
        Assert.Equal(toolsBefore, panels.FrameOf(toolsWindow));
        w.Close();
    }

    [AvaloniaFact]
    public void PaletteClicksAddModeAndTheChosenMark()
    {
        var (w, view, tools) = Ready();
        tools.Primary = new DocColor(1, 1, 1);
        tools.Secondary = new DocColor(2, 2, 2);
        Settle(w);
        var p = tools.Palette.Colors;
        var cells = view.Swatchboard.Cells;
        var middle = new Point(12, 12);
        Gesture.Click(cells[8], middle);
        Assert.Equal(p[8], tools.Primary);
        Assert.Equal(8, view.Swatchboard.Chosen);
        Gesture.Click(cells[9], middle, MouseButton.Right);
        Assert.Equal(p[9], tools.Secondary);
        Assert.Equal(p[8], tools.Primary);
        Gesture.Click(cells[10], middle, keys: RawInputModifiers.Shift);
        Assert.Equal(p[10], tools.Secondary);
        // With the secondary slot active, a click goes there.
        tools.ActiveSlot = ColorSlot.Secondary;
        Gesture.Click(cells[12], middle);
        Assert.Equal(p[12], tools.Secondary);
        Assert.Equal(p[8], tools.Primary);
        tools.ActiveSlot = ColorSlot.Primary;
        // Add mode puts the active color into the clicked slot, once.
        tools.Primary = new DocColor(9, 8, 7);
        Assert.Null(view.Swatchboard.Chosen);
        Fixture.Click(view.AddColorButton);
        Assert.True(view.Swatchboard.AddMode);
        Assert.True(view.AddColorButton.On);
        Gesture.Click(cells[3], middle);
        Assert.Equal(new DocColor(9, 8, 7), tools.Palette.Colors[3]);
        Assert.Equal(new DocColor(9, 8, 7), tools.Primary);
        Assert.False(view.AddColorButton.On);
        Assert.False(view.Swatchboard.AddMode);
        Assert.Equal(3, view.Swatchboard.Chosen);
        Assert.Equal("#090807: click for the active color, right-click or Shift-click for the other", ToolTip.GetTip(cells[3]));
        Assert.Equal("Palette color 4, #090807", AutomationProperties.GetName(cells[3]));
        Settle(w);
        Assert.Equal(24, cells[0].Bounds.Width);
        Assert.Equal(2, cells[1].Bounds.X - cells[0].Bounds.Right);
        Assert.Equal(2, cells[8].Bounds.Y - cells[0].Bounds.Bottom);
        Assert.Equal(cells[0].Bounds.X, cells[8].Bounds.X);
        w.Close();
    }

    [AvaloniaFact]
    public void TheCompactBodyFitsTheDefaultPanelWithNothingToScroll()
    {
        var (w, view, _) = Ready();
        Assert.True(ColorsPanel.CompactContentHeight <= Tokens.ColorsHeight - Tokens.PanelTitleHeight, $"{ColorsPanel.CompactContentHeight}");
        Settle(w);
        var scroller = view.Swatchboard.Scroller;
        Assert.True(scroller.Extent.Height <= scroller.Viewport.Height, $"{scroller.Extent} in {scroller.Viewport}");
        Assert.Equal(PaletteGrid.HeightOf(4), scroller.Extent.Height);
        w.Close();
    }

    [AvaloniaFact]
    public void TheMenuHasTheMacsItemsAndSaveWaitsForW10()
    {
        var (w, view, _) = Ready();
        var items = view.PaletteMenu.Items.OfType<MenuItem>().ToList();
        Assert.Equal(["Load Palette…", "Save Palette As…", "Reset to Default Palette"], items.Select(i => (string)i.Header!));
        Assert.Single(view.PaletteMenu.Items.OfType<Separator>());
        Assert.True(items[0].IsEnabled);
        Assert.False(items[1].IsEnabled);
        Assert.True(items[2].IsEnabled);
        Fixture.Click(view.PaletteButton);
        Dispatcher.UIThread.RunJobs();
        Assert.True(view.PaletteMenu.IsOpen);
        view.PaletteMenu.Close();
        w.Close();
    }

    [AvaloniaFact]
    public void ResetToDefaultPaletteBringsItBackAndSaysSo()
    {
        var (w, view, tools) = Ready();
        tools.Palette = tools.Palette.With(0, new DocColor(1, 2, 3));
        var reset = view.PaletteMenu.Items.OfType<MenuItem>().Single(i => (string?)i.Header == ColorsPanel.ResetItem);
        reset.RaiseEvent(new RoutedEventArgs(MenuItem.ClickEvent));
        Assert.Equal(ColorPalette.Standard, tools.Palette);
        Assert.Equal("Palette reset to the default.", view.LastReport);
        Assert.Equal("Palette reset to the default.", w.Status.Hint.Text);
        w.Close();
    }

    sealed class TempFolder : IDisposable
    {
        public string Path { get; } = Directory.CreateTempSubdirectory("openpainter-w8-").FullName;
        public string File(string name) => System.IO.Path.Combine(Path, name);
        public void Dispose() => Directory.Delete(Path, true);
    }

    [AvaloniaFact]
    public void PaletteSaveAndLoadRoundTripAndReport()
    {
        using var folder = new TempFolder();
        var (w, view, tools) = Ready();
        var reports = new List<string>();
        var shown = w.Panels.Context.Report;
        w.Panels.Context.Report = text => { reports.Add(text); shown?.Invoke(text); };
        var p = tools.Palette.With(0, new DocColor(1, 2, 3, 4)).With(95, new DocColor(250, 251, 252, 0));
        tools.Palette = p;
        var path = folder.File("mine.txt");
        Assert.True(view.SavePalette(path));
        var text = File.ReadAllText(path);
        Assert.Equal(96, text.Split('\n').Count(l => l.Length != 0 && !l.StartsWith(';')));
        Assert.Contains("\n04010203\n", text);
        Assert.Empty(Directory.GetFiles(folder.Path, "*.part"));
        tools.Palette = ColorPalette.Standard;
        var r = view.LoadPalette(path);
        Assert.NotNull(r);
        Assert.Empty(r.Issues);
        Assert.Equal(p, tools.Palette);

        // A file with bad lines loads the good ones and reports the rest.
        var bad = folder.File("bad.txt");
        File.WriteAllText(bad, "FFFF0000\nnot a color\nFF00FF00\n");
        view.LoadPalette(bad);
        Assert.Equal(Red, tools.Palette.Colors[0]);
        Assert.Equal(new DocColor(0, 255, 0), tools.Palette.Colors[1]);
        Assert.Equal(ColorPalette.Padding, tools.Palette.Colors[2]);
        Assert.Equal("bad.txt: Loaded 2 of 96 colors; skipped 1 malformed line (2).", view.LastReport);
        Assert.Equal(view.LastReport, ToolTip.GetTip(view.Swatchboard));

        // A file with no colors leaves the palette alone, and a missing file is reported.
        var before = tools.Palette;
        var empty = folder.File("empty.txt");
        File.WriteAllText(empty, "; nothing\n");
        view.LoadPalette(empty);
        Assert.Equal(before, tools.Palette);
        Assert.Contains("has no palette colors", view.LastReport);
        Assert.Null(view.LoadPalette(folder.File("missing.txt")));
        Assert.StartsWith("Could not open missing.txt", view.LastReport);
        Assert.Equal(5, reports.Count);
        // Reports reach the status bar.
        Assert.Equal(view.LastReport, w.Status.Hint.Text);

        // A file that is not UTF-8 still parses.
        var latin = folder.File("latin.txt");
        File.WriteAllBytes(latin, Encoding.Latin1.GetBytes("; café\nFF0000FF\n"));
        view.LoadPalette(latin);
        Assert.Equal(Blue, tools.Palette.Colors[0]);

        // A folder in the way of a save is reported, and the palette stays.
        Directory.CreateDirectory(folder.File("folder.txt"));
        Assert.False(view.SavePalette(folder.File("folder.txt")));
        Assert.StartsWith("Could not save the palette", view.LastReport);
        w.Close();
    }

    [AvaloniaFact]
    public void LoadPaletteAsksForAFileAndLoadsWhatComesBack()
    {
        using var folder = new TempFolder();
        var (w, view, tools) = Ready();
        var path = folder.File("one.txt");
        File.WriteAllText(path, "FF0000FF\n");
        var asked = 0;
        string? answer = null;
        w.Panels.Context.PickPaletteFile = _ =>
        {
            asked++;
            return Task.FromResult(answer);
        };
        var load = view.PaletteMenu.Items.OfType<MenuItem>().Single(i => (string?)i.Header == ColorsPanel.LoadItem);
        load.RaiseEvent(new RoutedEventArgs(MenuItem.ClickEvent));
        Dispatcher.UIThread.RunJobs();
        Assert.Equal(1, asked);
        Assert.Equal(ColorPalette.Standard, tools.Palette);   // No file chosen.
        answer = path;
        load.RaiseEvent(new RoutedEventArgs(MenuItem.ClickEvent));
        Dispatcher.UIThread.RunJobs();
        Assert.Equal(Blue, tools.Palette.Colors[0]);
        // A picker that fails reaches the status bar.
        w.Panels.Context.PickPaletteFile = _ => throw new IOException("no picker here");
        load.RaiseEvent(new RoutedEventArgs(MenuItem.ClickEvent));
        Dispatcher.UIThread.RunJobs();
        Assert.Equal("Could not open a palette: no picker here", w.Status.Hint.Text);
        w.Close();
    }

    [AvaloniaFact]
    public void AThemeChangeKeepsMoreTheColorsAndAddMode()
    {
        var (w, _, tools) = Ready();
        var first = w.Panels.Colors;
        first.SetExpanded(true);
        Fixture.Click(first.AddColorButton);
        tools.Primary = new DocColor(0, 0, 0);
        first.ChannelChanged(ColorChannel.Hue, 200);
        w.RequestedThemeVariant = Avalonia.Styling.ThemeVariant.Dark;
        Settle(w);
        var view = w.Panels.Colors;
        Assert.NotSame(first, view);
        Assert.True(view.IsExpanded);
        Assert.Equal("« Less", view.MoreButton.Content);
        Assert.Equal(96, view.Swatchboard.Cells.Count);
        Assert.True(view.Swatchboard.AddMode);
        Assert.Equal(200, view.Hsv.H, 6);
        // The old panel stopped listening, and the new one follows.
        tools.Primary = Red;
        Assert.Equal("#FF0000", view.HexField.Text);
        Assert.Equal("#000000", first.HexField.Text);
        w.Close();
    }

    /// The window's own pixels at a point in a control.
    static unsafe Avalonia.Media.Color Sample(Window window, Control control, Point at)
    {
        window.UpdateLayout();
        Dispatcher.UIThread.RunJobs();
        var frame = window.CaptureRenderedFrame() ?? throw new InvalidOperationException("no rendered frame");
        var point = control.TranslatePoint(at, window) ?? throw new InvalidOperationException("the control is not in its window");
        var bytes = new byte[frame.PixelSize.Width * frame.PixelSize.Height * 4];
        var stride = frame.PixelSize.Width * 4;
        fixed (byte* first = bytes) frame.CopyPixels(new PixelRect(frame.PixelSize), (nint)first, bytes.Length, stride);
        var i = (int)point.Y * stride + (int)point.X * 4;
        return frame.Format == Avalonia.Platform.PixelFormat.Rgba8888
            ? Avalonia.Media.Color.FromArgb(bytes[i + 3], bytes[i], bytes[i + 1], bytes[i + 2])
            : Avalonia.Media.Color.FromArgb(bytes[i + 3], bytes[i + 2], bytes[i + 1], bytes[i]);
    }

    [AvaloniaTheory]
    [InlineData("light")]
    [InlineData("dark")]
    public void DocumentColorsAreExactInEitherTheme(string theme)
    {
        var w = Shells.Open(theme);
        Dispatcher.UIThread.RunJobs();
        var window = w.Panels.Panels[PanelLayout.Colors];
        var view = w.Panels.Colors;
        w.Tools.Primary = new DocColor(0x12, 0x34, 0x56);
        w.Tools.Secondary = new DocColor(0xAB, 0xCD, 0xEF);
        Settle(w);
        Assert.Equal(Avalonia.Media.Color.FromRgb(0x12, 0x34, 0x56), Sample(window, view.Swatches, new Point(16, 16)));
        Assert.Equal(Avalonia.Media.Color.FromRgb(0xAB, 0xCD, 0xEF), Sample(window, view.Swatches, new Point(44, 44)));
        // Palette slot 8 is pure red in the middle of its swatch.
        Assert.Equal(Avalonia.Media.Color.FromRgb(255, 0, 0), Sample(window, view.Swatchboard.Cells[8], new Point(12, 12)));
        w.Close();
    }

    [AvaloniaFact]
    public void AColorWithAlphaShowsOverACheckerboard()
    {
        var w = Shells.Open();
        Dispatcher.UIThread.RunJobs();
        var window = w.Panels.Panels[PanelLayout.Colors];
        w.Tools.Primary = new DocColor(0, 0, 0, 128);
        Settle(w);
        var light = Sample(window, w.Panels.Colors.Swatches, new Point(9, 3));    // White under half black.
        var dark = Sample(window, w.Panels.Colors.Swatches, new Point(15, 3));    // Grey under half black.
        Assert.NotEqual(light, dark);
        w.Close();
    }
}
