// The engine, one document and one view of it.
//
// Slow work (starting the engine, decoding a PNG) runs on its own "canvas" thread,
// so the window never waits on it. The engine's own render thread draws the frames.
// op_view_set only stores the newest state and wakes that thread, so the UI thread
// can call it from any input message.
using System.Collections.Concurrent;
using System.Text;
using System.Text.Json;
using Avalonia.Threading;
using OpenPainter.Bridge;

namespace OpenPainter.Canvas;

public sealed unsafe class CanvasSession : IDisposable
{
    const uint BlankWidth = 800, BlankHeight = 600, OpaqueWhite = 0xffffffff;
    const double BlankDpi = 96;
    static readonly TimeSpan StopLimit = TimeSpan.FromSeconds(30);

    readonly BlockingCollection<Action> _work = [];
    readonly Thread _thread;
    OpEngine* _engine;
    ulong _doc, _view;

    public CanvasSession()
    {
        _thread = new Thread(Run) { IsBackground = true, Name = "canvas" };
        _thread.Start();
    }

    /// Runs on the canvas thread. A failure there stops the program with the failure's name.
    void Run()
    {
        foreach (var job in _work.GetConsumingEnumerable())
        {
            try
            {
                job();
            }
            catch (Exception e)
            {
                OpenPainter.Shell.Fatal.Raise("CANVAS_THREAD", e);
            }
        }
    }

    public static void Check(OpStatus status, string name)
    {
        if (status != OpStatus.OP_OK) throw new InvalidOperationException($"{name}: {status}");
    }

    /// Starts the engine and opens the PNG, or a blank image when there is none. `ready`
    /// runs on the UI thread with the image size.
    public void Load(string? png, Action<uint, uint> ready)
    {
        _work.Add(() =>
        {
            _engine = Native.op_engine_new(null, null, null);
            if (_engine == null) throw new InvalidOperationException("ENGINE_NEW: returned null");
            ulong doc;
            if (png is null)
            {
                Check(Native.op_doc_new(_engine, BlankWidth, BlankHeight, BlankDpi, OpaqueWhite, &doc), "DOC_NEW");
            }
            else
            {
                var path = Encoding.UTF8.GetBytes(png);
                fixed (byte* bytes = path) Check(Native.op_doc_open_png(_engine, bytes, (nuint)path.Length, &doc), $"DOC_OPEN_PNG {png}");
            }
            _doc = doc;
            var (width, height) = ImageSize();
            Dispatcher.UIThread.Post(() => ready(width, height));
        });
    }

    (uint Width, uint Height) ImageSize()
    {
        var query = Encoding.UTF8.GetBytes("doc.info\0");
        var empty = "{}"u8.ToArray();
        OpBuffer buffer = default;
        fixed (byte* id = query, json = empty)
            Check(Native.op_query(_engine, _doc, id, json, (nuint)empty.Length, &buffer), "DOC_INFO");
        try
        {
            using var info = JsonDocument.Parse(new ReadOnlySpan<byte>(buffer.ptr, (int)buffer.len).ToArray());
            return (info.RootElement.GetProperty("width").GetUInt32(), info.RootElement.GetProperty("height").GetUInt32());
        }
        finally
        {
            Native.op_buffer_free(&buffer);
        }
    }

    /// Runs on the UI thread, which owns the window.
    public void Attach(nint hwnd)
    {
        if (_engine == null) throw new InvalidOperationException("VIEW_ATTACH: no engine yet");
        ulong view;
        Check(Native.op_view_attach(_engine, _doc, (void*)hwnd, &view), "VIEW_ATTACH");
        _view = view;
    }

    public bool Attached => _view != 0;

    /// Stores the newest view state. It returns at once.
    public void SetView(double width, double height, double scale, double zoom, double x, double y)
    {
        if (_view == 0) return;
        var state = new OpViewState
        {
            width_pt = Math.Max(1, width),
            height_pt = Math.Max(1, height),
            scale = scale,
            zoom = zoom,
            origin_x = x,
            origin_y = y,
        };
        Check(Native.op_view_set(_engine, _view, &state), "VIEW_SET");
    }

    /// Shows the Zoom tool's rectangle, in document points, or clears it. The engine keeps
    /// a copy, and it draws in the same frame as the document.
    public void SetZoomRectangle(Avalonia.Rect? document)
    {
        if (_view == 0) return;
        const uint StaticZoomRectangle = 11;   // OpOverlayItem's kinds.
        if (document is not { } r)
        {
            Check(Native.op_view_set_overlay(_engine, _view, null, 0), "VIEW_OVERLAY");
            return;
        }
        var item = new OpOverlayItem { kind = StaticZoomRectangle, x = r.X, y = r.Y, width = r.Width, height = r.Height };
        Check(Native.op_view_set_overlay(_engine, _view, &item, 1), "VIEW_OVERLAY");
    }

    /// Runs `shown` on the UI thread once the engine has presented a frame. It waits on the
    /// canvas thread in short steps, so closing the window never waits long for it.
    public void WhenFirstFrame(Action shown)
    {
        const uint StepMs = 100;
        _work.Add(() =>
        {
            while (!_work.IsAddingCompleted)
            {
                if (!WaitFrames(1, StepMs)) continue;
                Dispatcher.UIThread.Post(shown);
                return;
            }
        });
    }

