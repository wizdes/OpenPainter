// W8 done-test 4: each rule of mac/OpenPainterKit/Sources/Shell/FocusRouter.swift, with a test. The
// Mac's FocusTests.swift is the model. Also Alt and the access keys, row keys in History and Layers,
// and the menu commands, all against the real engine.
using Avalonia;
using Avalonia.Controls;
using Avalonia.Headless;
using Avalonia.Headless.XUnit;
using Avalonia.Input;
using Avalonia.Controls.Primitives;
using Avalonia.Threading;
using Avalonia.VisualTree;
using OpenPainter.Bridge;
using OpenPainter.Catalog;
using OpenPainter.Shell;
using Xunit;

namespace OpenPainter.Tests;

public sealed class FocusRuleTests
{
    static void Press(TopLevel w, Key key, RawInputModifiers mods = RawInputModifiers.None, string? text = null)
    {
        w.KeyPress(key, mods, PhysicalKey.None, text);
        Dispatcher.UIThread.RunJobs();
    }

    static TextBox Field(Fixture f) => f.Window.Options.Option<TextBox>(OptionBinding.BrushWidth);

    static MenuItem Menu(MainWindow w, string title) => w.TitleBar.Menus.Items.OfType<MenuItem>().Single(m => (string?)m.Header == title);

    // Rule 1: the canvas context sends commands to the document.

    [AvaloniaFact]
    public void TheCanvasContextSendsCommandsToTheDocument()
    {
        using var f = new Fixture();
        f.Window.CanvasInput.Focus();
        Assert.Equal(FocusTarget.Canvas, FocusRouter.Target(false, f.Window.FocusManager?.GetFocusedElement()));
        var steps = f.Editor.Info.HistoryLabels.Count;
        Press(f.Window, Key.N, RawInputModifiers.Control | RawInputModifiers.Shift);
        f.Settle();
        Assert.Equal(steps + 1, f.Editor.Info.HistoryLabels.Count);
        Press(f.Window, Key.Z, RawInputModifiers.Control);
        f.Settle();
        Assert.Equal(steps - 1, f.Editor.Info.HistoryIndex);
        Press(f.Window, Key.Z, RawInputModifiers.Control | RawInputModifiers.Shift);
        f.Settle();
        Assert.Equal(steps, f.Editor.Info.HistoryIndex);
        Press(f.Window, Key.Z, RawInputModifiers.Control);
        f.Settle();
        Press(f.Window, Key.Y, RawInputModifiers.Control);
        f.Settle();
        Assert.Equal(steps, f.Editor.Info.HistoryIndex);
        // Enter reaches the document: it clears the message.
        f.Editor.Report("A message");
        Press(f.Window, Key.Enter);
        Assert.Null(f.Editor.Message);
    }

    /// A control that is not text still sends commands to the document.
    [AvaloniaFact]
    public void ANonTextControlStillSendsCommandsToTheDocument()
    {
        using var f = new Fixture();
        var item = f.Window.Strip.Items[0];
        item.Focus(NavigationMethod.Tab);
        Assert.Equal(FocusTarget.Canvas, FocusRouter.Target(false, f.Window.FocusManager?.GetFocusedElement()));
        var layers = f.Editor.Info.Layers.Count;
        Press(f.Window, Key.N, RawInputModifiers.Control | RawInputModifiers.Shift);
        f.Settle();
        Assert.Equal(layers + 1, f.Editor.Info.Layers.Count);
    }

    // Rule 2: the text field keeps its commands.

    [AvaloniaFact]
    public void ATextFieldKeepsItsCommandsAndLettersAndTheDocumentGetsNothing()
    {
        using var f = new Fixture();
        f.Run("layer.add", EngineParams.Of(("name", "Top")));
        var box = Field(f);
        box.Focus();
        Assert.Equal(FocusTarget.TextField, FocusRouter.Target(false, f.Window.FocusManager?.GetFocusedElement()));
        box.Text = "123";
        box.CaretIndex = 3;
        var (steps, index, tool, primary, width) = (f.Editor.Info.HistoryLabels.Count, f.Editor.Info.HistoryIndex, f.Window.Tools.Current, f.Window.Tools.Primary, f.Window.Tools.Settings.BrushWidth);
        Press(f.Window, Key.A, RawInputModifiers.Control);
        Assert.Equal("123".Length, box.SelectedText?.Length);
        Press(f.Window, Key.Z, RawInputModifiers.Control);
        Press(f.Window, Key.Z, RawInputModifiers.Control | RawInputModifiers.Shift);
        Press(f.Window, Key.Y, RawInputModifiers.Control);
        Press(f.Window, Key.Escape);
        Press(f.Window, Key.Delete);
        f.Settle();
        Assert.Equal(steps, f.Editor.Info.HistoryLabels.Count);
        Assert.Equal(index, f.Editor.Info.HistoryIndex);
        // Letter keys are the field's: no tool changes, no swap, no reset.
        foreach (var key in new[] { Key.S, Key.X, Key.D, Key.C, Key.OemOpenBrackets })
        {
            Press(f.Window, key);
            Press(f.Window, key, RawInputModifiers.Shift);
        }
        Assert.Equal(tool, f.Window.Tools.Current);
        Assert.Equal(primary, f.Window.Tools.Primary);
        Assert.Equal(width, f.Window.Tools.Settings.BrushWidth);
    }

