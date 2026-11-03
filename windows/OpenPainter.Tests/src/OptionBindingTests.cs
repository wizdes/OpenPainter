// The options row against real tool state, and the status bar's Units and Profile against the
// real engine. OptionsBarView.swift, SelectCropTool.swift and TextOptionsControls.swift hold the
// same rules on the Mac.
using Avalonia;
using Avalonia.Automation;
using Avalonia.Controls;
using Avalonia.Headless.XUnit;
using Avalonia.Input;
using Avalonia.Interactivity;
using Avalonia.Threading;
using Avalonia.VisualTree;
using OpenPainter.Bridge;
using OpenPainter.Shell;
using UiLook;
using Xunit;

namespace OpenPainter.Tests;

public sealed class OptionBindingTests
{
    static MainWindow Ready(string tool = "paintbrush")
    {
        var w = Shells.Open();
        w.Tools.Current = tool;
        Dispatcher.UIThread.RunJobs();
        return w;
    }

    static void Type(TextBox box, string text)
    {
        box.Text = text;
        Gesture.Enter(box);
    }

    [Fact]
    public void BrushWidthStepsArePaintDotNets()
    {
        var s = new ToolSettings();
        Assert.Equal(3, s.StepWidth(true).BrushWidth);
        Assert.Equal(1, s.StepWidth(false).BrushWidth);
        Assert.Equal(20, (s with { BrushWidth = 15 }).StepWidth(true).BrushWidth);
        Assert.Equal(2000, (s with { BrushWidth = 1999 }).StepWidth(true).BrushWidth);
        Assert.Equal(2000, (s with { BrushWidth = 2000 }).StepWidth(true).BrushWidth);
        Assert.Equal(1, (s with { BrushWidth = 1 }).StepWidth(false).BrushWidth);
        Assert.Equal(15, (s with { BrushWidth = 17.5 }).StepWidth(false).BrushWidth);
    }

    [Fact]
    public void TheStepListIsTheMacs()
    {
        var source = Repo.Read("mac/OpenPainterKit/Sources/Shell/ToolState.swift");
        var from = source.IndexOf("widthSteps: [Double] = [", StringComparison.Ordinal) + "widthSteps: [Double] = [".Length;
        var to = source.IndexOf(']', from);
        var numbers = System.Text.RegularExpressions.Regex.Matches(source[from..to], @"\b\d+\b").Select(m => double.Parse(m.Value)).ToArray();
        Assert.Equal(numbers, ToolSettings.WidthSteps);
    }

    [AvaloniaFact]
    public void BrushMinusAndPlusStepAndTurnOffAtTheEnds()
    {
        var w = Ready();
        var minus = w.Options.Option<IconCommand>("Decrease brush width");
        var plus = w.Options.Option<IconCommand>("Increase brush width");
        var field = w.Options.Option<TextBox>(OptionBinding.BrushWidth);
        Assert.Equal("2", field.Text);
        Fixture.Click(plus);
        Assert.Equal(3, w.Tools.Settings.BrushWidth);
        Assert.Equal("3", field.Text);
        Fixture.Click(minus);
        Fixture.Click(minus);
        Assert.Equal(1, w.Tools.Settings.BrushWidth);
        Assert.False(minus.IsEnabled);
        Assert.True(plus.IsEnabled);
        Type(field, "2000");
        Assert.False(plus.IsEnabled);
        Assert.True(minus.IsEnabled);
        w.Close();
    }

