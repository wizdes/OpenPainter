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
            r.GetProperty("dirty").GetBoolean(), r.GetProperty("revision").GetInt64());
    }
}

public sealed class Editor : IDisposable
{
    const int WaitMs = 120_000;   // Editor.runAndWait's limit.
    bool _refreshQueued, _closed;

    public EngineClient Client { get; }
    public ulong Document { get; }
    public DocInfo Info { get; private set; } = DocInfo.Empty;
    public IReadOnlyDictionary<string, bool> Enabled { get; private set; } = new Dictionary<string, bool>();

    /// The last message for the status bar: the engine's errors.
    public string? Message { get; private set; }

    public LayerThumbnails Thumbnails { get; }

    /// The state changed. It runs on the UI thread.
    public event Action? Changed;

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
            using var list = JsonDocument.Parse(Client.Query(Document, "commands.list", EngineParams.None));
            var enabled = new Dictionary<string, bool>();
            foreach (var command in list.RootElement.EnumerateArray())
                enabled[command.GetProperty("id").GetString() ?? throw new JsonException("a command with no id")] = command.GetProperty("enabled").GetBoolean();
            Enabled = enabled;
        }
        catch (Exception e) when (e is EngineCallException or JsonException or KeyNotFoundException or InvalidOperationException)
        {
            Report($"Could not read the document: {e.Message}");
            return;
        }
        Changed?.Invoke();
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

    /// Queues an engine command. A failure shows in the status bar.
    public ulong? Run(string command, byte[]? json = null)
    {
        if (_closed) throw new InvalidOperationException($"{command}: the document is closed");
        try
        {
            var ticket = Client.Dispatch(Document, command, json ?? EngineParams.None);
            Client.WhenFinished(ticket, result => Dispatcher.UIThread.Post(() => OnFinished(command, result)));
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
    /// on the UI thread, which the caller must be on. For scripted flows and tests.
    public void Settle()
    {
        var ticket = Client.Dispatch(Document, "doc.sync", EngineParams.None);
        var result = Client.Wait(ticket, WaitMs) ?? throw new TimeoutException($"doc.sync: no answer in {WaitMs} ms");
        if (result.Status != "ok") throw new InvalidOperationException($"doc.sync: {result.Status}");
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