    /// Cut, Copy and Paste are off in the menus until W10, and in a field they must stay the field's.
    [Theory]
    [InlineData("Ctrl+A")]
    [InlineData("Delete")]
    [InlineData("Esc")]
    [InlineData("Enter")]
    [InlineData("Ctrl+Z")]
    [InlineData("Ctrl+Shift+Z")]
    [InlineData("Ctrl+Y")]
    [InlineData("Ctrl+X")]
    [InlineData("Ctrl+C")]
    [InlineData("Ctrl+V")]
    [InlineData("S")]
    [InlineData("Shift+S")]
    [InlineData("[")]
    public void TheFieldKeepsThisKey(string words) => Assert.False(FocusRouter.DocumentGets(FocusTarget.TextField, words));

    /// Other menu keys still work from a field, as a Mac menu's do.
    [Theory]
    [InlineData("F5")]
    [InlineData("Ctrl+Shift+N")]
    [InlineData("Ctrl+0")]
    [InlineData("Ctrl++")]
    public void OtherMenuKeysPassAField(string words) => Assert.True(FocusRouter.DocumentGets(FocusTarget.TextField, words));

    [AvaloniaFact]
    public void AMenuKeyStillWorksWhileAFieldHasFocus()
    {
        using var f = new Fixture();
        Field(f).Focus();
        Press(f.Window, Key.F8);
        Assert.False(f.Window.Panels.IsShown("colors"));
        Press(f.Window, Key.F8);
        Assert.True(f.Window.Panels.IsShown("colors"));
    }

    // Rule 3: a dialog up, and the document gets nothing.

    [AvaloniaFact]
    public void ADialogGivesTheDocumentNothingFromAnyWindow()
    {
        using var f = new Fixture();
        f.Run("layer.add", EngineParams.Of(("name", "Top")));
        var d = f.Window.ShowLayerProperties(0)!;
        Dispatcher.UIThread.RunJobs();
        Assert.Equal(FocusTarget.Dialog, FocusRouter.Target(f.Window.Dialog is not null, null));
        var (index, tool) = (f.Editor.Info.HistoryIndex, f.Window.Tools.Current);
        Press(f.Window, Key.Z, RawInputModifiers.Control);
        Press(f.Window, Key.N, RawInputModifiers.Control | RawInputModifiers.Shift);
        f.Window.CanvasInput.Focus();
        Press(f.Window, Key.S);
        var colors = f.Window.Panels.Panels[PanelLayout.Colors];
        Press(colors, Key.Z, RawInputModifiers.Control);
        Press(colors, Key.S);
        f.Settle();
        Assert.Equal(index, f.Editor.Info.HistoryIndex);
        Assert.Equal(tool, f.Window.Tools.Current);
        // And the menu commands say so.
        Assert.False(Item(f.Window, "Edit > Undo").IsEnabled);
        Assert.False(Item(f.Window, "Layers > Add New Layer").IsEnabled);
        Assert.False(f.Window.Options.Commands["undo"].IsEnabled);
        d.Cancel();
        f.Settle();
        Assert.True(Item(f.Window, "Edit > Undo").IsEnabled);
    }

    // Rule 4: a panel with no field focused forwards to the document.

    [AvaloniaFact]
    public void APanelWithNoFieldForwardsCommandsAndCanvasKeys()
    {
        using var f = new Fixture();
        var panel = f.Window.Panels.Panels[PanelLayout.History];
        panel.Show();
        panel.Focus();
        Assert.Equal(FocusTarget.Canvas, FocusRouter.Target(false, panel.FocusManager?.GetFocusedElement()));
        var steps = f.Editor.Info.HistoryLabels.Count;
        Press(panel, Key.N, RawInputModifiers.Control | RawInputModifiers.Shift);
        f.Settle();
        Assert.Equal(steps + 1, f.Editor.Info.HistoryLabels.Count);
        Press(panel, Key.Z, RawInputModifiers.Control);
        f.Settle();
        Assert.Equal(steps - 1, f.Editor.Info.HistoryIndex);
        Press(panel, Key.E);
        Assert.Equal("eraser", f.Window.Tools.Current);
    }

