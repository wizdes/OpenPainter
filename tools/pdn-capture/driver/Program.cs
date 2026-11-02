using System.Collections;
using System.IO;
using System.ComponentModel;
using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Security.Cryptography;
using System.Text;
using System.Text.Json;
using System.Windows.Automation;

namespace OpenPainter.Capture.Driver;

internal static class Program
{
    private const string Pin = @"C:\agent-work\pdn-5.1.12";
    private const string ExpectedVersion = "5.112.9563.32325";
    private const string ExpectedExeSha256 = "73697e348b3ab3e4168befa5ceae6b04d6fcf5a4d4e83f301f6a1257cef08222";
    private static readonly string PinExe = Path.Combine(Pin, "paintdotnet.exe");
    private static readonly JsonSerializerOptions JsonOptions = new() { PropertyNameCaseInsensitive = false, WriteIndented = true };
    private static bool resultExistedAtStart;
    private static readonly List<object> ObservedSettings = [];

    [STAThread]
    private static int Main(string[] args)
    {
        try
        {
            resultExistedAtStart = args.Length >= 2 && args[0] == "--job" && ResultExists(args[1]);
            var anyPaint = RefuseOutsidePaint();
            if (args.Length == 3 && args[0] == "--install" && args[1] == "--plugin")
            {
                Install(args[2]);
                return 0;
            }
            if (args.Length != 4 || args[0] != "--job" || args[2] != "--timeout-seconds" ||
                !int.TryParse(args[3], out var seconds) || seconds is < 1 or > 3600)
                throw new ArgumentException("Usage: --job <job.json> --timeout-seconds <1..3600>, or --install --plugin <CaptureEffect.dll>");
            if (anyPaint) throw new InvalidOperationException("Close all paint.net copies before capture; a second launch may hand off to the first.");
            Run(args[1], TimeSpan.FromSeconds(seconds));
            return 0;
        }
        catch (Exception error)
        {
            Console.Error.WriteLine(args.Length >= 2 && args[0] == "--job" ? $"{args[1]}: {error}" : error);
            if (args.Length >= 2 && args[0] == "--job") WriteFailure(args[1], error);
            return 1;
        }
    }

    private static void WriteFailure(string jobPath, Exception error)
    {
        try
        {
            using var document = JsonDocument.Parse(File.ReadAllText(jobPath));
            var root = document.RootElement;
            var output = root.GetProperty("outputPath").GetString()!;
            Decisions.ValidateDistinctPaths(root.GetProperty("inputPath").GetString()!, output,
                root.GetProperty("resultPath").GetString()!, root.GetProperty("donePath").GetString()!,
                Path.Combine(Path.GetDirectoryName(output)!, "manifest.json"));
            var result = root.GetProperty("resultPath").GetString();
            if (!string.IsNullOrWhiteSpace(result) && Path.IsPathFullyQualified(result))
            {
                Decisions.WriteFailureResult(result, resultExistedAtStart, error.ToString());
            }
        }
        catch (Exception writeError) { Console.Error.WriteLine($"Could not write job failure result: {writeError}"); }
    }

    private static bool ResultExists(string jobPath)
    {
        using var document = JsonDocument.Parse(File.ReadAllText(jobPath));
        return document.RootElement.TryGetProperty("resultPath", out var path) &&
            path.ValueKind == JsonValueKind.String && File.Exists(path.GetString());
    }

    private static bool RefuseOutsidePaint()
    {
        var anyPaint = false;
        foreach (var process in Process.GetProcesses())
        {
            using (process)
            {
                if (!process.ProcessName.StartsWith("paintdotnet", StringComparison.OrdinalIgnoreCase)) continue;
                anyPaint = true;
                string path;
                try { path = process.MainModule?.FileName ?? throw new InvalidOperationException("No executable path."); }
                catch (Exception error) { throw new InvalidOperationException($"Cannot read paint.net PID {process.Id} path.", error); }
                if (!Path.GetFullPath(path).StartsWith(Pin + Path.DirectorySeparatorChar, StringComparison.OrdinalIgnoreCase))
                    throw new InvalidOperationException($"Refusing to start: paint.net PID {process.Id} is outside the pin: {path}");
            }
        }
        return anyPaint;
    }

    private static void Install(string plugin)
    {
        if (!File.Exists(plugin)) throw new FileNotFoundException("Plugin build is absent.", plugin);
        var destination = Path.Combine(Pin, "Effects", "CaptureEffect.dll");
        Directory.CreateDirectory(Path.GetDirectoryName(destination)!);
        File.Copy(plugin, destination, true);
        Console.WriteLine($"Installed {destination}");
    }

