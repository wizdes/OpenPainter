using System.ComponentModel;
using System.IO;
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
    [DllImport("user32.dll")]
    internal static extern bool EnumChildWindows(IntPtr parent, EnumWindowsProc callback, IntPtr context);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    internal static extern int GetClassNameW(IntPtr window, StringBuilder name, int count);
    [DllImport("user32.dll")]
    internal static extern int GetDlgCtrlID(IntPtr window);
    [DllImport("user32.dll", EntryPoint = "GetWindowLongPtrW", SetLastError = true)]
    private static extern IntPtr GetWindowLongPtr(IntPtr window, int index);

    [DllImport("user32.dll", CharSet = CharSet.Unicode, EntryPoint = "SendMessageTimeoutW", SetLastError = true)]
    private static extern IntPtr SendText(IntPtr window, uint message, IntPtr wParam, string text, uint flags, uint ms, out IntPtr result);
    [DllImport("user32.dll", CharSet = CharSet.Unicode, EntryPoint = "SendMessageTimeoutW", SetLastError = true)]
    private static extern IntPtr SendBuffer(IntPtr window, uint message, IntPtr wParam, StringBuilder text, uint flags, uint ms, out IntPtr result);

    /// <summary>The live text of a control in another process. GetWindowTextW gives a stale copy for these.</summary>
    internal static string ReadText(IntPtr window)
    {
        const uint WmGetText = 0x000D, AbortIfHung = 2;
        var buffer = new StringBuilder(256);
        if (SendBuffer(window, WmGetText, 256, buffer, AbortIfHung, 5000, out _) == IntPtr.Zero)
            throw new Win32Exception(Marshal.GetLastWin32Error(), "Could not read a dialog control.");
        return buffer.ToString();
    }

    /// <summary>
    /// Types a value into a dialog's number box by window message, then moves on with Tab so the box
    /// commits. Stays inside the app: this is not SendInput. Each label names the Edit control that
    /// follows it in the dialog's control order.
    /// </summary>
    internal static void SetLabelledNumber(IntPtr dialog, string label, string value)
    {
        var controls = new List<(IntPtr Handle, string Class, string Text)>();
        EnumChildWindows(dialog, (child, _) =>
        {
            var cls = new StringBuilder(256);
            var title = new StringBuilder(256);
            GetClassNameW(child, cls, 256);
            GetWindowTextW(child, title, 256);
            controls.Add((child, cls.ToString(), title.ToString()));
            return true;
        }, IntPtr.Zero);
        var at = controls.FindIndex(c => c.Text == label && !c.Class.Contains(".Edit.", StringComparison.Ordinal));
        if (at < 0) throw new System.IO.InvalidDataException($"Dialog has no label named '{label}'.");
        var edit = controls.Skip(at + 1)
            .TakeWhile(c => c.Text.Length == 0 || c.Class.Contains(".Edit.", StringComparison.Ordinal))
            .FirstOrDefault(c => c.Class.Contains(".Edit.", StringComparison.Ordinal));
        if (edit.Handle == IntPtr.Zero) throw new System.IO.InvalidDataException($"Label '{label}' has no number box after it.");

        const uint WmSetText = 0x000C, WmLButtonDown = 0x0201, WmLButtonUp = 0x0202, WmKeyDown = 0x0100, WmKeyUp = 0x0101, AbortIfHung = 2;
        RequirePosted(edit.Handle, WmLButtonDown, 1, 0x00050005, label);
        RequirePosted(edit.Handle, WmLButtonUp, 0, 0x00050005, label);
        Thread.Sleep(200);
        if (SendText(edit.Handle, WmSetText, 0, value, AbortIfHung, 5000, out _) == IntPtr.Zero)
            throw new Win32Exception(Marshal.GetLastWin32Error(), $"Could not type into '{label}'.");
        Thread.Sleep(100);
        RequirePosted(edit.Handle, WmKeyDown, 0x09, 1, label);
        RequirePosted(edit.Handle, WmKeyUp, 0x09, unchecked((IntPtr)0xC0000001), label);
        Thread.Sleep(400);
        var shown = ReadText(edit.Handle);
        var style = System.Globalization.NumberStyles.Float;
        var culture = System.Globalization.CultureInfo.InvariantCulture;
        if (!double.TryParse(shown, style, culture, out var got) || !double.TryParse(value, style, culture, out var want) || got != want)
            throw new InvalidOperationException($"'{label}' shows '{shown}' after setting '{value}'.");
    }

    internal static Dictionary<string,double> ReadAllNumbers(IntPtr dialog)
    {
        var controls = new List<(IntPtr Handle, string Class, string Text)>();
        EnumChildWindows(dialog, (child, _) =>
        {
            var cls = new StringBuilder(256);
            GetClassNameW(child, cls, 256);
            var title = new StringBuilder(256);
            GetWindowTextW(child, title, 256);
            controls.Add((child, cls.ToString(), title.ToString()));
            return true;
        }, IntPtr.Zero);
        var found = new Dictionary<string,double>(StringComparer.Ordinal);
        for (var i = 0; i < controls.Count; i++)
        {
            if (!controls[i].Class.Contains(".Edit.", StringComparison.Ordinal)) continue;
            var label = controls.Take(i).LastOrDefault(c => !string.IsNullOrWhiteSpace(c.Text) &&
                !c.Class.Contains(".Edit.", StringComparison.Ordinal)).Text;
            if (string.IsNullOrWhiteSpace(label)) throw new InvalidDataException("Number box has no label.");
            var shown = ReadText(controls[i].Handle);
            if (!double.TryParse(shown, System.Globalization.NumberStyles.Float,
                    System.Globalization.CultureInfo.InvariantCulture, out var number))
                throw new InvalidDataException($"Number box '{label}' shows '{shown}'.");
            if (!found.TryAdd(label, number)) throw new InvalidDataException($"Duplicate number label '{label}'.");
        }
        return found;
    }

    internal static Dictionary<string,string> ReadOtherControls(IntPtr dialog)
    {
        var found = new Dictionary<string,string>(StringComparer.Ordinal);
        var lastLabel = "";
        EnumChildWindows(dialog, (child, _) =>
        {
            var cls = new StringBuilder(256);
            GetClassNameW(child, cls, 256);
            var title = new StringBuilder(256);
            GetWindowTextW(child, title, 256);
            var kind = cls.ToString();
            var buttonStyle = kind.Equals("Button", StringComparison.OrdinalIgnoreCase) ?
                GetWindowLongPtr(child, -16).ToInt64() & 0xF : -1;
            if (kind.Contains("ComboBox", StringComparison.OrdinalIgnoreCase) ||
                kind.Contains("CheckBox", StringComparison.OrdinalIgnoreCase) ||
                buttonStyle is 2 or 3 or 5 or 6)
            {
                var name = string.IsNullOrWhiteSpace(title.ToString()) ? lastLabel : title.ToString();
                if (string.IsNullOrWhiteSpace(name)) throw new InvalidDataException("List or check box has no label.");
                if (!found.TryAdd(name, "unread")) throw new InvalidDataException($"Duplicate control label '{name}'.");
            }
            else if (!string.IsNullOrWhiteSpace(title.ToString())) lastLabel = title.ToString();
            return true;
        }, IntPtr.Zero);
        return found;
    }

    private static void RequirePosted(IntPtr window, uint message, IntPtr wParam, IntPtr lParam, string name)
    {
        Decisions.RequirePosted(PostMessageW(window, message, wParam, lParam), name);
    }

    /// <summary>Presses a dialog button by posting a click to it. Focus can sit on another button, so Enter is not safe here.</summary>
    internal static void ClickButton(IntPtr dialog, string text)
    {
        const uint WmLButtonDown = 0x0201, WmLButtonUp = 0x0202;
        var button = IntPtr.Zero;
        EnumChildWindows(dialog, (child, _) =>
        {
            var title = new StringBuilder(256);
            GetWindowTextW(child, title, 256);
            if (title.ToString() != text) return true;
            button = child;
            return false;
        }, IntPtr.Zero);
        if (button == IntPtr.Zero) throw new System.IO.InvalidDataException($"Dialog has no button named '{text}'.");
        RequirePosted(button, WmLButtonDown, 1, 0x00050005, text);
        RequirePosted(button, WmLButtonUp, 0, 0x00050005, text);
    }

    /// <summary>Class, id, position and title of every child of a window, for finding a dialog's controls.</summary>
    internal static List<string> DescribeChildren(IntPtr parent)
    {
        var lines = new List<string>();
        EnumChildWindows(parent, (child, _) =>
        {
            var cls = new StringBuilder(256);
            var title = new StringBuilder(256);
            GetClassNameW(child, cls, 256);
            GetWindowTextW(child, title, 256);
            GetWindowRect(child, out var r);
            lines.Add($"{child} visible={IsWindowVisible(child)} id={GetDlgCtrlID(child)} class={cls} rect={r.Left},{r.Top},{r.Right},{r.Bottom} text={title}");
            return true;
        }, IntPtr.Zero);
        return lines;
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
            var error = Marshal.GetLastWin32Error();
            throw new Win32Exception(error, "Could not post Enter.");
        }
    }
}
