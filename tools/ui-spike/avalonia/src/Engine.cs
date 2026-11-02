using System.Runtime.InteropServices;

namespace SpikeAvalonia;

[StructLayout(LayoutKind.Sequential)]
public struct Rect { public double X, Y, Width, Height; public Rect(double x, double y, double w, double h) => (X,Y,Width,Height)=(x,y,w,h); }
[StructLayout(LayoutKind.Sequential)]
public struct OverlayPoint { public double X, Y; public OverlayPoint(double x, double y) => (X,Y)=(x,y); }
[StructLayout(LayoutKind.Sequential)]
public unsafe struct OverlayItem { public uint Kind, Flags; public OverlayPoint* Points; public nuint PointCount; public double X, Y, Width, Height; }
[StructLayout(LayoutKind.Sequential)]
public unsafe struct ViewState { public double Width, Height, Scale, Zoom, X, Y; public byte* Icc; public nuint IccLength; public uint Flags; }
[StructLayout(LayoutKind.Sequential)]
public struct ViewInfo {
    public ulong Doc; public double Width, Height, Scale, Zoom, X, Y; public uint Flags;
    public byte Tagged, Table, Suspended, Pad; public ulong IccLength, Frames;
    public uint DrawableWidth, DrawableHeight; public ulong Configures, TagChanges, Retags, TableBuilds, MipBuilds, Uploads, Revision, FrameErrors;
    public uint LastError; public ulong RegionUploads, RegionTiles; public float BackgroundR, BackgroundG, BackgroundB, Pad2;
}
public static unsafe class Engine {
    [DllImport("op_ffi", CallingConvention=CallingConvention.Cdecl)] public static extern nint op_engine_new(nint config, nint callback, nint user);
    [DllImport("op_ffi", CallingConvention=CallingConvention.Cdecl)] public static extern void op_engine_free(nint engine);
    [DllImport("op_ffi", CallingConvention=CallingConvention.Cdecl)] public static extern int op_doc_new(nint engine, uint width, uint height, double dpi, uint rgba, out ulong doc);
    [DllImport("op_ffi", CallingConvention=CallingConvention.Cdecl)] public static extern int op_view_attach(nint engine, ulong doc, nint hwnd, out ulong view);
    [DllImport("op_ffi", CallingConvention=CallingConvention.Cdecl)] public static extern int op_view_set(nint engine, ulong view, ref ViewState state);
    [DllImport("op_ffi", CallingConvention=CallingConvention.Cdecl)] public static extern int op_view_set_overlay(nint engine, ulong view, OverlayItem* items, nuint count);
    [DllImport("op_ffi", CallingConvention=CallingConvention.Cdecl)] public static extern int op_view_set_overlay_dash(nint engine, ulong view, double dash);
    [DllImport("op_ffi", CallingConvention=CallingConvention.Cdecl)] public static extern int op_view_info(nint engine, ulong view, out ViewInfo info);
    [DllImport("op_ffi", CallingConvention=CallingConvention.Cdecl)] public static extern int op_view_take_frame_times(nint engine, ulong view, double* times, nuint cap, out nuint length);
    [DllImport("op_ffi", CallingConvention=CallingConvention.Cdecl)] public static extern int op_view_detach(nint engine, ulong view);
    public static void Check(int code, string name) { if (code != 0) throw new InvalidOperationException($"{name}: OP_STATUS_{code}"); }
}
