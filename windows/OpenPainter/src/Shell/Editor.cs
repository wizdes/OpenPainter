// One open image as the panels see it: the cached `doc.info`, which commands are on, and the
// last message for the status bar: mac/OpenPainterKit/Sources/Shell/Editor.swift. Everything
// goes through engine commands. Every failure ends in Report, so it reaches the user.
using System.Text.Json;
using Avalonia.Threading;
using OpenPainter.Bridge;

namespace OpenPainter.Shell;

public sealed record LayerInfo(string Name, bool Visible, long Id, int Opacity, string Blend, long Revision);

public sealed record DocInfo(int Width, int Height, IReadOnlyList<LayerInfo> Layers, int ActiveLayer, IReadOnlyList<string> HistoryLabels, IReadOnlyList<string> HistoryCommands, int HistoryIndex, bool Dirty, long Revision)
{
    public static readonly DocInfo Empty = new(0, 0, [], 0, [], [], 0, false, 0);

    public bool ProfileEmbedded { get; init; }
    public string ProfileDescription { get; init; } = "";

    public static DocInfo Parse(byte[] json)
    {
        using var document = JsonDocument.Parse(json);
        var r = document.RootElement;
        static List<string> Words(JsonElement list) => list.EnumerateArray().Select(v => v.GetString() ?? throw new JsonException("a null history word")).ToList();
        var layers = r.GetProperty("layers").EnumerateArray().Select(l => new LayerInfo(
            l.GetProperty("name").GetString() ?? throw new JsonException("a layer with no name"),
            l.GetProperty("visible").GetBoolean(),
            l.GetProperty("id").GetInt64(),
            l.GetProperty("opacity").GetInt32(),
            l.GetProperty("blend").GetString() ?? throw new JsonException("a layer with no blend"),
            l.GetProperty("revision").GetInt64())).ToList();
        return new DocInfo(r.GetProperty("width").GetInt32(), r.GetProperty("height").GetInt32(), layers, r.GetProperty("active_layer").GetInt32(),
            Words(r.GetProperty("history_labels")), Words(r.GetProperty("history_commands")), r.GetProperty("history_index").GetInt32(),
            r.GetProperty("dirty").GetBoolean(), r.GetProperty("revision").GetInt64())
        {
            ProfileEmbedded = r.GetProperty("profile_embedded").GetBoolean(),
            ProfileDescription = r.GetProperty("profile_description").GetString() ?? "",
        };
    }
}

/// The profile's names for the status bar: ColorProfileNames in ColorProfile.swift.
public static class ColorProfileNames
{
    public const string NoneTitle = "None, treated as sRGB";
    const int ShortLimit = 18;   // The longest status bar name before it is cut with "…".

    public static string Full(DocInfo info) => info.ProfileEmbedded ? info.ProfileDescription : NoneTitle;

    public static string Short(DocInfo info)
    {
        if (!info.ProfileEmbedded) return "sRGB (none)";
        var name = ColorText.TrimSpaces(info.ProfileDescription);
        return name.Length <= ShortLimit ? name : ColorText.TrimSpaces(name[..(ShortLimit - 1)]) + "…";
    }
}

public sealed class Editor : IDisposable
{
    const int WaitMs = 120_000;   // Editor.runAndWait's limit.
    /// Editor.updateThumbnail: 56 by 40 at 2x.
    public const int PreviewWidth = 112, PreviewHeight = 80;
    bool _refreshQueued, _closed, _previewBusy;
    long _previewRevision = -1;
    Task _previewTask = Task.CompletedTask;
    readonly object _commandsGate = new();
    int _commandsRunning;

    public EngineClient Client { get; }
    public ulong Document { get; }
    public DocInfo Info { get; private set; } = DocInfo.Empty;

    /// The document's pixels per inch: `doc.resolution`.
    public double Dpi { get; private set; } = 96;
    public IReadOnlyDictionary<string, bool> Enabled { get; private set; } = new Dictionary<string, bool>();

    /// The last message for the status bar: the engine's errors.
    public string? Message { get; private set; }

    public LayerThumbnails Thumbnails { get; }

    /// The state changed. It runs on the UI thread.
    public event Action? Changed;

    /// The strip's picture of the whole image, straight RGBA, on the UI thread.
    public event Action<byte[], int, int>? PreviewReady;

    /// The last picture made, so a window bound later can show it.
    public (byte[] Rgba, int Width, int Height)? Preview { get; private set; }

    /// Runs on the pool thread at each step of reading the strip's picture: "pixels" before the
    /// read and "check" after it. A test uses this to change the document at the worst moment.
    public Action<string>? PreviewStage { get; set; }

