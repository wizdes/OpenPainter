// Layer pictures from the engine: LayerThumbnailCache in LayersPanel.swift. Each is keyed by
// layer id and the layer's content key. A key comes back when history returns to the same
// pixels, so a History jump reuses the picture it made before.
using System.Runtime.InteropServices;
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
    const int Limit = 256, HeaderBytes = 16;

    readonly Dictionary<(long Layer, long Revision), Bitmap> _images = [];
    readonly Queue<(long Layer, long Revision)> _order = new();
    readonly HashSet<(long Layer, long Revision)> _pending = [];
    readonly List<Task> _running = [];

    /// Pictures made so far. A test uses this to see that a cached one renders nothing.
    public int Renders { get; private set; }

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
        _running.Add(Task.Run(() => Render(id, key)));
        return null;
    }

    void Render((long Layer, long Revision) key, byte[] data)
    {
        _pending.Remove(key);
        var (width, height, revision, rgba) = Decode(data);
        var got = (Layer: key.Layer, Revision: revision);
        var bitmap = new WriteableBitmap(new PixelSize(width, height), new Vector(96, 96), PixelFormats.Rgba8888, AlphaFormat.Unpremul);
        using (var locked = bitmap.Lock())
            for (var row = 0; row < height; row++)
                Marshal.Copy(rgba, row * width * 4, locked.Address + row * locked.RowBytes, width * 4);
        if (_images.TryAdd(got, bitmap)) _order.Enqueue(got);
        while (_order.Count > Limit) _images.Remove(_order.Dequeue());
        Renders++;
        Ready?.Invoke(got.Layer, got.Revision);
    }

    /// Runs on a pool thread, so a big layer never holds up the UI.
    void Render(long id, (long Layer, long Revision) key)
    {
        try
        {
            var data = editor.Client.Query(editor.Document, "layer.thumbnail", EngineParams.Of(("id", id), ("max", Pixels)));
            Dispatcher.UIThread.Post(() => Fatal.Guard("LAYER_THUMBNAIL", () => Render(key, data)));
        }
        catch (EngineClosedException)
        {
            // The app is ending and nothing can show this picture. No one is left to tell.
        }
        catch (EngineCallException e)
        {
            Dispatcher.UIThread.Post(() =>
            {
                _pending.Remove(key);
                editor.Report($"Layer thumbnail failed: {e.Status}");
            });
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
