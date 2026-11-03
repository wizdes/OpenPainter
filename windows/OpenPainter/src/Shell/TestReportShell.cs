// More of the real-window test hook: stage 2's shell. It asks Windows about the panels and
// sends the canvas the same messages Windows would send, so the user's pointer does not
// move. Headless tests cannot see owned windows, the native canvas or the screen.
using System.Runtime.InteropServices;
using System.Text;
using Avalonia;
using Avalonia.Controls;
using Avalonia.Media;
using Avalonia.Threading;
using Avalonia.VisualTree;
using OpenPainter.Canvas;
using UiLook;

namespace OpenPainter.Shell;

public sealed partial class TestReport
{
    const uint GwOwner = 4, GwHwndNext = 2;
    const int GwlExStyle = -20;
    const long WsExToolWindow = 0x80;
    const uint WmClose = 0x0010, WmMouseMove = 0x0200, WmLeftDown = 0x0201, WmLeftUp = 0x0202, WmRightDown = 0x0204, WmRightUp = 0x0205, WmMouseWheel = 0x020A, WmKeyDown = 0x0100, WmKeyUp = 0x0101;
    const uint SwpNoSize = 1, SwpNoZOrder = 4, SwpNoActivate = 0x10;
    const int MkControl = 8, MkLeft = 1, WheelNotch = 120, VkSpace = 0x20, VkF8 = 0x77;

    [StructLayout(LayoutKind.Sequential)] struct NativeRect { public int Left, Top, Right, Bottom; }

