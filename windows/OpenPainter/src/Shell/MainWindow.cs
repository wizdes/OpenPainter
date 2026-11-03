// The main window: the title row, the canvas and the status bar.
//
// The native canvas sits in the middle and Avalonia cannot draw over it, so nothing
// here overlaps it. Menus open as their own top-level windows.
using Avalonia;
using Avalonia.Automation;
using Avalonia.Controls;
using Avalonia.Controls.Chrome;
using Avalonia.Controls.Templates;
using Avalonia.Input;
using Avalonia.Media;
using Avalonia.Styling;
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

    readonly StartOptions _options;
    readonly IReadOnlyList<MenuEntry> _menus;
    readonly IReadOnlyDictionary<string, Action> _actions;
    readonly Grid _root = new() { Name = "Root", RowDefinitions = new RowDefinitions("Auto,*,Auto") };
    readonly Grid _canvasLayer = new() { Name = "CanvasLayer" };
    string _title;
    CanvasSession? _session;

    public TitleRow TitleBar { get; private set; } = null!;
    public StatusBar Status { get; private set; } = null!;
    public CanvasHost? Native { get; }
    public Palette Palette { get; private set; } = Tokens.Light;
    public CanvasSession? Session => _session;

    /// The focus stop for the canvas. In a real window it passes focus on to the native child.
    public Border CanvasInput { get; } = new() { Name = "CanvasInput", Focusable = true, Background = Brushes.Transparent };

    /// Runs on the UI thread once the first image is on screen in the view.
    public event Action? ImageReady;

    public MainWindow(StartOptions options, bool headless = false)
    {
        _options = options;
        _menus = WindowsMenus.Build(MacCatalog.Embedded());
        _actions = new Dictionary<string, Action> { [WindowsMenus.ExitAction] = Close };
        _title = TitleRow.TitleFor(options.TestPng is null ? "Untitled" : Path.GetFileName(options.TestPng));

        Title = _title;
        Width = Tokens.WindowWidth;
        Height = Tokens.WindowHeight;
        MinWidth = 1100;   // EditorWindowController.minimumSize.
        MinHeight = 792;
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

        AutomationProperties.SetName(CanvasInput, "Canvas");
        if (!headless)
        {
            Native = new CanvasHost();
            Native.TabOut += LeaveCanvas;
            _canvasLayer.Children.Add(Native);
            CanvasInput.GotFocus += (_, e) =>
            {
                if (e.NavigationMethod == NavigationMethod.Tab) Native.TakeFocus();
            };
            Native.SizeChanged += (_, _) => UpdateView();
            Opened += (_, _) => StartCanvas();
            Closed += (_, _) => StopCanvas();
        }
        _canvasLayer.Children.Add(CanvasInput);
        Content = _root;

        BuildRows();
        ActualThemeVariantChanged += (_, _) => BuildRows();
        PropertyChanged += (_, e) =>
        {
            if (e.Property == IsActiveProperty) TitleBar.SetActive(IsActive);
            if (e.Property == WindowStateProperty) ApplyWindowState();
        };
    }

    /// Light and dark differ only by the palette, so a theme change rebuilds the two rows.
    /// The native canvas stays where it is, because it must not be parented again.
    void BuildRows()
    {
        Palette = ActualThemeVariant == ThemeVariant.Dark ? Tokens.Dark : Tokens.Light;
        _root.Children.Clear();
        Background = Ui.B(Palette.Stage);
        _canvasLayer.Background = Ui.B(Palette.Stage);

        TitleBar = new TitleRow(Palette, _menus, _actions);
        TitleBar.SetTitle(_title, dirty: false);
        TitleBar.SetActive(IsActive);
        TitleBar.MinimizeButton.Click += (_, _) => WindowState = WindowState.Minimized;
        TitleBar.MaximizeButton.Click += (_, _) => WindowState = WindowState == WindowState.Maximized ? WindowState.Normal : WindowState.Maximized;
        TitleBar.CloseButton.Click += (_, _) => Close();
        _root.Children.Add(TitleBar);
        Grid.SetRow(_canvasLayer, 1);
        _root.Children.Add(_canvasLayer);

        var image = Status?.ImageValue.Text;
        Status = new StatusBar(Palette);
        if (image is not null) Status.ImageValue.Text = image;
        Grid.SetRow(Status, 2);
        _root.Children.Add(Status);
        ApplyWindowState();
        SendBackground();
    }

    void ApplyWindowState() => TitleBar.MaximizeButton.Maximized = WindowState == WindowState.Maximized;

    void SendBackground() => _session?.SetBackground(Palette.Stage.R, Palette.Stage.G, Palette.Stage.B);

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
            SendBackground();
            UpdateView();
            ImageReady?.Invoke();
        });
    }

    /// Zoom and Pan come in a later stage of W7. For now the image shows at 100% from its corner.
    void UpdateView()
    {
        if (Native is null || _session is null) return;
        _session.SetView(Native.Bounds.Width, Native.Bounds.Height, RenderScaling, zoom: 1, x: 0, y: 0);
    }

    void StopCanvas()
    {
        _session?.Dispose();
        _session = null;
    }
}
