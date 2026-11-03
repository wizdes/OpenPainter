// The native child window the engine draws into.
//
// It is a custom window class, because W5 found that a STATIC child passes the
// mouse through. The engine's render thread draws into it, so its frame rate does
// not depend on Avalonia's UI frame callback, which W5 measured at 41 to 44 per
// second. A native child keeps the Tab key, so it hands focus back on Tab.
using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;
using Avalonia;
using Avalonia.Controls;
using Avalonia.Platform;

namespace OpenPainter.Canvas;

public sealed unsafe class CanvasHost : NativeControlHost
{
    const string ClassName = "OpenPainterCanvas";
    const uint WsChild = 0x40000000, WsVisible = 0x10000000, WsTabStop = 0x00010000;
    const uint WmSetFocus = 0x0007, WmKillFocus = 0x0008, WmEraseBackground = 0x0014, WmSetCursor = 0x0020, WmKeyDown = 0x0100, WmKeyUp = 0x0101, WmChar = 0x0102, WmSysKeyDown = 0x0104, WmSysKeyUp = 0x0105, WmSysChar = 0x0106,
        WmMouseMove = 0x0200, WmLeftDown = 0x0201, WmLeftUp = 0x0202, WmRightDown = 0x0204, WmRightUp = 0x0205, WmMouseWheel = 0x020A, WmMouseHWheel = 0x020E,
        WmCaptureChanged = 0x0215, WmPointerUpdate = 0x0245, WmPointerDown = 0x0246, WmPointerUp = 0x0247, WmMouseLeave = 0x02A3;
    const int VkTab = 9, VkShift = 0x10, VkControl = 0x11, VkAlt = 0x12, VkSpace = 0x20, VkF4 = 0x73, VkF10 = 0x79, PointerTypePen = 3;
    const int MkShift = 4, MkControl = 8, WheelDelta = 120, HtClient = 1;
    const uint WheelScrollLines = 0x68, WheelPageScroll = uint.MaxValue, TrackLeave = 2;
    const int CursorArrow = 32512, CursorCross = 32515, CursorMove = 32646;
    const double PointsPerLine = 16;
    const uint RootAncestor = 2;
    const int PenInfoSize = 136, PenPressureOffset = 104;
    const double PenPressureRange = 1024;

    [StructLayout(LayoutKind.Sequential)]
    struct WindowClass
    {
        public uint Size, Style;
        public delegate* unmanaged[Stdcall]<nint, uint, nuint, nint, nint> Proc;
        public int ClassExtra, WindowExtra;
        public nint Instance, Icon, Cursor, Background;
        public char* Menu, Name;
        public nint SmallIcon;
    }

    [DllImport("user32.dll", SetLastError = true)] static extern ushort RegisterClassExW(WindowClass* value);
    [DllImport("user32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
    static extern nint CreateWindowExW(uint ex, string cls, string title, uint style, int x, int y, int w, int h, nint parent, nint menu, nint instance, nint param);
    [DllImport("user32.dll", SetLastError = true)] static extern bool DestroyWindow(nint hwnd);
    [DllImport("user32.dll")] static extern nint DefWindowProcW(nint hwnd, uint message, nuint w, nint l);
    [DllImport("user32.dll")] static extern nint SetFocus(nint hwnd);
    [DllImport("user32.dll")] static extern nint GetAncestor(nint hwnd, uint flags);
    [DllImport("user32.dll")] static extern short GetKeyState(int key);
    [DllImport("user32.dll")] static extern bool GetPointerType(uint id, out int type);
    [DllImport("user32.dll", SetLastError = true)] static extern bool GetPointerPenInfo(uint id, byte* info);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode)] static extern nint GetModuleHandleW(string? name);
    [DllImport("user32.dll")] static extern nint SetCapture(nint hwnd);
    [DllImport("user32.dll")] static extern bool ReleaseCapture();
    [DllImport("user32.dll")] static extern nint SetCursor(nint cursor);
    [DllImport("user32.dll")] static extern nint LoadCursorW(nint instance, nint name);
    [DllImport("user32.dll")] static extern bool ScreenToClient(nint hwnd, ref NativePoint point);
    [DllImport("user32.dll", SetLastError = true)] static extern bool SystemParametersInfoW(uint action, uint param, uint* value, uint winIni);
    [DllImport("user32.dll", SetLastError = true)] static extern bool TrackMouseEvent(ref TrackEvent track);
    [DllImport("user32.dll", SetLastError = true)] static extern int SetWindowRgn(nint hwnd, nint region, bool redraw);
    [DllImport("gdi32.dll")] static extern nint CreateRectRgn(int left, int top, int right, int bottom);
    [DllImport("gdi32.dll")] static extern bool DeleteObject(nint thing);

