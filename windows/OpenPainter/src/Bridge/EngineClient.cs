// The engine through the C bridge: its handle, commands, queries and events. It does what
// mac/EngineBridge/Sources/EngineBridge/EngineBridge.swift does for the Mac.
//
// Events arrive on the engine's own thread. Each one is copied at once, because its text is
// valid only during the call, and no op_ call may run inside it.
using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;
using System.Text;
using System.Text.Json;

namespace OpenPainter.Bridge;

/// One finished command: "ok", "cancelled", "dropped" or an error name.
public sealed record JobResult(string Status, string? Error);

/// An engine call returned an error status.
public sealed class EngineCallException(string call, OpStatus status) : InvalidOperationException($"{call}: {status}")
{
    public OpStatus Status { get; } = status;
}

/// A call came after the engine was freed, which only happens while the app ends.
public sealed class EngineClosedException(string call) : InvalidOperationException($"{call}: the engine is closed");

public sealed unsafe class EngineClient : IDisposable
{
    const int KeptResults = 4096;

    readonly object _jobs = new();
    readonly Dictionary<ulong, JobResult> _finished = [];
    readonly Queue<ulong> _order = new();
    readonly Dictionary<ulong, Action<JobResult>> _waiters = [];
    // Background reads, such as thumbnails, hold this so the engine cannot be freed under them.
    readonly ReaderWriterLockSlim _open = new();
    GCHandle _self;
    OpEngine* _engine;

    /// A document changed, by its id. It runs on the engine's thread and must not call the engine.
    public event Action<ulong>? DocumentChanged;

    public EngineClient()
    {
        _self = GCHandle.Alloc(this);
        _engine = Native.op_engine_new(null, &OnEvent, (void*)GCHandle.ToIntPtr(_self));
        if (_engine != null) return;
        _self.Free();
        throw new InvalidOperationException("ENGINE_NEW: returned null");
    }

    /// For the view calls, which take the handle directly. They run on the owner's threads.
    public OpEngine* Handle => _engine;

    public static void Check(OpStatus status, string name)
    {
        if (status != OpStatus.OP_OK) throw new EngineCallException(name, status);
    }

    [UnmanagedCallersOnly(CallConvs = [typeof(CallConvCdecl)])]
    static void OnEvent(OpEvent* e, void* user)
    {
        try
        {
            if (GCHandle.FromIntPtr((nint)user).Target is not EngineClient client) throw new InvalidOperationException("the engine's owner is gone");
            client.Deliver(e);
        }
        catch (Exception error)
        {
            // A throw must not cross into the engine's thread.
            OpenPainter.Shell.Fatal.Raise("ENGINE_EVENT", error);
        }
    }

    void Deliver(OpEvent* e)
    {
        switch ((OpEventKind)e->kind)
        {
            case OpEventKind.OP_EVENT_JOB_FINISHED:
                Finished(e->ticket, ParseResult(new ReadOnlySpan<byte>(e->json_ptr, (int)e->json_len)));
                break;
            case OpEventKind.OP_EVENT_DOC_CHANGED or OpEventKind.OP_EVENT_HISTORY_CHANGED or OpEventKind.OP_EVENT_LAYERS_CHANGED:
                DocumentChanged?.Invoke(e->doc);
                break;
        }
    }

    public static JobResult ParseResult(ReadOnlySpan<byte> json)
    {
        using var document = JsonDocument.Parse(json.ToArray());
        var root = document.RootElement;
        return new JobResult(root.GetProperty("status").GetString() ?? throw new JsonException("status is null"),
            root.TryGetProperty("error", out var error) ? error.GetString() : null);
    }

    void Finished(ulong ticket, JobResult result)
    {
        Action<JobResult>? waiter;
        lock (_jobs)
        {
            if (!_waiters.Remove(ticket, out waiter))
            {
                _finished[ticket] = result;
                _order.Enqueue(ticket);
                while (_order.Count > KeptResults) _finished.Remove(_order.Dequeue());
            }
        }
        waiter?.Invoke(result);
    }