    [LibraryImport("user32.dll")] private static partial nint GetWindow(nint hwnd, uint command);
    [LibraryImport("user32.dll", EntryPoint = "GetWindowLongPtrW")] private static partial nint GetWindowLongPtr(nint hwnd, int index);
    [LibraryImport("user32.dll")] private static partial nint WindowFromPoint(NativePoint point);
    [LibraryImport("user32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static partial bool GetWindowRect(nint hwnd, out NativeRect rect);
    [LibraryImport("user32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static partial bool IsWindowVisible(nint hwnd);
    [LibraryImport("user32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static partial bool SetWindowPos(nint hwnd, nint after, int x, int y, int cx, int cy, uint flags);
    [LibraryImport("user32.dll")] private static partial nint GetDC(nint hwnd);
    [LibraryImport("user32.dll")] private static partial int ReleaseDC(nint hwnd, nint dc);
    [LibraryImport("gdi32.dll")] private static partial uint GetPixel(nint dc, int x, int y);

    static nint HandleOf(TopLevel window) =>
        window.TryGetPlatformHandle()?.Handle ?? throw new InvalidOperationException("TEST_REPORT: a window has no handle");

    static bool IsAbove(nint window, nint below)
    {
        for (var at = GetWindow(window, GwHwndNext); at != 0; at = GetWindow(at, GwHwndNext))
            if (at == below) return true;
        return false;
    }

    static Color ScreenPixel(PixelPoint at)
    {
        var dc = GetDC(0);
        if (dc == 0) throw new InvalidOperationException("TEST_REPORT: no screen device");
        var value = GetPixel(dc, at.X, at.Y);
        ReleaseDC(0, dc);
        return Color.FromRgb((byte)value, (byte)(value >> 8), (byte)(value >> 16));
    }

    /// What the screen really shows at a point. Another program on top is not a fault of
    /// ours, so it is written down and the screen-side checks say NOT RUN.
    static string Top(PixelPoint at, nint expected, nint main, string name, List<string> problems)
    {
        var under = WindowFromPoint(new NativePoint { X = at.X, Y = at.Y });
        if (under == expected) return "on top";
        if (under == main || IsChild(main, under))
        {
            problems.Add($"{name}: the main window or canvas answers at {at}, not the window that must be over it");
            return "under the canvas";
        }
        return $"NOT RUN: class {ClassOf(under)} covers it";
    }

    async Task CheckPanels(nint main, List<string> problems, StringBuilder text)
    {
        var scale = _window.RenderScaling;
        var manager = _window.Panels;
        var native = _window.Native;
        text.Append("  \"panels\": {\n");
        var rows = new List<string>();
        foreach (var (id, panel) in manager.Panels)
        {
            var hwnd = HandleOf(panel);
            if (!IsWindowVisible(hwnd)) problems.Add($"panel {id}: Windows says it is not visible");
            if (GetWindow(hwnd, GwOwner) != main) problems.Add($"panel {id}: it is not owned by the main window");
            if (((long)GetWindowLongPtr(hwnd, GwlExStyle) & WsExToolWindow) == 0) problems.Add($"panel {id}: it is not a tool window");
            if (!IsAbove(hwnd, main)) problems.Add($"panel {id}: it is below the main window in the stacking order");
            if (!GetWindowRect(hwnd, out var rect)) throw new InvalidOperationException($"TEST_REPORT: GetWindowRect {id}: {Marshal.GetLastPInvokeError()}");
            var size = PanelLayout.DefaultSize(id);
            var width = rect.Right - rect.Left;
            var height = rect.Bottom - rect.Top;
            if (panel.ClientSize != size) problems.Add($"panel {id}: it shows {panel.ClientSize}, expected {size}");
            var title = CenterOf(panel.TitleRow);
            var titleAnswer = Ask(hwnd, new PixelPoint(rect.Left + 8, title.Y));
            if (titleAnswer != HtCaption) problems.Add($"panel {id}: Windows says {titleAnswer} on its title row, expected {HtCaption}");
            var closeAnswer = Ask(hwnd, CenterOf(panel.CloseButton));
            if (closeAnswer != HtClient) problems.Add($"panel {id}: Windows says {closeAnswer} on its close button, expected {HtClient}");

            // The panel must really cover the native canvas.
            var body = new PixelPoint((rect.Left + rect.Right) / 2, (rect.Top + rect.Bottom) / 2);
            var overCanvas = false;
            if (native is not null && GetWindowRect(native.Hwnd, out var canvas))
                overCanvas = body.X >= canvas.Left && body.X < canvas.Right && body.Y >= canvas.Top && body.Y < canvas.Bottom;
            if (!overCanvas) problems.Add($"panel {id}: its middle is not over the canvas, so this run proves nothing about it");
            var top = Top(body, hwnd, main, $"panel {id}", problems);
            var pixel = "";
            if (top == "on top")
            {
                var seen = ScreenPixel(body);
                pixel = seen.ToString();
                if (seen != _window.Palette.Panel) problems.Add($"panel {id}: the screen shows {seen} in its middle, not the panel color {_window.Palette.Panel}");
            }
            var shown = panel.PointToScreen(default);
            rows.Add($"    \"{id}\": {{ \"rect\": [{rect.Left}, {rect.Top}, {width}, {height}], \"shown_at\": [{shown.X}, {shown.Y}], \"client\": \"{panel.ClientSize}\", \"over_canvas\": {(overCanvas ? "true" : "false")}, \"screen\": \"{top}\", \"pixel\": \"{pixel}\" }}");
        }
        text.Append(string.Join(",\n", rows)).Append("\n  },\n");

        // A panel's close button hides it, clears its toggle and gives the canvas the room back.
        var colors = manager.Panels[PanelLayout.Colors];
        var colorsHwnd = HandleOf(colors);
        var clearBefore = _window.View.View.ClearRect;
        var client = colors.CloseButton.TranslatePoint(new Point(colors.CloseButton.Bounds.Width / 2, colors.CloseButton.Bounds.Height / 2), colors)
            ?? throw new InvalidOperationException("TEST_REPORT: the close button is not in its panel");
        var at = PointLparam(client, scale);
        SendMessage(colorsHwnd, WmMouseMove, 0, at);
        Dispatcher.UIThread.RunJobs();
        SendMessage(colorsHwnd, WmLeftDown, MkLeft, at);
        SendMessage(colorsHwnd, WmLeftUp, 0, at);
        await Task.Delay(SettleMs);
        var hidden = !manager.IsShown(PanelLayout.Colors) && !IsWindowVisible(colorsHwnd);
        text.Append($"  \"panel_close_click_hides\": {(hidden ? "true" : "false")},\n");
        if (!hidden) problems.Add("panel close: clicking Colors' close button did not hide it");
        manager.SetShown(PanelLayout.Colors, true);
        await Task.Delay(SettleMs);
        if (!IsWindowVisible(HandleOf(colors))) problems.Add("panel show: Colors did not come back");

        // WM_CLOSE, as Alt+F4 sends, must hide it and not destroy it.
        SendMessage(HandleOf(colors), WmClose, 0, 0);
        await Task.Delay(SettleMs);
        if (manager.IsShown(PanelLayout.Colors)) problems.Add("panel Alt+F4: Colors is still shown");
        manager.SetShown(PanelLayout.Colors, true);
        await Task.Delay(SettleMs);
        if (colors.PlatformImpl is null) problems.Add("panel Alt+F4: Colors was destroyed");

        // Moving a panel away from the left side widens the clear rectangle.
        var moveFrom = colors.Position;
        var moveTo = new PixelPoint(moveFrom.X + 300, moveFrom.Y - 100);
        if (!SetWindowPos(HandleOf(colors), 0, moveTo.X, moveTo.Y, 0, 0, SwpNoSize | SwpNoZOrder | SwpNoActivate))
            throw new InvalidOperationException($"TEST_REPORT: SetWindowPos: {Marshal.GetLastPInvokeError()}");
        await Task.Delay(SettleMs);
        var clearMoved = _window.View.View.ClearRect;
        text.Append($"  \"clear_before_move\": \"{clearBefore}\",\n  \"clear_after_move\": \"{clearMoved}\",\n");
        if (clearBefore == clearMoved) problems.Add("panel move: the clear rectangle did not change");
        manager.ResetLayout();
        await Task.Delay(SettleMs);
        var clearBack = _window.View.View.ClearRect;
        if (clearBefore != clearBack) problems.Add($"panel reset: the clear rectangle is {clearBack}, it was {clearBefore}");
    }

    static nint PointLparam(Point layout, double scale) =>
        (nint)(((long)(ushort)(short)Math.Round(layout.Y * scale) << 16) | (ushort)(short)Math.Round(layout.X * scale));

    async Task CheckCanvas(List<string> problems, StringBuilder text)
    {
        var native = _window.Native;
        var session = _window.Session;
        if (native is null || session is null || !session.Attached)
        {
            problems.Add("canvas input: no view is attached");
            return;
        }
        var view = _window.View;
        var scale = _window.RenderScaling;
        var hwnd = native.Hwnd;
        nint Lp(Point p) => PointLparam(p, scale);
        void Same(string name)
        {
            var info = session.Info();
            var v = view.View;
            if (Math.Abs(info.zoom - v.Zoom) > 1e-9 || Math.Abs(info.origin_x - v.Origin.X) > 1e-6 || Math.Abs(info.origin_y - v.Origin.Y) > 1e-6)
                problems.Add($"{name}: the engine holds zoom {info.zoom} at {info.origin_x},{info.origin_y}, the view says {v.Zoom} at {v.Origin}");
        }

        // On open a picture is placed as the Mac places it: fitted into the clear rectangle.
        Same("placed on open");
        var clear = view.View.ClearRect ?? throw new InvalidOperationException("TEST_REPORT: no clear rectangle");
        var openZoom = view.View.Zoom;
        var openOrigin = view.View.Origin;
        text.Append($"  \"stage\": \"{_window.Stage.Bounds.Size}\",\n  \"clear_rect\": \"{clear}\",\n  \"open_zoom\": {openZoom},\n  \"open_origin\": \"{openOrigin}\",\n");
        if (!view.IsFitted) problems.Add("placed on open: the view is not fitted");
        if (openOrigin == default) problems.Add("placed on open: the picture is still pinned to the top left");

        // Go to 400%, so every axis can scroll, and check Ctrl+wheel keeps the point under the pointer.
        view.SetZoom(4);
        Same("zoom 400");
        if (_window.Status.ZoomField.Text != "400%") problems.Add($"status zoom: the field says {_window.Status.ZoomField.Text}, not 400%");
        var middle = new Point(view.View.ViewSize.Width / 2 + 37, view.View.ViewSize.Height / 2 - 21);
        var before = view.View.DocumentPoint(middle);
        var zoomBefore = view.View.Zoom;
        var screen = native.PointToScreen(middle);
        SendMessage(hwnd, WmMouseWheel, (nint)((WheelNotch << 16) | MkControl), Lparam(screen));
        Same("Ctrl+wheel");
        var after = view.View.DocumentPoint(middle);
        text.Append($"  \"wheel_zoom\": [{zoomBefore}, {view.View.Zoom}],\n");
        if (Math.Abs(view.View.Zoom - zoomBefore * 1.12) > 1e-9) problems.Add($"Ctrl+wheel: zoom went {zoomBefore} to {view.View.Zoom}");
        if (Math.Abs(after.X - before.X) > 0.01 || Math.Abs(after.Y - before.Y) > 0.01) problems.Add($"Ctrl+wheel: the point under the pointer moved {before} to {after}");

        // A plain wheel turn scrolls.
        var originBefore = view.View.Origin;
        SendMessage(hwnd, WmMouseWheel, unchecked((nint)((-WheelNotch) << 16)), Lparam(screen));
        Same("wheel");
        if (view.View.Origin == originBefore) problems.Add("wheel: a plain turn did not scroll");

        // The Pan tool, by a pointer drag.
        _window.Tools.Current = ToolState.Pan;
        var framesBefore = session.Info().frames;
        var start = new Point(300, 300);
        originBefore = view.View.Origin;
        SendMessage(hwnd, WmLeftDown, MkLeft, Lp(start));
        SendMessage(hwnd, WmMouseMove, MkLeft, Lp(new Point(280, 310)));
        SendMessage(hwnd, WmMouseMove, MkLeft, Lp(new Point(260, 320)));
        SendMessage(hwnd, WmLeftUp, 0, Lp(new Point(260, 320)));
        Same("pan");
        var delta = view.View.Origin - originBefore;
        text.Append($"  \"pan_delta\": \"{delta}\",\n");
        if (Math.Abs(delta.X + 40) > 1e-6 || Math.Abs(delta.Y - 20) > 1e-6) problems.Add($"pan: the view moved by {delta}, expected -40, 20");

        // Space held pans with any tool.
        _window.Tools.Current = ToolState.Default;
        originBefore = view.View.Origin;
        SendMessage(hwnd, WmKeyDown, VkSpace, 0);
        if (view.Cursor != CanvasCursor.Move) problems.Add("space: the cursor did not change");
        SendMessage(hwnd, WmLeftDown, MkLeft, Lp(start));
        SendMessage(hwnd, WmMouseMove, MkLeft, Lp(new Point(310, 305)));
        SendMessage(hwnd, WmLeftUp, 0, Lp(new Point(310, 305)));
        SendMessage(hwnd, WmKeyUp, VkSpace, 0);
        var spaceDelta = view.View.Origin - originBefore;
        text.Append($"  \"space_pan_delta\": \"{spaceDelta}\",\n");
        if (Math.Abs(spaceDelta.X - 10) > 1e-6 || Math.Abs(spaceDelta.Y - 5) > 1e-6) problems.Add($"space pan: the view moved by {spaceDelta}, expected 10, 5");
        originBefore = view.View.Origin;
        SendMessage(hwnd, WmLeftDown, MkLeft, Lp(start));
        SendMessage(hwnd, WmMouseMove, MkLeft, Lp(new Point(330, 330)));
        SendMessage(hwnd, WmLeftUp, 0, Lp(new Point(330, 330)));
        if (view.View.Origin != originBefore) problems.Add("space up: a drag still pans with the Paintbrush");

        // The Zoom tool: click, right click, drag.
        _window.Tools.Current = ToolState.Zoom;
        view.SetZoom(1);
        SendMessage(hwnd, WmLeftDown, MkLeft, Lp(start));
        SendMessage(hwnd, WmLeftUp, 0, Lp(start));
        Same("zoom click");
        if (view.View.Zoom != 2) problems.Add($"zoom click: 100% stepped to {view.View.Zoom}, not 200%");
        SendMessage(hwnd, WmRightDown, 2, Lp(start));
        SendMessage(hwnd, WmRightUp, 0, Lp(start));
        if (view.View.Zoom != 1) problems.Add($"zoom right click: 200% stepped to {view.View.Zoom}, not 100%");
        SendMessage(hwnd, WmLeftDown, MkLeft, Lp(start));
        SendMessage(hwnd, WmMouseMove, MkLeft, Lp(new Point(400, 360)));
        await Task.Delay(100);
        var framesInDrag = session.Info();
        if (framesInDrag.frame_errors != 0) problems.Add($"zoom rectangle: {framesInDrag.frame_errors} frame errors, last {framesInDrag.last_error}");
        SendMessage(hwnd, WmLeftUp, 0, Lp(new Point(400, 360)));
        Same("zoom rectangle");
        text.Append($"  \"zoom_rectangle_zoom\": {view.View.Zoom},\n");
        if (view.View.Zoom <= 1) problems.Add($"zoom rectangle: the zoom is {view.View.Zoom}, it should grow");

        // The status bar's buttons and the keys.
        var status = _window.Status;
        status.FitButton.RaiseEvent(new Avalonia.Interactivity.RoutedEventArgs(Button.ClickEvent));
        Same("Fit");
        if (!view.IsFitted || view.View.Zoom != openZoom || view.View.Origin != openOrigin)
            problems.Add($"Fit: zoom {view.View.Zoom} at {view.View.Origin}, it opened at {openZoom} at {openOrigin}");
        status.ActualButton.RaiseEvent(new Avalonia.Interactivity.RoutedEventArgs(Button.ClickEvent));
        Same("100%");
        // MetalCanvasView.setViewport: only a change of zoom ends Fit, and a small image fits at 100%.
        if (view.View.Zoom != 1 || (openZoom != 1 && view.IsFitted)) problems.Add($"100%: zoom {view.View.Zoom}, fitted {view.IsFitted}");
        status.ZoomField.Text = "250";
        status.ZoomField.RaiseEvent(new Avalonia.Input.KeyEventArgs { RoutedEvent = Avalonia.Input.InputElement.KeyDownEvent, Key = Avalonia.Input.Key.Enter });
        if (view.View.Zoom != 2.5) problems.Add($"zoom field: 250 gave {view.View.Zoom}");
        if (Math.Abs(status.ZoomSlider.Value - Math.Log(2.5)) > 1e-6) problems.Add($"zoom slider: {status.ZoomSlider.Value} at 250%");

        SendMessage(hwnd, WmKeyDown, 'Z', 0);
        if (_window.Tools.Current != ToolState.Zoom) problems.Add($"key Z: the tool is {_window.Tools.Current}");
        SendMessage(hwnd, WmKeyDown, 'H', 0);
        if (_window.Tools.Current != ToolState.Pan) problems.Add($"key H: the tool is {_window.Tools.Current}");
        SendMessage(hwnd, WmKeyDown, VkF8, 0);
        if (_window.Panels.IsShown(PanelLayout.Colors)) problems.Add("key F8: Colors did not hide");
        SendMessage(hwnd, WmKeyDown, VkF8, 0);
        if (!_window.Panels.IsShown(PanelLayout.Colors)) problems.Add("key F8: Colors did not come back");
        await Task.Delay(SettleMs);

        var framesAfter = session.Info();
        text.Append($"  \"frames_during_input\": {framesAfter.frames - framesBefore},\n");
        if (framesAfter.frames == framesBefore) problems.Add("canvas input: no frame came from all that input");
        if (framesAfter.frame_errors != 0) problems.Add($"canvas input: {framesAfter.frame_errors} frame errors, last {framesAfter.last_error}");

        _window.Tools.Current = ToolState.Default;
        view.Fit();
    }

    /// Menus and option popups are their own windows. A press on the native canvas must close them.
    async Task CheckPopups(nint main, List<string> problems, StringBuilder text)
    {
        await Task.Delay(SettleMs);   // The tool just changed, so its row needs a layout.
        var native = _window.Native;
        var row = _window.Options;
        var hidden = row.HiddenGroups();
        text.Append($"  \"options_hidden_groups\": {hidden.Count},\n  \"options_right_cover\": {row.RightCover},\n");
        var cell = row.MoreOptions.Parent as Control;
        if (hidden.Count > 0 && cell?.IsVisible != true) problems.Add("options: groups are hidden but the more button is not there");
        if (hidden.Count == 0)
        {
            problems.Add("options: the Paintbrush row fits at this size, so this run proves nothing about more");
            return;
        }
        row.OpenMore();
        await Task.Delay(SettleMs);
        var top = TopLevel.GetTopLevel(row.MorePopup.Child);
        if (!row.MorePopup.IsOpen || top is null || ReferenceEquals(top, _window))
        {
            problems.Add("options more: it did not open as its own window");
        }
        else
        {
            var popup = HandleOf(top);
            var center = CenterOf(row.MorePopup.Child!);
            text.Append($"  \"more_popup_screen\": \"{Top(center, popup, main, "more popup", problems)}\",\n");
            var moved = row.MoreGroups.Children.Count;
            text.Append($"  \"more_popup_groups\": {moved},\n");
            if (moved != hidden.Count) problems.Add($"options more: it holds {moved} groups, expected {hidden.Count}");
        }
        if (native is not null)
        {
            SendMessage(native.Hwnd, WmLeftDown, MkLeft, PointLparam(new Point(300, 300), _window.RenderScaling));
            SendMessage(native.Hwnd, WmLeftUp, 0, PointLparam(new Point(300, 300), _window.RenderScaling));
            await Task.Delay(SettleMs);
            if (row.MorePopup.IsOpen) problems.Add("options more: a press on the canvas did not close it");

            var menu = _window.TitleBar.Menus.Items.OfType<MenuItem>().First();
            menu.Open();
            await Task.Delay(SettleMs);
            SendMessage(native.Hwnd, WmLeftDown, MkLeft, PointLparam(new Point(300, 300), _window.RenderScaling));
            SendMessage(native.Hwnd, WmLeftUp, 0, PointLparam(new Point(300, 300), _window.RenderScaling));
            await Task.Delay(SettleMs);
            if (menu.IsSubMenuOpen) problems.Add("menu: a press on the canvas did not close it");
        }

        row.ScrollBy(120);
        await Task.Delay(SettleMs);
        text.Append($"  \"options_scrolled\": {row.Scroller.Offset.X},\n");
        if (row.Scroller.Offset.X != 120) problems.Add($"options scroll: the offset is {row.Scroller.Offset.X}, not 120");
        row.ScrollBy(-120);
        await Task.Delay(SettleMs);
    }

    /// Every icon on screen must start on a whole pixel, at the size of its art.
    void CheckIcons(List<string> problems, StringBuilder text)
    {
        var count = 0;
        var off = new List<string>();
        void Look(Visual root, double scale)
        {
            foreach (var icon in root.GetVisualDescendants().OfType<IconView>())
            {
                if (!icon.IsEffectivelyVisible) continue;
                count++;
                var at = icon.TranslatePoint(default, root);
                if (at is not { } p) continue;
                bool Whole(double v) => Math.Abs(v * scale - Math.Round(v * scale)) < 0.01;
                if (!Whole(p.X) || !Whole(p.Y) || !Whole(icon.Bounds.Width)) off.Add($"{p}");
            }
        }
        Look(_window, _window.RenderScaling);
        foreach (var panel in _window.Panels.Panels.Values) Look(panel, panel.RenderScaling);
        text.Append($"  \"icons_checked\": {count},\n  \"icons_off_pixel\": {off.Count},\n");
        if (count == 0) problems.Add("icons: none was on screen");
        if (off.Count != 0) problems.Add($"icons: {off.Count} are not on whole pixels: {string.Join(" ", off.Take(5))}");
    }

    /// Asks Windows for a far smaller window. The client must stop at the Mac's minimum:
    /// a limit that counts a title bar this window has turned off stops too soon.
    async Task CheckMinimumSize(nint main, List<string> problems, StringBuilder text)
    {
        const int TooSmall = 200;
        if (!GetWindowRect(main, out var before)) throw new InvalidOperationException($"TEST_REPORT: GetWindowRect: {Marshal.GetLastPInvokeError()}");
        const uint SwpNoMove = 2;
        if (!SetWindowPos(main, 0, 0, 0, TooSmall, TooSmall, SwpNoMove | SwpNoZOrder | SwpNoActivate))
            throw new InvalidOperationException($"TEST_REPORT: SetWindowPos: {Marshal.GetLastPInvokeError()}");
        await Task.Delay(SettleMs);
        var client = _window.ClientSize;
        text.Append($"  \"minimum_client\": \"{client}\",\n");
        if (client.Width != MainWindow.MinimumClient.Width || client.Height != MainWindow.MinimumClient.Height)
            problems.Add($"window minimum: the client stops at {client}, not {MainWindow.MinimumClient}");
        if (!SetWindowPos(main, 0, 0, 0, before.Right - before.Left, before.Bottom - before.Top, SwpNoMove | SwpNoZOrder | SwpNoActivate))
            throw new InvalidOperationException($"TEST_REPORT: SetWindowPos: {Marshal.GetLastPInvokeError()}");
        await Task.Delay(SettleMs);
    }

    async Task CheckShell(nint main, List<string> problems, StringBuilder text)
    {
        await CheckPanels(main, problems, text);
        await CheckCanvas(problems, text);
        await CheckPopups(main, problems, text);
        CheckIcons(problems, text);
        await CheckMinimumSize(main, problems, text);
    }
}