    private static void Run(string jobPath, TimeSpan timeout)
    {
        jobPath = Path.GetFullPath(jobPath);
        var jobText = File.ReadAllText(jobPath);
        ObservedSettings.Clear();
        var job = Decisions.ParseJob(jobText);
        var clock = Stopwatch.StartNew();
        var manifestPath = Path.Combine(Path.GetDirectoryName(job.OutputPath)!, "manifest.json");
        Decisions.ValidateDistinctPaths(job.InputPath, job.OutputPath, job.ResultPath, job.DonePath, manifestPath);
        Decisions.RequireUnusedPaths(job.OutputPath, job.ResultPath, job.DonePath, manifestPath);
        var originalInputHash = Hash(job.InputPath);
        var catalogPath = Path.GetFullPath(Path.Combine(AppContext.BaseDirectory, "../../../../../../docs/windows/pdn-5.1-effects.toml"));
        var controlsPath = Path.GetFullPath(Path.Combine(AppContext.BaseDirectory, "../../../../../../docs/windows/pdn-5.1-controls.json"));
        if (!File.Exists(catalogPath)) throw new FileNotFoundException("Effect catalog is absent.", catalogPath);
        if (!File.Exists(controlsPath)) throw new FileNotFoundException("Effect controls are absent.", controlsPath);
        var catalog = File.ReadAllText(catalogPath);
        using var controls = JsonDocument.Parse(File.ReadAllText(controlsPath));
        foreach (var effect in job.Effects ?? [])
        {
            var id = effect.GetProperty("id").GetString() ?? throw new InvalidDataException("Effect id is empty.");
            if (!Decisions.CatalogContains(catalog, id)) throw new InvalidDataException($"Unknown effect id '{id}'.");
            var row = controls.RootElement.EnumerateArray().FirstOrDefault(r => r.GetProperty("id").GetString() == id);
            if (row.ValueKind == JsonValueKind.Undefined) throw new InvalidDataException($"No control list for '{id}'.");
            Decisions.ValidateEffectJson(effect.GetRawText(), row.GetProperty("labels").EnumerateArray().Select(x => x.GetString()!));
        }
        if (job.Mode is not ("capture" or "identity" or "catalog" or "api"))
            throw new InvalidDataException($"Unknown job mode '{job.Mode}'.");
        if (job.Mode == "capture" && (job.Effects is null || job.Effects.Count == 0))
            throw new InvalidDataException("Capture mode needs an effect; use identity mode to check unchanged input.");
        if (job.Mode == "identity" && job.Effects is { Count: > 0 })
            throw new InvalidDataException("Identity mode must have no effects.");
        if (job.Mode == "capture" && job.Effects is { Count: > 1 })
        {
            var temporary = new List<string>();
            var currentInput = job.InputPath;
            try
            {
                for (var index = 0; index < job.Effects.Count; index++)
                {
                    Decisions.RequireBeforeDeadline(clock.Elapsed, timeout);
                    var last = index == job.Effects.Count - 1;
                    var prefix = Path.Combine(Path.GetDirectoryName(job.OutputPath)!,
                        ".opcapture-" + Guid.NewGuid().ToString("N"));
                    var stepPath = prefix + ".job.json";
                    temporary.Add(stepPath);
                    var output = last ? job.OutputPath : prefix + ".png";
                    var result = last ? job.ResultPath : prefix + ".result.json";
                    var done = last ? job.DonePath : prefix + ".done";
                    if (!last) temporary.AddRange(new[] { output, result, done });
                    var step = job with { InputPath = currentInput, OutputPath = output,
                        ResultPath = result, DonePath = done, Effects = new List<JsonElement> { job.Effects[index] } };
                    File.WriteAllText(stepPath, Decisions.WriteJob(step));
                    RunOne(stepPath, timeout, clock);
                    currentInput = output;
                }
                Decisions.RequireUnchangedHash(originalInputHash, Hash(job.InputPath));
                WriteManifest(job, originalInputHash, catalogPath, clock, timeout);
            }
            finally
            {
                foreach (var path in temporary)
                    if (File.Exists(path)) File.Delete(path);
            }
            return;
        }
        RunOne(jobPath, timeout, clock);
        Decisions.RequireUnchangedHash(originalInputHash, Hash(job.InputPath));
        WriteManifest(job, originalInputHash, catalogPath, clock, timeout);
    }