    [AvaloniaFact]
    public void TheColorsHexFieldTakesItsKeysWhileItEdits()
    {
        using var f = new Fixture();
        var colors = f.Window.Panels.Panels[PanelLayout.Colors];
        var hex = f.Window.Panels.Colors.HexField;
        hex.Focus();
        Assert.True(hex.IsFocused);
        Assert.Equal(FocusTarget.TextField, FocusRouter.Target(false, colors.FocusManager?.GetFocusedElement()));
        var tool = f.Window.Tools.Current;
        hex.Text = "";
        Press(colors, Key.E);
        colors.KeyTextInput("e");
        Dispatcher.UIThread.RunJobs();
        Assert.Equal(tool, f.Window.Tools.Current);
        Assert.Equal("e", hex.Text);
    }

    /// Where keys go after Enter in the hex field: back to the canvas, so the next letter picks a tool.
    [AvaloniaFact]
    public void AfterEnterInTheHexFieldKeysGoToTheCanvas()
    {
        using var f = new Fixture();
        var colors = f.Window.Panels.Panels[PanelLayout.Colors];
        var hex = f.Window.Panels.Colors.HexField;
        hex.Focus();
        hex.Text = "102030";
        Press(colors, Key.Enter);
        Assert.Equal(new DocColor(0x10, 0x20, 0x30), f.Window.Tools.Primary);
        Assert.False(hex.IsFocused);
        Assert.True(f.Window.CanvasInput.IsFocused);
        Press(f.Window, Key.E);
        Assert.Equal("eraser", f.Window.Tools.Current);
        Press(f.Window, Key.X);
        Assert.Equal(new DocColor(0x10, 0x20, 0x30), f.Window.Tools.Secondary);
    }

    [AvaloniaFact]
    public void AfterEnterInAChannelFieldKeysGoToTheCanvas()
    {
        using var f = new Fixture();
        var colors = f.Window.Panels.Panels[PanelLayout.Colors];
        f.Window.Panels.Colors.SetExpanded(true);
        var red = f.Window.Panels.Colors.Channels[ColorChannel.Red].Field;
        red.Focus();
        red.Text = "17";
        Press(colors, Key.Enter);
        Assert.Equal(17, f.Window.Tools.Primary.R);
        Assert.True(f.Window.CanvasInput.IsFocused);
    }

    // The canvas's own keys stay on the canvas path in the main window: a committed rule.

    [AvaloniaFact]
    public void ControlsInTheMainWindowKeepTheirLetters()
    {
        using var f = new Fixture();
        f.Window.Strip.Items[0].Focus(NavigationMethod.Tab);
        var tool = f.Window.Tools.Current;
        Press(f.Window, Key.E);
        Assert.Equal(tool, f.Window.Tools.Current);
    }

    // The W5 Tab handoff.

    [AvaloniaFact]
    public void TabLeavesTheCanvasAndCtrlTabStays()
    {
        using var f = new Fixture();
        f.Window.CanvasInput.Focus(NavigationMethod.Tab);
        Assert.True(CanvasKeys.Run(f.Window.Tools, f.Editor, "Ctrl+Tab", repeat: false));
        Assert.True(CanvasKeys.Run(f.Window.Tools, f.Editor, "Ctrl+Shift+Tab", repeat: false));
        Assert.True(f.Window.CanvasInput.IsFocused);
        f.Window.LeaveCanvas(back: false);
        Dispatcher.UIThread.RunJobs();
        Assert.False(f.Window.CanvasInput.IsFocused);
        Assert.True(f.Window.Status.IsVisualAncestorOf((Visual)f.Window.FocusManager!.GetFocusedElement()!));
    }

    // Rows.

