using System.Runtime.InteropServices;

namespace SpikeWinUI;

[StructLayout(LayoutKind.Sequential)]
internal unsafe struct ViewState { public double Width, Height, Scale, Zoom, X, Y; public byte* Icc; public nuint IccLength; public uint Flags; }
[StructLayout(LayoutKind.Sequential)]
internal struct ViewInfo {
    public ulong Doc; public double Width, Height, Scale, Zoom, X, Y; public uint Flags;
    public byte Tagged, Table, Suspended, Pad; public ulong IccLength, Frames;
    public uint DrawableWidth, DrawableHeight; public ulong Configures, TagChanges, Retags, TableBuilds, MipBuilds, Uploads, Revision, FrameErrors;
    public uint LastError; public ulong RegionUploads, RegionTiles; public float BackgroundR, BackgroundG, BackgroundB, Pad2;
}
internal static unsafe class Engine {
    [DllImport("op_ffi", CallingConvention=CallingConvention.Cdecl)] public static extern nint op_engine_new(nint config, nint callback, nint user);
    [DllImport("op_ffi", CallingConvention=CallingConvention.Cdecl)] public static extern void op_engine_free(nint engine);
    [DllImport("op_ffi", CallingConvention=CallingConvention.Cdecl)] public static extern int op_doc_new(nint engine, uint width, uint height, double dpi, uint rgba, out ulong doc);
    [DllImport("op_ffi", CallingConvention=CallingConvention.Cdecl)] public static extern int op_view_attach_panel(nint engine, ulong doc, nint panel, out ulong view);
    [DllImport("op_ffi", CallingConvention=CallingConvention.Cdecl)] public static extern int op_view_set(nint engine, ulong view, ref ViewState state);
    [DllImport("op_ffi", CallingConvention=CallingConvention.Cdecl)] public static extern int op_view_info(nint engine, ulong view, out ViewInfo info);
    [DllImport("op_ffi", CallingConvention=CallingConvention.Cdecl)] public static extern int op_view_take_frame_times(nint engine, ulong view, double* times, nuint cap, out nuint length);
    [DllImport("op_ffi", CallingConvention=CallingConvention.Cdecl)] public static extern int op_view_detach(nint engine, ulong view);
    public static void Check(int code, string name) { if (code != 0) throw new InvalidOperationException($"{name}: OP_STATUS_{code}"); }
}