    private static void RunOne(string jobPath, TimeSpan timeout, Stopwatch deadline)
    {
        jobPath = Path.GetFullPath(jobPath);
        var job = Decisions.ParseJob(File.ReadAllText(jobPath));
        Decisions.RequireBeforeDeadline(deadline.Elapsed, timeout);
        Decisions.ValidateDistinctPaths(job.InputPath, job.OutputPath, job.DonePath, job.ResultPath,
            Path.Combine(Path.GetDirectoryName(job.OutputPath)!, "manifest.json"));
        if (!File.Exists(job.InputPath)) throw new FileNotFoundException("Input PNG is absent.", job.InputPath);
        if (!string.Equals(Path.GetExtension(job.InputPath), ".png", StringComparison.OrdinalIgnoreCase))
            throw new InvalidDataException("Input must be PNG.");
        Decisions.RequireUnusedPaths(job.DonePath, job.ResultPath, job.OutputPath,
            Path.Combine(Path.GetDirectoryName(job.OutputPath)!, "manifest.json"));
        if (!File.Exists(PinExe)) throw new FileNotFoundException("Pinned paint.net is absent.", PinExe);
        Decisions.RequirePin(FileVersionInfo.GetVersionInfo(PinExe).FileVersion ?? "", Hash(PinExe), ExpectedVersion, ExpectedExeSha256);

        var desktopName = "OpenPainterCapture_" + Guid.NewGuid().ToString("N");
        var desktop = Native.CreateDesktopW(desktopName, IntPtr.Zero, IntPtr.Zero, 0, Native.DesktopAllAccess, IntPtr.Zero);
        if (desktop == IntPtr.Zero) throw new Win32Exception(Marshal.GetLastWin32Error(), "CreateDesktop failed.");
        uint pid = 0;
        Thread? triggerThread = null;
        var inputHash = Hash(job.InputPath);
        var inputSize = Decisions.PngSize(File.ReadAllBytes(job.InputPath));
        var userFolder = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "Paint.NET");
        var userCache = Path.Combine(userFolder, "Optimization");
        var savedCache = Decisions.SaveCache(userCache);
        var userBefore = Snapshot(userFolder);
        var otherProcessSeen = false;
        try
        {
            if (PaintPids().Length > 0)
            {
                otherProcessSeen = true;
                throw new InvalidOperationException("Another paint.net process appeared before launch.");
            }
            pid = StartPin(desktopName, jobPath, job.InputPath);
            var startedPids = PaintPids();
            otherProcessSeen = Decisions.ObserveOtherProcess(otherProcessSeen, pid, startedPids);
            if (!Decisions.ProcessIsolated(pid, startedPids, pid))
                throw new InvalidOperationException($"Another paint.net process appeared after starting PID {pid}.");
            Exception? triggerError = null;
            triggerThread = new Thread(() =>
            {
                try { TriggerOnDesktop(desktop, pid, deadline, timeout, job); }
                catch (Exception error) { triggerError = error; }
            }) { IsBackground = true };
            triggerThread.SetApartmentState(ApartmentState.STA);
            triggerThread.Start();
            while (!File.Exists(job.DonePath))
            {
                otherProcessSeen = Decisions.ObserveOtherProcess(otherProcessSeen, pid, PaintPids());
                if (otherProcessSeen) throw new InvalidOperationException($"Another paint.net process appeared during capture beside PID {pid}.");
                if (triggerError is not null) throw new InvalidOperationException("Could not invoke the capture effect.", triggerError);
                Decisions.RequireBeforeDeadline(deadline.Elapsed, timeout);
                Thread.Sleep(100);
            }
            otherProcessSeen = Decisions.ObserveOtherProcess(otherProcessSeen, pid, PaintPids());
            if (otherProcessSeen) throw new InvalidOperationException($"Another paint.net process appeared during capture beside PID {pid}.");
            var joinWait = Stopwatch.StartNew();
            while (triggerThread.IsAlive && joinWait.Elapsed < TimeSpan.FromSeconds(3))
            {
                otherProcessSeen = Decisions.ObserveOtherProcess(otherProcessSeen, pid, PaintPids());
                if (otherProcessSeen) throw new InvalidOperationException($"Another paint.net process appeared during capture beside PID {pid}.");
                Thread.Sleep(100);
            }
            if (triggerThread.IsAlive)
                throw new TimeoutException("UI Automation thread did not finish.");
            if (triggerError is not null) throw new InvalidOperationException("Effect trigger failed.", triggerError);
            using var result = JsonDocument.Parse(File.ReadAllText(job.ResultPath));
            if (!result.RootElement.GetProperty("ok").GetBoolean())
                throw new InvalidOperationException(result.RootElement.GetProperty("error").GetString());
            if (!File.Exists(job.OutputPath)) throw new IOException("Plugin reported success without output.");
            if (job.Mode is "capture" or "identity")
            {
                Decisions.RequirePngSize(File.ReadAllBytes(job.OutputPath), inputSize.Width, inputSize.Height);
                using var decodedOutput = new System.Drawing.Bitmap(job.OutputPath);
                if (decodedOutput.Width != inputSize.Width || decodedOutput.Height != inputSize.Height)
                    throw new InvalidDataException("Output PNG could not decode at the expected size.");
            }
            Decisions.RequireCaptureResult(result.RootElement.GetProperty("width").GetInt32(),
                result.RootElement.GetProperty("height").GetInt32(), result.RootElement.GetProperty("layerCount").GetInt32(),
                inputSize.Width, inputSize.Height);
            Decisions.RequireUnchangedHash(inputHash, Hash(job.InputPath));
            if (job.Mode == "identity" && !string.Equals(result.RootElement.GetProperty("surfaceSha256").GetString(),
                    Decisions.DecodedSurfaceHash(job.InputPath), StringComparison.OrdinalIgnoreCase))
                throw new InvalidDataException("No-effect surface hash does not match decoded input.");
        }
        catch (Exception error)
        {
            // The checks in finally can throw too, and that would hide this one.
            Console.Error.WriteLine($"Capture step failed: {error}");
            throw;
        }
        finally
        {
            if (triggerThread is { IsAlive: true } && !triggerThread.Join(TimeSpan.FromSeconds(25)))
                Console.Error.WriteLine("UI Automation thread is still running; CloseDesktop may fail.");
            if (pid != 0) StopPinByPid(pid);
            if (!Native.CloseDesktop(desktop))
                Console.Error.WriteLine($"CloseDesktop failed: {Marshal.GetLastWin32Error()}");
            otherProcessSeen = Decisions.ObserveOtherProcess(otherProcessSeen, pid, PaintPids());
            if (!otherProcessSeen) RemoveRunSessionFiles(userFolder, userBefore, pid, ref otherProcessSeen);
            otherProcessSeen = Decisions.ObserveOtherProcess(otherProcessSeen, pid, PaintPids());
            if (!otherProcessSeen) Decisions.RestoreCache(userCache, savedCache);
            var changed = Decisions.ChangedEntries(userBefore, Snapshot(userFolder));
            if (otherProcessSeen)
                throw new InvalidOperationException($"Another paint.net process appeared during capture beside PID {pid}; cleanup and cache restore skipped. Changed user data: {string.Join(", ", changed)}");
            if (changed.Count > 0)
                throw new InvalidDataException($"User paint.net data changed during capture: {string.Join(", ", changed)}");
        }
    }

    private static uint[] PaintPids()
    {
        var found = new List<uint>();
        foreach (var process in Process.GetProcesses())
            using (process)
                if (process.ProcessName.StartsWith("paintdotnet", StringComparison.OrdinalIgnoreCase))
                    found.Add((uint)process.Id);
        return found.ToArray();
    }

    private static uint StartPin(string desktopName, string jobPath, string inputPath)
    {
        var environment = Environment.GetEnvironmentVariables().Cast<DictionaryEntry>()
            .ToDictionary(e => (string)e.Key, e => (string)e.Value!, StringComparer.OrdinalIgnoreCase);
        environment["OPENPAINTER_CAPTURE_JOB"] = jobPath;
        var block = string.Join('\0', environment.OrderBy(e => e.Key, StringComparer.OrdinalIgnoreCase)
            .Select(e => e.Key + "=" + e.Value)) + "\0\0";
        var environmentPointer = Marshal.StringToHGlobalUni(block);
        try
        {
            var startup = new Native.StartupInfo { cb = (uint)Marshal.SizeOf<Native.StartupInfo>(), lpDesktop = desktopName };
            var commandLine = new StringBuilder($"\"{PinExe}\" \"{inputPath}\"");
            if (!Native.CreateProcessW(PinExe, commandLine, IntPtr.Zero, IntPtr.Zero,
                    false, Native.CreateUnicodeEnvironment, environmentPointer, Pin, ref startup, out var process))
                throw new Win32Exception(Marshal.GetLastWin32Error(), "CreateProcess failed.");
            Native.CloseHandle(process.hThread);
            Native.CloseHandle(process.hProcess);
            return process.dwProcessId;
        }
        finally { Marshal.FreeHGlobal(environmentPointer); }
    }

    private static IntPtr FindWindow(uint pid, Func<string, bool> matches)
    {
        var found = IntPtr.Zero;
        var title = new StringBuilder(256);
        Native.EnumWindows((candidate, _) =>
        {
            Native.GetWindowThreadProcessId(candidate, out var owner);
            if (owner == pid && Native.GetWindowTextW(candidate, title, title.Capacity) > 0 && matches(title.ToString()))
            {
                found = candidate;
                return false;
            }
            return true;
        }, IntPtr.Zero);
        return found;
    }

    private static void TriggerOnDesktop(IntPtr desktop, uint pid, Stopwatch clock, TimeSpan timeout, CaptureJob job)
    {
        if (!Native.SetThreadDesktop(desktop))
            throw new Win32Exception(Marshal.GetLastWin32Error(), "SetThreadDesktop failed.");
        currentPid = pid;
        IntPtr window = IntPtr.Zero;
        while (window == IntPtr.Zero && clock.Elapsed < timeout)
        {
            window = FindWindow(pid, t => t.Contains("Paint.NET", StringComparison.Ordinal));
            if (window == IntPtr.Zero) Thread.Sleep(100);
        }
        if (window == IntPtr.Zero) throw new TimeoutException("Pinned paint.net did not create a hidden window.");
        var paintPids = Process.GetProcesses().Where(p => p.ProcessName.StartsWith("paintdotnet", StringComparison.OrdinalIgnoreCase))
            .Select(p => { try { return (uint)p.Id; } finally { p.Dispose(); } }).ToArray();
        Native.GetWindowThreadProcessId(window, out var windowPid);
        if (!Decisions.ProcessIsolated(pid, paintPids, windowPid))
            throw new InvalidOperationException($"Started PID {pid} is not the sole paint.net window owner.");
        var root = AutomationElement.FromHandle(window);
        if (root is null) throw new InvalidOperationException("UI Automation could not find paint.net window.");
        foreach (var effect in job.Effects ?? new List<JsonElement>())
        {
            var given = effect.TryGetProperty("parameters", out var p) && p.ValueKind == JsonValueKind.Object
                ? p.EnumerateObject().ToList()
                : new List<JsonProperty>();
            var id = effect.GetProperty("id").GetString() ?? throw new InvalidDataException("Effect id is empty.");
            try { ApplyThroughMenu(root, pid, desktop, id, given); }
            catch (Exception error) { throw new InvalidOperationException($"{id}: {error.Message}", error); }
        }
        var captureInvoke = OpenMenuItem(root, "Effects", null, "OpenPainter Capture");
        while (captureInvoke.Thread.IsAlive && !File.Exists(job.DonePath))
        {
            Decisions.RequireBeforeDeadline(clock.Elapsed, timeout);
            Thread.Sleep(100);
        }
        if (!captureInvoke.Thread.IsAlive && captureInvoke.Error is { } invokeError)
        {
            var finished = Stopwatch.StartNew();
            while (!File.Exists(job.DonePath) && finished.Elapsed < TimeSpan.FromSeconds(5)) Thread.Sleep(100);
            if (Decisions.InvocationErrorNeedsFailure(true, invokeError.GetBaseException().Message,
                    File.Exists(job.DonePath), finished.Elapsed))
                throw new InvalidOperationException($"OpenPainter Capture: {invokeError.GetBaseException().Message}", invokeError);
        }
    }

    private static void ApplyThroughMenu(AutomationElement root, uint pid, IntPtr desktop, string id, List<JsonProperty> parameters)
    {
        // Ids are <assembly>/<category>/<submenu>/<name>, as the catalog writes them.
        var parts = id.Split('/');
        if (parts.Length < 4) throw new InvalidDataException($"Bad effect id: {id}");
        var name = string.Join("/", parts[3..]);
        // Layers > Rotate / Zoom is listed under the hidden category "donotdisplay".
        var top = parts[1] switch { "adjustment" => "Adjustments", "donotdisplay" => "Layers", "effect" => "Effects", _ => throw new InvalidDataException($"Unknown effect category '{parts[1]}'.") };
        var invoke = OpenMenuItem(root, top, parts[2].Length == 0 ? null : parts[2], name);
        var timer = Stopwatch.StartNew();
        var dialog = IntPtr.Zero;
        while (dialog == IntPtr.Zero && timer.Elapsed < TimeSpan.FromSeconds(5))
        {
            dialog = FindWindow(pid, t => t.StartsWith(name, StringComparison.OrdinalIgnoreCase));
            if (dialog == IntPtr.Zero) Thread.Sleep(100);
        }
        var opened = false;
        var closed = false;
        if (dialog != IntPtr.Zero)
        {
            Thread.Sleep(1500);
            var stayed = FindWindow(pid, t => t.StartsWith(name, StringComparison.OrdinalIgnoreCase)) != IntPtr.Zero;
            var hasOk = stayed && Native.HasButton(dialog, "OK");
            opened = Decisions.RequireDialogOpened(name, stayed, hasOk);
            if (!stayed && parameters.Count > 0)
                throw new InvalidOperationException($"{name} has no dialog, so its parameters cannot be set.");
            if (opened)
            {
                SaveWindowImage(dialog, Environment.GetEnvironmentVariable("OPENPAINTER_CAPTURE_SHOT"));
                var dumpPath = Environment.GetEnvironmentVariable("OPENPAINTER_CAPTURE_CONTROLS");
                if (dumpPath is not null) File.WriteAllLines(dumpPath, Native.DescribeChildren(dialog));
                Decisions.RequireUnambiguous(parameters.Select(p => p.Name), Native.ReadAllNumbers(dialog));
                foreach (var parameter in parameters)
                    Native.SetLabelledNumber(dialog, parameter.Name, parameter.Value.GetRawText());
                var settings = Native.ReadAllNumbers(dialog);
                Decisions.RequireSettings(parameters.ToDictionary(p => p.Name, p => p.Value.GetDouble()), settings);
                lock (ObservedSettings) ObservedSettings.Add(new { id, numbers = settings, otherControls = Native.ReadOtherControls(dialog) });
                if (parameters.Count > 0)
                {
                    Thread.Sleep(1000);
                    SaveWindowImage(dialog, Environment.GetEnvironmentVariable("OPENPAINTER_CAPTURE_SET_SHOT"));
                }
                // The menu invoke stays in progress while the modal dialog is open,
                // so UI Automation calls into this process wait. A window message
                // to the button stays inside the app: this is not SendInput.
                Native.ClickButton(dialog, "OK");
                var closing = Stopwatch.StartNew();
                while (FindWindow(pid, t => t.StartsWith(name, StringComparison.OrdinalIgnoreCase)) != IntPtr.Zero)
                {
                    if (closing.Elapsed > TimeSpan.FromSeconds(60)) throw new TimeoutException($"{name} did not finish.");
                    Thread.Sleep(100);
                }
                closed = true;
            }
        }
        else if (parameters.Count > 0)
        {
            throw new InvalidOperationException($"{name} opened no dialog, so its parameters cannot be set.");
        }
        if (!invoke.Thread.Join(TimeSpan.FromSeconds(5))) throw new TimeoutException($"{id}: invoke did not finish.");
        if (!Decisions.InvocationSucceeded(name, opened, closed, invoke.Error?.GetBaseException().Message))
            throw new InvalidOperationException($"{id}: effect invocation did not complete: {invoke.Error?.GetBaseException().Message ?? "no dialog observed"}");
        if (!opened) lock (ObservedSettings) ObservedSettings.Add(new { id, numbers = new Dictionary<string,double>(), otherControls = new Dictionary<string,string>() });
        Thread.Sleep(500);
    }

    private static void SaveWindowImage(IntPtr window, string? path)
    {
        if (string.IsNullOrWhiteSpace(path)) return;
        var outcome = Native.PrintWindowToPng(window, path);
        Console.Error.WriteLine($"Dialog shot {path}: {outcome}");
    }

    private static Invocation OpenMenuItem(AutomationElement root, string top, string? submenu, string name)
    {
        // Menu items only: a floating panel is also called Layers.
        var branch = FindNamed(root, top, TimeSpan.FromSeconds(10), ControlType.MenuItem);
        Expand(branch);
        if (submenu is not null) { branch = FindNamed(branch, submenu, TimeSpan.FromSeconds(10), ControlType.MenuItem, true); Expand(branch); }
        var item = FindNamed(branch, name, TimeSpan.FromSeconds(10), ControlType.MenuItem, true);
        if (!item.TryGetCurrentPattern(InvokePattern.Pattern, out var action))
            throw new InvalidOperationException($"{name} has no UI Automation Invoke action.");
        var desktop = Native.GetThreadDesktop(Native.GetCurrentThreadId());
        return InvokeWithoutWaiting(item, desktop);
    }

    private static void Expand(AutomationElement element)
    {
        if (element.TryGetCurrentPattern(ExpandCollapsePattern.Pattern, out var expand))
            ((ExpandCollapsePattern)expand).Expand();
        else if (element.TryGetCurrentPattern(InvokePattern.Pattern, out var invoke))
            ((InvokePattern)invoke).Invoke();
        else throw new InvalidOperationException($"{element.Current.Name} has no UI Automation action.");
    }

    // A menu item that opens a modal dialog may not return until the dialog
    // closes, so the invoke runs on its own thread on the same desktop.
    private sealed class Invocation { public required Thread Thread; public Exception? Error; }
    private static Invocation InvokeWithoutWaiting(AutomationElement element, IntPtr desktop)
    {
        Invocation? state = null;
        var thread = new Thread(() =>
        {
            try
            {
                if (!Native.SetThreadDesktop(desktop)) throw new Win32Exception(Marshal.GetLastWin32Error());
                ((InvokePattern)element.GetCurrentPattern(InvokePattern.Pattern)).Invoke();
            }
            catch (Exception error) { state!.Error = error; }
        }) { IsBackground = true };
        state = new Invocation { Thread = thread };
        thread.SetApartmentState(ApartmentState.STA);
        thread.Start();
        return state;
    }

    private static uint currentPid;

    private static IEnumerable<AutomationElement> ProcessWindows(AutomationElement root)
    {
        yield return root;
        var handles = new List<IntPtr>();
        Native.EnumWindows((candidate, _) =>
        {
            Native.GetWindowThreadProcessId(candidate, out var owner);
            if (owner == currentPid && Native.IsWindowVisible(candidate)) handles.Add(candidate);
            return true;
        }, IntPtr.Zero);
        foreach (var handle in handles) yield return AutomationElement.FromHandle(handle);
    }

    private static AutomationElement FindNamed(AutomationElement root, string name, TimeSpan timeout,
        ControlType? type = null, bool branchOnly = false)
    {
        var timer = Stopwatch.StartNew();
        Condition condition = new OrCondition(
            new PropertyCondition(AutomationElement.NameProperty, name, PropertyConditionFlags.IgnoreCase),
            new PropertyCondition(AutomationElement.NameProperty, name + "...", PropertyConditionFlags.IgnoreCase),
            new PropertyCondition(AutomationElement.NameProperty, name + "\u2026", PropertyConditionFlags.IgnoreCase));
        if (type is not null)
            condition = new AndCondition(condition, new PropertyCondition(AutomationElement.ControlTypeProperty, type));
        while (timer.Elapsed < timeout)
        {
            foreach (var window in branchOnly ? new[] { root } : ProcessWindows(root))
            {
                var found = window.FindAll(TreeScope.Descendants, condition);
                Decisions.RequireSingleMatch(found.Count, name);
                if (found.Count == 1) return found[0];
            }
            Thread.Sleep(100);
        }
        var seen = new List<string>();
        foreach (var window in branchOnly ? new[] { root } : ProcessWindows(root))
        {
            var items = window.FindAll(TreeScope.Descendants,
                new PropertyCondition(AutomationElement.ControlTypeProperty, ControlType.MenuItem));
            seen.Add($"[{window.Current.Name}: {items.Count}]");
            foreach (AutomationElement item in items)
                if (seen.Count < 90) seen.Add(item.Current.Name);
        }
        throw new TimeoutException($"UI Automation did not find {name}. Menu items: {string.Join(" | ", seen)}");
    }

    private static void StopPinByPid(uint pid)
    {
        try
        {
            using var process = Process.GetProcessById((int)pid);
            var path = process.MainModule?.FileName;
            if (path is null || !Path.GetFullPath(path).StartsWith(Pin + Path.DirectorySeparatorChar,
                    StringComparison.OrdinalIgnoreCase))
                throw new InvalidOperationException($"Refusing to stop PID {pid}: path is not in pin.");
            process.Kill();
            if (!process.WaitForExit(10_000)) throw new TimeoutException($"PID {pid} did not stop.");
        }
        catch (ArgumentException) { /* Already exited. */ }
    }

    private static void WriteManifest(CaptureJob job, string originalInputHash, string catalogPath,
        Stopwatch clock, TimeSpan timeout)
    {
        Decisions.RequireBeforeDeadline(clock.Elapsed, timeout);
        var gpu = GpuInfo();
        Decisions.RequireBeforeDeadline(clock.Elapsed, timeout);
        var manifest = new
        {
            paintNetFileVersion = FileVersionInfo.GetVersionInfo(PinExe).FileVersion,
            paintNetExeSha256 = Hash(PinExe),
            driverSha256 = Hash(System.Reflection.Assembly.GetExecutingAssembly().Location),
            pluginSha256 = Hash(Path.Combine(Pin, "Effects", "CaptureEffect.dll")),
            effectCatalogSha256 = Hash(catalogPath),
            parameters = job.Effects,
            observedSettings = ObservedSettings,
            inputIcc = ReadPngIcc(job.InputPath),
            inputSha256 = originalInputHash, outputSha256 = Hash(job.OutputPath),
            gpu
        };
        var path = Path.Combine(Path.GetDirectoryName(job.OutputPath)!, "manifest.json");
        if (File.Exists(path)) throw new IOException($"Manifest already exists: {path}");
        File.WriteAllText(path, JsonSerializer.Serialize(manifest, JsonOptions));
        Decisions.RequireBeforeDeadline(clock.Elapsed, timeout);
    }

    private static string Hash(string path) => Convert.ToHexString(SHA256.HashData(File.ReadAllBytes(path))).ToLowerInvariant();

    private static Dictionary<string,string> Snapshot(string folder)
    {
        var entries = Decisions.SnapshotFiles(folder);
        SnapshotRegistry(@"Software\paint.net", entries);
        return entries;
    }

    private static void SnapshotRegistry(string path, Dictionary<string,string> entries)
    {
        using var key = Microsoft.Win32.Registry.CurrentUser.OpenSubKey(path);
        if (key is null) return;
        entries[$@"HKCU\{path}"] = "key";
        foreach (var name in key.GetValueNames())
        {
            var value = key.GetValue(name, null, Microsoft.Win32.RegistryValueOptions.DoNotExpandEnvironmentNames);
            var text = Decisions.FormatRegistryValue(value);
            entries[$@"HKCU\{path}\{name}"] = $"{key.GetValueKind(name)}:{text}";
        }
        foreach (var child in key.GetSubKeyNames()) SnapshotRegistry($@"{path}\{child}", entries);
    }

    private static void RemoveRunSessionFiles(string folder, Dictionary<string,string> before, uint pid, ref bool otherProcessSeen)
    {
        var files = Decisions.RunSessionFiles(before, Snapshot(folder));
        if (files.Count == 0) return;
        otherProcessSeen = Decisions.ObserveOtherProcess(otherProcessSeen, pid, PaintPids());
        if (otherProcessSeen) return;
        Decisions.RemoveRunFiles(folder, files);
    }

    private static object GpuInfo()
    {
        using var process = Process.Start(new ProcessStartInfo("nvidia-smi",
            "--query-gpu=name,driver_version --format=csv,noheader")
        { RedirectStandardOutput = true, RedirectStandardError = true, UseShellExecute = false, CreateNoWindow = true })
            ?? throw new IOException("nvidia-smi did not start.");
        var stdout = process.StandardOutput.ReadToEndAsync();
        var stderr = process.StandardError.ReadToEndAsync();
        var gpuClock = Stopwatch.StartNew();
        var readers = Task.WhenAll(stdout, stderr);
        var exited = process.WaitForExit(5000);
        var remaining = Math.Max(0, 5000 - (int)gpuClock.ElapsedMilliseconds);
        if (!exited || !Decisions.ReadersFinishedWithin(readers, TimeSpan.FromMilliseconds(remaining)))
        {
            if (!process.HasExited) process.Kill();
            throw new TimeoutException("nvidia-smi did not finish in five seconds.");
        }
        var output = stdout.GetAwaiter().GetResult().Trim();
        var error = stderr.GetAwaiter().GetResult();
        if (process.ExitCode != 0)
            throw new IOException($"nvidia-smi failed: {error}");
        var parts = output.Split(',', 2);
        if (parts.Length != 2) throw new InvalidDataException("nvidia-smi did not return GPU and driver.");
        return new { name = parts[0].Trim(), driverVersion = parts[1].Trim() };
    }

    private static object ReadPngIcc(string path)
    {
        using var stream = File.OpenRead(path);
        Span<byte> signature = stackalloc byte[8];
        stream.ReadExactly(signature);
        if (!signature.SequenceEqual(new byte[] { 137, 80, 78, 71, 13, 10, 26, 10 }))
            throw new InvalidDataException("Input is not PNG.");
        Span<byte> header = stackalloc byte[8];
        while (stream.Position < stream.Length)
        {
            stream.ReadExactly(header);
            var length = System.Buffers.Binary.BinaryPrimitives.ReadUInt32BigEndian(header[..4]);
            var type = Encoding.ASCII.GetString(header[4..]);
            if (length > stream.Length - stream.Position - 4) throw new InvalidDataException("Invalid PNG chunk length.");
            if (type == "iCCP")
            {
                var data = new byte[length];
                stream.ReadExactly(data);
                var terminator = Array.IndexOf(data, (byte)0);
                if (terminator < 0) throw new InvalidDataException("Invalid iCCP profile name.");
                return new { name = Encoding.Latin1.GetString(data, 0, terminator),
                    chunkSha256 = Convert.ToHexString(SHA256.HashData(data)).ToLowerInvariant() };
            }
            stream.Position += length + 4;
            if (type == "IEND") break;
        }
        return "none";
    }
}
