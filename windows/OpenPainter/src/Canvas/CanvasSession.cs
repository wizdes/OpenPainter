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
                Dispatcher.UIThread.Post(() => throw new InvalidOperationException($"CANVAS_THREAD: {e.Message}", e));
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
