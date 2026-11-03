// The main window: the title row, the options row, the strip, the stage with the canvas,
// and the status bar, with the four panels as owned windows over the stage.
//
// The native canvas sits in the stage and Avalonia cannot draw over it, so nothing here
// overlaps it. Menus, combo lists and panels open as their own top-level windows.
using System.Runtime.InteropServices;
using Avalonia;
using Avalonia.Automation;
using Avalonia.Controls;
using Avalonia.Controls.Chrome;
using Avalonia.Controls.Templates;
using Avalonia.Input;
using Avalonia.Media;
using Avalonia.Media.Imaging;
using Avalonia.Platform;
using Avalonia.Styling;
using Avalonia.VisualTree;
using OpenPainter.Canvas;
using OpenPainter.Catalog;
using UiLook;

namespace OpenPainter.Shell;

/// Gives Avalonia's drawn window decorations nothing to draw.
sealed class NoDrawnDecorations : IWindowDrawnDecorationsTemplate
{
    public TemplateResult<WindowDrawnDecorationsContent> Build() => new(new WindowDrawnDecorationsContent(), new NameScope());
    object? ITemplate.Build() => Build();
}

public sealed class MainWindow : Window
{
    public const string AppName = "OpenPainter";
    const uint WmDisplayChange = 0x007E;
    const int PreviewWidth = 112, PreviewHeight = 80;   // Editor.updateThumbnail: 56 by 40 at 2x.

    /// EditorWindowController.minimumSize.
    public static readonly Size MinimumClient = new(1100, 792);

    /// The minimum in force: the Mac's, or less on a monitor that cannot show that.
    public Size LeastClient { get; private set; } = MinimumClient;

    readonly StartOptions _options;
    readonly MacCatalog _catalog;
    readonly IReadOnlyList<MenuEntry> _menus;
    readonly Dictionary<string, Action> _actions;
    readonly Dictionary<string, Action> _shortcuts;
    readonly Grid _root = new() { Name = "Root", RowDefinitions = new RowDefinitions("Auto,*,Auto") };
    readonly Grid _canvasLayer = new() { Name = "CanvasLayer", RowDefinitions = new RowDefinitions("Auto,Auto,*") };
    string _title;
    string _documentName;
    StripImage _image;
    CanvasSession? _session;
    string _toolHint = "";

    public TitleRow TitleBar { get; private set; } = null!;
    public OptionsRow Options { get; private set; } = null!;
    public ImageStrip Strip { get; private set; } = null!;
    public StatusBar Status { get; private set; } = null!;
    public CanvasHost? Native { get; }
    public Palette Palette { get; private set; } = Tokens.Light;
    public CanvasSession? Session => _session;
    public ViewController View { get; } = new();
    public ToolState Tools { get; }
    public PanelManager Panels { get; }

    /// The open document as the panels see it. Null means no document.
    public Editor? Editor { get; private set; }

    /// The open Layer Properties dialog, if any: Workspace's sheet on the Mac.
    public LayerPropertiesDialog? Dialog { get; private set; }

    /// The part of the window the canvas shows in. The panels float over it.
    public Grid Stage { get; } = new() { Name = "Stage" };

    /// The focus stop for the canvas. In a real window it passes focus on to the native child.
    public Border CanvasInput { get; } = new() { Name = "CanvasInput", Focusable = true, Background = Brushes.Transparent };

    /// Runs on the UI thread once the first image is on screen in the view.
    public event Action? ImageReady;

    readonly TaskCompletionSource _canvasShown = new();

    /// Ends when the canvas shows the engine's first frame. Until then the stage shows there.
    public Task CanvasShown => _canvasShown.Task;

