// Written by windows/tools/bridge-gen from engine/op-ffi/include/openpainter.h. Do not edit by hand.
#nullable enable
using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;

namespace OpenPainter.Bridge;

public enum OpStatus
{
    OP_OK = 0,
    OP_ERR_BAD_ARGUMENT = 1,
    OP_ERR_NO_SUCH_DOC = 2,
    OP_ERR_READ_ONLY = 3,
    OP_ERR_PANIC = 4,
    OP_ERR_BUSY = 5,
    OP_ERR_INTERNAL = 6,
    OP_ERR_UNSUPPORTED = 7,
    OP_ERR_CANCELLED = 8,
}

public enum OpEventKind
{
    OP_EVENT_DOC_CHANGED = 1,
    OP_EVENT_HISTORY_CHANGED = 2,
    OP_EVENT_LAYERS_CHANGED = 3,
    OP_EVENT_JOB_FINISHED = 4,
    OP_EVENT_PROGRESS = 5,
    OP_EVENT_RECOVERY = 6,
}

public struct OpEngine { }

[StructLayout(LayoutKind.Sequential)]
public unsafe struct OpConfig
{
    public uint reserved;
}

[StructLayout(LayoutKind.Sequential)]
public unsafe struct OpEvent
{
    public uint kind;
    public ulong doc;
    public ulong ticket;
    public byte* json_ptr;
    public nuint json_len;
}

[StructLayout(LayoutKind.Sequential)]
public unsafe struct OpBuffer
{
    public byte* ptr;
    public nuint len;
}

[StructLayout(LayoutKind.Sequential)]
public unsafe struct OpPixelBuffer
{
    public byte* ptr;
    public nuint len;
    public uint width;
    public uint height;
}

[StructLayout(LayoutKind.Sequential)]
public unsafe struct OpPointerEvent
{
    public double x;
    public double y;
    public float pressure;
    public uint phase;
}

[StructLayout(LayoutKind.Sequential)]
public unsafe struct OpPipelineCounts
{
    public ulong effect;
    public ulong view;
    public ulong mip;
}

[StructLayout(LayoutKind.Sequential)]
public unsafe struct OpStrokeSample
{
    public double x;
    public double y;
    public double pressure;
    public double tilt_x;
    public double tilt_y;
    public ulong time_ms;
}

[StructLayout(LayoutKind.Sequential)]
public unsafe struct OpViewState
{
    public double width_pt;
    public double height_pt;
    public double scale;
    public double zoom;
    public double origin_x;
    public double origin_y;
    public byte* display_icc;
    public nuint display_icc_len;
    public uint flags;
}

[StructLayout(LayoutKind.Sequential)]
public unsafe struct OpOverlayPoint
{
    public double x;
    public double y;
}

[StructLayout(LayoutKind.Sequential)]
public unsafe struct OpOverlayItem
{
    public uint kind;
    public uint flags;
    public OpOverlayPoint* points;
    public nuint point_count;
    public double x;
    public double y;
    public double width;
    public double height;
}

[StructLayout(LayoutKind.Sequential)]
public unsafe struct OpViewInfo
{
    public ulong doc;
    public double width_pt;
    public double height_pt;
    public double scale;
    public double zoom;
    public double origin_x;
    public double origin_y;
    public uint flags;
    public byte layer_tag_is_display;
    public byte has_display_table;
    public byte suspended;
    public byte _pad;
    public ulong display_icc_len;
    public ulong frames;
    public uint drawable_width;
    public uint drawable_height;
    public ulong configures;
    public ulong tag_changed_by_configure;
    public ulong retags;
    public ulong table_builds;
    public ulong mip_builds;
    public ulong doc_uploads;
    public ulong doc_revision;
    public ulong frame_errors;
    public uint last_error;
    public ulong region_uploads;
    public ulong region_tiles;
    public float background_red;
    public float background_green;
    public float background_blue;
    public float _pad2;
}

[StructLayout(LayoutKind.Sequential)]
public unsafe struct OpViewCounters
{
    public ulong live_views;
    public ulong live_layers;
    public ulong live_surfaces;
    public ulong wgpu_textures;
    public ulong wgpu_texture_views;
    public ulong wgpu_buffers;
    public ulong wgpu_bind_groups;
    public ulong validation_errors;
    public ulong frames_total;
}