    /// Where an unexpected failure of the picture goes. Null means the fatal path.
    public Action<Exception>? PreviewFault { get; set; }

    /// A picture is being made.
    public bool PictureUnderWay => _previewBusy;

    public Editor(EngineClient client, ulong document)
    {
        Client = client;
        Document = document;
        Thumbnails = new LayerThumbnails(this);
        client.DocumentChanged += OnDocumentChanged;
        Refresh();
    }

    void OnDocumentChanged(ulong doc)
    {
        if (doc != Document) return;
        Dispatcher.UIThread.Post(ScheduleRefresh);
    }

    /// Re-reads the engine's published state. It never waits for work.
    public void Refresh()
    {
        if (_closed) return;
        try
        {
            Info = DocInfo.Parse(Client.Query(Document, "doc.info", EngineParams.None));
            using (var resolution = JsonDocument.Parse(Client.Query(Document, "doc.resolution", EngineParams.None)))
                Dpi = resolution.RootElement.GetProperty("dpi").GetDouble();
            using var list = JsonDocument.Parse(Client.Query(Document, "commands.list", EngineParams.None));
            var enabled = new Dictionary<string, bool>();
            foreach (var command in list.RootElement.EnumerateArray())
                enabled[command.GetProperty("id").GetString() ?? throw new JsonException("a command with no id")] = command.GetProperty("enabled").GetBoolean();
            Enabled = enabled;
        }
        catch (EngineClosedException)
        {
            // The app is ending, and no one is left to tell.
            return;
        }
        catch (Exception e) when (e is EngineCallException or JsonException or KeyNotFoundException or InvalidOperationException)
        {
            Report($"Could not read the document: {e.Message}");
            return;
        }
        UpdatePreview();
        Changed?.Invoke();
    }

    /// Editor.updateThumbnail: at most one picture at a time, and when it ends, another if the
    /// document has moved on. It runs on a pool thread, so a big image never holds up the UI.
    void UpdatePreview()
    {
        if (_previewBusy || _closed || Info.Revision == _previewRevision || Info.Width <= 0) return;
        _previewBusy = true;
        _previewTask = Task.Run(ReadPreview);
    }

    /// The engine keeps no size with `doc.pixels`, and a crop, resize or rotate can land between two
    /// queries. So the document's revision is read before and after: if they match, the size and
    /// the pixels belong together. If not, the picture goes away and another starts.
    void ReadPreview()
    {
        try
        {
            var before = DocInfo.Parse(Client.Query(Document, "doc.info", EngineParams.None));
            PreviewStage?.Invoke("pixels");
            var pixels = Client.Query(Document, "doc.pixels", EngineParams.None);
            PreviewStage?.Invoke("check");
            var after = DocInfo.Parse(Client.Query(Document, "doc.info", EngineParams.None));
            if (after.Revision != before.Revision)
            {
                Dispatcher.UIThread.Post(() => Fatal.Guard("STRIP_PREVIEW", () =>
                {
                    _previewBusy = false;
                    UpdatePreview();
                }));
                return;
            }
            var (w, h, small) = Canvas.CanvasSession.Shrink(pixels, before.Width, before.Height, PreviewWidth, PreviewHeight);
            Dispatcher.UIThread.Post(() => Fatal.Guard("STRIP_PREVIEW", () => TakePreview(before.Revision, small, w, h)));
        }
        catch (EngineClosedException)
        {
            // The app is ending and nothing can show this picture. No one is left to tell, but a
            // wait for this picture must still end.
            Dispatcher.UIThread.Post(() => _previewBusy = false);
        }
        catch (Exception e) when (e is EngineCallException or JsonException or KeyNotFoundException or InvalidDataException)
        {
            var words = e is EngineCallException call ? call.Status.ToString() : e.Message;
            Dispatcher.UIThread.Post(() =>
            {
                _previewBusy = false;
                if (!_closed) Report($"The image strip's picture failed: {words}");
            });
        }
        catch (Exception e)
        {
            // A task's failure reaches nobody by itself, so this one is named here.
            Dispatcher.UIThread.Post(() => _previewBusy = false);
            if (PreviewFault is { } fault) fault(e);
            else Fatal.Raise("STRIP_PREVIEW", e);
        }
    }

    void TakePreview(long revision, byte[] rgba, int width, int height)
    {
        _previewBusy = false;
        _previewRevision = revision;
        if (_closed) return;
        Preview = (rgba, width, height);
        PreviewReady?.Invoke(rgba, width, height);
        UpdatePreview();
    }

