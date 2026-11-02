using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Text.Json;
using System.Text.Json.Serialization;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Input;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Shapes;

namespace SpikeWinUI;

public sealed class Shell : Window {
    readonly string? measurePath;
    readonly Stopwatch clock = Stopwatch.StartNew();
    readonly List<string> events = [];
    readonly SwapChainPanel panel = new();
    readonly Rectangle ants = new() { Width = 400, Height = 300, Stroke = new SolidColorBrush(Microsoft.UI.Colors.Black), StrokeThickness = 2, StrokeDashArray = new DoubleCollection { 4, 4 } };
    readonly Rectangle crop = new() { Width = 650, Height = 450, Stroke = new SolidColorBrush(Microsoft.UI.Colors.Black), StrokeThickness = 40, Opacity = .55 };
    readonly Rectangle caret = new() { Width = 2, Height = 22, Fill = new SolidColorBrush(Microsoft.UI.Colors.Blue) };
    bool ticking;
    nint engine, panelPtr;
    ulong doc, view;
    int ticks;
    double firstMs, resizeStart;
    long firstWorkingSet;
    ulong firstFrames, resizeFrames;
    bool resizing;
    public Shell(string? measurePath) {
        this.measurePath = measurePath;
        Title = "OpenPainter WinUI W5";
        var root = new Grid(); root.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto }); root.RowDefinitions.Add(new RowDefinition()); Content = root;
        var menu = new MenuBar(); AutomationProperties.SetName(menu, "Menu bar");
        var item = new MenuBarItem { Title = "Test menu" }; AutomationProperties.SetName(item, "Test menu");
        var open = new MenuFlyoutItem { Text = "Open dialog" }; open.Click += async (_, _) => await OpenDialog(); item.Items.Add(open); menu.Items.Add(item);
        root.Children.Add(menu);
        var surface = new Grid(); Grid.SetRow(surface, 1); root.Children.Add(surface);
        AutomationProperties.SetName(panel, "Engine canvas"); surface.Children.Add(panel);
        var overlays = new Canvas { IsHitTestVisible = false }; surface.Children.Add(overlays);
        Canvas.SetLeft(crop, 40); Canvas.SetTop(crop, 40); overlays.Children.Add(crop);
        Canvas.SetLeft(ants, 80); Canvas.SetTop(ants, 80); overlays.Children.Add(ants);
        Canvas.SetLeft(caret, 120); Canvas.SetTop(caret, 130); overlays.Children.Add(caret);
        var input = new UserControl { Background = new SolidColorBrush(Microsoft.UI.Colors.Transparent), BorderThickness = new Thickness(0), IsTabStop = true, UseSystemFocusVisuals = true, Content = new Grid { Background = new SolidColorBrush(Microsoft.UI.Colors.Transparent) } };
        AutomationProperties.SetName(input, "Canvas"); surface.Children.Add(input);
        // Handled, or XAML moves focus away again when the pointer goes up.
        input.PointerPressed += (_, e) => { input.Focus(FocusState.Pointer); LogPointer("down", e); e.Handled = true; };
        input.PointerMoved += (_, e) => LogPointer("move", e);
        input.PointerReleased += (_, e) => { LogPointer("up", e); e.Handled = true; };
        input.PointerWheelChanged += (_, e) => events.Add($"wheel:{e.GetCurrentPoint(input).Properties.MouseWheelDelta}");
        input.KeyDown += (_, e) => events.Add($"key:{e.Key}");
        input.CharacterReceived += (_, e) => events.Add($"text:{(char)e.Character}");
        input.GotFocus += (_, _) => { events.Add("focus:canvas"); input.BorderThickness = new Thickness(2); };
        input.LostFocus += (_, _) => { events.Add("blur:canvas"); input.BorderThickness = new Thickness(0); };
        var floating = new Border { Width = 220, Height = 90, Background = new SolidColorBrush(Microsoft.UI.Colors.LightGray), Child = new TextBlock { Text = "Floating panel", Margin = new Thickness(12) }, HorizontalAlignment = HorizontalAlignment.Right, VerticalAlignment = VerticalAlignment.Top, Margin = new Thickness(12) };
        AutomationProperties.SetName(floating, "Floating panel"); surface.Children.Add(floating);
        floating.PointerPressed += (_, e) => { events.Add("panel:drag"); floating.Translation += new System.Numerics.Vector3(10, 10, 0); };
        surface.SizeChanged += (_, _) => UpdateView();
        panel.Loaded += (_, _) => StartEngine();
        Closed += (_, _) => StopEngine();
        // Runs once per display frame, so the ants move at the rate the screen shows.
        CompositionTarget.Rendering += (_, _) => { if (ticking) Tick(); };
    }
    void LogPointer(string name, PointerRoutedEventArgs e) {
        var p = e.GetCurrentPoint(panel);
        events.Add($"pointer:{name}:{p.Position.X:F1},{p.Position.Y:F1}:{p.Properties.Pressure:F2}:{e.Pointer.PointerDeviceType}");
    }
    async Task OpenDialog() {
        var number = new NumberBox { Header = "Number", Value = 12, Minimum = 0, Maximum = 100 };
        AutomationProperties.SetName(number, "Number box");
        var dialog = new ContentDialog { Title = "Test dialog", Content = number, CloseButtonText = "OK", XamlRoot = ((FrameworkElement)Content).XamlRoot };
        AutomationProperties.SetName(dialog, "Test dialog");
        events.Add("dialog:open"); await dialog.ShowAsync();
    }
    void StartEngine() {
        if (engine != 0) return;
        engine = Engine.op_engine_new(0, 0, 0);
        if (engine == 0) throw new InvalidOperationException("ENGINE_NEW: null");
        Engine.Check(Engine.op_doc_new(engine, 1600, 1000, 96, 0xffffffff, out doc), "DOC_NEW");
        // wgpu calls SetSwapChain on this pointer directly, so it must be ISwapChainPanelNative, not IInspectable.
        var inspectable = WinRT.MarshalInspectable<SwapChainPanel>.FromManaged(panel);
        var nativeIid = new Guid("63AAD0B8-7C24-40FF-85A8-640D944CC325");
        var queried = Marshal.QueryInterface(inspectable, in nativeIid, out panelPtr);
        Marshal.Release(inspectable);
        Marshal.ThrowExceptionForHR(queried);
        Engine.Check(Engine.op_view_attach_panel(engine, doc, PanelBridge.Create(panelPtr, DispatcherQueue), out view), "VIEW_ATTACH_PANEL");
        UpdateView(); ticking = true;
    }
    void UpdateView() {
        if (view == 0) return;
        var s = new ViewState { Width = Math.Max(1, panel.ActualWidth), Height = Math.Max(1, panel.ActualHeight), Scale = panel.XamlRoot?.RasterizationScale ?? 1, Zoom = 1 };
        Engine.Check(Engine.op_view_set(engine, view, ref s), "VIEW_SET");
    }
    static readonly bool MoveCanvas = Environment.GetEnvironmentVariable("SPIKE_CANVAS_MOVE") == "1";
    void Tick() {
        ants.StrokeDashOffset = ticks++ % 8;
        if (MoveCanvas) {
            var moved = new ViewState { Width = Math.Max(1, panel.ActualWidth), Height = Math.Max(1, panel.ActualHeight), Scale = panel.XamlRoot?.RasterizationScale ?? 1, Zoom = 1, X = ticks % 2, Y = 0 };
            Engine.Check(Engine.op_view_set(engine, view, ref moved), "VIEW_SET");
        }
        if (measurePath is not null) Measure();
    }
    unsafe void Measure() {
        Engine.Check(Engine.op_view_info(engine, view, out var info), "VIEW_INFO");
        if (firstFrames == 0 && info.Frames > 0) { firstFrames = info.Frames; firstMs = Stopwatch.GetElapsedTime(Program.StartTimestamp).TotalMilliseconds; firstWorkingSet = Process.GetCurrentProcess().WorkingSet64; }
        if (firstFrames == 0) { if (clock.Elapsed.TotalSeconds > 5) throw new InvalidOperationException("FIRST_FRAME_TIMEOUT"); return; }
        if (clock.Elapsed.TotalSeconds >= 10 && !resizing) { resizing = true; resizeStart = clock.Elapsed.TotalSeconds; resizeFrames = info.Frames; }
        if (resizing && clock.Elapsed.TotalSeconds < resizeStart + 3) { AppWindow.Resize(new Windows.Graphics.SizeInt32(1050 + ticks % 8 * 10, 750)); UpdateView(); return; }
        if (!resizing || clock.Elapsed.TotalSeconds < resizeStart + 3) return;
        ticking = false; double[] times = new double[16384]; nuint count;
        fixed (double* p = times) Engine.Check(Engine.op_view_take_frame_times(engine, view, p, (nuint)times.Length, out count), "FRAME_TIMES");
        var result = new MeasureResult("WinUI", firstMs, firstWorkingSet, times.AsSpan(0, (int)count).ToArray(), info.Frames - resizeFrames, ticks, events.ToArray());
        File.WriteAllText(measurePath!, JsonSerializer.Serialize(result, MeasureJson.Default.MeasureResult));
        Close();
    }
    void StopEngine() {
        ticking = false;
        PanelBridge.Clear();
        if (view != 0) { Engine.Check(Engine.op_view_detach(engine, view), "VIEW_DETACH"); view = 0; }
        if (panelPtr != 0) { Marshal.Release(panelPtr); panelPtr = 0; }
        if (engine != 0) { Engine.op_engine_free(engine); engine = 0; }
    }
}
internal sealed record MeasureResult(string Stack, double FirstFrameMs, long FirstWorkingSet, double[] FrameTimesMs, ulong ResizeFrames, int XamlAnimationTicks, string[] InputEvents);
[JsonSerializable(typeof(MeasureResult))]
internal partial class MeasureJson : JsonSerializerContext { }