[StructLayout(LayoutKind.Sequential)]
public unsafe struct OpFrameStamp
{
    public ulong time_ns;
    public ulong revision;
    public ulong preview_serial;
}

public static unsafe partial class Native
{
    public const string Library = "op_ffi";

    public const long MAX_PNG_FILE = ((((2L * 1024L) * 1024L) * 1024L) + ((16L * 1024L) * 1024L));
    public const long OP_VIEW_PIXEL_GRID = 1L;
    public const long OP_VIEW_SUSPENDED = 2L;

    [LibraryImport(Library)]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    public static partial OpEngine* op_engine_new(OpConfig* _config, delegate* unmanaged[Cdecl]<OpEvent*, void*, void> event_fn, void* user);

    [LibraryImport(Library)]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    public static partial void op_engine_free(OpEngine* engine);

    [LibraryImport(Library)]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    public static partial OpStatus op_import_pixels(OpEngine* p, uint width, uint height, byte* rgba, nuint len, byte* icc, nuint icc_len, ulong* out_doc);

    [LibraryImport(Library)]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    public static partial OpStatus op_doc_open_png(OpEngine* p, byte* path, nuint len, ulong* out_doc);

    [LibraryImport(Library)]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    public static partial OpStatus op_dispatch(OpEngine* p, ulong doc, byte* cmd_id, byte* json, nuint len, ulong* out_ticket);

    [LibraryImport(Library)]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    public static partial OpStatus op_query(OpEngine* p, ulong doc, byte* query_id, byte* json, nuint len, OpBuffer* @out);

    [LibraryImport(Library)]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    public static partial OpStatus op_cancel(OpEngine* p, ulong ticket);

    [LibraryImport(Library)]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    public static partial OpStatus op_doc_close(OpEngine* p, ulong doc);

    [LibraryImport(Library)]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    public static partial void op_buffer_free(OpBuffer* b);

    [LibraryImport(Library)]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    public static partial void op_pixels_free(OpPixelBuffer* b);

    [LibraryImport(Library)]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    public static partial OpStatus op_pointer(OpEngine* _p, ulong _view, OpPointerEvent* _events, nuint _len);

    [LibraryImport(Library)]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    public static partial OpStatus op_paste_begin(OpEngine* p, ulong doc, byte* json, nuint len, byte* rgba, nuint rgba_len, byte* icc, nuint icc_len, OpBuffer* @out);

    [LibraryImport(Library)]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    public static partial OpStatus op_png_decode(byte* png, nuint len, OpPixelBuffer* out_pixels, OpBuffer* out_icc);

    [LibraryImport(Library)]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    public static partial OpStatus op_doc_new(OpEngine* p, uint width, uint height, double dpi, uint background_rgba, ulong* out_doc);

    [LibraryImport(Library)]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    public static partial OpStatus op_doc_open_opaint(OpEngine* p, byte* path, nuint len, ulong* out_doc);

    [LibraryImport(Library)]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    public static partial OpStatus op_doc_save(OpEngine* p, ulong doc, byte* json, nuint len);

    [LibraryImport(Library)]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    public static partial OpStatus op_doc_mark_saved(OpEngine* p, ulong doc, ulong revision);

    [LibraryImport(Library)]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    public static partial OpStatus op_doc_replace(OpEngine* p, ulong doc, ulong source);

    [LibraryImport(Library)]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    public static partial OpStatus op_doc_mark_unsaved(OpEngine* p, ulong doc);

    [LibraryImport(Library)]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    public static partial OpStatus op_doc_finish_sessions(OpEngine* p, ulong doc);

    [LibraryImport(Library)]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    public static partial OpStatus op_doc_set_image_metadata(OpEngine* p, ulong doc, byte* text, nuint len);

    [LibraryImport(Library)]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    public static partial OpStatus op_encode_image(byte* json, nuint json_len, uint width, uint height, byte* rgba, nuint rgba_len, byte* icc, nuint icc_len, byte* exif, nuint exif_len, byte* xmp, nuint xmp_len, OpBuffer* @out);

    [LibraryImport(Library)]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    public static partial OpStatus op_import_pixels_at(OpEngine* p, uint width, uint height, byte* rgba, nuint len, byte* icc, nuint icc_len, double dpi, ulong* out_doc);