    [AvaloniaFact]
    public void AFieldClampsAndBadInputShowsTheOldValue()
    {
        var w = Ready();
        var width = w.Options.Option<TextBox>(OptionBinding.BrushWidth);
        Type(width, "5000");
        Assert.Equal(2000, w.Tools.Settings.BrushWidth);
        Assert.Equal("2000", width.Text);
        Type(width, "12.5");
        Assert.Equal(12.5, w.Tools.Settings.BrushWidth);
        Assert.Equal("12.5", width.Text);
        Type(width, "abc");
        Assert.Equal(12.5, w.Tools.Settings.BrushWidth);
        Assert.Equal("12.5", width.Text);
        var hardness = w.Options.Option<TextBox>(OptionBinding.Hardness);
        Type(hardness, "-5");
        Assert.Equal(0, w.Tools.Settings.Hardness);
        var opacity = w.Options.Option<TextBox>(OptionBinding.Opacity);
        Type(opacity, "150");
        Assert.Equal(100, w.Tools.Settings.Opacity);
        Type(opacity, "40");
        Assert.Equal(40, w.Tools.Settings.Opacity);
        w.Close();
    }

    [AvaloniaFact]
    public void ACheckBoxAndAChoiceWriteTheirSetting()
    {
        var w = Ready();
        w.Options.Option<CheckBox>(OptionBinding.Antialiasing).IsChecked = false;
        Assert.False(w.Tools.Settings.Antialias);
        w.Options.Option<CheckBox>(OptionBinding.PenSize).IsChecked = true;
        Assert.True(w.Tools.Settings.PressureSize);
        w.Options.Option<CheckBox>(OptionBinding.PenOpacity).IsChecked = true;
        Assert.True(w.Tools.Settings.PressureOpacity);
        w.Options.Option<ComboBox>(OptionBinding.BlendMode).SelectedIndex = 1;
        Assert.Equal(BlendChoice.Overwrite, w.Tools.Settings.Blend);
        w.Close();
    }

    [AvaloniaFact]
    public void TheFloodRowsBindEveryControlAndTheModeFollowsTheTool()
    {
        var w = Ready("magicWand");
        var o = w.Options;
        Assert.Equal(50, o.Option<Slider>(OptionBinding.Tolerance).Value);
        o.Option<Slider>(OptionBinding.Tolerance).Value = 70.4;
        Assert.Equal(70, w.Tools.Settings.Tolerance);
        Assert.Equal("70", o.Option<TextBox>(OptionBinding.Tolerance).Text);
        Type(o.Option<TextBox>(OptionBinding.Tolerance), "25");
        Assert.Equal(25, o.Option<Slider>(OptionBinding.Tolerance).Value);
        o.Option<ComboBox>(OptionBinding.ToleranceAlpha).SelectedIndex = 1;
        Assert.Equal(ToleranceAlphaChoice.Straight, w.Tools.Settings.ToleranceAlpha);
        o.Option<ComboBox>(OptionBinding.SelectionMode).SelectedIndex = 4;
        Assert.Equal(SelectionModeChoice.Invert, w.Tools.Settings.SelectionMode);
        o.Option<ComboBox>(OptionBinding.Sampling).SelectedIndex = 1;
        Assert.Equal(SamplingChoice.Image, w.Tools.Settings.Sampling);
        o.Option<ComboBox>(OptionBinding.FloodMode).SelectedIndex = 1;
        Assert.Equal(FloodModeChoice.Global, w.Tools.Settings.FloodMode);
        Assert.Equal("Premultiplied: transparent pixels match. Straight: they match only when their colors match.", ToolTip.GetTip(o.Option<ComboBox>(OptionBinding.ToleranceAlpha)));
        // Paint Bucket has Blend mode in that place. Tolerance carries over.
        w.Tools.Current = "paintBucket";
        Dispatcher.UIThread.RunJobs();
        Assert.Equal("25", w.Options.Option<TextBox>(OptionBinding.Tolerance).Text);
        Assert.Throws<InvalidOperationException>(() => w.Options.Option<ComboBox>(OptionBinding.SelectionMode));
        w.Options.Option<ComboBox>(OptionBinding.BlendMode).SelectedIndex = 1;
        Assert.Equal(BlendChoice.Overwrite, w.Tools.Settings.Blend);
        // A Finish button waits for W9's session.
        Assert.False(w.Options.Option<Button>("Finish fill or selection").IsEnabled);
        w.Close();
    }