    [StructLayout(LayoutKind.Sequential)] struct NativePoint { public int X, Y; }
    [StructLayout(LayoutKind.Sequential)] struct TrackEvent { public uint Size, Flags; public nint Target; public uint Hover; }

    static bool s_registered;
    static CanvasHost? s_current;

    /// Canvas input in W5's text form, for a test harness and later tools.
    public event Action<string>? NativeInput;

    /// Tab was pressed on the canvas. True means Shift was held.
    public event Action<bool>? TabOut;

    public nint Hwnd { get; private set; }

    /// Where pointer, wheel and space bar input goes. Each call runs on the UI thread, right
    /// in the window message, so it does not wait for a UI frame.
    public ViewController? View { get; set; }

    /// Layout pixels to window pixels, kept by the main window.
    public double Scale { get; set; } = 1;

    /// A mouse button went down here. Avalonia never sees it, so open popups must be told.
    public event Action? Pressed;

    /// A key other than Tab and space, with whether it repeats. True means it was used.
    public event Func<int, bool, bool>? KeyPressed;

    /// A key other than F4 and space with Alt down, or F10, with whether it repeats. True means it was used.
    public event Func<int, bool, bool>? SystemKeyPressed;

    /// Alt came up on the canvas.
    public event Action? AltReleased;

    /// Gives the keyboard to the main window, so a menu or a control can take it.
    public void GiveFocusToWindow()
    {
        if (Hwnd != 0) SetFocus(GetAncestor(Hwnd, RootAncestor));
    }

    bool _leaveTracked;
    nint _holder;

    /// A test plant: the window keeps its old fault and shows black until the first frame.
    public bool TestNoClip { get; init; }

    /// Until the engine's first frame this window has nothing to show, so it shows black. A
    /// plain Win32 fill does not help: a real run still read black on the screen. Avalonia's
    /// main window has WS_EX_NOREDIRECTIONBITMAP, so it keeps no surface for GDI to reach. So
    /// the window Avalonia holds this one in clips away all of it, and Avalonia's stage shows
    /// through in the right color, in both themes. The engine draws as usual meanwhile, and
    /// later frames pay nothing.
    void ClipAway()
    {
        var empty = CreateRectRgn(0, 0, 0, 0);
        if (empty == 0) throw new InvalidOperationException("CANVAS_CLIP: CreateRectRgn failed");
        if (SetWindowRgn(_holder, empty, false) == 0)
        {
            DeleteObject(empty);
            throw new InvalidOperationException($"CANVAS_CLIP: SetWindowRgn: {Marshal.GetLastWin32Error()}");
        }
    }

    /// The engine has shown a frame, so the canvas may show it.
    public void ShowCanvas()
    {
        if (_holder != 0 && SetWindowRgn(_holder, 0, true) == 0)
            throw new InvalidOperationException($"CANVAS_CLIP: SetWindowRgn: {Marshal.GetLastWin32Error()}");
    }

    Point Points(nint l) => new((short)(l & 0xffff) / Scale, (short)((l >> 16) & 0xffff) / Scale);

    void ShowCursor()
    {
        var name = View?.Cursor switch { CanvasCursor.Cross => CursorCross, CanvasCursor.Move => CursorMove, _ => CursorArrow };
        SetCursor(LoadCursorW(0, name));
    }

    static double LinesPerNotch(double viewHeight)
    {
        uint lines = 3;
        if (!SystemParametersInfoW(WheelScrollLines, 0, &lines, 0))
            throw new InvalidOperationException($"CANVAS_WHEEL: {Marshal.GetLastWin32Error()}");
        // Windows' "one screen at a time" setting.
        return lines == WheelPageScroll ? viewHeight / PointsPerLine : lines;
    }