    /// Runs `done` when the command ends, on the engine's thread, or here if it already has.
    public void WhenFinished(ulong ticket, Action<JobResult> done)
    {
        JobResult? ready;
        lock (_jobs)
        {
            if (!_finished.Remove(ticket, out ready)) _waiters[ticket] = done;
        }
        if (ready is not null) done(ready);
    }

    /// Waits on this thread for a command. Null means the time ran out.
    public JobResult? Wait(ulong ticket, int timeoutMs)
    {
        using var ended = new ManualResetEventSlim();
        JobResult? result = null;
        WhenFinished(ticket, r =>
        {
            result = r;
            ended.Set();
        });
        return ended.Wait(timeoutMs) ? result : null;
    }

    T Open<T>(string call, Func<T> body)
    {
        _open.EnterReadLock();
        try
        {
            if (_engine == null) throw new EngineClosedException(call);
            return body();
        }
        finally
        {
            _open.ExitReadLock();
        }
    }

    public ulong NewDocument(uint width, uint height, double dpi, uint backgroundRgba) => Open("DOC_NEW", () =>
    {
        ulong doc;
        Check(Native.op_doc_new(_engine, width, height, dpi, backgroundRgba, &doc), "DOC_NEW");
        return doc;
    });

    public ulong OpenPng(string path) => Open($"DOC_OPEN_PNG {path}", () =>
    {
        var bytes = Encoding.UTF8.GetBytes(path);
        ulong doc;
        fixed (byte* text = bytes) Check(Native.op_doc_open_png(_engine, text, (nuint)bytes.Length, &doc), $"DOC_OPEN_PNG {path}");
        return doc;
    });

    public ulong ImportPixels(uint width, uint height, byte[] rgba) => Open("IMPORT_PIXELS", () =>
    {
        ulong doc;
        fixed (byte* pixels = rgba) Check(Native.op_import_pixels(_engine, width, height, pixels, (nuint)rgba.Length, null, 0, &doc), "IMPORT_PIXELS");
        return doc;
    });

    /// Queues a command. Its result comes through WhenFinished.
    public ulong Dispatch(ulong doc, string command, byte[] json) => Open(command, () =>
    {
        var id = Encoding.UTF8.GetBytes(command + "\0");
        ulong ticket;
        fixed (byte* idBytes = id, jsonBytes = json) Check(Native.op_dispatch(_engine, doc, idBytes, jsonBytes, (nuint)json.Length, &ticket), command);
        return ticket;
    });

    public byte[] Query(ulong doc, string query, byte[] json) => Open(query, () =>
    {
        var id = Encoding.UTF8.GetBytes(query + "\0");
        OpBuffer buffer = default;
        fixed (byte* idBytes = id, jsonBytes = json) Check(Native.op_query(_engine, doc, idBytes, jsonBytes, (nuint)json.Length, &buffer), query);
        try
        {
            return new ReadOnlySpan<byte>(buffer.ptr, (int)buffer.len).ToArray();
        }
        finally
        {
            Native.op_buffer_free(&buffer);
        }
    });

    public void Dispose()
    {
        _open.EnterWriteLock();
        try
        {
            if (_engine == null) return;
            // It waits for queued work, so events still arrive during this call.
            Native.op_engine_free(_engine);
            _engine = null;
            _self.Free();
        }
        finally
        {
            _open.ExitWriteLock();
        }
    }
}

/// A command's parameters as JSON. A nested object comes as bytes from this same call.
public static class EngineParams
{
    public static readonly byte[] None = "{}"u8.ToArray();

    public static byte[] Of(params (string Key, object Value)[] fields)
    {
        using var stream = new MemoryStream();
        using (var json = new Utf8JsonWriter(stream))
        {
            json.WriteStartObject();
            foreach (var (key, value) in fields)
            {
                json.WritePropertyName(key);
                switch (value)
                {
                    case int number: json.WriteNumberValue(number); break;
                    case long number: json.WriteNumberValue(number); break;
                    case bool flag: json.WriteBooleanValue(flag); break;
                    case string text: json.WriteStringValue(text); break;
                    case byte[] nested: json.WriteRawValue(nested); break;
                    default: throw new ArgumentException($"ENGINE_PARAMS: {key} has the type {value.GetType().Name}");
                }
            }
            json.WriteEndObject();
        }
        return stream.ToArray();
    }
}
