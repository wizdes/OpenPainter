// Windows calls for the harness. Every capture is of one window of the app, through
// PrintWindow, so nothing of the user's desktop can reach a picture. PLAN 4.3 bans
// whole-screen shots, raw SendInput and moving the pointer, and none of them is here.
using System.Runtime.InteropServices;
using System.Text;

namespace OpenPainter.Harness;

public static unsafe partial class Win32
{
    [StructLayout(LayoutKind.Sequential)] public struct NativeRect { public int Left, Top, Right, Bottom; public readonly int Width => Right - Left; public readonly int Height => Bottom - Top; }

    [StructLayout(LayoutKind.Sequential)]
    struct BitmapInfoHeader
    {
        public uint Size;
        public int Width, Height;
        public ushort Planes, BitCount;
        public uint Compression, SizeImage;
        public int XPelsPerMeter, YPelsPerMeter;
        public uint ClrUsed, ClrImportant;
    }

    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    struct MonitorInfoEx
    {
        public uint Size;
        public NativeRect Monitor, Work;
        public uint Flags;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 32)] public string Device;
    }

    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    struct DevMode
    {
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 32)] public string DeviceName;
        public ushort SpecVersion, DriverVersion, Size, DriverExtra;
        public uint Fields;
        public int PositionX, PositionY;
        public uint DisplayOrientation, DisplayFixedOutput;
        public short Color, Duplex, YResolution, TTOption, Collate;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 32)] public string FormName;
        public ushort LogPixels;
        public uint BitsPerPel, PelsWidth, PelsHeight, DisplayFlags, DisplayFrequency;
        public uint IcmMethod, IcmIntent, MediaType, DitherType, Reserved1, Reserved2, PanningWidth, PanningHeight;
    }

    [StructLayout(LayoutKind.Sequential)] struct NativePoint { public int X, Y; }

    delegate bool EnumProc(nint hwnd, nint context);

    [DllImport("user32.dll", SetLastError = true)] static extern bool EnumWindows(EnumProc proc, nint context);
    [DllImport("user32.dll")] static extern bool EnumChildWindows(nint parent, EnumProc proc, nint context);
    [DllImport("user32.dll", SetLastError = true)] static extern uint GetWindowThreadProcessId(nint hwnd, out uint pid);
    [DllImport("user32.dll")] static extern bool IsWindowVisible(nint hwnd);
    [DllImport("user32.dll", SetLastError = true)] public static extern bool GetWindowRect(nint hwnd, out NativeRect rect);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetClassNameW(nint hwnd, StringBuilder name, int size);
    [DllImport("user32.dll", SetLastError = true)] static extern bool PrintWindow(nint hwnd, nint dc, uint flags);
    [DllImport("user32.dll", SetLastError = true)] static extern bool PostMessageW(nint hwnd, uint message, nint w, nint l);
    [DllImport("user32.dll")] static extern nint MonitorFromWindow(nint hwnd, uint flags);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern bool GetMonitorInfoW(nint monitor, ref MonitorInfoEx info);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern bool EnumDisplaySettingsW(string? device, int mode, ref DevMode info);
    [DllImport("user32.dll")] static extern nint GetDC(nint hwnd);
    [DllImport("user32.dll")] static extern int ReleaseDC(nint hwnd, nint dc);
    [DllImport("gdi32.dll")] static extern uint GetPixel(nint dc, int x, int y);
    [DllImport("user32.dll")] static extern nint WindowFromPoint(NativePoint point);
    [DllImport("user32.dll")] static extern bool IsChild(nint parent, nint hwnd);
    [DllImport("user32.dll", SetLastError = true)] static extern bool SetWindowPos(nint hwnd, nint after, int x, int y, int cx, int cy, uint flags);
    [DllImport("gdi32.dll")] static extern nint CreateCompatibleDC(nint dc);
    [DllImport("gdi32.dll")] static extern bool DeleteDC(nint dc);
    [DllImport("gdi32.dll")] static extern nint SelectObject(nint dc, nint thing);
    [DllImport("gdi32.dll")] static extern bool DeleteObject(nint thing);
    [DllImport("gdi32.dll", SetLastError = true)] static extern nint CreateDIBSection(nint dc, BitmapInfoHeader* info, uint usage, void** bits, nint section, uint offset);
    [DllImport("gdi32.dll", CharSet = CharSet.Unicode)] static extern bool GetICMProfileW(nint dc, ref uint size, StringBuilder? name);
    [DllImport("dwmapi.dll")] static extern int DwmFlush();
    [DllImport("dwmapi.dll")] static extern int DwmGetWindowAttribute(nint hwnd, uint attribute, out NativeRect value, int size);
    [DllImport("kernel32.dll", SetLastError = true)] static extern bool GetSystemTimes(out long idle, out long kernel, out long user);
    [DllImport("user32.dll")] static extern bool SetProcessDpiAwarenessContext(nint context);

    const uint PwRenderFullContent = 2, WmClose = 0x10, MonitorDefaultToNearest = 2, DwmExtendedFrameBounds = 9;
    const int EnumCurrentSettings = -1;

    /// The harness reads real pixels, so it must not get a scaled copy of the window.
    public static void UsePhysicalPixels()
    {
        if (!SetProcessDpiAwarenessContext(-4)) throw new InvalidOperationException($"HARNESS: SetProcessDpiAwarenessContext: {Marshal.GetLastWin32Error()}");
    }

    public static string ClassOf(nint hwnd)
    {
        var name = new StringBuilder(128);
        GetClassNameW(hwnd, name, name.Capacity);
        return name.ToString();
    }

    /// The process's visible top-level windows, topmost first.
    public static List<nint> WindowsOf(int pid)
    {
        var found = new List<nint>();
        if (!EnumWindows((hwnd, _) =>
            {
                GetWindowThreadProcessId(hwnd, out var owner);
                if (owner == pid && IsWindowVisible(hwnd)) found.Add(hwnd);
                return true;
            }, 0)) throw new InvalidOperationException($"HARNESS: EnumWindows: {Marshal.GetLastWin32Error()}");
        return found;
    }

    /// The largest of them, which is the main window.
    public static nint MainWindowOf(int pid)
    {
        var best = (nint)0;
        var area = 0L;
        foreach (var hwnd in WindowsOf(pid))
        {
            if (!GetWindowRect(hwnd, out var r)) continue;
            var size = (long)r.Width * r.Height;
            if (size > area) (best, area) = (hwnd, size);
        }
        return best;
    }

    /// The canvas's child window, found by its class name.
    public static nint CanvasOf(nint main)
    {
        var found = (nint)0;
        EnumChildWindows(main, (hwnd, _) =>
        {
            if (ClassOf(hwnd) != "OpenPainterCanvas") return true;
            found = hwnd;
            return false;
        }, 0);
        return found;
    }

    /// Asks one window to draw itself into a private bitmap. It sees only that window,
    /// and works with another program on top.
    public static Picture Capture(nint hwnd)
    {
        if (!GetWindowRect(hwnd, out var r)) throw new InvalidOperationException($"HARNESS: GetWindowRect: {Marshal.GetLastWin32Error()}");
        var screen = GetDC(0);
        if (screen == 0) throw new InvalidOperationException("HARNESS: GetDC");
        var dc = CreateCompatibleDC(screen);
        if (dc == 0)
        {
            ReleaseDC(0, screen);
            throw new InvalidOperationException("HARNESS: CreateCompatibleDC");
        }
        try
        {
            var info = new BitmapInfoHeader { Size = (uint)sizeof(BitmapInfoHeader), Width = r.Width, Height = -r.Height, Planes = 1, BitCount = 32 };
            void* bits;
            var bitmap = CreateDIBSection(dc, &info, 0, &bits, 0, 0);
            if (bitmap == 0) throw new InvalidOperationException($"HARNESS: CreateDIBSection {r.Width}x{r.Height}: {Marshal.GetLastWin32Error()}");
            var old = SelectObject(dc, bitmap);
            try
            {
                if (!PrintWindow(hwnd, dc, PwRenderFullContent)) throw new InvalidOperationException($"HARNESS: PrintWindow: {Marshal.GetLastWin32Error()}");
                var bytes = new byte[r.Width * r.Height * 4];
                Marshal.Copy((nint)bits, bytes, 0, bytes.Length);
                return new Picture(r.Width, r.Height, bytes);
            }
            finally
            {
                SelectObject(dc, old);
                DeleteObject(bitmap);
            }
        }
        finally
        {
            DeleteDC(dc);
            ReleaseDC(0, screen);
        }
    }

    /// All of the app's visible windows, bottom first, painted over each other in the
    /// main window's frame: what the app shows, and nothing of any other program.
    public static Picture CaptureApp(int pid)
    {
        var windows = WindowsOf(pid);
        var main = MainWindowOf(pid);
        if (main == 0) throw new InvalidOperationException("HARNESS: the app has no window");
        var rects = new List<(nint Hwnd, NativeRect Rect, NativeRect Shown)>();
        foreach (var hwnd in windows)
        {
            if (!GetWindowRect(hwnd, out var r)) continue;
            // The window's rectangle holds invisible resize edges, which print as black bars.
            var status = DwmGetWindowAttribute(hwnd, DwmExtendedFrameBounds, out var shown, sizeof(NativeRect));
            if (status != 0) throw new InvalidOperationException($"HARNESS: DwmGetWindowAttribute: 0x{status:X}");
            rects.Add((hwnd, r, shown));
        }
        var left = rects.Min(w => w.Shown.Left);
        var top = rects.Min(w => w.Shown.Top);
        var right = rects.Max(w => w.Shown.Right);
        var bottom = rects.Max(w => w.Shown.Bottom);
        var canvas = Picture.Filled(right - left, bottom - top, new Rgb(128, 128, 128));
        foreach (var (hwnd, rect, shown) in Enumerable.Reverse(rects))
        {
            var shot = Capture(hwnd);
            for (var y = shown.Top; y < shown.Bottom; y++)
                Buffer.BlockCopy(shot.Bgra, ((y - rect.Top) * shot.Width + shown.Left - rect.Left) * 4, canvas.Bgra, ((y - top) * canvas.Width + shown.Left - left) * 4, shown.Width * 4);
        }
        return canvas;
    }

    /// False when a window has gone, which happens while the app is still starting.
    public static bool TryCanvasRect(nint main, nint canvas, out PixelRect rect)
    {
        rect = default;
        if (!GetWindowRect(main, out var m) || !GetWindowRect(canvas, out var c)) return false;
        rect = new PixelRect(c.Left - m.Left, c.Top - m.Top, c.Width, c.Height);
        return true;
    }

    public static PixelRect CanvasRect(nint main, nint canvas)
    {
        if (!GetWindowRect(main, out var m)) throw new InvalidOperationException($"HARNESS: GetWindowRect main: {Marshal.GetLastWin32Error()}");
        if (!GetWindowRect(canvas, out var c)) throw new InvalidOperationException($"HARNESS: GetWindowRect canvas: {Marshal.GetLastWin32Error()}");
        return new PixelRect(c.Left - m.Left, c.Top - m.Top, c.Width, c.Height);
    }

    /// One pixel from the screen, read only after Windows says the app's own window is at
    /// that point. Null means another window is there. This is what the user sees, and
    /// PrintWindow cannot tell that apart from what the window would draw.
    public static Rgb? ScreenPixelOf(nint window, int x, int y, out string coveredBy)
    {
        coveredBy = "";
        var there = WindowFromPoint(new NativePoint { X = x, Y = y });
        if (there != window)
        {
            coveredBy = ClassOf(there);
            return null;
        }
        var screen = GetDC(0);
        if (screen == 0) throw new InvalidOperationException("HARNESS: GetDC");
        var value = GetPixel(screen, x, y);
        ReleaseDC(0, screen);
        return new Rgb((byte)value, (byte)(value >> 8), (byte)(value >> 16));
    }

    /// One pixel from the screen, read only when the app's window or one of its children is at
    /// that point. A canvas held back until its first frame is not there, and the stage is.
    public static Rgb? ScreenPixelInApp(nint main, int x, int y, out string coveredBy)
    {
        coveredBy = "";
        var there = WindowFromPoint(new NativePoint { X = x, Y = y });
        if (there != main && !IsChild(main, there))
        {
            coveredBy = ClassOf(there);
            return null;
        }
        var screen = GetDC(0);
        if (screen == 0) throw new InvalidOperationException("HARNESS: GetDC");
        var value = GetPixel(screen, x, y);
        ReleaseDC(0, screen);
        return new Rgb((byte)value, (byte)(value >> 8), (byte)(value >> 16));
    }

    /// Puts the app above other programs and leaves the focus where it is.
    public static void RaiseWithoutFocus(nint hwnd)
    {
        const nint TopMost = -1, NotTopMost = -2;
        const uint NoSize = 1, NoMove = 2, NoActivate = 0x10;
        foreach (var band in new[] { TopMost, NotTopMost })
            if (!SetWindowPos(hwnd, band, 0, 0, 0, 0, NoSize | NoMove | NoActivate))
                throw new InvalidOperationException($"HARNESS: SetWindowPos: {Marshal.GetLastWin32Error()}");
    }

    /// Waits for the desktop to show what the app presented.
    public static void WaitForDisplay()
    {
        const int Frames = 2;
        for (var i = 0; i < Frames; i++)
        {
            var result = DwmFlush();
            if (result != 0) throw new InvalidOperationException($"HARNESS: DwmFlush: 0x{result:X}");
        }
    }

    public static void AskToClose(nint hwnd)
    {
        if (!PostMessageW(hwnd, WmClose, 0, 0)) throw new InvalidOperationException($"HARNESS: PostMessage WM_CLOSE: {Marshal.GetLastWin32Error()}");
    }

    /// Posts a key press to one window of the app. It reaches nothing else.
    public static void PostKey(nint hwnd, int virtualKey)
    {
        const uint WmKeyDown = 0x0100, WmKeyUp = 0x0101;
        if (!PostMessageW(hwnd, WmKeyDown, virtualKey, 0) || !PostMessageW(hwnd, WmKeyUp, virtualKey, 0))
            throw new InvalidOperationException($"HARNESS: PostMessage key: {Marshal.GetLastWin32Error()}");
    }

    public static void Minimize(nint hwnd)
    {
        const uint WmSysCommand = 0x0112;
        const nint ScMinimize = 0xF020;
        if (!PostMessageW(hwnd, WmSysCommand, ScMinimize, 0)) throw new InvalidOperationException($"HARNESS: PostMessage minimize: {Marshal.GetLastWin32Error()}");
    }

    /// Asks one window of the app for a size, with no focus change.
    public static void Resize(nint hwnd, int width, int height)
    {
        const uint NoMove = 2, NoZOrder = 4, NoActivate = 0x10;
        if (!SetWindowPos(hwnd, 0, 0, 0, width, height, NoMove | NoZOrder | NoActivate))
            throw new InvalidOperationException($"HARNESS: SetWindowPos: {Marshal.GetLastWin32Error()}");
    }

    /// Moves one window of the app a little, with no focus change.
    public static void MoveBy(nint hwnd, int pixels)
    {
        const uint NoSize = 1, NoZOrder = 4, NoActivate = 0x10;
        if (!GetWindowRect(hwnd, out var r)) throw new InvalidOperationException($"HARNESS: GetWindowRect: {Marshal.GetLastWin32Error()}");
        if (!SetWindowPos(hwnd, 0, r.Left + pixels, r.Top + pixels, 0, 0, NoSize | NoZOrder | NoActivate))
            throw new InvalidOperationException($"HARNESS: SetWindowPos: {Marshal.GetLastWin32Error()}");
    }

    public static int RefreshRate(nint hwnd)
    {
        var info = new MonitorInfoEx { Size = (uint)Marshal.SizeOf<MonitorInfoEx>() };
        if (!GetMonitorInfoW(MonitorFromWindow(hwnd, MonitorDefaultToNearest), ref info)) throw new InvalidOperationException("HARNESS: GetMonitorInfo");
        var mode = new DevMode { Size = (ushort)Marshal.SizeOf<DevMode>() };
        if (!EnumDisplaySettingsW(info.Device, EnumCurrentSettings, ref mode)) throw new InvalidOperationException("HARNESS: EnumDisplaySettings");
        return (int)mode.DisplayFrequency;
    }

    /// The display profile Windows gives this window, which is what the engine reads too.
    public static string DisplayProfile(nint hwnd)
    {
        var dc = GetDC(hwnd);
        if (dc == 0) throw new InvalidOperationException("HARNESS: GetDC for the profile");
        try
        {
            uint size = 512;
            var name = new StringBuilder((int)size);
            if (!GetICMProfileW(dc, ref size, name)) throw new InvalidOperationException($"HARNESS: GetICMProfile: {Marshal.GetLastWin32Error()}");
            return name.ToString();
        }
        finally
        {
            ReleaseDC(hwnd, dc);
        }
    }

    /// The whole machine's time so far, in 100 ns units across all cores.
    public static (long Busy, long Total) CpuTimes()
    {
        if (!GetSystemTimes(out var idle, out var kernel, out var user)) throw new InvalidOperationException($"HARNESS: GetSystemTimes: {Marshal.GetLastWin32Error()}");
        return (kernel + user - idle, kernel + user);
    }

    /// The whole machine's busy share over a time, from 0 to 100.
    public static double CpuBusyPercent(int ms)
    {
        var a = CpuTimes();
        Thread.Sleep(ms);
        var b = CpuTimes();
        return b.Total <= a.Total ? 0 : 100.0 * (b.Busy - a.Busy) / (b.Total - a.Total);
    }
}