    [AvaloniaFact]
    public void LayerRowsTakeTheKeyboard()
    {
        using var f = new Fixture();
        f.Run("layer.add", EngineParams.Of(("name", "Two")));
        f.Run("layer.add", EngineParams.Of(("name", "Three")));
        var panel = f.Window.Panels.Panels[PanelLayout.Layers];
        var rows = f.Layers.LayerRows;
        Assert.Equal(["Three", "Two", "Background"], rows.Select(r => r.Layer.Name));
        rows[0].Focus(NavigationMethod.Tab);
        Press(panel, Key.Down);
        Assert.True(f.Layers.LayerRows[1].IsFocused);
        Press(panel, Key.Down);
        Assert.True(f.Layers.LayerRows[2].IsFocused);
        // At the end the arrow stays.
        Press(panel, Key.Down);
        Assert.True(f.Layers.LayerRows[2].IsFocused);
        Press(panel, Key.Up);
        Assert.True(f.Layers.LayerRows[1].IsFocused);
        // Enter picks it. The list rebuilds, and the keyboard stays on the same row.
        Press(panel, Key.Enter);
        f.Settle();
        Assert.Equal(1, f.Editor.Info.ActiveLayer);
        Assert.True(f.Layers.LayerRows[1].IsFocused);
        Assert.True(f.Layers.LayerRows[1].IsCurrent);
        Press(panel, Key.Up);
        Press(panel, Key.Space);
        f.Settle();
        Assert.Equal(2, f.Editor.Info.ActiveLayer);
    }

    [AvaloniaFact]
    public void HistoryRowsTakeTheKeyboard()
    {
        using var f = new Fixture();
        f.Run("layer.add", EngineParams.Of(("name", "Two")));
        f.Run("layer.add", EngineParams.Of(("name", "Three")));
        var panel = f.Window.Panels.Panels[PanelLayout.History];
        var count = f.History.HistoryRows.Count;
        f.History.HistoryRows[^1].Focus(NavigationMethod.Tab);
        Press(panel, Key.Up);
        Assert.True(f.History.HistoryRows[count - 2].IsFocused);
        Press(panel, Key.Enter);
        f.Settle();
        Assert.Equal(count - 2, f.Editor.Info.HistoryIndex);
        Assert.True(f.History.HistoryRows[count - 2].IsFocused);
    }

    /// Reproduced first: a release away from a history row still jumped to that step.
    [AvaloniaFact]
    public void AReleaseOffARowIsNoClick()
    {
        using var f = new Fixture();
        f.Run("layer.add", EngineParams.Of(("name", "Two")));
        var panel = f.Window.Panels.Panels[PanelLayout.History];
        var row = f.History.HistoryRows[0];
        var at = row.TranslatePoint(new Point(20, 10), panel)!.Value;
        panel.MouseMove(at, RawInputModifiers.None);
        panel.MouseDown(at, MouseButton.Left, RawInputModifiers.LeftMouseButton);
        var away = new Point(panel.Bounds.Width - 2, 1);
        panel.MouseMove(away, RawInputModifiers.LeftMouseButton);
        panel.MouseUp(away, MouseButton.Left, RawInputModifiers.None);
        f.Settle();
        Assert.Equal(1, f.Editor.Info.HistoryIndex);
    }

    [AvaloniaFact]
    public void TabReachesEveryRowOfAPanel()
    {
        using var f = new Fixture();
        f.Run("layer.add", EngineParams.Of(("name", "Two")));
        var panel = f.Window.Panels.Panels[PanelLayout.Layers];
        f.Layers.LayerRows[0].Focus(NavigationMethod.Tab);
        var seen = new List<object?>();
        for (var i = 0; i < 12; i++)
        {
            seen.Add(panel.FocusManager?.GetFocusedElement());
            Press(panel, Key.Tab);
        }
        Assert.Contains(f.Layers.LayerRows[1], seen);
        Assert.Contains(f.Layers.LayerRows[0].Eye, seen);
        Assert.Contains(f.Layers.Buttons["layer.add"], seen);
    }

    // Next Panel and Previous Panel.

    [AvaloniaFact]
    public void NextPanelGoesThroughTheShownPanelsInTheMacsOrder()
    {
        using var f = new Fixture();
        var w = f.Window;
        var order = new List<string>();
        for (var i = 0; i < 5; i++)
        {
            var at = w.CyclePanels(forward: true);
            order.Add(at is PanelWindow p ? p.Panel.Title : at == w ? "image" : "?");
        }
        Assert.Equal(["Tools", "History", "Layers", "Colors", "image"], order);
        Assert.True(w.CanvasInput.IsFocused);
        Assert.Same(w.Panels.Panels[PanelLayout.Colors], w.CyclePanels(forward: false));
        // A hidden panel leaves the ring.
        w.Panels.SetShown(PanelLayout.History, false);
        w.CyclePanels(forward: true);
        Assert.Same(w.Panels.Panels[PanelLayout.Tools], w.CyclePanels(forward: true));
        Assert.Same(w.Panels.Panels[PanelLayout.Layers], w.CyclePanels(forward: true));
    }

