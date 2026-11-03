// Size limits in client pixels, for windows that draw their own title row. Avalonia counts
// a title bar they have turned off, so they answer WM_GETMINMAXINFO themselves.
using System.Runtime.InteropServices;
using Avalonia;

namespace OpenPainter.Shell;

public static unsafe partial class WindowLimits
{
    const uint WmGetMinMaxInfo = 0x0024;
    const nint TopMost = -1, NotTopMost = -2;
    const uint SwpNoSize = 1, SwpNoMove = 2, SwpNoActivate = 0x10;

    [StructLayout(LayoutKind.Sequential)] struct NativePoint { public int X, Y; }
    [StructLayout(LayoutKind.Sequential)] struct MinMaxInfo { public NativePoint Reserved, MaxSize, MaxPosition, MinTrack, MaxTrack; }
    [StructLayout(LayoutKind.Sequential)] struct NativeRect { public int Left, Top, Right, Bottom; }
    [StructLayout(LayoutKind.Sequential)] struct MonitorInfo { public uint Size; public NativeRect Monitor, Work; public uint Flags; }

    const uint MonitorDefaultToNearest = 2;

    [LibraryImport("user32.dll")] private static partial nint MonitorFromWindow(nint hwnd, uint flags);
    [LibraryImport("user32.dll", EntryPoint = "GetMonitorInfoW")]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static partial bool GetMonitorInfo(nint monitor, ref MonitorInfo info);

    [LibraryImport("user32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static partial bool GetWindowRect(nint hwnd, out NativeRect rect);
    [LibraryImport("user32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static partial bool SetWindowPos(nint hwnd, nint after, int x, int y, int cx, int cy, uint flags);

    /// The window size that gives this client size. The system's invisible resize edges
    /// count in the window's size and not in the client's.
    public static (int X, int Y) Track(Size client, double scale, int frameX, int frameY) =>
        ((int)Math.Round(client.Width * scale) + frameX, (int)Math.Round(client.Height * scale) + frameY);

    /// The most client this work area can show, in layout points.
    public static Size RoomOn(PixelSize work, double scale, int frameX, int frameY) =>
        new(Math.Max(1, work.Width - frameX) / scale, Math.Max(1, work.Height - frameY) / scale);

    /// A minimum no monitor can show would push the window's far side off the screen, so this
    /// monitor's work area caps it: 792 points are 1188 pixels at 150%, more than 1080p has.
    public static Size Capped(Size minimum, double scale, PixelSize work, int frameX, int frameY)
    {
        var room = RoomOn(work, scale, frameX, frameY);
        return new Size(Math.Min(minimum.Width, room.Width), Math.Min(minimum.Height, room.Height));
    }

    /// The work area of the monitor the window is on, in pixels.
    public static PixelRect WorkAreaOf(nint hwnd)
    {
        var info = new MonitorInfo { Size = (uint)sizeof(MonitorInfo) };
        if (!GetMonitorInfo(MonitorFromWindow(hwnd, MonitorDefaultToNearest), ref info)) throw new InvalidOperationException("WINDOW_SIZE: GetMonitorInfo failed");
        return new PixelRect(info.Work.Left, info.Work.Top, info.Work.Right - info.Work.Left, info.Work.Bottom - info.Work.Top);
    }

    /// What the window's rectangle holds beyond its client: its invisible resize edges.
    public static (int X, int Y) FrameOf(nint hwnd, Size client, double scale)
    {
        if (!GetWindowRect(hwnd, out var rect)) throw new InvalidOperationException($"WINDOW_SIZE: GetWindowRect: {Marshal.GetLastPInvokeError()}");
        return (rect.Right - rect.Left - (int)Math.Round(client.Width * scale), rect.Bottom - rect.Top - (int)Math.Round(client.Height * scale));
    }

    public static nint OnMessage(nint hwnd, uint message, nint l, Size client, double scale, Size minimum, double? maximumWidth, double? maximumHeight, PixelSize? testWork, ref bool handled)
    {
        if (message != WmGetMinMaxInfo) return 0;
        var (frameX, frameY) = FrameOf(hwnd, client, scale);
        var work = testWork ?? WorkAreaOf(hwnd).Size;
        var info = (MinMaxInfo*)l;
        (info->MinTrack.X, info->MinTrack.Y) = Track(Capped(minimum, scale, work, frameX, frameY), scale, frameX, frameY);
        if (maximumWidth is { } width) info->MaxTrack.X = (int)Math.Round(width * scale) + frameX;
        if (maximumHeight is { } height) info->MaxTrack.Y = (int)Math.Round(height * scale) + frameY;
        handled = true;
        return 0;
    }

    /// A test hook: puts the window above other programs and leaves the focus where it is,
    /// so a test window that opened behind a terminal can be checked with no key going astray.
    public static void RaiseWithoutFocus(nint hwnd)
    {
        foreach (var band in new[] { TopMost, NotTopMost })
            if (!SetWindowPos(hwnd, band, 0, 0, 0, 0, SwpNoSize | SwpNoMove | SwpNoActivate))
                throw new InvalidOperationException($"TEST_RAISE: SetWindowPos: {Marshal.GetLastPInvokeError()}");
    }
}
