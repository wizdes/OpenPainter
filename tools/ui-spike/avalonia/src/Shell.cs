using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Text.Json;
using System.Text.Json.Serialization;
using Avalonia;
using Avalonia.Automation;
using Avalonia.Controls;
using Avalonia.Input;
using Avalonia.Interactivity;
using Avalonia.Platform;
using Avalonia.Threading;

namespace SpikeAvalonia;

public sealed class CanvasHost : NativeControlHost {
    const string ClassName = "OpenPainterW5Canvas";
    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    struct WindowClass {
        public uint Size, Style; public nint Proc; public int ClassExtra, WindowExtra;
        public nint Instance, Icon, Cursor, Background; public string? Menu; public string Name; public nint SmallIcon;
    }
    [DllImport("user32.dll", CharSet=CharSet.Unicode, SetLastError=true)] static extern ushort RegisterClassExW(ref WindowClass value);
    [DllImport("user32.dll", CharSet=CharSet.Unicode, SetLastError=true)] static extern nint CreateWindowExW(uint ex, string cls, string title, uint style, int x, int y, int w, int h, nint parent, nint menu, nint instance, nint param);
    [DllImport("user32.dll", SetLastError=true)] static extern bool DestroyWindow(nint hwnd);
    [DllImport("user32.dll")] static extern nint DefWindowProcW(nint hwnd, uint message, nuint w, nint l);
    [DllImport("user32.dll")] static extern nint SetFocus(nint hwnd);
    [DllImport("user32.dll")] static extern nint GetAncestor(nint hwnd, uint flags);
    [DllImport("user32.dll")] static extern bool GetPointerType(uint id, out int type);
    [DllImport("user32.dll", SetLastError=true)] static extern bool GetPointerPenInfo(uint id, byte[] info);
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode)] static extern nint GetModuleHandleW(string? name);
    delegate nint WindowProc(nint hwnd, uint message, nuint w, nint l);
    static WindowProc? callback;
    static CanvasHost? current;
    public event Action<string>? NativeInput;
    public event Action? TabOut;
    public event Action<int>? PanTo;
    public nint Hwnd { get; private set; }
    public void TakeFocus() { if (Hwnd != 0) SetFocus(Hwnd); }
    protected override IPlatformHandle CreateNativeControlCore(IPlatformHandle parent) {
        current = this;
        var instance = GetModuleHandleW(null);
        if (callback is null) {
            callback = OnNativeMessage;
            var cls = new WindowClass { Size = (uint)Marshal.SizeOf<WindowClass>(), Proc = Marshal.GetFunctionPointerForDelegate(callback), Instance = instance, Name = ClassName };
            if (RegisterClassExW(ref cls) == 0) throw new InvalidOperationException($"CANVAS_CLASS: {Marshal.GetLastWin32Error()}");
        }
        Hwnd = CreateWindowExW(0, ClassName, "Canvas", 0x50010000, 0, 0, 100, 100, parent.Handle, 0, instance, 0);
        if (Hwnd == 0) throw new InvalidOperationException($"CANVAS_HWND: {Marshal.GetLastWin32Error()}");
        return new PlatformHandle(Hwnd, "HWND");
    }
    protected override void DestroyNativeControlCore(IPlatformHandle control) {
        if (!DestroyWindow(control.Handle)) throw new InvalidOperationException($"DESTROY_CANVAS: {Marshal.GetLastWin32Error()}");
        Hwnd = 0;
    }
    static void LogPen(string name, nuint w) {
        var id = (uint)((ulong)w & 0xffff);
        if (!GetPointerType(id, out var type) || type != 3) return;
        var info = new byte[136];
        if (!GetPointerPenInfo(id, info)) { current?.NativeInput?.Invoke($"pen:{name}:error:{Marshal.GetLastWin32Error()}"); return; }
        current?.NativeInput?.Invoke($"pen:{name}:{BitConverter.ToUInt32(info, 104) / 1024.0:F2}");
    }
    static nint OnNativeMessage(nint hwnd, uint message, nuint w, nint l) {
        var input = current?.NativeInput;
        switch (message) {
            case 0x0201: SetFocus(hwnd); input?.Invoke($"pointer:down:{(short)(l.ToInt64() & 65535)},{(short)((l.ToInt64() >> 16) & 65535)}:1.00:Mouse"); return 0;
            case 0x0200: input?.Invoke("pointer:move:1.00:Mouse"); current?.PanTo?.Invoke((short)(l.ToInt64() & 65535)); return 0;
            case 0x0202: input?.Invoke("pointer:up:1.00:Mouse"); return 0;
            case 0x020A: input?.Invoke($"wheel:{(short)((long)w >> 16)}"); return 0;
            case 0x0246: LogPen("down", w); break;
            case 0x0245: LogPen("move", w); break;
            case 0x0247: LogPen("up", w); break;
            case 0x0100:
                input?.Invoke($"key:{w}");
                // A native child keeps Tab unless it hands focus back to Avalonia.
                if (w == 9) { SetFocus(GetAncestor(hwnd, 2)); current?.TabOut?.Invoke(); return 0; }
                break;
            case 0x0102: if (w != 9) input?.Invoke($"text:{(char)w}"); return 0;
            case 0x0007: input?.Invoke("focus:canvas"); break;
            case 0x0008: input?.Invoke("blur:canvas"); break;
        }
        return DefWindowProcW(hwnd, message, w, l);
    }
}