    [AvaloniaFact]
    public void TheColorPickerHasItsOwnSampling()
    {
        var w = Ready("colorPicker");
        var o = w.Options;
        o.Option<ComboBox>(OptionBinding.Sampling).SelectedIndex = 1;
        Assert.Equal(SamplingChoice.Image, w.Tools.Settings.PickerSampling);
        Assert.Equal(SamplingChoice.Layer, w.Tools.Settings.Sampling);
        o.Option<ComboBox>(OptionBinding.SampleSize).SelectedIndex = 7;
        Assert.Equal(PickerSizeChoice.FiftyOne, w.Tools.Settings.PickerSize);
        o.Option<ComboBox>(OptionBinding.AfterClick).SelectedIndex = 1;
        Assert.Equal(PickerAfterClickChoice.PreviousTool, w.Tools.Settings.PickerAfterClick);
        Assert.Equal("paintbrush", w.Tools.PreviousTool);
        w.Close();
    }

    [AvaloniaFact]
    public void TheMoveToolsResamplingBindsAndItsButtonsWaitForW9()
    {
        var w = Ready("moveSelection");
        var resampling = w.Options.Option<ComboBox>(OptionBinding.Resampling);
        Assert.Equal(2, resampling.SelectedIndex);   // Bicubic, paint.net's default.
        resampling.SelectedIndex = 4;
        Assert.Equal(MoveResamplingChoice.BestQuality, w.Tools.Settings.MoveResampling);
        Assert.False(w.Options.Option<Button>("Finish move (Enter)").IsEnabled);
        Assert.False(w.Options.Option<Button>("Cancel move (Escape)").IsEnabled);
        w.Close();
    }

    [AvaloniaFact]
    public void RatioAndSizeShowByTheStyleChoiceAndBind()
    {
        var w = Ready("rectangleSelect");
        var row = w.Options;
        var style = row.Option<ComboBox>(OptionBinding.Style);
        Assert.Equal(3, row.Groups.Count);
        Assert.Throws<InvalidOperationException>(() => row.Option<TextBox>(OptionBinding.RatioWidth));

        style.SelectedIndex = 1;
        Dispatcher.UIThread.RunJobs();
        Assert.Same(style, row.Option<ComboBox>(OptionBinding.Style));
        Assert.Equal(SelectStyleChoice.FixedRatio, w.Tools.Settings.SelectStyle);
        Assert.Equal(4, row.Groups.Count);
        var ratioWidth = row.Option<TextBox>(OptionBinding.RatioWidth);
        Assert.Equal(44, ratioWidth.Width);
        Type(ratioWidth, "16");
        Type(row.Option<TextBox>(OptionBinding.RatioHeight), "9");
        Assert.Equal(16, w.Tools.Settings.RatioWidth);
        Assert.Equal(9, w.Tools.Settings.RatioHeight);
        Type(ratioWidth, "0");   // SelectCropController takes numbers above 0.
        Assert.Equal(16, w.Tools.Settings.RatioWidth);
        Assert.Equal("16", ratioWidth.Text);
        Assert.Throws<InvalidOperationException>(() => row.Option<TextBox>(OptionBinding.SizeWidth));

        style.SelectedIndex = 2;
        Dispatcher.UIThread.RunJobs();
        Assert.Equal(4, row.Groups.Count);
        Assert.Throws<InvalidOperationException>(() => row.Option<TextBox>(OptionBinding.RatioWidth));
        Type(row.Option<TextBox>(OptionBinding.SizeWidth), "320.6");
        Assert.Equal(321, w.Tools.Settings.SizeWidth);   // Whole pixels.
        Type(row.Option<TextBox>(OptionBinding.SizeHeight), "200");
        Assert.Equal(200, w.Tools.Settings.SizeHeight);
        var words = OptionRows.Words(OptionRows.For("rectangleSelect", "none", w.Tools.Settings)).ToList();
        Assert.Contains("Size", words);
        Assert.Contains("×", words);
        Assert.DoesNotContain("Ratio", words);

        // Ellipse Select shares them and shows them on arrival.
        w.Tools.Current = "ellipseSelect";
        Dispatcher.UIThread.RunJobs();
        Assert.Equal("321", w.Options.Option<TextBox>(OptionBinding.SizeWidth).Text);
        Assert.Equal(4, w.Options.Groups.Count);
        w.Options.Option<ComboBox>(OptionBinding.Style).SelectedIndex = 0;
        Dispatcher.UIThread.RunJobs();
        Assert.Equal(3, w.Options.Groups.Count);
        // Lasso has no styles, and the style changing elsewhere does not touch its row.
        w.Tools.Current = "lassoSelect";
        Dispatcher.UIThread.RunJobs();
        w.Tools.Settings = w.Tools.Settings with { SelectStyle = SelectStyleChoice.FixedSize };
        Assert.Equal(2, w.Options.Groups.Count);
        w.Close();
    }