    [AvaloniaFact]
    public void AFocusedPanelPutsTheKeyboardOnItsFirstControl()
    {
        using var f = new Fixture();
        var panel = f.Window.Panels.Panels[PanelLayout.Layers];
        Assert.Same(f.Window.Panels.Panels[PanelLayout.Tools], f.Window.CyclePanels(true));
        f.Window.CyclePanels(true);
        Assert.Same(panel, f.Window.CyclePanels(true));
        var focused = panel.FocusManager?.GetFocusedElement();
        Assert.True(focused is LayerRow or IconCommand, $"{focused}");
    }

    // Alt.

    [AvaloniaFact]
    public void TheAltTapMachineEndsOnAnythingBetween()
    {
        var alt = new AltGesture();
        alt.Press();
        Assert.True(alt.Release());
        alt.Press();
        alt.Spoil();
        Assert.True(alt.Spoiled);
        Assert.False(alt.Release());
        // A held key repeats and starts no new tap.
        alt.Press();
        alt.Spoil();
        alt.Press();
        Assert.False(alt.Release());
        // The next press is clean again.
        alt.Press();
        Assert.False(alt.Spoiled);
        Assert.True(alt.Release());
        // Ctrl with Alt is AltGr on many keyboards.
        alt.Press(otherModifier: true);
        Assert.False(alt.Release());
        // The window lost the keyboard while Alt was down.
        alt.Press();
        alt.Forget();
        Assert.False(alt.Release());
    }

    [AvaloniaFact]
    public void AnAltTapOpensTheMenuBarAndAnotherClosesIt()
    {
        using var f = new Fixture();
        var w = f.Window;
        w.Focus();
        w.KeyPressQwerty(PhysicalKey.AltLeft, RawInputModifiers.Alt);
        w.KeyReleaseQwerty(PhysicalKey.AltLeft, RawInputModifiers.None);
        Dispatcher.UIThread.RunJobs();
        Assert.True(w.TitleBar.Menus.IsOpen);
        w.KeyPressQwerty(PhysicalKey.AltLeft, RawInputModifiers.Alt);
        w.KeyReleaseQwerty(PhysicalKey.AltLeft, RawInputModifiers.None);
        Dispatcher.UIThread.RunJobs();
        Assert.False(w.TitleBar.Menus.IsOpen);
    }

    /// The W6 question: Alt-click must not open the menu bar when Alt is released.
    [AvaloniaFact]
    public void AltClickInTheMainWindowLeavesTheMenusShut()
    {
        using var f = new Fixture();
        var w = f.Window;
        w.Focus();
        w.KeyPressQwerty(PhysicalKey.AltLeft, RawInputModifiers.Alt);
        var at = w.Options.ToolPicker.TranslatePoint(new Point(4, 4), w)!.Value;
        w.MouseDown(at, MouseButton.Left, RawInputModifiers.Alt);
        w.MouseUp(at, MouseButton.Left, RawInputModifiers.Alt);
        w.KeyReleaseQwerty(PhysicalKey.AltLeft, RawInputModifiers.None);
        Dispatcher.UIThread.RunJobs();
        Assert.False(w.TitleBar.Menus.IsOpen);
    }

    [AvaloniaFact]
    public void AltClickOnTheColorsWheelKeepsTheSpokeAndLeavesTheMenusShut()
    {
        using var f = new Fixture();
        var w = f.Window;
        var panel = w.Panels.Panels[PanelLayout.Colors];
        Dispatcher.UIThread.RunJobs();
        var wheel = w.Panels.Colors.Wheel;
        var hue = w.Panels.Colors.Hsv.H;
        panel.KeyPressQwerty(PhysicalKey.AltLeft, RawInputModifiers.Alt);
        var at = wheel.TranslatePoint(new Point(ColorWheel.Diameter - 8, ColorWheel.Diameter / 2), panel)!.Value;
        panel.MouseDown(at, MouseButton.Left, RawInputModifiers.Alt);
        panel.MouseUp(at, MouseButton.Left, RawInputModifiers.Alt);
        panel.KeyReleaseQwerty(PhysicalKey.AltLeft, RawInputModifiers.None);
        Dispatcher.UIThread.RunJobs();
        Assert.Equal(hue, w.Panels.Colors.Hsv.H, 6);
        Assert.False(w.TitleBar.Menus.IsOpen);
    }

    [AvaloniaFact]
    public void ACleanAltTapInAPanelOpensTheMainWindowsMenuBar()
    {
        using var f = new Fixture();
        var panel = f.Window.Panels.Panels[PanelLayout.Colors];
        panel.KeyPressQwerty(PhysicalKey.AltLeft, RawInputModifiers.Alt);
        panel.KeyReleaseQwerty(PhysicalKey.AltLeft, RawInputModifiers.None);
        Dispatcher.UIThread.RunJobs();
        Assert.True(f.Window.TitleBar.Menus.IsOpen);
    }