    /// Waits for the picture under way and any that follows. The caller must be on the UI thread,
    /// and it takes in the result. For scripted flows and tests.
    public void WaitForPreview()
    {
        var end = Environment.TickCount64 + WaitMs;
        while (_previewBusy)
        {
            var left = (int)Math.Max(end - Environment.TickCount64, 0);
            if (!_previewTask.Wait(left) || (left == 0 && _previewBusy)) throw new TimeoutException($"the strip's picture: no answer in {WaitMs} ms");
            Dispatcher.UIThread.RunJobs();
        }
    }

    /// Joins engine events into one refresh on the next turn of the UI thread.
    public void ScheduleRefresh()
    {
        if (_refreshQueued || _closed) return;
        _refreshQueued = true;
        Dispatcher.UIThread.Post(() =>
        {
            _refreshQueued = false;
            Refresh();
        });
    }

    public bool IsEnabled(string command) => Enabled.GetValueOrDefault(command);

    /// Asks the engine now, not the cached state, so it can run on any thread.
    public bool HasLayer(long id) => DocInfo.Parse(Client.Query(Document, "doc.info", EngineParams.None)).Layers.Any(l => l.Id == id);

    /// Queues an engine command. A failure shows in the status bar.
    public ulong? Run(string command, byte[]? json = null)
    {
        if (_closed) throw new InvalidOperationException($"{command}: the document is closed");
        try
        {
            var ticket = Client.Dispatch(Document, command, json ?? EngineParams.None);
            lock (_commandsGate) _commandsRunning++;
            Client.WhenFinished(ticket, result =>
            {
                Dispatcher.UIThread.Post(() => OnFinished(command, result));
                lock (_commandsGate)
                {
                    _commandsRunning--;
                    Monitor.PulseAll(_commandsGate);
                }
            });
            return ticket;
        }
        catch (EngineCallException e)
        {
            Report($"{command} failed: {e.Status}");
            return null;
        }
    }

    void OnFinished(string command, JobResult result)
    {
        if (_closed) return;
        // "dropped" is a stale preview render that a newer one replaced, so nothing went wrong.
        if (result.Status is not ("ok" or "dropped")) Report(FailureText(command, result.Status, result.Error));
        Refresh();
    }

    /// The status bar's words for a failed command. A limit shows the engine's own words.
    public static string FailureText(string label, string status, string? error) =>
        status == "limit" && !string.IsNullOrEmpty(error) ? Sentence(error) : $"{label} failed: {status}";

    public static string Sentence(string text)
    {
        var words = char.ToUpperInvariant(text[0]) + text[1..];
        return words[^1] is '.' or '!' or '?' ? words : words + ".";
    }

    public void Report(string text)
    {
        Message = text;
        Changed?.Invoke();
    }

    public void ClearMessage()
    {
        if (Message is null) return;
        Message = null;
        Changed?.Invoke();
    }

    /// Waits on this thread until the engine ran every queued command, then takes in the results
    /// on the UI thread, which the caller must be on. `doc.sync` alone is not enough: a preview's
    /// render runs on the engine's pool after its command, and only its own end says it is done.
    /// For scripted flows and tests.
    public void Settle()
    {
        var ticket = Client.Dispatch(Document, "doc.sync", EngineParams.None);
        var result = Client.Wait(ticket, WaitMs) ?? throw new TimeoutException($"doc.sync: no answer in {WaitMs} ms");
        if (result.Status != "ok") throw new InvalidOperationException($"doc.sync: {result.Status}");
        var end = Environment.TickCount64 + WaitMs;
        lock (_commandsGate)
        {
            while (_commandsRunning > 0)
            {
                var left = (int)Math.Max(end - Environment.TickCount64, 0);
                if (left == 0 || !Monitor.Wait(_commandsGate, left)) throw new TimeoutException($"{_commandsRunning} commands: no end in {WaitMs} ms");
            }
        }
        Dispatcher.UIThread.RunJobs();
        Refresh();
    }

    public void Dispose()
    {
        _closed = true;
        Client.DocumentChanged -= OnDocumentChanged;
    }
}

/// The 14 blend modes: engine names and paint.net's titles, in paint.net's order: LayerBlend.
public static class LayerBlend
{
    public static readonly (string Id, string Title)[] Modes =
    [
        ("normal", "Normal"), ("multiply", "Multiply"), ("additive", "Additive"), ("color_burn", "Color Burn"),
        ("color_dodge", "Color Dodge"), ("reflect", "Reflect"), ("glow", "Glow"), ("overlay", "Overlay"),
        ("difference", "Difference"), ("negation", "Negation"), ("lighten", "Lighten"), ("darken", "Darken"),
        ("screen", "Screen"), ("xor", "Xor"),
    ];

    public static string Title(string id)
    {
        foreach (var (mode, title) in Modes) if (mode == id) return title;
        return id;
    }
}
