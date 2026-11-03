// One run of the app under test. Each run has its own temp data root and output files, and
// ends by itself. The harness stops only the process it started, by its handle.
using System.Diagnostics;
using System.Text.Json;

namespace OpenPainter.Harness;

public sealed class AppRun : IDisposable
{
    /// Every run also ends by itself after this, so a hang cannot leave a window behind.
    const int SelfEndMs = 600_000;
    const int PollMs = 10;

    readonly Process _process;
    readonly StreamWriter _output;
    public string Folder { get; }
    public string DataRoot { get; }
    public string FirstFramePath { get; }
    public string OutputPath { get; }
    public DateTime Started { get; }
    public int Pid => _process.Id;
    public bool Exited => _process.HasExited;
    public int ExitCode => _process.ExitCode;

    public AppRun(string app, string folder, IEnumerable<string> args, bool selfEnd = true)
    {
        Folder = folder;
        Directory.CreateDirectory(folder);
        DataRoot = Path.Combine(folder, "data");
        Directory.CreateDirectory(DataRoot);
        FirstFramePath = Path.Combine(folder, "first-frame.json");
        OutputPath = Path.Combine(folder, "app-output.txt");
        var info = new ProcessStartInfo(app)
        {
            WorkingDirectory = Path.GetDirectoryName(app)!,
            UseShellExecute = false,
            RedirectStandardError = true,
            RedirectStandardOutput = true,
            CreateNoWindow = true,
        };
        info.ArgumentList.Add("--data-root");
        info.ArgumentList.Add(DataRoot);
        foreach (var arg in args) info.ArgumentList.Add(arg);
        if (selfEnd)
        {
            info.ArgumentList.Add("--exit-after-ms");
            info.ArgumentList.Add(SelfEndMs.ToString());
        }
        // The RTX. Analytics and update checks do not exist in this app; these two stop .NET's and Avalonia's.
        info.Environment["OP_GPU_ADAPTER"] = "hardware";
        info.Environment["AVALONIA_TELEMETRY_OPTOUT"] = "1";
        info.Environment["DOTNET_CLI_TELEMETRY_OPTOUT"] = "1";
        Started = DateTime.Now;
        _process = Process.Start(info) ?? throw new InvalidOperationException($"HARNESS: could not start {app}");
        // Both streams drain to a file, so a full pipe cannot stop the app.
        _output = new StreamWriter(OutputPath) { AutoFlush = true };
        var gate = new object();
        void Line(object _, DataReceivedEventArgs e)
        {
            if (e.Data is null) return;
            lock (gate) _output.WriteLine(e.Data);
        }
        _process.OutputDataReceived += Line;
        _process.ErrorDataReceived += Line;
        _process.BeginOutputReadLine();
        _process.BeginErrorReadLine();
    }

    public double MsSinceStart => (DateTime.Now - Started).TotalMilliseconds;

    public string Output()
    {
        try
        {
            using var stream = new FileStream(OutputPath, FileMode.Open, FileAccess.Read, FileShare.ReadWrite);
            using var reader = new StreamReader(stream);
            return reader.ReadToEnd().Trim();
        }
        catch (IOException e)
        {
            return $"(the app's output file could not be read: {e.Message})";
        }
    }

    /// Waits for the app's first-frame file. It ends early when the app ends.
    public FirstFrame WaitFirstFrame(int limitMs)
    {
        var clock = Stopwatch.StartNew();
        while (!File.Exists(FirstFramePath))
        {
            if (Exited) throw new InvalidOperationException($"the app exited {ExitCode} before its first frame: {Output()}");
            if (clock.ElapsedMilliseconds > limitMs) throw new TimeoutException($"no first-frame signal in {limitMs} ms: {Output()}");
            Thread.Sleep(PollMs);
        }
        return FirstFrame.Read(FirstFramePath);
    }

    public nint WaitMainWindow(int limitMs)
    {
        var clock = Stopwatch.StartNew();
        while (true)
        {
            var hwnd = Win32.MainWindowOf(Pid);
            if (hwnd != 0) return hwnd;
            if (Exited) throw new InvalidOperationException($"the app exited {ExitCode} with no window: {Output()}");
            if (clock.ElapsedMilliseconds > limitMs) throw new TimeoutException($"no window in {limitMs} ms");
            Thread.Sleep(PollMs);
        }
    }

    public bool WaitExit(int limitMs) => _process.WaitForExit(limitMs);

    /// Closes the main window, and stops the process only when it will not end.
    public void Close(nint main)
    {
        const int LimitMs = 15_000;
        if (Exited) return;
        Win32.AskToClose(main);
        if (WaitExit(LimitMs)) return;
        Kill();
        throw new TimeoutException($"the app did not close in {LimitMs} ms, so the harness stopped it");
    }

    public void Kill()
    {
        if (!_process.HasExited)
        {
            _process.Kill();
            _process.WaitForExit(5000);
        }
    }

    public void Dispose()
    {
        Kill();
        // With no time limit, this also waits for both output streams to end.
        _process.WaitForExit();
        _process.Dispose();
        _output.Dispose();
    }
}

public sealed record FirstFrame(
    double AttachedMs, double FirstFrameMs, double SignalMs, nint Main, nint Canvas, ViewMap Map,
    int DrawableWidth, int DrawableHeight, long Frames, long FrameErrors, long LastError, long ValidationErrors,
    int HasDisplayTable, long DisplayIccLength, long DocUploads, Rgb Stage, string Theme)
{
    public static FirstFrame Read(string path)
    {
        using var file = JsonDocument.Parse(File.ReadAllText(path));
        return From(file.RootElement);
    }

    public static FirstFrame From(JsonElement r)
    {
        double D(string name) => r.GetProperty(name).GetDouble();
        long L(string name) => r.GetProperty(name).GetInt64();
        var stage = r.GetProperty("stage").EnumerateArray().Select(v => (byte)v.GetInt32()).ToArray();
        var map = new ViewMap(D("zoom"), D("origin_x"), D("origin_y"), D("scale"), D("document_width"), D("document_height"));
        return new FirstFrame(
            r.TryGetProperty("attached_ms", out var a) ? a.GetDouble() : 0,
            r.TryGetProperty("first_frame_ms", out var f) ? f.GetDouble() : 0,
            r.TryGetProperty("signal_ms", out var s) ? s.GetDouble() : 0,
            (nint)L("main_hwnd"), (nint)L("canvas_hwnd"), map,
            (int)L("drawable_width"), (int)L("drawable_height"), L("frames"), L("frame_errors"), L("last_error"), L("validation_errors"),
            (int)L("has_display_table"), L("display_icc_len"), L("doc_uploads"), new Rgb(stage[0], stage[1], stage[2]), r.GetProperty("theme").GetString() ?? "");
    }
}