    [Fact]
    public void TheStyleAndSizeWordsAreTheMacs()
    {
        var select = Repo.Read("mac/OpenPainterKit/Sources/Shell/SelectCropTool.swift");
        Assert.Contains("ratioViews = [Labels.label(\"Ratio\"), ratioWidth, Labels.label(\":\"), ratioHeight]", select);
        Assert.Contains("sizeViews = [Labels.label(\"Size\"), sizeWidth, Labels.label(\"×\"), sizeHeight, Labels.label(\"px\")]", select);
        foreach (var name in new[] { OptionBinding.Style, OptionBinding.RatioWidth, OptionBinding.RatioHeight, OptionBinding.SizeWidth, OptionBinding.SizeHeight })
            Assert.Contains($"setAccessibilityLabel(\"{name}\")", select);
        var style = Repo.Read("mac/OpenPainterKit/Sources/Shell/SelectCropGeometry.swift");
        foreach (var label in OptionRows.SelectStyles) Assert.Contains($"\"{label}\"", style);
    }

    [AvaloniaFact]
    public void TheTextRowListsRealFontsAndBindsItsControls()
    {
        var w = Ready("text");
        var o = w.Options;
        var font = o.Option<ComboBox>(OptionBinding.Font);
        var families = font.Items.Cast<string>().ToList();
        Assert.True(families.Count > 10, $"only {families.Count} fonts");
        Assert.Contains("Segoe UI", families);
        Assert.Contains("Arial", families);
        Assert.Equal("Segoe UI", font.SelectedItem);
        Assert.Equal(families.OrderBy(f => f, StringComparer.OrdinalIgnoreCase), families);
        font.SelectedItem = "Arial";
        Assert.Equal("Arial", w.Tools.Settings.Text.Family);

        Fixture.Click(o.Option<IconCommand>(OptionBinding.Bold));
        Assert.True(w.Tools.Settings.Text.Bold);
        Assert.True(o.Option<IconCommand>(OptionBinding.Bold).On);
        Fixture.Click(o.Option<IconCommand>(OptionBinding.Bold));
        Assert.False(w.Tools.Settings.Text.Bold);
        Fixture.Click(o.Option<IconCommand>(OptionBinding.Strikethrough));
        Assert.True(w.Tools.Settings.Text.Strikethrough);
        Assert.True(o.Option<IconCommand>(OptionBinding.AlignLeft).On);
        Fixture.Click(o.Option<IconCommand>(OptionBinding.AlignCenter));
        Assert.Equal(TextAlignChoice.Center, w.Tools.Settings.Text.Alignment);
        Assert.True(o.Option<IconCommand>(OptionBinding.AlignCenter).On);
        Assert.False(o.Option<IconCommand>(OptionBinding.AlignLeft).On);

        var size = o.Option<ComboBox>(OptionBinding.FontSize);
        Assert.Equal("12", size.Text);
        size.SelectedItem = "48";
        Assert.Equal(48, w.Tools.Settings.Text.Size);
        size.Text = "33.5";
        size.RaiseEvent(new KeyEventArgs { RoutedEvent = InputElement.KeyDownEvent, Key = Key.Enter });
        Assert.Equal(33.5, w.Tools.Settings.Text.Size);
        size.Text = "9000";
        size.RaiseEvent(new KeyEventArgs { RoutedEvent = InputElement.KeyDownEvent, Key = Key.Enter });
        Assert.Equal(2000, w.Tools.Settings.Text.Size);
        Assert.Equal("2000", size.Text);
        size.Text = "abc";
        size.RaiseEvent(new KeyEventArgs { RoutedEvent = InputElement.KeyDownEvent, Key = Key.Enter });
        Assert.Equal(2000, w.Tools.Settings.Text.Size);
        Assert.Equal("2000", size.Text);
        Assert.False(o.Option<Button>("Finish text").IsEnabled);
        w.Close();
    }

