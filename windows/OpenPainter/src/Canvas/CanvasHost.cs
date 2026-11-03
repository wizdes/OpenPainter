// The native child window the engine draws into.
//
// It is a custom window class, because W5 found that a STATIC child passes the
// mouse through. The engine's render thread draws into it, so its frame rate does
// not depend on Avalonia's UI frame callback, which W5 measured at 41 to 44 per
// second. A native child keeps the Tab key, so it hands focus back on Tab.
using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;
using Avalonia.Controls;
using Avalonia.Platform;

namespace OpenPainter.Canvas;

public sealed unsafe class CanvasHost : NativeControlHost
{
    const string ClassName = "OpenPainterCanvas";
    const uint WsChild = 0x40000000, WsVisible = 0x10000000, WsTabStop = 0x00010000;
    const uint WmSetFocus = 0x0007, WmKillFocus = 0x0008, WmEraseBackground = 0x0014, WmKeyDown = 0x0100, WmChar = 0x0102,
        WmMouseMove = 0x0200, WmLeftDown = 0x0201, WmLeftUp = 0x0202, WmMouseWheel = 0x020A,
        WmPointerUpdate = 0x0245, WmPointerDown = 0x0246, WmPointerUp = 0x0247;
    const int VkTab = 9, VkShift = 0x10, PointerTypePen = 3;
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

    static bool s_registered;
    static CanvasHost? s_current;

    /// Canvas input in W5's text form, for a test harness and later tools.
    public event Action<string>? NativeInput;

    /// Tab was pressed on the canvas. True means Shift was held.
    public event Action<bool>? TabOut;

    public nint Hwnd { get; private set; }

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
        return new PlatformHandle(Hwnd, "HWND");
    }

    protected override void DestroyNativeControlCore(IPlatformHandle control)
    {
        if (!DestroyWindow(control.Handle)) throw new InvalidOperationException($"DESTROY_CANVAS: {Marshal.GetLastWin32Error()}");
        Hwnd = 0;
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

    [UnmanagedCallersOnly(CallConvs = [typeof(CallConvStdcall)])]
    static nint OnMessage(nint hwnd, uint message, nuint w, nint l)
    {
        var host = s_current;
        var input = host?.NativeInput;
        switch (message)
        {
            // The engine paints every pixel, so Windows must not clear it first.
            case WmEraseBackground: return 1;
            case WmLeftDown:
                SetFocus(hwnd);
                input?.Invoke($"pointer:down:{(short)(l & 0xffff)},{(short)((l >> 16) & 0xffff)}:1.00:Mouse");
                return 0;
            case WmMouseMove: input?.Invoke($"pointer:move:{(short)(l & 0xffff)},{(short)((l >> 16) & 0xffff)}:1.00:Mouse"); return 0;
            case WmLeftUp: input?.Invoke("pointer:up:1.00:Mouse"); return 0;
            case WmMouseWheel: input?.Invoke($"wheel:{(short)((long)w >> 16)}"); return 0;
            case WmPointerDown: LogPen("down", w); break;
            case WmPointerUpdate: LogPen("move", w); break;
            case WmPointerUp: LogPen("up", w); break;
            case WmKeyDown:
                input?.Invoke($"key:{w}");
                if ((int)w == VkTab)
                {
                    SetFocus(GetAncestor(hwnd, RootAncestor));
                    host?.TabOut?.Invoke(GetKeyState(VkShift) < 0);
                    return 0;
                }
                break;
            case WmChar:
                if ((int)w != VkTab) input?.Invoke($"text:{(char)w}");
                return 0;
            case WmSetFocus: input?.Invoke("focus:canvas"); break;
            case WmKillFocus: input?.Invoke("blur:canvas"); break;
        }
        return DefWindowProcW(hwnd, message, w, l);
    }
}