    void Wheel(nint hwnd, nuint w, nint l, bool sideways)
    {
        if (View is null) return;
        var at = new NativePoint { X = (short)(l & 0xffff), Y = (short)((l >> 16) & 0xffff) };
        if (!ScreenToClient(hwnd, ref at)) throw new InvalidOperationException($"CANVAS_WHEEL: ScreenToClient: {Marshal.GetLastWin32Error()}");
        var flags = (int)((ulong)w & 0xffff);
        var notches = (short)(((ulong)w >> 16) & 0xffff) / (double)WheelDelta;
        // A tilt to the right is a positive delta, and it moves the image left.
        var (x, y) = sideways ? (-notches, 0.0) : (0.0, notches);
        View.Wheel(new Point(at.X / Scale, at.Y / Scale), x, y, LinesPerNotch(View.View.ViewSize.Height), (flags & MkControl) != 0, (flags & MkShift) != 0);
    }

    public void TakeFocus()
    {
        if (Hwnd != 0) SetFocus(Hwnd);
    }

    protected override IPlatformHandle CreateNativeControlCore(IPlatformHandle parent)
    {
        s_current = this;
        var instance = GetModuleHandleW(null);
        if (!s_registered)
        {
            fixed (char* name = ClassName)
            {
                var cls = new WindowClass
                {
                    Size = (uint)sizeof(WindowClass),
                    Proc = &OnMessage,
                    Instance = instance,
                    Name = name,
                };
                if (RegisterClassExW(&cls) == 0) throw new InvalidOperationException($"CANVAS_CLASS: {Marshal.GetLastWin32Error()}");
            }
            s_registered = true;
        }
        Hwnd = CreateWindowExW(0, ClassName, "Canvas", WsChild | WsVisible | WsTabStop, 0, 0, 100, 100, parent.Handle, 0, instance, 0);
        if (Hwnd == 0) throw new InvalidOperationException($"CANVAS_HWND: {Marshal.GetLastWin32Error()}");
        _holder = parent.Handle;
        if (!TestNoClip) ClipAway();
        return new PlatformHandle(Hwnd, "HWND");
    }

    protected override void DestroyNativeControlCore(IPlatformHandle control)
    {
        if (!DestroyWindow(control.Handle)) throw new InvalidOperationException($"DESTROY_CANVAS: {Marshal.GetLastWin32Error()}");
        Hwnd = 0;
        _holder = 0;
        s_current = null;
    }

    static void LogPen(string phase, nuint w)
    {
        var id = (uint)((ulong)w & 0xffff);
        if (!GetPointerType(id, out var type) || type != PointerTypePen) return;
        var info = stackalloc byte[PenInfoSize];
        var input = s_current?.NativeInput;
        if (!GetPointerPenInfo(id, info))
        {
            input?.Invoke($"pen:{phase}:error:{Marshal.GetLastWin32Error()}");
            return;
        }
        input?.Invoke($"pen:{phase}:{*(uint*)(info + PenPressureOffset) / PenPressureRange:F2}");
    }

    /// A throw must not cross this native boundary, because it would end the process with
    /// no words. It goes to the fatal path, which shows it and stops the program.
    [UnmanagedCallersOnly(CallConvs = [typeof(CallConvStdcall)])]
    static nint OnMessage(nint hwnd, uint message, nuint w, nint l)
    {
        try
        {
            return Handle(hwnd, message, w, l);
        }
        catch (Exception e)
        {
            OpenPainter.Shell.Fatal.Raise("CANVAS_INPUT", e);
            return 0;
        }
    }