    [AvaloniaFact]
    public void TheFontSizeBoxCentersItsTextInItsShortBox()
    {
        var w = Ready("text");
        w.UpdateLayout();
        Dispatcher.UIThread.RunJobs();
        var box = w.Options.Option<ComboBox>(OptionBinding.FontSize);
        var text = box.GetVisualDescendants().OfType<TextBox>().Single();
        var top = text.TranslatePoint(default, box)!.Value.Y;
        Assert.Equal((box.Bounds.Height - text.Bounds.Height) / 2, top, 0);
        Assert.Equal(Avalonia.Layout.VerticalAlignment.Center, text.VerticalContentAlignment);
        w.Close();
    }

    [AvaloniaFact]
    public void AFamilyThatIsNotInstalledStillShows()
    {
        var w = Ready();
        w.Tools.Settings = w.Tools.Settings with { Text = w.Tools.Settings.Text with { Family = "No Such Family" } };
        w.Tools.Current = "text";
        Dispatcher.UIThread.RunJobs();
        Assert.Equal("No Such Family", w.Options.Option<ComboBox>(OptionBinding.Font).SelectedItem);
        w.Close();
    }

    [AvaloniaFact]
    public void TheRowShowsChangesFromElsewhereAndKeepsThemAcrossAToolAndATheme()
    {
        var w = Ready();
        var hints = w.Status.Hint.Text;
        w.Tools.Settings = w.Tools.Settings with { BrushWidth = 44, Blend = BlendChoice.Overwrite, Antialias = false };
        Assert.Equal("44", w.Options.Option<TextBox>(OptionBinding.BrushWidth).Text);
        Assert.Equal(1, w.Options.Option<ComboBox>(OptionBinding.BlendMode).SelectedIndex);
        Assert.False(w.Options.Option<CheckBox>(OptionBinding.Antialiasing).IsChecked);
        Assert.Equal(hints, w.Status.Hint.Text);
        // A settings change is not a tool change, so a message stays.
        w.Tools.Current = "eraser";
        Dispatcher.UIThread.RunJobs();
        Assert.Equal("44", w.Options.Option<TextBox>(OptionBinding.BrushWidth).Text);
        Assert.False(w.Options.Option<CheckBox>(OptionBinding.Antialiasing).IsChecked);
        w.RequestedThemeVariant = Avalonia.Styling.ThemeVariant.Dark;
        Dispatcher.UIThread.RunJobs();
        Assert.Equal("44", w.Options.Option<TextBox>(OptionBinding.BrushWidth).Text);
        w.Options.Option<TextBox>(OptionBinding.BrushWidth).Text = "50";
        Gesture.Enter(w.Options.Option<TextBox>(OptionBinding.BrushWidth));
        Assert.Equal(50, w.Tools.Settings.BrushWidth);
        w.Close();
    }

