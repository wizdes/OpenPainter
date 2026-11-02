using System.ComponentModel;
using System.Text;
using System.Runtime.InteropServices;

namespace OpenPainter.Capture.Driver;

internal static class Native
{
    internal const uint CreateUnicodeEnvironment = 0x400;
    internal const uint DesktopAllAccess = 0x01FF;
    internal const uint WaitObject0 = 0;

    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    internal struct StartupInfo
    {
        public uint cb;
        public string? lpReserved;
        public string? lpDesktop;
        public string? lpTitle;
        public uint dwX, dwY, dwXSize, dwYSize, dwXCountChars, dwYCountChars;
        public uint dwFillAttribute, dwFlags;
        public short wShowWindow, cbReserved2;
        public IntPtr lpReserved2, hStdInput, hStdOutput, hStdError;
    }

    [StructLayout(LayoutKind.Sequential)]
    internal struct ProcessInfo
    {
        public IntPtr hProcess, hThread;
        public uint dwProcessId, dwThreadId;
    }

    internal delegate bool EnumWindowsProc(IntPtr window, IntPtr context);

    [DllImport("user32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    internal static extern IntPtr CreateDesktopW(string name, IntPtr device, IntPtr devMode,
        uint flags, uint desiredAccess, IntPtr securityAttributes);
    [DllImport("user32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    internal static extern bool CloseDesktop(IntPtr desktop);
    [DllImport("user32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    internal static extern bool SetThreadDesktop(IntPtr desktop);
    [DllImport("user32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    internal static extern bool EnumWindows(EnumWindowsProc callback, IntPtr context);
    [DllImport("user32.dll", SetLastError = true)]
    internal static extern uint GetWindowThreadProcessId(IntPtr window, out uint processId);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    internal static extern bool CreateProcessW(string application, System.Text.StringBuilder commandLine, IntPtr processAttributes,
        IntPtr threadAttributes, bool inheritHandles, uint creationFlags, IntPtr environment,
        string workingDirectory, ref StartupInfo startupInfo, out ProcessInfo processInfo);
    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    internal static extern bool CloseHandle(IntPtr handle);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    internal static extern int GetWindowTextW(IntPtr window, StringBuilder text, int count);
    [DllImport("user32.dll", SetLastError = true)]
    internal static extern IntPtr GetThreadDesktop(uint threadId);
    [DllImport("kernel32.dll")]
    internal static extern uint GetCurrentThreadId();
    [DllImport("user32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    internal static extern bool IsWindowVisible(IntPtr window);
    [DllImport("user32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool PrintWindow(IntPtr window, IntPtr dc, uint flags);
    [DllImport("user32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool GetWindowRect(IntPtr window, out Rect rect);
    [StructLayout(LayoutKind.Sequential)]
    private struct Rect { public int Left, Top, Right, Bottom; }

    internal static string PrintWindowToPng(IntPtr window, string path)
    {
        if (!GetWindowRect(window, out var r)) return "no rectangle";
        using var image = new System.Drawing.Bitmap(Math.Max(1, r.Right - r.Left), Math.Max(1, r.Bottom - r.Top));
        using var g = System.Drawing.Graphics.FromImage(image);
        var dc = g.GetHdc();
        var ok = PrintWindow(window, dc, 2);
        g.ReleaseHdc(dc);
        image.Save(path, System.Drawing.Imaging.ImageFormat.Png);
        return $"{image.Width}x{image.Height} printed={ok}";
    }
    [DllImport("user32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool PostMessageW(IntPtr window, uint message, IntPtr wParam, IntPtr lParam);
    [DllImport("user32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool GetGUIThreadInfo(uint thread, ref GuiThreadInfo info);
    [StructLayout(LayoutKind.Sequential)]
    private struct GuiThreadInfo
    {
        public int cbSize, flags;
        public IntPtr hwndActive, hwndFocus, hwndCapture, hwndMenuOwner, hwndMoveSize, hwndCaret;
        public Rect rcCaret;
    }

    internal static void PressEnter(IntPtr dialog)
    {
        const uint WmKeyDown = 0x0100, WmKeyUp = 0x0101, WmChar = 0x0102;
        var thread = GetWindowThreadProcessId(dialog, out _);
        var info = new GuiThreadInfo { cbSize = Marshal.SizeOf<GuiThreadInfo>() };
        var target = GetGUIThreadInfo(thread, ref info) && info.hwndFocus != IntPtr.Zero ? info.hwndFocus : dialog;
        if (!PostMessageW(target, WmKeyDown, 0x0D, 1) || !PostMessageW(target, WmChar, 0x0D, 1) ||
            !PostMessageW(target, WmKeyUp, 0x0D, unchecked((IntPtr)0xC0000001)))
        {
            const int ErrorInvalidWindowHandle = 1400;
            var error = Marshal.GetLastWin32Error();
            if (error != ErrorInvalidWindowHandle) throw new Win32Exception(error, "Could not post Enter.");
        }
    }
}