    public MainWindow(StartOptions options, bool headless = false)
    {
        _options = options;
        _catalog = MacCatalog.Embedded();
        _menus = WindowsMenus.Build(_catalog);
        Tools = new ToolState(_catalog.Tools);
        if (options.TestTool is { } tool) Tools.Current = tool;
        _documentName = options.TestPng is null ? "Untitled" : Path.GetFileName(options.TestPng);
        _image = new StripImage(_documentName, Modified: false, Active: true, Preview: null, 0, 0);
        _title = TitleRow.TitleFor(_documentName);
        Panels = new PanelManager(this, _catalog.Panels, Palette, StageOnScreen);
        Panels.Context.ShowLayerProperties = index => ShowLayerProperties(index);
        _actions = BuildActions();
        _shortcuts = BuildShortcuts();

        Title = _title;
        Width = Tokens.WindowWidth;
        Height = Tokens.WindowHeight;
        MinWidth = MinimumClient.Width;
        MinHeight = MinimumClient.Height;
        FontFamily = new FontFamily(Tokens.FontFamily);
        if (options.Theme is { } theme) RequestedThemeVariant = theme == "dark" ? ThemeVariant.Dark : ThemeVariant.Light;

        // The window draws its own title row. An empty decorations template turns off
        // Avalonia's drawn title bar and keeps the system's frame, shadow and resize edges.
        ExtendClientAreaToDecorationsHint = true;
        ExtendClientAreaTitleBarHeightHint = Tokens.TitleHeight;
        WindowDecorationsTheme = new ControlTheme(typeof(WindowDrawnDecorations))
        {
            Setters = { new Setter(WindowDrawnDecorations.TemplateProperty, new NoDrawnDecorations()) },
        };

        // Tree order is Tab order, so the canvas stop comes after the rows above it. It
        // spans them and sits under them. In a real window the stage's native child takes the
        // focus from it, and with no window it stands for the canvas.
        AutomationProperties.SetName(CanvasInput, "Canvas");
        Grid.SetRow(Stage, 2);
        Grid.SetRowSpan(CanvasInput, 3);
        CanvasInput.ZIndex = -1;
        _canvasLayer.Children.Add(Stage);
        _canvasLayer.Children.Add(CanvasInput);
        if (!headless)
        {
            Native = new CanvasHost { View = View, TestNoClip = options.TestPaintFault == StartOptions.BlackCanvasFault };
            Native.TabOut += LeaveCanvas;
            Native.Pressed += DismissPopups;
            Native.KeyPressed += OnCanvasKey;
            Stage.Children.Add(Native);
            CanvasInput.GotFocus += (_, e) =>
            {
                if (e.NavigationMethod == NavigationMethod.Tab) Native.TakeFocus();
            };
        }
        // Closed runs in a window message, where Avalonia would keep an exception to itself.
        Closed += (_, _) => Fatal.Guard("WINDOW_CLOSED", () =>
        {
            Fatal.TestThrow(Fatal.ClosedPlace);
            StopCanvas();
            Panels.EndAll();
        });
        Stage.SizeChanged += (_, _) =>
        {
            View.Resize(Stage.Bounds.Size);
            UpdateClearRect();
        };
        Opened += (_, _) =>
        {
            if (Native is not null)
            {
                Win32Properties.AddWndProcHookCallback(this, LimitSize);
                FitToScreen(shrink: true);
                StartCanvas();
                if (Screens is { } screens) screens.Changed += OnScreensChanged;
            }
            Panels.ShowAll();
        };
        PositionChanged += (_, _) => Fatal.Guard("WINDOW_MOVED", () =>
        {
            Fatal.TestThrow(Fatal.MovedPlace);
            UpdateClearRect();
            FitToScreen(shrink: false);
        });
        ScalingChanged += (_, _) => Fatal.Guard("WINDOW_SCALE", () =>
        {
            FitToScreen(shrink: false);
            PushView();
        });
        Panels.Changed += OnPanelsChanged;
        Tools.Changed += OnToolChanged;
        View.Tool = Tools.CanvasMode;
        View.Changed += OnViewChanged;
        View.ZoomRectangle += OnZoomRectangle;
        View.PointerAt += p => Status.ShowCursor(p);
        KeyDown += OnWindowKey;
        Content = _root;

        BuildRows();
        ActualThemeVariantChanged += (_, _) => BuildRows();
        PropertyChanged += (_, e) =>
        {
            if (e.Property == IsActiveProperty) UpdateActive();
            if (e.Property == WindowStateProperty) ApplyWindowState();
        };
        foreach (var panel in Panels.Panels.Values)
            panel.PropertyChanged += (_, e) => { if (e.Property == IsActiveProperty) UpdateActive(); };
    }

