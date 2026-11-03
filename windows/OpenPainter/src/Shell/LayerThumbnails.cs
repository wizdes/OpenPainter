// Layer pictures from the engine: LayerThumbnailCache in LayersPanel.swift. Each is keyed by
// layer id and the layer's content key. A key comes back when history returns to the same
// pixels, so a History jump reuses the picture it made before.
using System.Runtime.InteropServices;
using System.Text.Json;
using Avalonia;
using Avalonia.Media.Imaging;
using Avalonia.Platform;
using Avalonia.Threading;
using OpenPainter.Bridge;

namespace OpenPainter.Shell;

public sealed class LayerThumbnails(Editor editor)
{
    /// 40 point boxes at 2x.
    public const int Pixels = 80;
    const int DefaultLimit = 256;
    const int HeaderBytes = 16;

    readonly Dictionary<(long Layer, long Revision), Bitmap> _images = [];
    readonly Queue<(long Layer, long Revision)> _order = new();
    readonly HashSet<(long Layer, long Revision)> _pending = [];
    readonly List<Task> _running = [];

    /// Pictures made so far. A test uses this to see that a cached one renders nothing.
    public int Renders { get; private set; }

    /// The size each query asks for. A test sets a size the engine refuses, to make a real failure.
    public int RequestSize { get; set; } = Pixels;

    /// Render tasks still held.
    public int HeldTasks => _running.Count;

    /// Pictures kept now.
    public int Kept => _images.Count;

    /// The most pictures to keep. A picture a row still shows stays over this.
    public int Limit { get; set; } = DefaultLimit;

    /// True for a picture a row still shows, so it must not be freed. The Layers panel sets this.
    public Func<Bitmap, bool>? IsShown { get; set; }

    /// A picture is ready, on the UI thread.
    public event Action<long, long>? Ready;

    public Bitmap? Cached(long layer, long revision) => _images.GetValueOrDefault((layer, revision));

    /// The cached picture, or null after starting a render that raises Ready.
    public Bitmap? Image(LayerInfo layer)
    {
        var key = (layer.Id, layer.Revision);
        if (_images.TryGetValue(key, out var image)) return image;
        if (!_pending.Add(key)) return null;
        var id = layer.Id;
        _running.RemoveAll(t => t.IsCompleted);
        _running.Add(Task.Run(() => Render(id, key)));
        return null;
    }

    void Render((long Layer, long Revision) key, byte[] data)
    {
        _pending.Remove(key);
        // The layer had gone.
        if (data.Length == 0)
        {
            Renders++;
            return;
        }
        var (width, height, revision, rgba) = Decode(data);
        var got = (Layer: key.Layer, Revision: revision);
        var bitmap = new WriteableBitmap(new PixelSize(width, height), new Vector(96, 96), PixelFormats.Rgba8888, AlphaFormat.Unpremul);
        using (var locked = bitmap.Lock())
            for (var row = 0; row < height; row++)
                Marshal.Copy(rgba, row * width * 4, locked.Address + row * locked.RowBytes, width * 4);
        if (_images.TryAdd(got, bitmap)) _order.Enqueue(got);
        else bitmap.Dispose();
        Evict();
        Renders++;
        Ready?.Invoke(got.Layer, got.Revision);
    }

    /// The oldest pictures leave and free their memory. A picture on screen goes to the back.
    void Evict()
    {
        var look = _order.Count;
        while (_order.Count > Limit && look-- > 0)
        {
            var oldest = _order.Dequeue();
            var image = _images[oldest];
            if (IsShown?.Invoke(image) == true)
            {
                _order.Enqueue(oldest);
                continue;
            }
            _images.Remove(oldest);
            image.Dispose();
        }
    }

    /// Runs on a pool thread, so a big layer never holds up the UI.
    void Render(long id, (long Layer, long Revision) key)
    {
        try
        {
            var data = Query(id);
            Dispatcher.UIThread.Post(() => Fatal.Guard("LAYER_THUMBNAIL", () => Render(key, data)));
        }
        catch (EngineClosedException)
        {
            // The app is ending and nothing can show this picture. No one is left to tell.
        }
        catch (Exception e) when (e is EngineCallException or JsonException or KeyNotFoundException or InvalidDataException)
        {
            var words = e is EngineCallException call ? call.Status.ToString() : e.Message;
            Dispatcher.UIThread.Post(() =>
            {
                _pending.Remove(key);
                editor.Report($"Layer thumbnail failed: {words}");
            });
        }
        catch (Exception e)
        {
            Fatal.Raise("LAYER_THUMBNAIL", e);
        }
    }

    /// The picture's data, or an empty answer for a layer that has gone. A list asks for a layer,
    /// and an undo can take that layer away before the query runs. The Mac writes this to its log
    /// only. A failed query for a layer that is still there asks once more, in case the layer came
    /// back between the two, and then it is a real failure.
    byte[] Query(long id)
    {
        var parameters = EngineParams.Of(("id", id), ("max", RequestSize));
        try
        {
            return editor.Client.Query(editor.Document, "layer.thumbnail", parameters);
        }
        catch (EngineCallException)
        {
            if (!editor.HasLayer(id)) return [];
            return editor.Client.Query(editor.Document, "layer.thumbnail", parameters);
        }
    }

    /// `layer.thumbnail`: width, height and content key, then straight RGBA.
    public static (int Width, int Height, long Revision, byte[] Rgba) Decode(byte[] data)
    {
        if (data.Length < HeaderBytes) throw new InvalidDataException($"LAYER_THUMBNAIL: {data.Length} bytes");
        var width = BitConverter.ToInt32(data, 0);
        var height = BitConverter.ToInt32(data, 4);
        var revision = BitConverter.ToInt64(data, 8);
        var rgba = data[HeaderBytes..];
        if (width <= 0 || height <= 0 || rgba.Length != (long)width * height * 4) throw new InvalidDataException($"LAYER_THUMBNAIL: {width} by {height} with {rgba.Length} bytes");
        return (width, height, revision, rgba);
    }

    /// Waits for the renders started so far. The caller must be on the UI thread, and it takes
    /// in the results. For scripted flows and tests.
    public void WaitForRenders()
    {
        Task.WaitAll(_running.ToArray());
        _running.Clear();
        Dispatcher.UIThread.RunJobs();
    }
}