    static nint Handle(nint hwnd, uint message, nuint w, nint l)
    {
        var host = s_current;
        var input = host?.NativeInput;
        switch (message)
        {
            // The engine paints every pixel, so Windows must not clear it first.
            case WmEraseBackground: return 1;
            case WmSetCursor when (l & 0xffff) == HtClient && host is not null:
                host.ShowCursor();
                return 1;
            case WmLeftDown:
                SetFocus(hwnd);
                input?.Invoke($"pointer:down:{(short)(l & 0xffff)},{(short)((l >> 16) & 0xffff)}:1.00:Mouse");
                if (host is not null)
                {
                    host.Pressed?.Invoke();
                    SetCapture(hwnd);
                    host.View?.PointerDown(host.Points(l), CanvasButton.Left, GetKeyState(VkAlt) < 0);
                }
                return 0;
            case WmMouseMove:
                input?.Invoke($"pointer:move:{(short)(l & 0xffff)},{(short)((l >> 16) & 0xffff)}:1.00:Mouse");
                if (host is not null)
                {
                    if (!host._leaveTracked)
                    {
                        var track = new TrackEvent { Size = (uint)sizeof(TrackEvent), Flags = TrackLeave, Target = hwnd };
                        if (!TrackMouseEvent(ref track)) throw new InvalidOperationException($"CANVAS_LEAVE: {Marshal.GetLastWin32Error()}");
                        host._leaveTracked = true;
                    }
                    host.View?.PointerMove(host.Points(l));
                }
                return 0;
            case WmMouseLeave:
                if (host is not null)
                {
                    host._leaveTracked = false;
                    host.View?.PointerLeft();
                }
                return 0;
            case WmLeftUp:
                input?.Invoke("pointer:up:1.00:Mouse");
                host?.View?.PointerUp(host.Points(l));
                ReleaseCapture();
                return 0;
            case WmRightDown:
                SetFocus(hwnd);
                if (host is not null)
                {
                    host.Pressed?.Invoke();
                    host.View?.PointerDown(host.Points(l), CanvasButton.Right, GetKeyState(VkAlt) < 0);
                }
                return 0;
            case WmRightUp:
                return 0;
            case WmCaptureChanged:
                host?.View?.CancelDrag();
                return 0;
            case WmMouseWheel:
                input?.Invoke($"wheel:{(short)((long)w >> 16)}");
                host?.Wheel(hwnd, w, l, sideways: false);
                return 0;
            case WmMouseHWheel:
                host?.Wheel(hwnd, w, l, sideways: true);
                return 0;
            case WmPointerDown: LogPen("down", w); break;
            case WmPointerUpdate: LogPen("move", w); break;
            case WmPointerUp: LogPen("up", w); break;
            case WmKeyDown:
                input?.Invoke($"key:{w}");
                // Ctrl+Tab moves between images, and only plain Tab leaves the canvas.
                if ((int)w == VkTab && GetKeyState(VkControl) >= 0)
                {
                    SetFocus(GetAncestor(hwnd, RootAncestor));
                    host?.TabOut?.Invoke(GetKeyState(VkShift) < 0);
                    return 0;
                }
                if ((int)w == VkSpace)
                {
                    host?.View?.SpaceDown();
                    host?.ShowCursor();
                    return 0;
                }
                if (host?.KeyPressed?.Invoke((int)w, ((l >> 30) & 1) != 0) == true) return 0;
                break;
            case WmKeyUp:
                if ((int)w == VkSpace)
                {
                    host?.View?.SpaceUp();
                    host?.ShowCursor();
                    return 0;
                }
                break;
            case WmChar:
                if ((int)w != VkTab) input?.Invoke($"text:{(char)w}");
                return 0;
            // Windows sends Alt and F10 here. Alt+F4 and Alt+Space stay with Windows.
            case WmSysKeyDown:
                input?.Invoke($"syskey:{w}");
                if ((int)w == VkF4 || (int)w == VkSpace) break;
                if (host?.SystemKeyPressed?.Invoke((int)w, ((l >> 30) & 1) != 0) == true) return 0;
                if ((int)w == VkAlt || (int)w == VkF10) return 0;
                break;
            case WmSysKeyUp:
                if ((int)w == VkAlt)
                {
                    host?.AltReleased?.Invoke();
                    return 0;
                }
                if ((int)w == VkF10) return 0;
                break;
            // A used Alt key must not ring the bell that Windows rings for an unknown menu letter.
            case WmSysChar when GetKeyState(VkControl) >= 0 && (char)w != ' ':
                return 0;
            case WmSetFocus: input?.Invoke("focus:canvas"); break;
            case WmKillFocus:
                input?.Invoke("blur:canvas");
                host?.View?.FocusLost();
                break;
        }
        return DefWindowProcW(hwnd, message, w, l);
    }
}