    /// Avalonia's MinHeight counts a title bar this window has turned off, so the window
    /// stopped 31 pixels short of the Mac's minimum. Here the limit is the client size.
    nint LimitSize(nint hwnd, uint message, nint w, nint l, ref bool handled)
    {
        try
        {
            Fatal.TestThrow(Fatal.SizeLimitPlace);
            // Avalonia hears this too and fires its Screens event when the list really changed.
            // This one goes after that, so the screens are fresh, and a test can send it.
            if (message == WmDisplayChange) Avalonia.Threading.Dispatcher.UIThread.Post(() => OnScreensChanged(this, EventArgs.Empty));
            return WindowLimits.OnMessage(hwnd, message, l, ClientSize, RenderScaling, MinimumClient, null, null, _options.TestWorkArea, ref handled);
        }
        catch (Exception e)
        {
            Fatal.Raise("WINDOW_SIZE", e);
            return 0;
        }
    }

    void OnScreensChanged(object? sender, EventArgs e) => Fatal.Guard("SCREENS_CHANGED", () =>
    {
        FitToScreen(shrink: false);
        Panels.ScreensChanged();
    });

    /// Keeps the window usable on the monitor it is on. The minimum follows the monitor on every
    /// move and scale change. Only a launch shrinks the window, because a user may spread a big
    /// window over two monitors, and a move must not undo that.
    void FitToScreen(bool shrink)
    {
        if (TryGetPlatformHandle() is not { HandleDescriptor: "HWND", Handle: var hwnd }) return;
        var scale = RenderScaling;
        var (frameX, frameY) = WindowLimits.FrameOf(hwnd, ClientSize, scale);
        var work = WindowLimits.WorkAreaOf(hwnd);
        if (_options.TestWorkArea is { } size) work = new PixelRect(work.X, work.Y, size.Width, size.Height);
        CapMinimum(work.Size, scale, frameX, frameY);
        if (!shrink || !FitTo(work.Size, scale, frameX, frameY)) return;
        // The size just changed, so the panels and the view must see the new layout.
        UpdateLayout();
        var (right, bottom) = (Position.X + (int)Math.Round(Width * scale) + frameX, Position.Y + (int)Math.Round(Height * scale) + frameY);
        Position = new PixelPoint(Math.Max(work.X, Position.X - Math.Max(0, right - work.Right)), Math.Max(work.Y, Position.Y - Math.Max(0, bottom - work.Bottom)));
    }

    /// The minimum in force is the Mac's, less what this work area cannot show.
    public void CapMinimum(PixelSize work, double scale, int frameX, int frameY)
    {
        LeastClient = WindowLimits.Capped(MinimumClient, scale, work, frameX, frameY);
        MinWidth = LeastClient.Width;
        MinHeight = LeastClient.Height;
    }

    /// Caps the minimum, and shrinks a window too big for this work area. It says whether the
    /// size changed. A maximized window already fits, so it keeps its size to come back to.
    public bool FitTo(PixelSize work, double scale, int frameX, int frameY)
    {
        CapMinimum(work, scale, frameX, frameY);
        if (WindowState != WindowState.Normal) return false;
        var room = WindowLimits.RoomOn(work, scale, frameX, frameY);
        var changed = false;
        if (Width > room.Width)
        {
            Width = room.Width;
            changed = true;
        }
        if (Height > room.Height)
        {
            Height = room.Height;
            changed = true;
        }
        return changed;
    }

    /// The commands that work in W7. Every other menu item and button is present and off.
    Dictionary<string, Action> BuildActions()
    {
        var actions = new Dictionary<string, Action>
        {
            [WindowsMenus.ExitAction] = Close,
            ["selector:zoomIn:"] = View.ZoomIn,
            ["selector:zoomOut:"] = View.ZoomOut,
            ["selector:zoomToWindow:"] = View.Fit,
            ["selector:actualSize:"] = View.ActualSize,
            ["reset_panels"] = Panels.ResetLayout,
            [CommandBar.WandAction] = () => Tools.Current = ToolState.MagicWand,
        };
        foreach (var panel in _catalog.Panels)
        {
            var id = panel.Id;
            actions[MenuBuilder.PanelActionPrefix + id] = () => Panels.Toggle(id);
        }
        foreach (var value in ZoomPresets.Values)
        {
            var zoom = value;
            actions[$"View > Zoom Presets > {ZoomPresets.Label(zoom)}"] = () => View.SetZoom(zoom);
        }
        return actions;
    }