    /// Makes a small straight-alpha RGBA copy of the image on the canvas thread, then gives
    /// it to `ready` on the UI thread: the strip's preview.
    public void LoadPreview(int maxWidth, int maxHeight, Action<byte[], int, int> ready)
    {
        _work.Add(() =>
        {
            var (width, height) = ImageSize();
            var query = Encoding.UTF8.GetBytes("doc.pixels\0");
            var empty = "{}"u8.ToArray();
            OpBuffer buffer = default;
            fixed (byte* id = query, json = empty)
                Check(Native.op_query(_engine, _doc, id, json, (nuint)empty.Length, &buffer), "DOC_PIXELS");
            try
            {
                var expected = (long)width * height * 4;
                if ((long)buffer.len != expected) throw new InvalidOperationException($"DOC_PIXELS: {buffer.len} bytes, expected {expected}");
                var (w, h, small) = Shrink(new ReadOnlySpan<byte>(buffer.ptr, (int)buffer.len), (int)width, (int)height, maxWidth, maxHeight);
                Dispatcher.UIThread.Post(() => ready(small, w, h));
            }
            finally
            {
                Native.op_buffer_free(&buffer);
            }
        });
    }

    /// A box average of up to 4 by 4 samples per preview pixel, so a 24 MP image stays quick.
    public static (int Width, int Height, byte[] Rgba) Shrink(ReadOnlySpan<byte> rgba, int width, int height, int maxWidth, int maxHeight)
    {
        const int Samples = 4;
        var fit = Math.Min(1, Math.Min((double)maxWidth / width, (double)maxHeight / height));
        var w = Math.Max(1, (int)Math.Round(width * fit));
        var h = Math.Max(1, (int)Math.Round(height * fit));
        var result = new byte[w * h * 4];
        for (var y = 0; y < h; y++)
        {
            for (var x = 0; x < w; x++)
            {
                long r = 0, g = 0, b = 0, a = 0;
                for (var sy = 0; sy < Samples; sy++)
                {
                    var py = Math.Min(height - 1, (int)((y + (sy + .5) / Samples) * height / h));
                    for (var sx = 0; sx < Samples; sx++)
                    {
                        var px = Math.Min(width - 1, (int)((x + (sx + .5) / Samples) * width / w));
                        var at = (py * (long)width + px) * 4;
                        var alpha = rgba[(int)at + 3];
                        r += rgba[(int)at] * alpha;
                        g += rgba[(int)at + 1] * alpha;
                        b += rgba[(int)at + 2] * alpha;
                        a += alpha;
                    }
                }
                var to = (y * w + x) * 4;
                if (a > 0)
                {
                    result[to] = (byte)(r / a);
                    result[to + 1] = (byte)(g / a);
                    result[to + 2] = (byte)(b / a);
                    result[to + 3] = (byte)(a / (Samples * Samples));
                }
            }
        }
        return (w, h, result);
    }

    /// The fill around the image: the stage color, as on the Mac.
    public void SetBackground(byte red, byte green, byte blue)
    {
        if (_view == 0) return;
        Check(Native.op_view_set_background(_engine, _view, red / 255.0, green / 255.0, blue / 255.0), "VIEW_BACKGROUND");
    }

    public OpViewInfo Info()
    {
        if (_view == 0) throw new InvalidOperationException("VIEW_INFO: no view yet");
        OpViewInfo info;
        Check(Native.op_view_info(_engine, _view, &info), "VIEW_INFO");
        return info;
    }

    /// Waits on this thread until the engine has presented `frames` frames. False means
    /// the time ran out. It is a test hook's wait, so it never runs on the UI thread.
    public bool WaitFrames(ulong frames, uint timeoutMs)
    {
        if (_view == 0) throw new InvalidOperationException("VIEW_WAIT: no view yet");
        var status = Native.op_view_wait_frames(_engine, _view, frames, timeoutMs, null);
        if (status == OpStatus.OP_ERR_BUSY) return false;
        Check(status, "VIEW_WAIT");
        return true;
    }

    /// Moves the engine's frame times out of its log: milliseconds since attach, stamped on
    /// the render thread when each present returns, so no UI callback is in the count.
    public double[] TakeFrameTimes()
    {
        const int Room = 16384;   // FRAME_LOG_CAP in op-ffi's view.rs.
        var times = new double[Room];
        nuint count;
        fixed (double* to = times) Check(Native.op_view_take_frame_times(_engine, _view, to, (nuint)Room, &count), "VIEW_FRAME_TIMES");
        return times[..(int)count];
    }

    public OpViewCounters Counters()
    {
        OpViewCounters counters;
        Check(Native.op_view_counters(_engine, &counters), "VIEW_COUNTERS");
        return counters;
    }

    public void Dispose()
    {
        _work.CompleteAdding();
        if (!_thread.Join(StopLimit)) throw new InvalidOperationException($"CANVAS_THREAD: still running after {StopLimit.TotalSeconds} s");
        _work.Dispose();
        if (_view != 0)
        {
            Check(Native.op_view_detach(_engine, _view), "VIEW_DETACH");
            _view = 0;
        }
        if (_engine != null)
        {
            Native.op_engine_free(_engine);
            _engine = null;
        }
    }
}