    [AvaloniaFact]
    public void AltWithAMenusLetterOpensThatMenuFromEachPlace()
    {
        using var f = new Fixture();
        var w = f.Window;
        foreach (var (title, letter) in MenuLetters.Letters)
        {
            Assert.True(w.OpenMenuByLetter(letter, fromCanvas: false));
            Dispatcher.UIThread.RunJobs();
            Assert.True(Menu(w, title).IsSubMenuOpen, title);
            w.TitleBar.Menus.Close();
            Dispatcher.UIThread.RunJobs();
        }
        Assert.False(w.OpenMenuByLetter('Q', fromCanvas: false));
        var panel = w.Panels.Panels[PanelLayout.Colors];
        Press(panel, Key.L, RawInputModifiers.Alt);
        Assert.True(Menu(w, "Layers").IsSubMenuOpen);
        w.TitleBar.Menus.Close();
        Dispatcher.UIThread.RunJobs();
        w.Focus();
        Press(w, Key.V, RawInputModifiers.Alt);
        Assert.True(Menu(w, "View").IsSubMenuOpen);
        w.TitleBar.Menus.Close();
    }

    [Fact]
    public void EachMenuHasOneLetterAndTheLetterIsInItsWords()
    {
        var titles = WindowsMenus.Build(MacCatalog.Embedded()).Select(m => m.Title).ToList();
        Assert.Equal(titles.Order(StringComparer.Ordinal), MenuLetters.Letters.Keys.Order(StringComparer.Ordinal));
        Assert.Equal(titles.Count, MenuLetters.Letters.Values.Distinct().Count());
        foreach (var title in titles) Assert.Contains('_', MenuLetters.Marked(title));
        Assert.Equal("Effe_cts", MenuLetters.Marked("Effects"));
    }

    [AvaloniaFact]
    public void ALetterInAnOpenMenuMovesToAnItemWithThatFirstLetter()
    {
        using var f = new Fixture();
        var w = f.Window;
        w.OpenMenuByLetter('L', fromCanvas: false);
        Dispatcher.UIThread.RunJobs();
        Press(w, Key.G);
        var layers = Menu(w, "Layers");
        var goTo = layers.Items.OfType<MenuItem>().Where(i => i.IsEnabled && ((string)i.Header!).StartsWith('G')).ToList();
        // With one layer no Go to item is on, so the letter does not move.
        Assert.Empty(goTo);
        w.TitleBar.Menus.Close();
        Dispatcher.UIThread.RunJobs();
        f.Run("layer.add", EngineParams.Of(("name", "Two")));
        w.OpenMenuByLetter('L', fromCanvas: false);
        Dispatcher.UIThread.RunJobs();
        Press(w, Key.G);
        var focused = w.FocusManager?.GetFocusedElement() as CatalogMenuItem;
        Assert.NotNull(focused);
        Assert.StartsWith("Go to", (string)focused.Header!);
        w.TitleBar.Menus.Close();
    }

    [AvaloniaFact]
    public void TheMenuLetterIsUnderlinedWhileAltIsDownAndTheHeaderStaysPlain()
    {
        var w = Shells.Open();
        var file = Menu(w, "File");
        Assert.Equal("File", file.Header);
        Dispatcher.UIThread.RunJobs();
        var text = Avalonia.VisualTree.VisualExtensions.GetVisualDescendants(file).OfType<AccessText>().Single();
        Assert.Equal("_File", text.Text);
        Assert.Equal('F', text.AccessKey is null ? '\0' : char.ToUpperInvariant(text.AccessKey[0]));
        w.Close();
    }

    // The Edit and Layers menus.

    static CatalogMenuItem Item(MainWindow w, string path)
    {
        static IEnumerable<CatalogMenuItem> All(IEnumerable<object?> items)
        {
            foreach (var item in items.OfType<MenuItem>())
            {
                if (item is CatalogMenuItem c) yield return c;
                foreach (var inner in All(item.Items)) yield return inner;
            }
        }
        return All(w.TitleBar.Menus.Items).Single(i => i.Entry.Path == path);
    }

    static void Click(MainWindow w, string path)
    {
        var item = Item(w, path);
        Assert.True(item.IsEnabled, $"{path} is off");
        item.RaiseEvent(new Avalonia.Interactivity.RoutedEventArgs(MenuItem.ClickEvent));
    }