    [LibraryImport(Library)]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    public static partial OpStatus op_move_begin(OpEngine* p, ulong doc, byte* json, nuint len);

    [LibraryImport(Library)]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    public static partial OpStatus op_move_update(OpEngine* p, ulong doc, byte* json, nuint len, ulong* out_serial);

    [LibraryImport(Library)]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    public static partial OpStatus op_move_finish(OpEngine* p, ulong doc);

    [LibraryImport(Library)]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    public static partial OpStatus op_move_cancel(OpEngine* p, ulong doc);

    [LibraryImport(Library)]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    public static partial OpStatus op_gpu_pipeline_counts(OpPipelineCounts* @out);

    [LibraryImport(Library)]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    public static partial OpStatus op_recovery_attach(OpEngine* p, ulong doc, byte* json, nuint len);

    [LibraryImport(Library)]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    public static partial OpStatus op_recovery_flush(OpEngine* p, ulong doc);

    [LibraryImport(Library)]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    public static partial OpStatus op_recovery_stop(OpEngine* p, ulong doc, [MarshalAs(UnmanagedType.U1)] bool delete_record);

    [LibraryImport(Library)]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    public static partial OpStatus op_recovery_detach(OpEngine* p, ulong doc, [MarshalAs(UnmanagedType.U1)] bool delete_record);

    [LibraryImport(Library)]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    public static partial OpStatus op_recovery_wait_stopped(OpEngine* p, ulong timeout_ms);

    [LibraryImport(Library)]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    public static partial OpStatus op_recovery_scan(byte* root, nuint len, OpBuffer* @out);

    [LibraryImport(Library)]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    public static partial OpStatus op_recovery_open(OpEngine* p, byte* json, nuint len, ulong* out_doc);

    [LibraryImport(Library)]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    public static partial OpStatus op_recovery_discard(byte* json, nuint len);

    [LibraryImport(Library)]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    public static partial OpStatus op_stroke_begin(OpEngine* p, ulong doc, byte* json, nuint len);

    [LibraryImport(Library)]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    public static partial OpStatus op_stroke_append(OpEngine* p, ulong doc, OpStrokeSample* samples, nuint count, ulong* out_serial);

    [LibraryImport(Library)]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    public static partial OpStatus op_stroke_end(OpEngine* p, ulong doc);

    [LibraryImport(Library)]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    public static partial OpStatus op_stroke_cancel(OpEngine* p, ulong doc);

    [LibraryImport(Library)]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    public static partial OpStatus op_view_attach(OpEngine* p, ulong doc, void* layer, ulong* out_view);

    [LibraryImport(Library)]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    public static partial OpStatus op_view_attach_panel(OpEngine* p, ulong doc, void* panel, ulong* out_view);

    [LibraryImport(Library)]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    public static partial OpStatus op_view_set(OpEngine* p, ulong view, OpViewState* state);

    [LibraryImport(Library)]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    public static partial OpStatus op_view_set_overlay(OpEngine* p, ulong view, OpOverlayItem* items, nuint count);

    [LibraryImport(Library)]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    public static partial OpStatus op_view_set_overlay_dash(OpEngine* p, ulong view, double dash_offset);

    [LibraryImport(Library)]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    public static partial OpStatus op_view_set_background(OpEngine* p, ulong view, double red, double green, double blue);

    [LibraryImport(Library)]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    public static partial OpStatus op_view_detach(OpEngine* p, ulong view);

    [LibraryImport(Library)]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    public static partial OpStatus op_view_info(OpEngine* p, ulong view, OpViewInfo* @out);

    [LibraryImport(Library)]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    public static partial OpStatus op_view_counters(OpEngine* p, OpViewCounters* @out);

    [LibraryImport(Library)]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    public static partial OpStatus op_view_take_frame_times(OpEngine* p, ulong view, double* @out, nuint cap, nuint* out_len);

    [LibraryImport(Library)]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    public static partial OpStatus op_view_wait_frames(OpEngine* p, ulong view, ulong min_frames, uint timeout_ms, ulong* out_frames);

    [LibraryImport(Library)]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    public static partial OpStatus op_view_take_frame_stamps(OpEngine* p, ulong view, OpFrameStamp* @out, nuint cap, nuint* out_len);

}
