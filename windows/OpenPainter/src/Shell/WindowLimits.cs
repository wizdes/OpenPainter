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

    public static nint OnMessage(nint hwnd, uint message, nint l, Size client, double scale, Size minimum, double? maximumWidth, double? maximumHeight, ref bool handled)
    {
        if (message != WmGetMinMaxInfo) return 0;
        if (!GetWindowRect(hwnd, out var rect)) throw new InvalidOperationException($"WINDOW_SIZE: GetWindowRect: {Marshal.GetLastPInvokeError()}");
        var frameX = rect.Right - rect.Left - (int)Math.Round(client.Width * scale);
        var frameY = rect.Bottom - rect.Top - (int)Math.Round(client.Height * scale);
        var info = (MinMaxInfo*)l;
        (info->MinTrack.X, info->MinTrack.Y) = Track(minimum, scale, frameX, frameY);
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
