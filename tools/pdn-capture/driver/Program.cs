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
    private static readonly string PinExe = Path.Combine(Pin, "paintdotnet.exe");
    private static readonly JsonSerializerOptions JsonOptions = new() { PropertyNameCaseInsensitive = true, WriteIndented = true };

    [STAThread]
    private static int Main(string[] args)
    {
        try
        {
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
            Console.Error.WriteLine(error);
            if (args.Length >= 2 && args[0] == "--job") WriteFailure(args[1], error);
            return 1;
        }
    }

    private static void WriteFailure(string jobPath, Exception error)
    {
        try
        {
            using var document = JsonDocument.Parse(File.ReadAllText(jobPath));
            var result = document.RootElement.GetProperty("resultPath").GetString();
            if (!string.IsNullOrWhiteSpace(result) && Path.IsPathFullyQualified(result))
                File.WriteAllText(result, JsonSerializer.Serialize(new { ok = false, error = error.ToString() }, JsonOptions));
        }
        catch (Exception writeError) { Console.Error.WriteLine($"Could not write job failure result: {writeError}"); }
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
        var job = JsonSerializer.Deserialize<Job>(File.ReadAllText(jobPath), JsonOptions)
            ?? throw new InvalidDataException("Job JSON is empty.");
        if (job.Mode == "capture" && job.Effects is { Count: > 1 })
        {
            var temporary = new List<string>();
            var currentInput = job.InputPath;
            try
            {
                for (var index = 0; index < job.Effects.Count; index++)
                {
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
                    File.WriteAllText(stepPath, JsonSerializer.Serialize(step, JsonOptions));
                    RunOne(stepPath, timeout);
                    currentInput = output;
                }
                WriteManifest(job);
            }
            finally
            {
                foreach (var path in temporary)
                    if (File.Exists(path)) File.Delete(path);
            }
            return;
        }
        RunOne(jobPath, timeout);
        WriteManifest(job);
    }

    private static void RunOne(string jobPath, TimeSpan timeout)
    {
        jobPath = Path.GetFullPath(jobPath);
        var job = JsonSerializer.Deserialize<Job>(File.ReadAllText(jobPath), JsonOptions)
            ?? throw new InvalidDataException("Job JSON is empty.");
        foreach (var value in new[] { job.InputPath, job.OutputPath, job.DonePath, job.ResultPath })
            if (string.IsNullOrWhiteSpace(value) || !Path.IsPathFullyQualified(value))
                throw new InvalidDataException("Job paths must be absolute.");
        if (!File.Exists(job.InputPath)) throw new FileNotFoundException("Input PNG is absent.", job.InputPath);
        if (!string.Equals(Path.GetExtension(job.InputPath), ".png", StringComparison.OrdinalIgnoreCase))
            throw new InvalidDataException("Input must be PNG.");
        if (File.Exists(job.DonePath) || File.Exists(job.ResultPath) || File.Exists(job.OutputPath))
            throw new IOException("Done, result, and output paths must be unused for each run.");
        if (!File.Exists(PinExe)) throw new FileNotFoundException("Pinned paint.net is absent.", PinExe);

        var desktopName = "OpenPainterCapture_" + Guid.NewGuid().ToString("N");
        var desktop = Native.CreateDesktopW(desktopName, IntPtr.Zero, IntPtr.Zero, 0, Native.DesktopAllAccess, IntPtr.Zero);
        if (desktop == IntPtr.Zero) throw new Win32Exception(Marshal.GetLastWin32Error(), "CreateDesktop failed.");
        uint pid = 0;
        Thread? triggerThread = null;
        var deadline = Stopwatch.StartNew();
        try
        {
            pid = StartPin(desktopName, jobPath, job.InputPath);
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
                if (triggerError is not null) throw new InvalidOperationException("Could not invoke the capture effect.", triggerError);
                if (deadline.Elapsed >= timeout) throw new TimeoutException($"No done file after {timeout.TotalSeconds} seconds.");
                Thread.Sleep(100);
            }
            if (!triggerThread.Join(TimeSpan.FromSeconds(3)))
                throw new TimeoutException("UI Automation thread did not finish.");
            if (triggerError is not null) throw new InvalidOperationException("Effect trigger failed.", triggerError);
            using var result = JsonDocument.Parse(File.ReadAllText(job.ResultPath));
            if (!result.RootElement.GetProperty("ok").GetBoolean())
                throw new InvalidOperationException(result.RootElement.GetProperty("error").GetString());
            if (!File.Exists(job.OutputPath)) throw new IOException("Plugin reported success without output.");
        }
        finally
        {
            if (triggerThread is { IsAlive: true } && !triggerThread.Join(TimeSpan.FromSeconds(25)))
                Console.Error.WriteLine("UI Automation thread is still running; CloseDesktop may fail.");
            if (pid != 0) StopPinByPid(pid);
            if (!Native.CloseDesktop(desktop))
                Console.Error.WriteLine($"CloseDesktop failed: {Marshal.GetLastWin32Error()}");
        }
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

    private static void TriggerOnDesktop(IntPtr desktop, uint pid, Stopwatch clock, TimeSpan timeout, Job job)
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
        var root = AutomationElement.FromHandle(window);
        if (root is null) throw new InvalidOperationException("UI Automation could not find paint.net window.");
        foreach (var effect in job.Effects ?? new List<JsonElement>())
        {
            // Setting parameters is W14's work. Refuse them rather than capture defaults by mistake.
            if (effect.TryGetProperty("parameters", out var given) && given.ValueKind == JsonValueKind.Object &&
                given.EnumerateObject().Any())
                throw new NotSupportedException("This driver captures default parameters only.");
            ApplyThroughMenu(root, pid, desktop, effect.GetProperty("id").GetString()
                ?? throw new InvalidDataException("Effect id is empty."));
        }
        OpenMenuItem(root, "Effects", null, "OpenPainter Capture");
    }

    private static void ApplyThroughMenu(AutomationElement root, uint pid, IntPtr desktop, string id)
    {
        // Ids are <assembly>/<category>/<submenu>/<name>, as the catalog writes them.
        var parts = id.Split('/');
        if (parts.Length < 4) throw new InvalidDataException($"Bad effect id: {id}");
        var name = string.Join("/", parts[3..]);
        // Layers > Rotate / Zoom is listed under the hidden category "donotdisplay".
        var top = parts[1] switch { "adjustment" => "Adjustments", "donotdisplay" => "Layers", _ => "Effects" };
        OpenMenuItem(root, top, parts[2].Length == 0 ? null : parts[2], name);
        var timer = Stopwatch.StartNew();
        var dialog = IntPtr.Zero;
        while (dialog == IntPtr.Zero && timer.Elapsed < TimeSpan.FromSeconds(5))
        {
            dialog = FindWindow(pid, t => t.StartsWith(name, StringComparison.OrdinalIgnoreCase));
            if (dialog == IntPtr.Zero) Thread.Sleep(100);
        }
        if (dialog != IntPtr.Zero)
        {
            Thread.Sleep(1500);
            SaveWindowImage(dialog, Environment.GetEnvironmentVariable("OPENPAINTER_CAPTURE_SHOT"));
            // The menu invoke stays in progress while the modal dialog is open,
            // so UI Automation calls into this process wait. A window message
            // to the dialog's focused control does not, and it stays inside
            // the app: this is not SendInput.
            // A window that has gone was a progress window: the effect has no dialog.
            if (FindWindow(pid, t => t.StartsWith(name, StringComparison.OrdinalIgnoreCase)) != IntPtr.Zero)
                Native.PressEnter(dialog);
            var closing = Stopwatch.StartNew();
            while (FindWindow(pid, t => t.StartsWith(name, StringComparison.OrdinalIgnoreCase)) != IntPtr.Zero)
            {
                if (closing.Elapsed > TimeSpan.FromSeconds(60)) throw new TimeoutException($"{name} did not finish.");
                Thread.Sleep(100);
            }
        }
        Thread.Sleep(500);
    }

    private static void SaveWindowImage(IntPtr window, string? path)
    {
        if (string.IsNullOrWhiteSpace(path)) return;
        var outcome = Native.PrintWindowToPng(window, path);
        Console.Error.WriteLine($"Dialog shot {path}: {outcome}");
    }

    private static void OpenMenuItem(AutomationElement root, string top, string? submenu, string name)
    {
        // Menu items only: a floating panel is also called Layers.
        Expand(FindNamed(root, top, TimeSpan.FromSeconds(10), ControlType.MenuItem));
        if (submenu is not null) Expand(FindNamed(root, submenu, TimeSpan.FromSeconds(10), ControlType.MenuItem));
        var item = FindNamed(root, name, TimeSpan.FromSeconds(10), ControlType.MenuItem);
        if (!item.TryGetCurrentPattern(InvokePattern.Pattern, out var action))
            throw new InvalidOperationException($"{name} has no UI Automation Invoke action.");
        var desktop = Native.GetThreadDesktop(Native.GetCurrentThreadId());
        InvokeWithoutWaiting(item, desktop);
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
    private static void InvokeWithoutWaiting(AutomationElement element, IntPtr desktop)
    {
        var thread = new Thread(() =>
        {
            try
            {
                if (!Native.SetThreadDesktop(desktop)) throw new Win32Exception(Marshal.GetLastWin32Error());
                ((InvokePattern)element.GetCurrentPattern(InvokePattern.Pattern)).Invoke();
            }
            catch (Exception error) { Console.Error.WriteLine($"Invoke failed: {error.GetBaseException().Message}"); }
        }) { IsBackground = true };
        thread.SetApartmentState(ApartmentState.STA);
        thread.Start();
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

    private static AutomationElement FindNamed(AutomationElement root, string name, TimeSpan timeout, ControlType? type = null)
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
            foreach (var window in ProcessWindows(root))
            {
                var found = window.FindFirst(TreeScope.Descendants, condition);
                if (found is not null) return found;
            }
            Thread.Sleep(100);
        }
        var seen = new List<string>();
        foreach (var window in ProcessWindows(root))
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

    private static void WriteManifest(Job job)
    {
        var manifest = new
        {
            paintNetFileVersion = FileVersionInfo.GetVersionInfo(PinExe).FileVersion,
            parameters = job.Effects,
            inputIcc = ReadPngIcc(job.InputPath),
            inputSha256 = Hash(job.InputPath), outputSha256 = Hash(job.OutputPath),
            gpu = GpuInfo()
        };
        var path = Path.Combine(Path.GetDirectoryName(job.OutputPath)!, "manifest.json");
        if (File.Exists(path)) throw new IOException($"Manifest already exists: {path}");
        File.WriteAllText(path, JsonSerializer.Serialize(manifest, JsonOptions));
    }

    private static string Hash(string path) => Convert.ToHexString(SHA256.HashData(File.ReadAllBytes(path))).ToLowerInvariant();

    private static object GpuInfo()
    {
        using var process = Process.Start(new ProcessStartInfo("nvidia-smi",
            "--query-gpu=name,driver_version --format=csv,noheader")
        { RedirectStandardOutput = true, RedirectStandardError = true, UseShellExecute = false, CreateNoWindow = true })
            ?? throw new IOException("nvidia-smi did not start.");
        var output = process.StandardOutput.ReadToEnd().Trim();
        var error = process.StandardError.ReadToEnd();
        if (!process.WaitForExit(5000))
        {
            process.Kill();
            process.WaitForExit();
            throw new TimeoutException("nvidia-smi did not finish in five seconds.");
        }
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

    private sealed record Job(string Mode, string InputPath, string OutputPath, string ResultPath,
        string DonePath, List<JsonElement>? Effects);
}