    /// Menu shortcuts that work. A native child gets its keys before Avalonia does, so the
    /// canvas and the window both come here.
    Dictionary<string, Action> BuildShortcuts()
    {
        var shortcuts = new Dictionary<string, Action>();
        void Walk(IEnumerable<MenuEntry> entries)
        {
            foreach (var entry in entries)
            {
                if (entry.Key is not null && MenuBuilder.TryAction(entry, _actions, out var action) && entry.Action != WindowsMenus.ExitAction)
                    shortcuts[entry.Key] = action;
                Walk(entry.Items);
            }
        }
        Walk(_menus);
        return shortcuts;
    }

    bool RunShortcut(string? words)
    {
        if (words is null || !_shortcuts.TryGetValue(words, out var action)) return false;
        action();
        return true;
    }

    [DllImport("user32.dll")] static extern short GetKeyState(int key);

    bool OnCanvasKey(int vk, bool repeat)
    {
        if (Dialog is not null) return false;
        const int VkShift = 0x10, VkControl = 0x11, VkAlt = 0x12;
        bool ctrl = GetKeyState(VkControl) < 0, shift = GetKeyState(VkShift) < 0, alt = GetKeyState(VkAlt) < 0;
        if (RunShortcut(WindowsKeys.FromVirtualKey(vk, ctrl, shift, alt))) return true;
        return !ctrl && !alt && !shift && !repeat && vk is >= 'A' and <= 'Z' && UseToolKey((char)vk);
    }

    /// A letter on the canvas picks a tool, as MetalCanvasView.onToolKey does.
    public bool UseToolKey(char key)
    {
        if (Tools.ToolForKey(key) is not { } tool) return false;
        Tools.Current = tool;
        return true;
    }

    void OnWindowKey(object? sender, KeyEventArgs e)
    {
        if (Dialog is not null) return;
        if (!e.Handled && RunShortcut(WindowsKeys.FromAvalonia(e.Key, e.KeyModifiers))) e.Handled = true;
    }

    /// Light and dark differ only by the palette, so a theme change rebuilds the rows. The
    /// stage stays where it is, because the native canvas must not be parented again.
    void BuildRows()
    {
        Palette = ActualThemeVariant == ThemeVariant.Dark ? Tokens.Dark : Tokens.Light;
        var stageFill = _options.TestPaintFault == StartOptions.WrongStageFault ? Avalonia.Media.Colors.Red : Palette.Stage;
        Background = Ui.B(stageFill);
        _canvasLayer.Background = Ui.B(stageFill);
        Stage.Background = Ui.B(stageFill);

        if (TitleBar is not null) _root.Children.Remove(TitleBar);
        TitleBar = new TitleRow(Palette, _menus, _actions);
        TitleBar.SetTitle(_title, dirty: false);
        TitleBar.MinimizeButton.Click += (_, _) => WindowState = WindowState.Minimized;
        TitleBar.MaximizeButton.Click += (_, _) => WindowState = WindowState == WindowState.Maximized ? WindowState.Normal : WindowState.Maximized;
        TitleBar.CloseButton.Click += (_, _) => Close();
        _root.Children.Insert(0, TitleBar);
        if (!_root.Children.Contains(_canvasLayer))
        {
            Grid.SetRow(_canvasLayer, 1);
            _root.Children.Add(_canvasLayer);
        }

        // Tab order: options, strip, canvas stop, status bar.
        if (Options is not null) _canvasLayer.Children.Remove(Options);
        Options = new OptionsRow(Palette, Tools, _menus, _catalog.Panels, _actions);
        _canvasLayer.Children.Insert(0, Options);
        if (Strip is not null) _canvasLayer.Children.Remove(Strip);
        Strip = new ImageStrip(Palette);
        Strip.Show([_image]);
        Grid.SetRow(Strip, 1);
        _canvasLayer.Children.Insert(1, Strip);

        var image = Status?.ImageValue.Text;
        if (Status is not null) _root.Children.Remove(Status);
        Status = new StatusBar(Palette);
        if (image is not null) Status.ImageValue.Text = image;
        Grid.SetRow(Status, 2);
        _root.Children.Add(Status);
        Status.FitChosen += () => { View.Fit(); Native?.TakeFocus(); };
        Status.ZoomChosen += zoom => { View.SetZoom(zoom); Native?.TakeFocus(); };
        _toolHint = ToolState.Hint(Tools.CurrentTool);
        ShowHint();
        Status.ShowZoom(View.View.Zoom);

        Panels.ApplyLook(Palette, RequestedThemeVariant);
        if (Dialog is { } dialog)
        {
            dialog.RequestedThemeVariant = RequestedThemeVariant;
            dialog.ApplyLook(Palette);
        }
        UpdateActive();
        UpdatePanelChecks();
        ApplyWindowState();
        SendBackground();
    }