    [AvaloniaFact]
    public void EveryToolsRowBuildsWithRealControlsAndNoStubsLeft()
    {
        var w = Ready();
        foreach (var tool in w.Tools.Tools)
        {
            w.Tools.Current = tool.Id;
            Dispatcher.UIThread.RunJobs();
            w.UpdateLayout();
            foreach (var item in OptionRows.For(tool.Id, tool.Options, w.Tools.Settings).SelectMany(g => g))
            {
                Control found = item switch
                {
                    OptionField f => w.Options.Option<TextBox>(f.Name),
                    OptionChoice c => w.Options.Option<ComboBox>(c.Name),
                    OptionCheck c => w.Options.Option<CheckBox>(c.Name),
                    OptionStep s => w.Options.Option<IconCommand>(s.Name),
                    OptionToggle t => w.Options.Option<IconCommand>(t.Name),
                    OptionSlide s => w.Options.Option<Slider>(s.Name),
                    OptionPush b => w.Options.Option<Button>(b.Name),
                    _ => w.Options,
                };
                Assert.NotNull(found);
            }
        }
        w.Close();
    }
}

public sealed class StatusValueTests
{
    [Fact]
    public void AUnitFormatsAsTheMacDoes()
    {
        Assert.Equal("12", Measurement.Format(MeasurementUnit.Pixels, 12.9, 96));
        Assert.Equal("-1", Measurement.Format(MeasurementUnit.Pixels, -.5, 96));
        Assert.Equal("1.00", Measurement.Format(MeasurementUnit.Inches, 96, 96));
        Assert.Equal("2.54", Measurement.Format(MeasurementUnit.Centimeters, 96, 96));
        Assert.Equal("0.50", Measurement.Format(MeasurementUnit.Inches, 150, 300));
        Assert.Equal(["px", "in", "cm"], Measurement.All.Select(Measurement.Abbreviation));
    }

    [Fact]
    public void AProfileNameIsCutAsTheMacCutsIt()
    {
        Assert.Equal("sRGB (none)", ColorProfileNames.Short(DocInfo.Empty));
        Assert.Equal("None, treated as sRGB", ColorProfileNames.Full(DocInfo.Empty));
        var named = DocInfo.Empty with { ProfileEmbedded = true, ProfileDescription = "Display P3" };
        Assert.Equal("Display P3", ColorProfileNames.Short(named));
        Assert.Equal("Display P3", ColorProfileNames.Full(named));
        var long_ = named with { ProfileDescription = "A Very Long Profile Name Indeed" };
        Assert.Equal("A Very Long Profi…", ColorProfileNames.Short(long_));
    }

    [AvaloniaFact]
    public void TheProfileAndImageComeFromTheEngine()
    {
        using var f = new Fixture();
        var s = f.Window.Status;
        Assert.Equal("sRGB (none)", s.ProfileValue.Text);
        Assert.Equal("None, treated as sRGB", ToolTip.GetTip(s.ProfileValue));
        Assert.Equal($"{Fixture.Width} × {Fixture.Height} px", s.ImageValue.Text);
        f.Run("image.assign_profile", EngineParams.Of(("profile", "display_p3")));
        Assert.True(f.Editor.Info.ProfileEmbedded);
        Assert.Equal(ColorProfileNames.Short(f.Editor.Info), s.ProfileValue.Text);
        Assert.NotEqual("sRGB (none)", s.ProfileValue.Text);
    }