    [AvaloniaFact]
    public void WithNoDocumentTheDocumentCommandsAreOff()
    {
        var w = Shells.Open();
        foreach (var path in new[] { "Edit > Undo", "Edit > Redo", "Layers > Add New Layer", "Layers > Delete Layer", "Layers > Layer Properties…", "Layers > Go to Top Layer" })
            Assert.False(Item(w, path).IsEnabled, path);
        Assert.False(w.Options.Commands["undo"].IsEnabled);
        // The key does nothing, and nothing throws.
        w.Focus();
        Press(w, Key.Z, RawInputModifiers.Control);
        w.Close();
    }

    [AvaloniaFact]
    public void TheEditMenuAndTheOptionsRowUndoAndRedo()
    {
        using var f = new Fixture();
        var w = f.Window;
        Assert.False(Item(w, "Edit > Undo").IsEnabled);
        Assert.False(Item(w, "Edit > Redo").IsEnabled);
        Assert.False(w.Options.Commands["undo"].IsEnabled);
        f.Run("layer.add", EngineParams.Of(("name", "Two")));
        Assert.True(w.Options.Commands["undo"].IsEnabled);
        Click(w, "Edit > Undo");
        f.Settle();
        Assert.Single(f.Editor.Info.Layers);
        Assert.True(Item(w, "Edit > Redo").IsEnabled);
        Assert.False(Item(w, "Edit > Undo").IsEnabled);
        w.Options.Commands["redo"].RaiseEvent(new Avalonia.Interactivity.RoutedEventArgs(Button.ClickEvent));
        f.Settle();
        Assert.Equal(2, f.Editor.Info.Layers.Count);
        w.Options.Commands["undo"].RaiseEvent(new Avalonia.Interactivity.RoutedEventArgs(Button.ClickEvent));
        f.Settle();
        Assert.Single(f.Editor.Info.Layers);
    }

    [AvaloniaFact]
    public void TheLayersMenuDoesWhatThePanelDoes()
    {
        using var f = new Fixture();
        var w = f.Window;
        // With one layer: no delete, no merge, no moves, no go-to.
        foreach (var path in new[] { "Layers > Delete Layer", "Layers > Merge Layer Down", "Layers > Move Layer Up", "Layers > Move Layer Down", "Layers > Go to Top Layer", "Layers > Go to Layer Above", "Layers > Go to Layer Below", "Layers > Go to Bottom Layer", "Layers > Move Layer to Top", "Layers > Move Layer to Bottom" })
            Assert.False(Item(w, path).IsEnabled, path);
        Click(w, "Layers > Add New Layer");
        f.Settle();
        Assert.Equal(["Background", "Layer 2"], f.Editor.Info.Layers.Select(l => l.Name));
        Click(w, "Layers > Duplicate Layer");
        f.Settle();
        Assert.Equal(3, f.Editor.Info.Layers.Count);
        Assert.Equal(2, f.Editor.Info.ActiveLayer);
        Assert.False(Item(w, "Layers > Go to Layer Above").IsEnabled);
        Click(w, "Layers > Go to Bottom Layer");
        f.Settle();
        Assert.Equal(0, f.Editor.Info.ActiveLayer);
        Click(w, "Layers > Go to Layer Above");
        f.Settle();
        Assert.Equal(1, f.Editor.Info.ActiveLayer);
        Click(w, "Layers > Go to Layer Below");
        f.Settle();
        Assert.Equal(0, f.Editor.Info.ActiveLayer);
        Click(w, "Layers > Go to Top Layer");
        f.Settle();
        Assert.Equal(2, f.Editor.Info.ActiveLayer);
        Click(w, "Layers > Toggle Layer Visibility");
        f.Settle();
        Assert.False(f.Editor.Info.Layers[2].Visible);
        Click(w, "Layers > Toggle Layer Visibility");
        f.Settle();
        var top = f.Editor.Info.Layers[2].Id;
        Click(w, "Layers > Move Layer to Bottom");
        f.Settle();
        Assert.Equal(top, f.Editor.Info.Layers[0].Id);
        Click(w, "Layers > Move Layer to Top");
        f.Settle();
        Assert.Equal(top, f.Editor.Info.Layers[2].Id);
        Click(w, "Layers > Move Layer Down");
        f.Settle();
        Assert.Equal(top, f.Editor.Info.Layers[1].Id);
        Click(w, "Layers > Move Layer Up");
        f.Settle();
        Assert.Equal(top, f.Editor.Info.Layers[2].Id);
        Click(w, "Layers > Flip Horizontal");
        f.Settle();
        Click(w, "Layers > Flip Vertical");
        f.Settle();
        Click(w, "Layers > Rotate 180°");
        f.Settle();
        Assert.Null(f.Editor.Message);
        Click(w, "Layers > Merge Layer Down");
        f.Settle();
        Assert.Equal(2, f.Editor.Info.Layers.Count);
        Click(w, "Layers > Delete Layer");
        f.Settle();
        Assert.Single(f.Editor.Info.Layers);
        Assert.False(Item(w, "Layers > Delete Layer").IsEnabled);
    }

