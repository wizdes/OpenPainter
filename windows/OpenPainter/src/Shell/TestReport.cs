// A test hook for a real window. It asks Windows itself what each part of the title
// row is, by sending WM_NCHITTEST, and it sends the pointer messages Windows would
// send, so the user's own pointer does not move. Then it writes the answers and the
// engine's frame counters to a file. Headless tests cannot see any of this.
using System.Runtime.InteropServices;
using System.Text;
using Avalonia;
using Avalonia.Controls;
using Avalonia.Media;
using Avalonia.Threading;

namespace OpenPainter.Shell;

public sealed partial class TestReport
{
    public const int HtClient = 1, HtCaption = 2, HtMinButton = 8, HtMaxButton = 9, HtTop = 12, HtClose = 20;
    const uint WmNcHitTest = 0x0084, WmNcMouseMove = 0x00A0, WmNcLeftDown = 0x00A1, WmNcLeftUp = 0x00A2, WmNcMouseLeave = 0x02A2;

    [StructLayout(LayoutKind.Sequential)]
    struct NativePoint { public int X, Y; }

    [LibraryImport("user32.dll", EntryPoint = "SendMessageW")] private static partial nint SendMessage(nint hwnd, uint message, nint w, nint l);
    [LibraryImport("user32.dll")] private static partial nint ChildWindowFromPointEx(nint parent, NativePoint point, uint flags);
    [LibraryImport("user32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static partial bool ScreenToClient(nint hwnd, ref NativePoint point);
    [LibraryImport("user32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static partial bool IsChild(nint parent, nint hwnd);
    [LibraryImport("user32.dll", EntryPoint = "GetClassNameW", StringMarshalling = StringMarshalling.Utf16)]
    private static partial int GetClassName(nint hwnd, [Out] char[] name, int capacity);

    const int SettleMs = 500;   // Time for the window to lay out after it changes state.

    readonly MainWindow _window;
    public TestReport(MainWindow window) => _window = window;

    /// Each part's expected answer, so a wrong one fails by the part's name.
    public static IReadOnlyDictionary<string, int> Expected { get; } = new Dictionary<string, int>
    {
        ["top menu"] = HtClient,
        ["title text"] = HtCaption,
        ["gap beside the menus"] = HtCaption,
        ["minimize"] = HtMinButton,
        ["maximize"] = HtMaxButton,
        ["close"] = HtClose,
        ["top resize edge"] = HtTop,
    };

    static string ClassOf(nint hwnd)
    {
        var name = new char[128];
        var length = GetClassName(hwnd, name, name.Length);
        return length > 0 ? new string(name, 0, length) : "(unknown)";
    }

    static nint Lparam(PixelPoint screen) => (nint)(((long)(ushort)(short)screen.Y << 16) | (ushort)(short)screen.X);

    static int Ask(nint hwnd, PixelPoint screen) => (int)SendMessage(hwnd, WmNcHitTest, 0, Lparam(screen));

    static PixelPoint CenterOf(Control control) =>
        control.PointToScreen(new Point(control.Bounds.Width / 2, control.Bounds.Height / 2));

    /// Proves the path from the system's caption answer to the button's hover and click.
    async Task CheckCaptionInput(nint hwnd, List<string> problems, StringBuilder text)
    {
        var row = _window.TitleBar;
        var close = CenterOf(row.CloseButton);
        SendMessage(hwnd, WmNcMouseMove, HtClose, Lparam(close));
        Dispatcher.UIThread.RunJobs();
        var fill = row.CloseButton.CurrentFill;
        text.Append($"  \"close_hover_fill\": \"{fill}\",\n");
        if (fill != CaptionLook.CloseHover) problems.Add($"close hover: the fill is {fill}, not the red {CaptionLook.CloseHover}");
        SendMessage(hwnd, WmNcMouseLeave, 0, 0);
        Dispatcher.UIThread.RunJobs();
        if (row.CloseButton.CurrentFill != Colors.Transparent) problems.Add("close hover: the red stayed after the pointer left");

        // A press that no control takes would go to the system's own button loop, which
        // waits for a real mouse release. So each click first checks what is under it.
        async Task<bool> ClickMaximize()
        {
            var at = CenterOf(_window.TitleBar.MaximizeButton);
            var answer = Ask(hwnd, at);
            if (answer != HtMaxButton)
            {
                problems.Add($"maximize click: Windows says {answer} at the button, expected {HtMaxButton}");
                return false;
            }
            SendMessage(hwnd, WmNcMouseMove, HtMaxButton, Lparam(at));
            Dispatcher.UIThread.RunJobs();
            if (!_window.TitleBar.MaximizeButton.IsPointerOver)
            {
                problems.Add("maximize click: the pointer message did not reach the button");
                return false;
            }
            SendMessage(hwnd, WmNcLeftDown, HtMaxButton, Lparam(at));
            SendMessage(hwnd, WmNcLeftUp, HtMaxButton, Lparam(at));
            SendMessage(hwnd, WmNcMouseLeave, 0, 0);
            await Task.Delay(SettleMs);
            return true;
        }
        if (!await ClickMaximize()) return;
        text.Append($"  \"after_maximize_click\": \"{_window.WindowState}\",\n");
        if (_window.WindowState != WindowState.Maximized)
        {
            problems.Add($"maximize click: the window is {_window.WindowState}");
            return;
        }
        var screen = _window.Screens.ScreenFromWindow(_window);
        var top = _window.TitleBar.PointToScreen(new Point(0, 0)).Y;
        text.Append($"  \"maximized_title_top\": {top},\n  \"screen_work_top\": {screen?.WorkingArea.Y},\n");
        if (screen is not null && top < screen.WorkingArea.Y) problems.Add($"maximized: the title row starts at {top}, above the screen's top {screen.WorkingArea.Y}");
        var got = Ask(hwnd, CenterOf(_window.TitleBar.CloseButton));
        if (got != HtClose) problems.Add($"maximized close: Windows says {got}, expected {HtClose}");
        if (!await ClickMaximize()) return;
        text.Append($"  \"after_restore_click\": \"{_window.WindowState}\",\n");
        if (_window.WindowState != WindowState.Normal) problems.Add($"restore click: the window is {_window.WindowState}");
    }

    /// Runs the checks and writes the file. Returns the problems, by name.
    public async Task<IReadOnlyList<string>> Write(string path)
    {
        var hwnd = _window.TryGetPlatformHandle()?.Handle ?? throw new InvalidOperationException("TEST_REPORT: the window has no handle");
        var row = _window.TitleBar;
        var menu = row.Menus.Items.OfType<MenuItem>().First();
        var points = new Dictionary<string, PixelPoint>
        {
            ["top menu"] = CenterOf(menu),
            ["title text"] = CenterOf(row),
            ["gap beside the menus"] = row.PointToScreen(new Point(row.Menus.Bounds.Right + 8, row.Bounds.Height / 2)),
            ["minimize"] = CenterOf(row.MinimizeButton),
            ["maximize"] = CenterOf(row.MaximizeButton),
            ["close"] = CenterOf(row.CloseButton),
            ["top resize edge"] = row.PointToScreen(new Point(row.Bounds.Width / 2, 1)),
        };
        var problems = new List<string>();
        var text = new StringBuilder("{\n  \"hit_test\": {\n");
        foreach (var (name, point) in points)
        {
            var got = Ask(hwnd, point);
            text.Append($"    \"{name}\": {got},\n");
            if (got != Expected[name]) problems.Add($"{name}: Windows says {got}, expected {Expected[name]}");
        }
        text.Length -= 2;
        text.Append("\n  },\n");

        await CheckCaptionInput(hwnd, problems, text);
        await CheckShell(hwnd, problems, text);

        // A real menu opens as its own window, with its icons from disk.
        menu.Open();
        await Task.Delay(SettleMs);
        var shownIcons = menu.Items.OfType<MenuItem>().Count(i => i.Icon is not null);
        text.Append($"  \"menu_open\": {(menu.IsSubMenuOpen ? "true" : "false")},\n  \"menu_icons\": {shownIcons},\n");
        if (!menu.IsSubMenuOpen) problems.Add("menu: the first top menu did not open");
        if (shownIcons == 0) problems.Add("menu: the first top menu loaded no icons");
        menu.Close();

        var session = _window.Session;
        var native = _window.Native;
        if (session is null || native is null || !session.Attached)
        {
            problems.Add("canvas: no view is attached");
        }
        else
        {
            // Asks about this window's own children, so another program on top changes nothing.
            const uint SkipInvisible = 1;
            var center = CenterOf(native);
            var inside = new NativePoint { X = center.X, Y = center.Y };
            if (!ScreenToClient(hwnd, ref inside)) problems.Add($"canvas: ScreenToClient failed: {Marshal.GetLastPInvokeError()}");
            var under = ChildWindowFromPointEx(hwnd, inside, SkipInvisible);
            if (under != native.Hwnd && !IsChild(under, native.Hwnd))
                problems.Add($"canvas: the child under its center is class {ClassOf(under)}, not the canvas");
            var info = session.Info();
            if (info.frames == 0) problems.Add("canvas: no frame was presented");
            if (info.frame_errors != 0) problems.Add($"canvas: {info.frame_errors} frame errors, last {info.last_error}");
            text.Append($"  \"frames\": {info.frames},\n  \"frame_errors\": {info.frame_errors},\n");
            text.Append($"  \"drawable\": [{info.drawable_width}, {info.drawable_height}],\n  \"scale\": {info.scale},\n");
        }
        text.Append($"  \"image\": \"{_window.Status.ImageValue.Text}\",\n");
        text.Append($"  \"title\": \"{_window.Title}\",\n  \"palette\": \"{_window.Palette.Name}\",\n  \"theme\": \"{_window.ActualThemeVariant}\",\n");
        text.Append($"  \"problems\": [{string.Join(", ", problems.Select(p => $"\"{p}\""))}]\n}}\n");
        File.WriteAllText(path, text.ToString());
        return problems;
    }

    /// Ends the run after `ms`, with a nonzero exit code when a check failed.
    public static void Arrange(MainWindow window, int ms, string? path, Action<int> setExitCode)
    {
        window.Opened += (_, _) => DispatcherTimer.RunOnce(async () =>
        {
            if (path is not null)
            {
                var problems = await new TestReport(window).Write(path);
                foreach (var problem in problems) Console.Error.WriteLine($"TEST_REPORT: {problem}");
                if (problems.Count != 0) setExitCode(1);
            }
            window.Close();
        }, TimeSpan.FromMilliseconds(ms));
    }
}