    void UpdateActive() => TitleBar.SetActive(IsActive || Panels.AnyActive || Dialog?.IsActive == true);

    /// The engine's last error, or else the tool's hint.
    void ShowHint() => Status.Hint.Text = Editor?.Message ?? _toolHint;

    /// Gives the panels a document, or none. A later stage sets this as images open and close.
    public void Bind(Editor? editor)
    {
        if (Editor is not null) Editor.Changed -= OnEditorChanged;
        Editor = editor;
        Panels.Context.Editor = editor;
        if (editor is not null) editor.Changed += OnEditorChanged;
        OnEditorChanged();
    }

    void OnEditorChanged()
    {
        Panels.ReloadPanels();
        ShowHint();
    }

    /// Opens Layer Properties for a layer: EditorWindowController.showLayerProperties. Null when
    /// there is no such layer, or a dialog is already up. Like a modal dialog, it holds off the
    /// main window for its life. The panels stay open and turn their commands off.
    public LayerPropertiesDialog? ShowLayerProperties(int index)
    {
        if (Dialog is not null || Editor is not { } editor || index < 0 || index >= editor.Info.Layers.Count) return null;
        var dialog = new LayerPropertiesDialog(editor, index, Palette) { WorkAreas = () => Panels.Screens() };
        if (RequestedThemeVariant is { } theme) dialog.RequestedThemeVariant = theme;
        Dialog = dialog;
        dialog.PropertyChanged += (_, e) => { if (e.Property == IsActiveProperty) UpdateActive(); };
        dialog.Finished += _ =>
        {
            // Before the dialog goes, so Windows gives the main window the focus.
            HoldOff(false);
            Dialog = null;
            Panels.Context.Busy = false;
            Panels.ReloadPanels();
        };
        Panels.Context.Busy = true;
        Panels.ReloadPanels();
        HoldOff(true);
        dialog.Show(this);
        return dialog;
    }

    [DllImport("user32.dll")] static extern bool EnableWindow(nint hwnd, bool enable);

    /// EnableWindow's result says what the window was before, so it says nothing of a failure.
    void HoldOff(bool off)
    {
        if (TryGetPlatformHandle() is { HandleDescriptor: "HWND", Handle: var hwnd }) EnableWindow(hwnd, !off);
    }

    void ApplyWindowState() => TitleBar.MaximizeButton.Maximized = WindowState == WindowState.Maximized;

    void SendBackground() => _session?.SetBackground(Palette.Stage.R, Palette.Stage.G, Palette.Stage.B);

    void OnToolChanged()
    {
        View.Tool = Tools.CanvasMode;
        _toolHint = ToolState.Hint(Tools.CurrentTool);
        Editor?.ClearMessage();
        ShowHint();
    }

    void OnPanelsChanged()
    {
        UpdateClearRect();
        UpdatePanelChecks();
    }

    /// Each panel's toggle in the Window menu and the More menu shows whether it is shown.
    void UpdatePanelChecks()
    {
        void Mark(IEnumerable<object?> items)
        {
            foreach (var item in items.OfType<CatalogMenuItem>())
            {
                if (item.Entry.Action.StartsWith(MenuBuilder.PanelActionPrefix, StringComparison.Ordinal))
                    item.IsChecked = Panels.IsShown(item.Entry.Action[MenuBuilder.PanelActionPrefix.Length..]);
            }
        }
        Mark(TitleBar.Menus.Items.OfType<MenuItem>().Single(m => (string?)m.Header == "Window").Items);
        Mark(Options.MoreMenu.Items);
    }