    [AvaloniaFact]
    public void LayerPropertiesOpensFromItsMenuItemAndF4()
    {
        using var f = new Fixture();
        var w = f.Window;
        Click(w, "Layers > Layer Properties…");
        Dispatcher.UIThread.RunJobs();
        Assert.NotNull(w.Dialog);
        w.Dialog!.Cancel();
        f.Settle();
        w.CanvasInput.Focus();
        Press(w, Key.F4);
        Assert.NotNull(w.Dialog);
        w.Dialog!.Cancel();
        f.Settle();
    }

    [AvaloniaFact]
    public void AKeyForACommandThatIsOffDoesNothing()
    {
        using var f = new Fixture();
        f.Window.CanvasInput.Focus();
        var (layers, steps) = (f.Editor.Info.Layers.Count, f.Editor.Info.HistoryLabels.Count);
        Press(f.Window, Key.Delete, RawInputModifiers.Control | RawInputModifiers.Shift);
        f.Settle();
        Assert.Equal(layers, f.Editor.Info.Layers.Count);
        Assert.Equal(steps, f.Editor.Info.HistoryLabels.Count);
        Assert.Null(f.Editor.Message);
    }

    // The modified mark and the strip's picture.

    [AvaloniaFact]
    public void TheTitleAndTheStripShowTheModifiedMark()
    {
        using var f = new Fixture();
        var w = f.Window;
        Assert.False(f.Editor.Info.Dirty);
        Assert.False(w.TitleBar.IsModifiedShown);
        Assert.False(w.Strip.Items[0].Image.Modified);
        f.Run("layer.add", EngineParams.Of(("name", "Two")));
        Assert.True(f.Editor.Info.Dirty);
        Assert.True(w.TitleBar.IsModifiedShown);
        Assert.True(w.Strip.Items[0].Image.Modified);
        Assert.EndsWith(" *", w.Strip.Items[0].Image.ShownName);
        f.Run("edit.undo");
        Assert.Equal(f.Editor.Info.Dirty, w.TitleBar.IsModifiedShown);
        Assert.Equal(f.Editor.Info.Dirty, w.Strip.Items[0].Image.Modified);
        // A theme change keeps the mark.
        f.Run("edit.redo");
        w.RequestedThemeVariant = Avalonia.Styling.ThemeVariant.Dark;
        Dispatcher.UIThread.RunJobs();
        Assert.True(w.TitleBar.IsModifiedShown);
        Assert.True(w.Strip.Items[0].Image.Modified);
    }

    static string Middle(Avalonia.Media.Imaging.Bitmap bitmap)
    {
        var (width, height) = (bitmap.PixelSize.Width, bitmap.PixelSize.Height);
        var bytes = new byte[width * height * 4];
        unsafe
        {
            fixed (byte* first = bytes) bitmap.CopyPixels(new PixelRect(bitmap.PixelSize), (nint)first, bytes.Length, width * 4);
        }
        var i = (height / 2 * width + width / 2) * 4;
        return $"{bytes[i]},{bytes[i + 1]},{bytes[i + 2]}";
    }

    [AvaloniaFact]
    public void TheStripsPictureFollowsEdits()
    {
        using var f = new Fixture();
        f.Editor.WaitForPreview();
        Dispatcher.UIThread.RunJobs();
        var picture = f.Window.Strip.Items[0].Image.Preview;
        Assert.NotNull(picture);
        Assert.Equal("200,40,10", Middle(picture));
        f.Run("image.invert_colors");
        f.Editor.WaitForPreview();
        Dispatcher.UIThread.RunJobs();
        var after = f.Window.Strip.Items[0].Image.Preview;
        Assert.NotSame(picture, after);
        Assert.Equal("55,215,245", Middle(after!));
        f.Run("edit.undo");
        f.Editor.WaitForPreview();
        Dispatcher.UIThread.RunJobs();
        Assert.Equal("200,40,10", Middle(f.Window.Strip.Items[0].Image.Preview!));
    }

    [AvaloniaFact]
    public void AnUneditedDocumentMakesOnePicture()
    {
        using var f = new Fixture();
        f.Editor.WaitForPreview();
        Dispatcher.UIThread.RunJobs();
        var picture = f.Window.Strip.Items[0].Image.Preview;
        f.Editor.Refresh();
        f.Editor.WaitForPreview();
        Dispatcher.UIThread.RunJobs();
        Assert.Same(picture, f.Window.Strip.Items[0].Image.Preview);
    }
}