public sealed class Shell : Window {
    readonly string? measurePath;
    readonly bool headless;
    readonly Stopwatch clock = Stopwatch.StartNew();
    readonly List<string> events = [];
    readonly CanvasHost? native;
    volatile bool ticking;
    Thread? overlayClockThread;
    nint engine;
    ulong doc, view;
    int ticks;
    Window? panelWindow;
    public Button MenuButton { get; } = new() { Content = "Test menu", Name = "TestMenu" };
    public Button FileMenuButton { get; } = new() { Content = "File", Name = "FileMenu", Focusable = false };
    public Button PanelButton { get; } = new() { Content = "Floating panel", Name = "FloatingPanel" };
    public Border CanvasInput { get; } = new() { Name = "Canvas", Focusable = true, Background = Avalonia.Media.Brushes.Transparent };
    public List<string> EventLog => events;
    public Shell(string? measurePath = null, bool headless = false) {
        this.measurePath = measurePath; this.headless = headless;
        Title = "OpenPainter Avalonia W5"; Width = 1050; Height = 750;
        AutomationProperties.SetName(MenuButton, "Test menu");
        AutomationProperties.SetName(PanelButton, "Floating panel button");
        AutomationProperties.SetName(CanvasInput, "Canvas");
        var root = new DockPanel(); Content = root;
        var top = new StackPanel { Orientation = Avalonia.Layout.Orientation.Horizontal };
        DockPanel.SetDock(top, Dock.Top); root.Children.Add(top);
        top.Children.Add(MenuButton); top.Children.Add(PanelButton);
        // A real pop-up menu, to prove it draws above the native canvas window.
        var fileMenu = FileMenuButton;
        AutomationProperties.SetName(fileMenu, "File menu");
        var openItem = new MenuItem { Header = "Open dialog" };
        openItem.Click += (_, _) => OpenDialog();
        var flyout = new MenuFlyout();
        flyout.Items.Add(openItem);
        flyout.Items.Add(new MenuItem { Header = "A second item over the canvas" });
        fileMenu.Flyout = flyout;
        top.Children.Add(fileMenu);
        if (Environment.GetEnvironmentVariable("SPIKE_SHOW_POPUPS") == "1")
            Opened += (_, _) => DispatcherTimer.RunOnce(() => { ShowPanel(); flyout.ShowAt(fileMenu); }, TimeSpan.FromSeconds(1));
        MenuButton.Click += (_, _) => OpenDialog();
        PanelButton.Click += (_, _) => ShowPanel();
        var canvasLayer = new Grid(); root.Children.Add(canvasLayer);
        if (!headless) {
            native = new CanvasHost();
            native.NativeInput += events.Add;
            if (Pan) native.PanTo += x => {
                if (view == 0) return;
                var moved = new ViewState { Width = Math.Max(1, native.Bounds.Width), Height = Math.Max(1, native.Bounds.Height), Scale = RenderScaling, Zoom = 1, X = x % 64, Y = 0 };
                Engine.Check(Engine.op_view_set(engine, view, ref moved), "VIEW_SET");
            };
            native.TabOut += () => { events.Add("tab:out"); Dispatcher.UIThread.Post(() => MenuButton.Focus(NavigationMethod.Tab)); };
            canvasLayer.Children.Add(native);
            CanvasInput.GotFocus += (_, e) => { if (e.NavigationMethod == NavigationMethod.Tab) native.TakeFocus(); };
        }
        canvasLayer.Children.Add(CanvasInput);
        CanvasInput.KeyDown += (_, e) => {
            events.Add($"key:{e.Key}");
            if (e.Key == Key.Tab) { MenuButton.Focus(NavigationMethod.Tab); e.Handled = true; }
        };
        CanvasInput.TextInput += (_, e) => events.Add($"text:{e.Text}");
        CanvasInput.PointerPressed += (_, e) => LogPointer("down", e);
        CanvasInput.PointerMoved += (_, e) => LogPointer("move", e);
        CanvasInput.PointerReleased += (_, e) => LogPointer("up", e);
        CanvasInput.PointerWheelChanged += (_, e) => events.Add($"wheel:{e.Delta.X},{e.Delta.Y}");
        CanvasInput.GotFocus += (_, _) => { events.Add("focus:canvas"); CanvasInput.BorderBrush = Avalonia.Media.Brushes.Blue; CanvasInput.BorderThickness = new Thickness(2); };
        CanvasInput.LostFocus += (_, _) => { events.Add("blur:canvas"); CanvasInput.BorderThickness = new Thickness(0); };
        Opened += (_, _) => { if (!headless) StartEngine(); };
        SizeChanged += (_, _) => UpdateView();
        Closed += (_, _) => StopEngine();
    }
    // Runs once per display frame, so the ants move at the rate the screen shows.
    void NextFrame() {
        if (!ticking) return;
        RequestAnimationFrame(_ => { Tick(); NextFrame(); });
    }
    void LogPointer(string name, PointerEventArgs e) {
        var p = e.GetCurrentPoint(CanvasInput);
        events.Add($"pointer:{name}:{p.Position.X:F1},{p.Position.Y:F1}:{p.Properties.Pressure:F2}:{e.Pointer.Type}");
    }
    public Window OpenDialog() {
        var dialog = new Window { Title = "Test dialog", Width = 300, Height = 160, WindowStartupLocation = WindowStartupLocation.CenterOwner };
        var box = new NumericUpDown { Name = "NumberBox", Value = 12, Minimum = 0, Maximum = 100, Increment = 1 };
        AutomationProperties.SetName(box, "Number box");
        var done = new Button { Content = "OK" };
        done.Click += (_, _) => dialog.Close();
        dialog.Content = new StackPanel { Margin = new Thickness(12), Children = { new TextBlock { Text = "Number" }, box, done } };
        dialog.Show(this); events.Add("dialog:open");
        return dialog;
    }
    void ShowPanel() {
        if (panelWindow is { IsVisible: true }) { panelWindow.Activate(); return; }
        panelWindow = new Window { Title = "Floating panel", Width = 240, Height = 120, Topmost = true, Content = new TextBlock { Text = "Drag this panel", Margin = new Thickness(14) } };
        panelWindow.Show(this); events.Add("panel:open");
    }
    void StartEngine() {
        if (native?.Hwnd is null or 0) throw new InvalidOperationException("CANVAS_HWND: missing native child");
        engine = Engine.op_engine_new(0, 0, 0);
        if (engine == 0) throw new InvalidOperationException("ENGINE_NEW: null");
        Engine.Check(Engine.op_doc_new(engine, 1600, 1000, 96, 0xffffffff, out doc), "DOC_NEW");
        Engine.Check(Engine.op_view_attach(engine, doc, native.Hwnd, out view), "VIEW_ATTACH");
        UpdateView(); ticking = true; NextFrame();
        SetOverlay();
        if (OverlayThread && !Pan && !CallbackOnly) StartOverlayThread();
    }
    void UpdateView() {
        if (view == 0 || native is null) return;
        var s = new ViewState { Width = Math.Max(1, native.Bounds.Width), Height = Math.Max(1, native.Bounds.Height), Scale = RenderScaling, Zoom = 1, X = 0, Y = 0 };
        Engine.Check(Engine.op_view_set(engine, view, ref s), "VIEW_SET");
    }
    static readonly bool MoveCanvas = Environment.GetEnvironmentVariable("SPIKE_CANVAS_MOVE") == "1";
    static readonly bool Pan = Environment.GetEnvironmentVariable("SPIKE_PAN") == "1";
    // SPIKE_CALLBACK_ONLY: the UI frame callback does almost nothing, to see its own rate.
    static readonly bool CallbackOnly = Environment.GetEnvironmentVariable("SPIKE_CALLBACK_ONLY") == "1";
    static readonly bool OverlaySet = Environment.GetEnvironmentVariable("SPIKE_OVERLAY_SET") == "1";
    unsafe void SetOverlay() {
        OverlayPoint* lasso = stackalloc OverlayPoint[200];
        for (int i = 0; i < 200; i++) {
            var a = 2 * Math.PI * i / 200;
            lasso[i] = new OverlayPoint(280 + 110 * Math.Cos(a) + 12 * Math.Sin(3 * a), 240 + 85 * Math.Sin(a));
        }
        OverlayPoint* crop = stackalloc OverlayPoint[4] {
            new(90, 70), new(620, 100), new(590, 490), new(60, 460)
        };
        OverlayItem* items = stackalloc OverlayItem[12];
        int count = 0;
        if (OverlaySet) {
            items[count++] = new OverlayItem { Kind=1, Flags=1, Points=lasso, PointCount=200 };
            items[count++] = new OverlayItem { Kind=3, X=420, Y=150, Width=180, Height=130 };
            foreach (var p in new[] { new OverlayPoint(100,80), new OverlayPoint(350,80), new OverlayPoint(600,80),
                new OverlayPoint(100,300), new OverlayPoint(600,300), new OverlayPoint(100,460), new OverlayPoint(600,460) })
                items[count++] = new OverlayItem { Kind=4, X=p.X, Y=p.Y };
        } else {
            items[count++] = new OverlayItem { Kind=2, X=80, Y=80, Width=400, Height=300 };
        }
        items[count++] = new OverlayItem { Kind=6, Points=crop, PointCount=4 };
        items[count++] = new OverlayItem { Kind=7, X=120, Y=130, Height=22 };
        Engine.Check(Engine.op_view_set_overlay(engine, view, items, (nuint)count), "VIEW_OVERLAY_SET");
    }
    // The ants clock runs off the UI thread: op_view_set_overlay only stores the newest state and wakes the render thread.
    static readonly bool OverlayThread = Environment.GetEnvironmentVariable("SPIKE_OVERLAY_THREAD") != "0";
    void StartOverlayThread() {
        overlayClockThread = new Thread(() => {
            var clock = Stopwatch.StartNew();
            long next = 0;
            while (ticking) {
                Engine.Check(Engine.op_view_set_overlay_dash(engine, view, (clock.Elapsed.TotalSeconds / 0.6 * 8) % 8), "VIEW_OVERLAY_DASH");
                next += 16;
                var wait = next - clock.ElapsedMilliseconds;
                if (wait > 0) Thread.Sleep((int)wait);
            }
        }) { IsBackground = true, Name = "ants" };
        overlayClockThread.Start();
    }
    void Tick() {
        if (view == 0 || native is null) return;
        if (MoveCanvas) {
            var moved = new ViewState { Width = Math.Max(1, native.Bounds.Width), Height = Math.Max(1, native.Bounds.Height), Scale = RenderScaling, Zoom = 1, X = ticks % 2, Y = 0 };
            Engine.Check(Engine.op_view_set(engine, view, ref moved), "VIEW_SET");
        }
        ticks++;
        if (CallbackOnly && ticks % 30 != 0) return;
        if (!OverlayThread && !Pan && !CallbackOnly) {
            Engine.Check(Engine.op_view_set_overlay_dash(engine, view, (clock.Elapsed.TotalSeconds / 0.6 * 8) % 8), "VIEW_OVERLAY_DASH");
        }
        if (measurePath is not null) Measure();
    }
    ulong firstFrames;
    double firstMs;
    long firstWorkingSet;
    bool resizing;
    double resizeStart;
    ulong resizeFrames;
    unsafe void Measure() {
        Engine.Check(Engine.op_view_info(engine, view, out var info), "VIEW_INFO");
        if (firstFrames == 0 && info.Frames > 0) { firstFrames = info.Frames; firstMs = Stopwatch.GetElapsedTime(Program.StartTimestamp).TotalMilliseconds; firstWorkingSet = Process.GetCurrentProcess().WorkingSet64; }
        if (firstFrames == 0) { if (clock.Elapsed.TotalSeconds > 5) throw new InvalidOperationException("FIRST_FRAME_TIMEOUT"); return; }
        if (clock.Elapsed.TotalSeconds >= 10 && !resizing) { resizing = true; resizeStart = clock.Elapsed.TotalSeconds; resizeFrames = info.Frames; }
        if (resizing && clock.Elapsed.TotalSeconds < resizeStart + 3) { Width = 1050 + (ticks % 8) * 10; UpdateView(); return; }
        if (!resizing || clock.Elapsed.TotalSeconds < resizeStart + 3) return;
        ticking = false;
        double[] times = new double[16384]; nuint length;
        fixed (double* p = times) Engine.Check(Engine.op_view_take_frame_times(engine, view, p, (nuint)times.Length, out length), "FRAME_TIMES");
        var result = new MeasureResult("Avalonia", firstMs, firstWorkingSet, times.AsSpan(0, (int)length).ToArray(), info.Frames - resizeFrames, ticks, events.ToArray());
        File.WriteAllText(measurePath!, JsonSerializer.Serialize(result, MeasureJson.Default.MeasureResult));
        Close();
    }
    void StopEngine() {
        ticking = false;
        if (overlayClockThread is not null && !overlayClockThread.Join(1000)) throw new InvalidOperationException("ANTS_THREAD_TIMEOUT");
        panelWindow?.Close();
        if (view != 0) { Engine.Check(Engine.op_view_detach(engine, view), "VIEW_DETACH"); view = 0; }
        if (engine != 0) { Engine.op_engine_free(engine); engine = 0; }
    }
}

public sealed record MeasureResult(string Stack, double FirstFrameMs, long FirstWorkingSet, double[] FrameTimesMs, ulong ResizeFrames, int Ticks, string[] InputEvents);
[JsonSerializable(typeof(MeasureResult))]
internal partial class MeasureJson : JsonSerializerContext { }
