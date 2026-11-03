// The four floating panels as top-level tool windows owned by the main window, as the
// Mac's are separate windows (PanelManager.swift). The native canvas cannot have Avalonia
// controls drawn over it, and an owned window can sit over it. The panels are empty until
// W8 and W9. Each keeps the system frame, its shadow and its resize edges.
using System.Runtime.InteropServices;
using Avalonia;
using Avalonia.Automation;
using Avalonia.Controls;
using Avalonia.Controls.Chrome;
using Avalonia.Input;
using Avalonia.Layout;
using Avalonia.Media;
using Avalonia.Styling;
using OpenPainter.Catalog;
using UiLook;

namespace OpenPainter.Shell;

public sealed partial class PanelWindow : Window
{
    const int GwlExStyle = -20;
    const long WsExToolWindow = 0x80, WsExAppWindow = 0x40000;
    const uint DwmCornerPreference = 33, WmGetMinMaxInfo = 0x0024;
    const int DwmCornerRound = 2;

    [StructLayout(LayoutKind.Sequential)] struct NativePoint { public int X, Y; }
    [StructLayout(LayoutKind.Sequential)] struct MinMaxInfo { public NativePoint Reserved, MaxSize, MaxPosition, MinTrack, MaxTrack; }
    [StructLayout(LayoutKind.Sequential)] struct NativeRect { public int Left, Top, Right, Bottom; }
    [LibraryImport("user32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static partial bool GetWindowRect(nint hwnd, out NativeRect rect);

    [LibraryImport("user32.dll", EntryPoint = "GetWindowLongPtrW", SetLastError = true)] private static partial nint GetWindowLongPtr(nint hwnd, int index);
    [LibraryImport("user32.dll", EntryPoint = "SetWindowLongPtrW", SetLastError = true)] private static partial nint SetWindowLongPtr(nint hwnd, int index, nint value);
    [LibraryImport("dwmapi.dll")] private static partial int DwmSetWindowAttribute(nint hwnd, uint attribute, ref int value, int size);

    public CatalogPanel Panel { get; }
    public IconCommand CloseButton { get; private set; } = null!;
    public Control TitleRow { get; private set; } = null!;

    /// The Mac's limits on the panel's size, in layout points. Null means no limit.
    public Size MinimumClient { get; }
    public double? MaximumClientWidth { get; }
    public double? MaximumClientHeight { get; }

    /// Close asks to hide the panel. Only the app's end really closes it.
    public event Action<PanelWindow>? HideRequested;

    public PanelWindow(CatalogPanel panel, Palette palette)
    {
        Panel = panel;
        Name = "Panel" + panel.Title;
        Title = panel.Title;
        var size = PanelLayout.DefaultSize(panel.Id);
        MinimumClient = PanelLayout.MinimumSize(panel.Id);
        Width = size.Width;
        Height = size.Height;
        // PanelManager.init: Colors keeps its width and grows to 470. Tools keeps its width.
        if (panel.Id == PanelLayout.Colors) { MaximumClientWidth = MinimumClient.Width; MaximumClientHeight = Tokens.ColorsExpandedHeight; }
        if (panel.Id == PanelLayout.Tools) MaximumClientWidth = size.Width;
        ShowInTaskbar = false;
        ShowActivated = false;
        WindowStartupLocation = WindowStartupLocation.Manual;
        FontFamily = new FontFamily(Tokens.FontFamily);
        ExtendClientAreaToDecorationsHint = true;
        ExtendClientAreaTitleBarHeightHint = Tokens.PanelTitleHeight;
        WindowDecorationsTheme = new ControlTheme(typeof(WindowDrawnDecorations))
        {
            Setters = { new Setter(WindowDrawnDecorations.TemplateProperty, new NoDrawnDecorations()) },
        };
        AutomationProperties.SetName(this, $"{panel.Title} panel");
        ApplyLook(palette);
        Opened += (_, _) => MakeToolWindow();
        Closing += (_, e) =>
        {
            if (e.CloseReason != WindowCloseReason.WindowClosing) return;
            // The close button and Alt+F4 hide the panel and clear its toggle.
            e.Cancel = true;
            HideRequested?.Invoke(this);
        };
    }

    /// Light and dark differ only by the palette, so a theme change rebuilds the contents.
    public void ApplyLook(Palette p)
    {
        Background = Ui.B(p.Panel);
        var row = new Grid { Name = "PanelTitleRow", Height = Tokens.PanelTitleHeight, Background = Ui.B(p.TitleBar), VerticalAlignment = VerticalAlignment.Top };
        WindowDecorationProperties.SetElementRole(row, WindowDecorationsElementRole.TitleBar);
        row.Children.Add(new TextBlock
        {
            Text = Panel.Title,
            FontSize = Tokens.FontSmall,
            Foreground = Ui.B(p.Muted),
            HorizontalAlignment = HorizontalAlignment.Center,
            VerticalAlignment = VerticalAlignment.Center,
            IsHitTestVisible = false,
        });
        CloseButton = new IconCommand(p, "window-close", $"Close {Panel.Title}", target: Tokens.PanelTitleHeight, iconSize: 12)
        {
            HorizontalAlignment = HorizontalAlignment.Right,
            VerticalAlignment = VerticalAlignment.Center,
            Focusable = false,
        };
        WindowDecorationProperties.SetElementRole(CloseButton, WindowDecorationsElementRole.User);
        CloseButton.Click += (_, _) => HideRequested?.Invoke(this);
        row.Children.Add(CloseButton);
        row.Children.Add(new Border { Height = 1, Background = Ui.B(p.PanelEdge), VerticalAlignment = VerticalAlignment.Bottom, IsHitTestVisible = false });
        TitleRow = row;

        var root = new Grid { RowDefinitions = new RowDefinitions("Auto,*") };
        root.Children.Add(row);
        Content = new Grid
        {
            Children =
            {
                root,
                new Border { BorderBrush = Ui.B(p.PanelEdge), BorderThickness = new Thickness(1), IsHitTestVisible = false },
            },
        };
    }

    /// Avalonia's MinHeight counts a title bar this window has turned off, so Colors could
    /// not be its 246 pixels. Here the limits are the client sizes the Mac names.
    unsafe nint LimitSize(nint hwnd, uint message, nint w, nint l, ref bool handled)
    {
        if (message != WmGetMinMaxInfo) return 0;
        if (!GetWindowRect(hwnd, out var rect)) throw new InvalidOperationException($"PANEL_SIZE: {Panel.Title}: {Marshal.GetLastPInvokeError()}");
        var scale = RenderScaling;
        // The system's invisible resize edges count in the window's size and not in the client's.
        var frameX = rect.Right - rect.Left - (int)Math.Round(ClientSize.Width * scale);
        var frameY = rect.Bottom - rect.Top - (int)Math.Round(ClientSize.Height * scale);
        var info = (MinMaxInfo*)l;
        info->MinTrack.X = (int)Math.Round(MinimumClient.Width * scale) + frameX;
        info->MinTrack.Y = (int)Math.Round(MinimumClient.Height * scale) + frameY;
        if (MaximumClientWidth is { } width) info->MaxTrack.X = (int)Math.Round(width * scale) + frameX;
        if (MaximumClientHeight is { } height) info->MaxTrack.Y = (int)Math.Round(height * scale) + frameY;
        handled = true;
        return 0;
    }

    /// On a real window only. Headless windows have no handle of this kind.
    void MakeToolWindow()
    {
        if (TryGetPlatformHandle() is not { HandleDescriptor: "HWND" } handle) return;
        Win32Properties.AddWndProcHookCallback(this, LimitSize);
        var style = (long)GetWindowLongPtr(handle.Handle, GwlExStyle);
        style = (style | WsExToolWindow) & ~WsExAppWindow;
        Marshal.SetLastPInvokeError(0);
        if (SetWindowLongPtr(handle.Handle, GwlExStyle, (nint)style) == 0 && Marshal.GetLastPInvokeError() != 0)
            throw new InvalidOperationException($"PANEL_STYLE: {Panel.Title}: {Marshal.GetLastPInvokeError()}");
        var round = DwmCornerRound;
        var status = DwmSetWindowAttribute(handle.Handle, DwmCornerPreference, ref round, sizeof(int));
        // Windows 10 has no such attribute, and there its square corners are right.
        if (status != 0 && OperatingSystem.IsWindowsVersionAtLeast(10, 0, 22000))
            throw new InvalidOperationException($"PANEL_CORNER: {Panel.Title}: 0x{status:x8}");
    }
}

/// Which panels show and where: PanelManager.swift, without saving a layout, which comes with settings.
public sealed class PanelManager
{
    readonly Window _owner;
    readonly Dictionary<string, PanelWindow> _panels = [];
    readonly HashSet<string> _shown = [];
    readonly Func<Rect> _stageOnScreen;

    /// A panel showed, hid, moved or resized.
    public event Action? Changed;

    public PanelManager(Window owner, IReadOnlyList<CatalogPanel> panels, Palette palette, Func<Rect> stageOnScreen)
    {
        _owner = owner;
        _stageOnScreen = stageOnScreen;
        foreach (var panel in panels)
        {
            var window = new PanelWindow(panel, palette);
            window.HideRequested += p => SetShown(p.Panel.Id, false);
            window.PositionChanged += (_, _) => Changed?.Invoke();
            window.Resized += (_, _) => Changed?.Invoke();
            _panels[panel.Id] = window;
            _shown.Add(panel.Id);
        }
    }

    public IReadOnlyDictionary<string, PanelWindow> Panels => _panels;
    public bool IsShown(string id) => _shown.Contains(id);
    public bool AnyActive => _panels.Values.Any(p => p.IsActive);

    public void Toggle(string id) => SetShown(id, !IsShown(id));

    public void SetShown(string id, bool on)
    {
        if (on) _shown.Add(id);
        else _shown.Remove(id);
        Apply(id);
        Changed?.Invoke();
    }

    void Apply(string id)
    {
        var panel = _panels[id];
        if (!IsShown(id)) panel.Hide();
        else if (!panel.IsVisible) panel.Show(_owner);
    }

    /// Opens the shown panels. The main window must be open, because it owns them.
    public void ShowAll()
    {
        foreach (var id in _panels.Keys) Apply(id);
        Place();
        Changed?.Invoke();
    }

    /// Puts the open panels at their default places. A window's position counts its invisible
    /// resize edges, so this moves each panel by what it shows, which needs it open.
    void Place()
    {
        var scale = _owner.RenderScaling;
        foreach (var (id, frame) in PanelLayout.Defaults(_stageOnScreen()))
        {
            if (!_panels.TryGetValue(id, out var panel)) throw new InvalidDataException($"CATALOG: no panel named {id}");
            panel.Width = frame.Width;
            panel.Height = frame.Height;
            if (!panel.IsVisible) continue;
            var edge = panel.PointToScreen(default) - panel.Position;
            panel.Position = new PixelPoint((int)Math.Round(frame.X * scale) - edge.X, (int)Math.Round(frame.Y * scale) - edge.Y);
        }
    }

    /// Window > Reset Panel Layout: default places, all panels shown.
    public void ResetLayout()
    {
        foreach (var id in _panels.Keys) _shown.Add(id);
        ShowAll();
    }

    /// What the shown panels cover, in screen layout points.
    public IEnumerable<Rect> ShownFrames()
    {
        var scale = _owner.RenderScaling;
        foreach (var (id, panel) in _panels)
        {
            if (!IsShown(id) || !panel.IsVisible) continue;
            var at = panel.PointToScreen(default);
            yield return new Rect(at.X / scale, at.Y / scale, panel.ClientSize.Width, panel.ClientSize.Height);
        }
    }

    public void ApplyLook(Palette palette, Avalonia.Styling.ThemeVariant? theme)
    {
        foreach (var panel in _panels.Values)
        {
            panel.RequestedThemeVariant = theme;
            panel.ApplyLook(palette);
        }
    }
}