    /// The stage on the screen, in layout points. Panels and the clear rectangle use this.
    public Rect StageOnScreen()
    {
        var scale = RenderScaling;
        var at = Stage.PointToScreen(default);
        return new Rect(at.X / scale, at.Y / scale, Stage.Bounds.Width, Stage.Bounds.Height);
    }

    /// Gives the canvas the part of the stage the panels leave clear.
    void UpdateClearRect()
    {
        if (Stage.Bounds.Width <= 0 || Stage.Bounds.Height <= 0 || !IsVisible) return;
        var stage = StageOnScreen();
        var clear = PanelLayout.ClearRect(stage, Panels.ShownFrames());
        View.SetClearRect(new Rect(clear.X - stage.X, clear.Y - stage.Y, clear.Width, clear.Height));
    }

    /// Avalonia sees no press on the native canvas, so it cannot close what is open.
    public void DismissPopups()
    {
        Status.CommitTyping();
        TitleBar.Menus.Close();
        Options.CloseMore();
        Options.MoreMenu.Close();
        foreach (var box in this.GetVisualDescendants().OfType<ComboBox>()) box.IsDropDownOpen = false;
    }

    /// Tab left the native canvas: focus moves to the next control in Tab order.
    /// `back` means Shift was held.
    public void LeaveCanvas(bool back)
    {
        var manager = FocusManager ?? throw new InvalidOperationException("CANVAS_TAB: the window has no focus manager");
        var options = new FindNextElementOptions { FocusedElement = CanvasInput };
        var next = manager.FindNextElement(back ? NavigationDirection.Previous : NavigationDirection.Next, options);
        if (next is null) throw new InvalidOperationException("CANVAS_TAB: nothing takes focus after the canvas");
        next.Focus(NavigationMethod.Tab);
    }

    void StartCanvas()
    {
        _session = new CanvasSession();
        _session.Load(_options.TestPng, (width, height) =>
        {
            if (Native is null || Native.Hwnd == 0) throw new InvalidOperationException("CANVAS_HWND: the native child is missing");
            _session.Attach(Native.Hwnd);
            Status.ImageValue.Text = $"{width} × {height} px";
            Bind(new Editor(_session.Client, _session.Document));
            SendBackground();
            View.SetDocumentSize(new Size(width, height));
            UpdateClearRect();
            PushView();
            _session.WhenFirstFrame(() =>
            {
                // The window may have closed while this waited in the queue.
                if (_session is null) return;
                Native.ShowCanvas();
                // The first frame may have gone to a window with nothing on screen.
                PushView();
                _canvasShown.TrySetResult();
            });
            ImageReady?.Invoke();
            _image = _image with { ImageWidth = width, ImageHeight = height };
            Strip.Show([_image]);
            _session.LoadPreview(PreviewWidth, PreviewHeight, ShowPreview);
        });
    }

    void ShowPreview(byte[] rgba, int width, int height)
    {
        var bitmap = new WriteableBitmap(new PixelSize(width, height), new Vector(96, 96), PixelFormats.Rgba8888, AlphaFormat.Unpremul);
        using (var locked = bitmap.Lock())
            for (var row = 0; row < height; row++)
                Marshal.Copy(rgba, row * width * 4, locked.Address + row * locked.RowBytes, width * 4);
        _image = _image with { Preview = bitmap };
        Strip.Show([_image]);
    }

    void OnViewChanged()
    {
        PushView();
        // Set only when it changes, so a pan does no text layout.
        Status.ShowZoom(View.View.Zoom);
    }

    /// Sends the newest view to the engine. It returns at once and runs from the input
    /// message, so Pan does not wait for Avalonia's UI frame.
    void PushView()
    {
        if (Native is null || _session is null) return;
        Native.Scale = RenderScaling;
        var v = View.View;
        _session.SetView(Stage.Bounds.Width, Stage.Bounds.Height, RenderScaling, v.Zoom, v.Origin.X, v.Origin.Y);
    }

    void OnZoomRectangle(Rect? rect)
    {
        var v = View.View;
        _session?.SetZoomRectangle(rect is { } r ? new Rect(v.DocumentPoint(r.TopLeft), v.DocumentPoint(r.BottomRight)) : null);
    }

    void StopCanvas()
    {
        Dialog?.Cancel();
        var editor = Editor;
        Bind(null);
        editor?.Dispose();
        _session?.Dispose();
        _session = null;
    }
}