    [AvaloniaFact]
    public void UnitsRewriteTheValuesAtTheDocumentsResolution()
    {
        var client = new EngineClient();
        var editor = new Editor(client, client.NewDocument(600, 300, 300, 0xFFFFFFFF));
        var w = Shells.Open();
        Dispatcher.UIThread.RunJobs();
        w.Bind(editor);
        editor.Settle();
        Assert.Equal(300, editor.Dpi);
        var s = w.Status;
        Assert.Equal("600 × 300 px", s.ImageValue.Text);
        s.ShowCursor(new Point(150.5, 75));
        s.ShowSelection(new Rect(0, 0, 300, 150));
        Assert.Equal("150, 75", s.CursorValue.Text);
        Assert.Equal("300 × 150 px", s.SelectionValue.Text);

        s.Units.SelectedIndex = 1;
        Assert.Equal(MeasurementUnit.Inches, w.Unit);
        Assert.Equal("2.00 × 1.00 in", s.ImageValue.Text);
        Assert.Equal("0.50, 0.25", s.CursorValue.Text);
        Assert.Equal("1.00 × 0.50 in", s.SelectionValue.Text);

        s.Units.SelectedIndex = 2;
        Assert.Equal("5.08 × 2.54 cm", s.ImageValue.Text);

        // A theme change keeps the unit and the values.
        w.RequestedThemeVariant = Avalonia.Styling.ThemeVariant.Dark;
        Dispatcher.UIThread.RunJobs();
        Assert.Equal(2, w.Status.Units.SelectedIndex);
        Assert.Equal("5.08 × 2.54 cm", w.Status.ImageValue.Text);
        w.Status.Units.SelectedIndex = 0;
        Assert.Equal("600 × 300 px", w.Status.ImageValue.Text);
        w.Close();
        editor.Dispose();
        client.Dispose();
    }

    [AvaloniaFact]
    public void TheViewMenusUnitsPickAndMarkTheUnit()
    {
        var w = Shells.Open();
        Dispatcher.UIThread.RunJobs();
        var view = w.TitleBar.Menus.Items.OfType<MenuItem>().Single(m => (string?)m.Header == "View");
        MenuItem Item(string title) => view.Items.OfType<MenuItem>().Single(i => (string?)i.Header == title);
        Assert.True(Item("Pixels").IsChecked);
        Assert.False(Item("Inches").IsChecked);
        Item("Inches").RaiseEvent(new RoutedEventArgs(MenuItem.ClickEvent));
        Assert.Equal(MeasurementUnit.Inches, w.Unit);
        Assert.Equal(1, w.Status.Units.SelectedIndex);
        Assert.True(Item("Inches").IsChecked);
        Assert.False(Item("Pixels").IsChecked);
        w.Status.Units.SelectedIndex = 2;
        Assert.True(Item("Centimeters").IsChecked);
        Assert.False(Item("Inches").IsChecked);
        w.Close();
    }
}

public sealed class LayerDetailLineTests
{
    /// The words the panel shows are whole, at the panel's default width, in the real font.
    [AvaloniaFact]
    public void TheDetailLineFitsTheDefaultPanelWidth()
    {
        using var f = new Fixture();
        f.Run("layer.properties", EngineParams.Of(("index", 0), ("name", "Background"), ("visible", false), ("blend", "multiply"), ("opacity", 128)));
        var row = f.Row(0);
        Assert.Equal("Hidden · Multiply · 50%", row.DetailLabel!.Text);
        f.Window.Panels.Panels[PanelLayout.Layers].UpdateLayout();
        Dispatcher.UIThread.RunJobs();
        var line = Assert.Single(row.DetailLabel.TextLayout.TextLines);
        Assert.False(line.HasCollapsed, $"the line is cut: {line.Width} in {row.DetailLabel.Bounds.Width}");
    }

    [AvaloniaFact]
    public void TheDetailLineStaysClearOfTheEye()
    {
        using var f = new Fixture();
        f.Run("layer.properties", EngineParams.Of(("index", 0), ("name", "Background"), ("visible", false), ("blend", "multiply"), ("opacity", 128)));
        var row = f.Row(0);
        var layers = f.Window.Panels.Panels[PanelLayout.Layers];
        layers.UpdateLayout();
        Assert.True(row.DetailLabel!.Bounds.Right <= row.Eye.Bounds.X, $"{row.DetailLabel.Bounds} runs under {row.Eye.Bounds}");
    }
}
