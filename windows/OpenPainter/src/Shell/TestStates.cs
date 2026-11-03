// A test hook for a real window: --test-states <folder> steps through hover, press, focus and Alt,
// so the harness can look at each from outside. Each step posts mouse and key messages to the app's
// own windows, so the user's pointer and keyboard stay out of it. The app writes ready-<n>.json, and
// the harness answers with go-<n>.
using System.Globalization;
using System.Runtime.InteropServices;
using Avalonia;
using Avalonia.Controls;
using Avalonia.Input;
using Avalonia.Threading;

namespace OpenPainter.Shell;

public static partial class TestStates
{
    const int PollMs = 20, LimitMs = 120_000, SettleMs = 300;
    const uint WmKeyDown = 0x0100, WmSysKeyDown = 0x0104, WmSysKeyUp = 0x0105, WmMouseMove = 0x0200, WmLeftDown = 0x0201, WmLeftUp = 0x0202, WmMouseLeave = 0x02A3;
    const nint MkLeft = 1;
    const int VkMenu = 0x12;

    [LibraryImport("user32.dll", EntryPoint = "PostMessageW", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static partial bool PostMessage(nint hwnd, uint message, nint w, nint l);

    public static void Arrange(MainWindow window, StartOptions options, Action<int> setExitCode)
    {
        if (options.TestStatesFolder is not { } folder) return;
        window.ImageReady += () => _ = Run(window, folder, setExitCode);
    }

    static nint Handle(Window window) => window.TryGetPlatformHandle()?.Handle ?? throw new InvalidOperationException("TEST_STATES: no window handle");

    static void Post(nint hwnd, uint message, nint w, nint l)
    {
        if (!PostMessage(hwnd, message, w, l)) throw new InvalidOperationException($"TEST_STATES: PostMessage: {Marshal.GetLastPInvokeError()}");
    }

    /// A window message's point: client pixels, y in the high word.
    static nint Point(Control control, Window window)
    {
        var at = control.TranslatePoint(new Point(control.Bounds.Width / 2, control.Bounds.Height / 2), window) ?? throw new InvalidOperationException("TEST_STATES: the control is not in the window");
        var (x, y) = ((int)(at.X * window.RenderScaling), (int)(at.Y * window.RenderScaling));
        return (nint)(((long)(ushort)(short)y << 16) | (ushort)(short)x);
    }

    static void Ready(string folder, int stage, string facts)
    {
        var path = Path.Combine(folder, $"ready-{stage}.json");
        File.WriteAllText(path + ".part", $"{{ {facts} }}\n");
        File.Move(path + ".part", path);
    }

    static async Task WaitGo(string folder, int stage)
    {
        var path = Path.Combine(folder, $"go-{stage}");
        var waited = 0;
        while (!File.Exists(path))
        {
            await Task.Delay(PollMs);
            waited += PollMs;
            if (waited > LimitMs) throw new TimeoutException($"no {path} in {LimitMs} ms");
        }
    }

    static string Bool(bool value) => value ? "true" : "false";

    /// A step: post, settle, tell the harness, wait for its shots.
    static async Task Step(string folder, int stage, Func<string> facts)
    {
        await Task.Delay(SettleMs);
        Ready(folder, stage, facts());
        await WaitGo(folder, stage);
    }

    /// Windows sends a leave message at once when the real pointer is elsewhere, and that would
    /// end a posted hover. This hook holds the leave back, and only in this test run.
    static bool s_holdLeave = true;

    static nint HoldLeave(nint hwnd, uint message, nint w, nint l, ref bool handled)
    {
        if (message == WmMouseLeave && s_holdLeave) handled = true;
        return 0;
    }

    static async Task Settled(Editor editor)
    {
        var ticket = editor.Client.Dispatch(editor.Document, "doc.sync", Bridge.EngineParams.None);
        if (await Task.Run(() => editor.Client.Wait(ticket, LimitMs)) is null) throw new TimeoutException("doc.sync did not end");
        await Task.Delay(PollMs);
        editor.Refresh();
        await Task.Delay(SettleMs);
    }

    static async Task Run(MainWindow window, string folder, Action<int> setExitCode)
    {
        try
        {
            await window.CanvasShown;
            var editor = window.Editor ?? throw new InvalidOperationException("TEST_STATES: no document");
            Directory.CreateDirectory(folder);
            editor.Run("layer.duplicate");
            await Settled(editor);
            var colorsWindow = window.Panels.Panels[PanelLayout.Colors];
            var layersWindow = window.Panels.Panels[PanelLayout.Layers];
            var toolsWindow = window.Panels.Panels[PanelLayout.Tools];
            foreach (var panel in new[] { colorsWindow, layersWindow, toolsWindow }) Win32Properties.AddWndProcHookCallback(panel, HoldLeave);
            var colors = window.Panels.Colors;
            var layers = window.Panels.Layers;
            var tools = window.Panels.Tools;
            var hwndColors = Handle(colorsWindow);
            var hwndLayers = Handle(layersWindow);
            var hwndTools = Handle(toolsWindow);
            string Hwnds() => $"\"main_hwnd\": {Handle(window)}, \"colors_hwnd\": {hwndColors}, \"layers_hwnd\": {hwndLayers}, \"tools_hwnd\": {hwndTools}, \"palette\": \"{window.Palette.Name}\"";

            // Stage 1: at rest.
            await Step(folder, 1, Hwnds);

            // Stage 2: hover, by a message to each panel. A tool cell, a layer row and the wheel.
            var cell = tools.Cells["eraser"];
            var row = layers.LayerRows[^1];
            var wheel = colors.Wheel;
            Post(hwndTools, WmMouseMove, 0, Point(cell, toolsWindow));
            Post(hwndLayers, WmMouseMove, 0, Point(row, layersWindow));
            Post(hwndColors, WmMouseMove, 0, Point(wheel, colorsWindow));
            await Step(folder, 2, () => $"{Hwnds()}, \"cell_hover\": {Bool(cell.IsPointerOver)}, \"row_hover\": {Bool(row.IsPointerOver)}, \"wheel_hover\": {Bool(wheel.IsPointerOver)}");

            // Stage 3: press, on a tool cell and a layer row. The release comes off the control.
            Post(hwndTools, WmLeftDown, MkLeft, Point(cell, toolsWindow));
            await Step(folder, 3, () => $"{Hwnds()}, \"cell_pressed\": {Bool(cell.IsPressed)}");
            Post(hwndTools, WmLeftUp, 0, unchecked((nint)0x7FFF7FFF));
            await Task.Delay(SettleMs);
            Post(hwndLayers, WmLeftDown, MkLeft, Point(row, layersWindow));
            await Step(folder, 9, () => $"{Hwnds()}, \"row_held\": {Bool(row.Held)}");
            Post(hwndLayers, WmLeftUp, 0, unchecked((nint)0x7FFF7FFF));
            s_holdLeave = false;
            Post(hwndTools, WmMouseLeave, 0, 0);
            Post(hwndLayers, WmMouseLeave, 0, 0);
            Post(hwndColors, WmMouseLeave, 0, 0);
            await Task.Delay(SettleMs);

            // Stages 4 to 6: keyboard focus. One element holds it at a time, so one step each.
            var focusRow = layers.LayerRows[0];
            focusRow.Focus(NavigationMethod.Tab);
            await Step(folder, 4, () => $"{Hwnds()}, \"focused\": {Bool(focusRow.IsFocused)}");
            var focusCell = tools.Cells["pencil"];
            focusCell.Focus(NavigationMethod.Tab);
            await Step(folder, 5, () => $"{Hwnds()}, \"focused\": {Bool(focusCell.IsFocused)}");
            colors.HexField.Focus(NavigationMethod.Tab);
            await Step(folder, 6, () => $"{Hwnds()}, \"focused\": {Bool(colors.HexField.IsFocused)}");

            // Stage 7: Alt-click on the wheel, by messages, must leave the menus shut and still pick.
            var before = window.Tools.Primary;
            Post(hwndColors, WmSysKeyDown, VkMenu, 0);
            await Task.Delay(SettleMs);
            Post(hwndColors, WmMouseMove, 0, Point(wheel, colorsWindow));
            Post(hwndColors, WmLeftDown, MkLeft, Point(wheel, colorsWindow));
            Post(hwndColors, WmLeftUp, 0, Point(wheel, colorsWindow));
            await Task.Delay(SettleMs);
            Post(hwndColors, WmSysKeyUp, VkMenu, 0);
            await Step(folder, 7, () => $"{Hwnds()}, \"menu_open\": {Bool(window.TitleBar.Menus.IsOpen)}, \"any_submenu\": {Bool(window.TitleBar.Menus.Items.OfType<MenuItem>().Any(m => m.IsSubMenuOpen))}, \"color_changed\": {Bool(window.Tools.Primary != before)}");

            // Stage 8: a clean Alt tap on the Colors panel opens the menu bar on the main window.
            Post(hwndColors, WmSysKeyDown, VkMenu, 0);
            await Task.Delay(SettleMs);
            Post(hwndColors, WmSysKeyUp, VkMenu, 0);
            await Step(folder, 8, () => $"{Hwnds()}, \"menu_open\": {Bool(window.TitleBar.Menus.IsOpen)}");
            window.TitleBar.Menus.Close();
            await Task.Delay(SettleMs);

            // Stage 10: the native canvas. A letter picks a tool, Alt-click leaves the menus shut,
            // and a clean Alt tap opens them.
            var canvas = window.Native?.Hwnd ?? throw new InvalidOperationException("TEST_STATES: no canvas");
            Post(canvas, WmKeyDown, 'E', 0);
            await Task.Delay(SettleMs);
            var picked = window.Tools.Current == "eraser";
            Post(canvas, WmKeyDown, 'X', 0);
            await Task.Delay(SettleMs);
            var swapped = window.Tools.Primary == before || window.Tools.Secondary != before;
            Post(canvas, WmSysKeyDown, VkMenu, 0);
            await Task.Delay(SettleMs);
            Post(canvas, WmLeftDown, MkLeft, 0);
            Post(canvas, WmLeftUp, 0, 0);
            await Task.Delay(SettleMs);
            Post(canvas, WmSysKeyUp, VkMenu, 0);
            await Task.Delay(SettleMs);
            var openAfterClick = window.TitleBar.Menus.IsOpen;
            Post(canvas, WmSysKeyDown, VkMenu, 0);
            await Task.Delay(SettleMs);
            Post(canvas, WmSysKeyUp, VkMenu, 0);
            await Step(folder, 10, () => $"{Hwnds()}, \"picked\": {Bool(picked)}, \"swapped\": {Bool(swapped)}, \"open_after_click\": {Bool(openAfterClick)}, \"open_after_tap\": {Bool(window.TitleBar.Menus.IsOpen)}");
            window.TitleBar.Menus.Close();
        }
        catch (Exception e)
        {
            // A failed hook must end the run with its name, not leave a window waiting.
            Console.Error.WriteLine($"TEST_STATES: {e.Message}");
            setExitCode(1);
            Dispatcher.UIThread.Post(window.Close);
        }
    }
}
