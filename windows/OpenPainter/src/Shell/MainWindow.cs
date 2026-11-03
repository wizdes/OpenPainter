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
using Avalonia.Controls.Primitives;
using Avalonia.Controls.Templates;
using Avalonia.Input;
using Avalonia.Interactivity;
using Avalonia.Media;
using Avalonia.Media.Imaging;
using Avalonia.Platform;
using Avalonia.Styling;
using Avalonia.Threading;
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

    /// EditorWindowController.minimumSize.
    public static readonly Size MinimumClient = new(1100, 792);

    /// The minimum in force: the Mac's, or less on a monitor that cannot show that.
    public Size LeastClient { get; private set; } = MinimumClient;

    readonly StartOptions _options;
    readonly MacCatalog _catalog;
    readonly IReadOnlyList<MenuEntry> _menus;
    readonly Dictionary<string, Func<bool>> _states = [];
    readonly Dictionary<string, Action> _actions;
    readonly Dictionary<string, Action> _shortcuts;
    readonly AltGesture _alt = new();
    readonly SystemColors? _testContrast;
    bool _menuFromCanvas;
    Window? _menuFromPanel;
    IInputElement? _menuReturn;
    Window? _ringAt;
    readonly Grid _root = new() { Name = "Root", RowDefinitions = new RowDefinitions("Auto,*,Auto") };
    readonly Grid _canvasLayer = new() { Name = "CanvasLayer", RowDefinitions = new RowDefinitions("Auto,Auto,*") };
    string _title;
    string _documentName;
    StripImage _image;
    CanvasSession? _session;
    string _toolHint = "";
    MeasurementUnit _unit = MeasurementUnit.Pixels;
    Size? _imageSize;

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

    /// The keys that work right now, in the catalog's words: a menu key whose item works, and each
    /// key Windows adds. docs/windows/shortcuts-windows.md and a test hold this list.
    public IReadOnlyCollection<string> WorkingKeys => _shortcuts.Keys;

    /// How many times a field refused what was typed, for tests.
    public int RefusedInputs { get; private set; }

    /// The units for the status bar. Saving them comes with settings.
    public MeasurementUnit Unit
    {
        get => _unit;
        set => SetUnit(value);
    }

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
        Panels = new PanelManager(this, _catalog.Panels, Palette, StageOnScreen, Tools);
        Panels.Context.ShowLayerProperties = index => ShowLayerProperties(index);
        Panels.Context.Report = ShowReport;
        Panels.Context.GiveKeyboardBack = GiveKeyboardToCanvas;
        if (options.TestContrastTheme is { } file) _testContrast = HighContrast.FromThemeText(File.ReadAllText(file), Path.GetFileName(file));
        Panels.Context.Refused = () =>
        {
            RefusedInputs++;
            // A test run stays quiet. A real window sounds as the Mac's does.
            if (Native is not null) MessageBeep(0);
        };
        _actions = BuildActions();
        _shortcuts = BuildShortcuts();

        Title = _title;
        Width = Tokens.WindowWidth;
        Height = Tokens.WindowHeight;
        MinWidth = MinimumClient.Width;
        MinHeight = MinimumClient.Height;
        FontFamily = new FontFamily(Tokens.FontFamily);
        if (options.Theme is { } theme) RequestedThemeVariant = theme == "dark" ? ThemeVariant.Dark : ThemeVariant.Light;
        // Fluent's own colors for what the palette does not name follow the system's window color.
        if (_testContrast is not null || (!headless && HighContrast.IsOn()))
            RequestedThemeVariant = HighContrast.IsDark(_testContrast ?? HighContrast.FromSystem()) ? ThemeVariant.Dark : ThemeVariant.Light;

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
            Native.Pressed += _alt.Spoil;
            Native.KeyPressed += OnCanvasKey;
            Native.SystemKeyPressed += OnCanvasSystemKey;
            Native.AltReleased += () => AltUp(fromCanvas: true);
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
                // Windows' high-contrast switch can change while the app is open.
                if (Application.Current?.PlatformSettings is { } settings) settings.ColorValuesChanged += (_, _) => Fatal.Guard("CONTRAST_CHANGED", BuildRows);
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
        AddHandler(KeyDownEvent, OnMenuLetter, RoutingStrategies.Tunnel);
        WatchAlt(this);
        foreach (var panel in Panels.Panels.Values)
        {
            WatchAlt(panel);
            panel.KeyDown += OnPanelKey;
        }
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

    /// The commands that work so far. Every other menu item and button is present and off.
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
        AddDocumentCommands(actions);
        actions["Window > Next Panel"] = () => CyclePanels(forward: true);
        actions["Window > Previous Panel"] = () => CyclePanels(forward: false);
        _states["Window > Next Panel"] = _states["Window > Previous Panel"] = () => Panels.Panels.Keys.Any(Panels.IsShown);
        foreach (var value in ZoomPresets.Values)
        {
            var zoom = value;
            actions[$"View > Zoom Presets > {ZoomPresets.Label(zoom)}"] = () => View.SetZoom(zoom);
        }
        foreach (var unit in Measurement.All)
        {
            var chosen = unit;
            actions[$"View > {Measurement.Title(chosen)}"] = () => SetUnit(chosen);
        }
        return actions;
    }

    /// The Edit and Layers items that work on the open document, with the rule for when each is
    /// on: EditorWindowController and MenuStates.swift. Each runs what its panel button runs.
    void AddDocumentCommands(Dictionary<string, Action> actions)
    {
        void Doc(string path, Func<Editor, bool> on, Action<Editor> run)
        {
            _states[path] = () => !Panels.Context.Busy && Editor is { } e && on(e);
            actions[path] = () =>
            {
                if (_states[path]() && Editor is { } e) run(e);
            };
        }
        void Layer(string path, string id) => Doc(path, e => e.IsEnabled(id), _ => Panels.Layers.Act(id));
        void Plain(string path, string id) => Doc(path, e => e.IsEnabled(id), e => e.Run(id));
        Doc("Edit > Undo", e => e.Info.HistoryIndex > 0, _ => Panels.History.Undo());
        Doc("Edit > Redo", e => e.Info.HistoryIndex + 1 < e.Info.HistoryLabels.Count, _ => Panels.History.Redo());
        Layer("Layers > Add New Layer", "layer.add");
        Layer("Layers > Delete Layer", "layer.delete");
        Layer("Layers > Duplicate Layer", "layer.duplicate");
        Layer("Layers > Merge Layer Down", "layer.merge_down");
        Layer("Layers > Move Layer Up", "layer.move_up");
        Layer("Layers > Move Layer Down", "layer.move_down");
        Plain("Layers > Flip Horizontal", "layer.flip_horizontal");
        Plain("Layers > Flip Vertical", "layer.flip_vertical");
        Plain("Layers > Rotate 180°", "layer.rotate_180");
        static bool Any(Editor e) => e.Info.Layers.Count > 0;
        static bool Above(Editor e) => e.Info.ActiveLayer < e.Info.Layers.Count - 1;
        static bool Below(Editor e) => e.Info.ActiveLayer > 0;
        Doc("Layers > Toggle Layer Visibility", Any, e => Panels.Layers.ToggleVisible(e.Info.ActiveLayer));
        Doc("Layers > Layer Properties…", Any, e => ShowLayerProperties(e.Info.ActiveLayer));
        Doc("Layers > Go to Top Layer", Above, e => Panels.Layers.Select(e.Info.Layers.Count - 1));
        Doc("Layers > Go to Layer Above", Above, e => Panels.Layers.Select(e.Info.ActiveLayer + 1));
        Doc("Layers > Go to Layer Below", Below, e => Panels.Layers.Select(e.Info.ActiveLayer - 1));
        Doc("Layers > Go to Bottom Layer", Below, e => Panels.Layers.Select(0));
        Doc("Layers > Move Layer to Top", Above, e => e.Run("layer.move", Bridge.EngineParams.Of(("from", e.Info.ActiveLayer), ("to", e.Info.Layers.Count - 1))));
        Doc("Layers > Move Layer to Bottom", Below, e => e.Run("layer.move", Bridge.EngineParams.Of(("from", e.Info.ActiveLayer), ("to", 0))));
        // The options row's Undo and Redo find their work by the menu item's action.
        foreach (var path in new[] { "Edit > Undo", "Edit > Redo" })
            actions[Find(_menus, path).Action] = actions[path];
    }

    static MenuEntry Find(IEnumerable<MenuEntry> entries, string path)
    {
        foreach (var entry in entries)
        {
            if (entry.Path == path) return entry;
            if (entry.Items.Count != 0 && path.StartsWith(entry.Path, StringComparison.Ordinal)) return Find(entry.Items, path);
        }
        throw new InvalidDataException($"CATALOG: no menu item {path}");
    }

    /// Turns each document command on or off for the state of the document.
    void UpdateMenuStates()
    {
        void Walk(IEnumerable<object?> items)
        {
            foreach (var item in items.OfType<CatalogMenuItem>())
            {
                if (_states.TryGetValue(item.Entry.Path, out var on)) item.IsEnabled = on();
                else if (item.Entry.Kind == EntryKind.Menu) Walk(item.Items);
            }
        }
        foreach (var top in TitleBar.Menus.Items.OfType<MenuItem>()) Walk(top.Items);
        Options.Commands["undo"].IsEnabled = _states["Edit > Undo"]();
        Options.Commands["redo"].IsEnabled = _states["Edit > Redo"]();
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
        // Windows users expect Ctrl+Y beside the Mac's key: docs/windows/shortcuts-windows.md.
        shortcuts[RedoBeside] = _actions["Edit > Redo"];
        // MenuSkeleton.swift's hidden twins of the panel toggles: Option-Command-digit.
        var digit = 1;
        foreach (var panel in _catalog.Panels)
            shortcuts[$"Ctrl+Alt+{digit++}"] = _actions[MenuBuilder.PanelActionPrefix + panel.Id];
        return shortcuts;
    }

    public const string RedoBeside = "Ctrl+Y";

    bool RunShortcut(string? words)
    {
        if (words is null || !_shortcuts.TryGetValue(words, out var action)) return false;
        action();
        return true;
    }

    [DllImport("user32.dll")] static extern short GetKeyState(int key);

    const int VkShift = 0x10, VkControl = 0x11, VkAlt = 0x12, VkF10 = 0x79;

    /// Whether Alt is down now, for a window that comes back. A test sets this, because a headless
    /// run has no keyboard to ask.
    public Func<bool> AltIsDown { get; set; } = () => GetKeyState(VkAlt) < 0;

    string? CanvasWords(int vk) => WindowsKeys.FromVirtualKey(vk, GetKeyState(VkControl) < 0, GetKeyState(VkShift) < 0, GetKeyState(VkAlt) < 0);

    bool OnCanvasKey(int vk, bool repeat)
    {
        if (vk == VkAlt)
        {
            _alt.Press(GetKeyState(VkControl) < 0);
            return true;
        }
        _alt.Spoil();
        return Route(CanvasWords(vk), repeat, KeySource.Canvas, null);
    }

    /// Windows sends Alt, an Alt combination and F10 to the canvas as system keys.
    bool OnCanvasSystemKey(int vk, bool repeat)
    {
        if (vk == VkAlt)
        {
            _alt.Press(GetKeyState(VkControl) < 0);
            return true;
        }
        if (vk == VkF10)
        {
            if (!repeat) PressF10(fromCanvas: true, null);
            return true;
        }
        _alt.Spoil();
        if (Route(CanvasWords(vk), repeat, KeySource.Canvas, null)) return true;
        return !repeat && GetKeyState(VkAlt) < 0 && GetKeyState(VkControl) >= 0 && vk is >= 'A' and <= 'Z' && OpenMenuByLetter((char)vk, fromCanvas: true);
    }

    /// One key, from anywhere: FocusRouter says who may have it. True means it was used.
    public bool Route(string? words, bool repeat, KeySource source, object? focused)
    {
        if (words is null) return false;
        var target = FocusRouter.Target(Dialog is not null, focused);
        if (!FocusRouter.DocumentGets(target, words)) return false;
        if (RunShortcut(words)) return true;
        return FocusRouter.CanvasKeysRun(target, source) && CanvasKeys.Run(Tools, Editor, words, repeat);
    }

    /// Window > Next Panel and Previous Panel: PanelFocusCycler in KeyboardShortcuts.swift. The
    /// keyboard moves from the image through the shown panels in the catalog's order and back.
    /// It returns the window that has it, or null when no panel is shown.
    public Window? CyclePanels(bool forward)
    {
        if (Dialog is not null) return null;
        var ring = new List<Window> { this };
        ring.AddRange(Panels.Panels.Values.Where(p => Panels.IsShown(p.Panel.Id) && p.IsVisible));
        if (ring.Count < 2) return null;
        // The last step counts until the user picks a window, and then the active one does.
        var current = _ringAt ?? ring.FirstOrDefault(w => w.IsActive) ?? this;
        var at = ring.IndexOf(current);
        var next = ring[at < 0 ? 0 : (at + (forward ? 1 : ring.Count - 1)) % ring.Count];
        if (next == this)
        {
            GiveKeyboardToCanvas();
            _ringAt = next;
            return next;
        }
        next.Activate();
        _ringAt = next;
        var manager = next.FocusManager ?? throw new InvalidOperationException("PANEL_FOCUS: the panel has no focus manager");
        // The first control that takes focus, or else the panel itself.
        var first = manager.FindNextElement(NavigationDirection.Next, new FindNextElementOptions { SearchRoot = next, FocusedElement = next });
        if (first is null || !first.Focus(NavigationMethod.Tab)) next.Focus();
        return next;
    }

    /// Back to the canvas with no click, as after Enter in a color field.
    public void GiveKeyboardToCanvas()
    {
        if (Dialog is not null) return;
        if (Native is null)
        {
            CanvasInput.Focus(NavigationMethod.Unspecified);
            return;
        }
        Activate();
        Native.TakeFocus();
    }

    /// Listens for Alt on one window, ahead of its controls. A press anywhere in it spoils the tap.
    void WatchAlt(Window window)
    {
        window.AddHandler(KeyDownEvent, (_, e) =>
        {
            if (e.Key is Key.LeftAlt or Key.RightAlt)
            {
                _alt.Press(e.KeyModifiers.HasFlag(KeyModifiers.Control));
                // Alt with a menu open closes it, and its release must not open it again.
                if (TitleBar.Menus.IsOpen) _alt.Spoil();
            }
            else if (e.Key is not (Key.LeftCtrl or Key.RightCtrl or Key.LeftShift or Key.RightShift)) _alt.Spoil();
        }, RoutingStrategies.Tunnel, handledEventsToo: true);
        window.AddHandler(KeyUpEvent, (_, e) =>
        {
            if (e.Key is Key.LeftAlt or Key.RightAlt) AltUp(fromCanvas: false, ReferenceEquals(window, this) ? null : window);
        }, RoutingStrategies.Tunnel, handledEventsToo: true);
        window.AddHandler(PointerPressedEvent, (_, _) => _alt.Spoil(), RoutingStrategies.Tunnel, handledEventsToo: true);
        window.Deactivated += (_, _) =>
        {
            _alt.Forget();
            // The release may go to another program, so the underlines must not wait for it.
            window.SetValue(AccessText.ShowAccessKeyProperty, false);
        };
        window.Activated += (_, _) =>
        {
            if (!ReferenceEquals(window, _ringAt)) _ringAt = null;
            if (!AltIsDown()) _alt.ReleaseHandled();
        };
    }

    void AltUp(bool fromCanvas, Window? panel = null)
    {
        if (_alt.Release() && Dialog is null) OpenMenuBar(fromCanvas, panel);
        // Avalonia's handler may run after this one for the same release. A job runs after both.
        Dispatcher.UIThread.Post(_alt.ReleaseHandled);
    }

    /// A clean Alt tap or F10: the menu bar takes the keyboard, on the first menu. `panel` is the
    /// floating panel the key came from.
    public void OpenMenuBar(bool fromCanvas, Window? panel = null)
    {
        TakeKeyboardForMenus(fromCanvas, panel);
        TitleBar.Menus.OpenFromKeys();
        if (TitleBar.Menus.Items.OfType<MenuItem>().FirstOrDefault() is { } first) first.Focus(NavigationMethod.Directional);
    }

    /// F10 does what an Alt tap does: shortcuts-windows.md. With the bar open it closes it.
    bool PressF10(bool fromCanvas, Window? panel)
    {
        if (Dialog is not null) return false;
        if (TitleBar.Menus.IsOpen) TitleBar.Menus.Close();
        else OpenMenuBar(fromCanvas, panel);
        return true;
    }

    /// Alt with a menu's letter opens that menu.
    public bool OpenMenuByLetter(char letter, bool fromCanvas, Window? panel = null)
    {
        if (Dialog is not null || MenuLetters.Title(letter) is not { } title) return false;
        var menu = TitleBar.Menus.Items.OfType<MenuItem>().Single(m => (string?)m.Header == title);
        TakeKeyboardForMenus(fromCanvas, panel);
        TitleBar.Menus.OpenFromKeys();
        menu.Focus(NavigationMethod.Directional);
        menu.Open();
        return true;
    }

    void TakeKeyboardForMenus(bool fromCanvas, Window? panel)
    {
        if (!TitleBar.Menus.IsOpen)
        {
            _menuFromCanvas = fromCanvas;
            _menuFromPanel = panel;
            _menuReturn = (panel ?? (Window)this).FocusManager?.GetFocusedElement();
        }
        if (Native is null) return;
        Activate();
        Native.GiveFocusToWindow();
    }

    /// With a menu open, a letter moves to the next item that starts with it, as a Windows menu
    /// with no marked letters does, and Enter then runs it.
    void OnMenuLetter(object? sender, KeyEventArgs e)
    {
        if (!TitleBar.Menus.IsOpen || e.KeyModifiers != KeyModifiers.None || e.Key is < Key.A or > Key.Z) return;
        if (TitleBar.Menus.Items.OfType<MenuItem>().FirstOrDefault(m => m.IsSubMenuOpen) is not { } open) return;
        var deepest = open;
        while (deepest.Items.OfType<MenuItem>().FirstOrDefault(m => m.IsSubMenuOpen) is { } inner) deepest = inner;
        var items = deepest.Items.OfType<MenuItem>().Where(i => i.IsEnabled).ToList();
        var letter = (char)('A' + (e.Key - Key.A));
        var matches = items.Where(i => i.Header is string text && text.StartsWith(letter.ToString(), StringComparison.OrdinalIgnoreCase)).ToList();
        if (matches.Count == 0) return;
        var at = matches.FindIndex(i => i.IsFocused);
        matches[(at + 1) % matches.Count].Focus(NavigationMethod.Directional);
        e.Handled = true;
    }

    void OnPanelKey(object? sender, KeyEventArgs e)
    {
        if (e.Handled) return;
        var panel = (PanelWindow)sender!;
        if (e.Key == Key.F10 && e.KeyModifiers == KeyModifiers.None && PressF10(fromCanvas: false, panel))
        {
            e.Handled = true;
            return;
        }
        if (Route(WindowsKeys.FromAvalonia(e.Key, e.KeyModifiers), false, KeySource.Panel, panel.FocusManager?.GetFocusedElement()))
        {
            e.Handled = true;
            return;
        }
        if (e.KeyModifiers == KeyModifiers.Alt && e.Key is >= Key.A and <= Key.Z && OpenMenuByLetter((char)('A' + (e.Key - Key.A)), fromCanvas: false, panel)) e.Handled = true;
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
        if (e.Handled || Dialog is not null) return;
        var focused = FocusManager?.GetFocusedElement();
        var source = ReferenceEquals(focused, CanvasInput) ? KeySource.Canvas : KeySource.Window;
        if (e.Key == Key.F10 && e.KeyModifiers == KeyModifiers.None && PressF10(source == KeySource.Canvas, null))
        {
            e.Handled = true;
            return;
        }
        if (Route(WindowsKeys.FromAvalonia(e.Key, e.KeyModifiers), false, source, focused))
        {
            e.Handled = true;
            return;
        }
        if (e.KeyModifiers == KeyModifiers.Alt && e.Key is >= Key.A and <= Key.Z && OpenMenuByLetter((char)('A' + (e.Key - Key.A)), source == KeySource.Canvas)) e.Handled = true;
    }

    /// Light and dark differ only by the palette, so a theme change rebuilds the rows. The
    /// stage stays where it is, because the native canvas must not be parented again.
    void BuildRows()
    {
        Palette = LookNow();
        ControlStates.Apply(Resources, Palette);
        var stageFill = _options.TestPaintFault == StartOptions.WrongStageFault ? Avalonia.Media.Colors.Red : Palette.Stage;
        Background = Ui.B(stageFill);
        _canvasLayer.Background = Ui.B(stageFill);
        Stage.Background = Ui.B(stageFill);

        if (TitleBar is not null) _root.Children.Remove(TitleBar);
        TitleBar = new TitleRow(Palette, _menus, _actions);
        TitleBar.SetTitle(_title, Editor?.Info.Dirty ?? false);
        TitleBar.Menus.BlockOpen = () => _alt.Spoiled;
        TitleBar.Menus.Closed += OnMenusClosed;
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

        var imageText = Status?.ImageValue.Text;
        if (Status is not null) _root.Children.Remove(Status);
        Status = new StatusBar(Palette);
        Status.Unit = Unit;
        if (_imageSize is not null) Status.ShowImage(_imageSize);
        else if (imageText is not null) Status.ImageValue.Text = imageText;
        Status.UnitChosen += SetUnit;
        ShowDocumentValues();
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
        UpdateUnitChecks();
        UpdateMenuStates();
        ApplyWindowState();
        SendBackground();
    }

    /// The palette for this moment: a high-contrast session's system colors, or else the theme's.
    Palette LookNow()
    {
        if (_testContrast is { } test) return HighContrast.Palette(test);
        if (Native is not null && HighContrast.IsOn()) return HighContrast.Palette(HighContrast.FromSystem());
        return ActualThemeVariant == ThemeVariant.Dark ? Tokens.Dark : Tokens.Light;
    }

    /// The menu took the keyboard from the canvas, so it goes back there.
    void OnMenusClosed(object? sender, Avalonia.Interactivity.RoutedEventArgs e)
    {
        var (fromCanvas, panel, back) = (_menuFromCanvas, _menuFromPanel, _menuReturn);
        (_menuFromCanvas, _menuFromPanel, _menuReturn) = (false, null, null);
        // A click on another window closed it, or a menu item opened a dialog: the keyboard is
        // where the person put it.
        if (Dialog is not null || (Native is not null && !IsActive)) return;
        if (panel is { IsVisible: true }) panel.Activate();
        if (back is Visual visual && TopLevel.GetTopLevel(visual) is not null) back.Focus();
        if (fromCanvas) Native?.TakeFocus();
    }

    void UpdateActive() => TitleBar.SetActive(IsActive || Panels.AnyActive || Dialog?.IsActive == true);

    /// The engine's last error, or else the tool's hint.
    void ShowHint() => Status.Hint.Text = Editor?.Message ?? _toolHint;

    /// Gives the panels a document, or none. A later stage sets this as images open and close.
    public void Bind(Editor? editor)
    {
        if (Editor is not null)
        {
            Editor.Changed -= OnEditorChanged;
            Editor.PreviewReady -= ShowPreview;
        }
        Editor = editor;
        Panels.Context.Editor = editor;
        if (editor is not null)
        {
            editor.Changed += OnEditorChanged;
            editor.PreviewReady += ShowPreview;
            if (editor.Preview is var (rgba, width, height)) ShowPreview(rgba, width, height);
        }
        OnEditorChanged();
    }

    void OnEditorChanged()
    {
        Panels.ReloadPanels();
        ShowDocumentValues();
        ShowHint();
        ShowModified();
        UpdateMenuStates();
    }

    /// The title's dot and the strip's asterisk: Editor.dirty on the Mac. The strip rebuilds only
    /// when its picture or words change, so a focused item keeps the keyboard.
    void ShowModified()
    {
        var dirty = Editor?.Info.Dirty ?? false;
        TitleBar.SetTitle(_title, dirty);
        var image = Editor is { Info: { Width: > 0 } info }
            ? _image with { Modified = dirty, ImageWidth = info.Width, ImageHeight = info.Height }
            : _image with { Modified = dirty };
        if (image == _image) return;
        _image = image;
        Strip.Show([_image]);
    }

    /// The image size, resolution and profile the status bar shows for the document.
    void ShowDocumentValues()
    {
        if (Editor is not { } e) return;
        _imageSize = new Size(e.Info.Width, e.Info.Height);
        Status.Dpi = e.Dpi;
        Status.ShowImage(_imageSize);
        Status.ShowProfile(ColorProfileNames.Short(e.Info), ColorProfileNames.Full(e.Info));
    }

    /// The status bar's list or the View menu: ViewMenu.chooseUnits.
    public void SetUnit(MeasurementUnit unit)
    {
        _unit = unit;
        Status.Unit = unit;
        UpdateUnitChecks();
    }

    void UpdateUnitChecks()
    {
        var view = TitleBar.Menus.Items.OfType<MenuItem>().Single(m => (string?)m.Header == "View");
        foreach (var item in view.Items.OfType<CatalogMenuItem>())
        {
            foreach (var unit in Measurement.All)
            {
                if (item.Entry.Path != $"View > {Measurement.Title(unit)}") continue;
                item.ToggleType = MenuItemToggleType.CheckBox;
                item.IsChecked = unit == Unit;
            }
        }
    }

    /// A panel's report, for the status bar.
    void ShowReport(string text)
    {
        if (Editor is { } e) e.Report(text);
        else Status.Hint.Text = text;
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
            UpdateMenuStates();
        };
        Panels.Context.Busy = true;
        Panels.ReloadPanels();
        UpdateMenuStates();
        HoldOff(true);
        dialog.Show(this);
        return dialog;
    }

    [DllImport("user32.dll")] static extern bool EnableWindow(nint hwnd, bool enable);
    [DllImport("user32.dll")] static extern bool MessageBeep(uint type);

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
        _session.Load(_options.TestPng, CanvasLoaded);
    }

    /// The engine has the image, on the UI thread.
    public void CanvasLoaded(uint width, uint height)
    {
        // The window may have closed while this waited in the queue.
        if (_session is null) return;
        if (Native is null || Native.Hwnd == 0) throw new InvalidOperationException("CANVAS_HWND: the native child is missing");
        _session.Attach(Native.Hwnd);
        _imageSize = new Size(width, height);
        Status.ShowImage(_imageSize);
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
