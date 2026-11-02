using System.Diagnostics;
using System.IO;
using System.Runtime.InteropServices;
using System.Text;
using System.Text.Json;
using System.Windows.Automation;
using Vortice.Direct3D;
using Vortice.Direct3D12;
using Vortice.DXGI;
using Vortice.Mathematics;

record Item(string status, string? error = null);
class Report
{
    public long windowHandle { get; set; }
    public long buttonHandle { get; set; }
    public Dictionary<string, Item> checks { get; set; } = new();
    public void Set(string key, bool pass, string? error = null) => checks[key] = new(pass ? "pass" : "fail", pass ? null : error);
    public void Unknown(string key, string error) => checks[key] = new("could not test", error);
}

class Program
{
    const string WindowKey = "window created";
    const string DeviceKey = "D3D12 device created and frame presented";
    const string ReadKey = "pixels read back";
    const string PrintKey = "PrintWindow capture shows the GPU frame";
    const string FocusKey = "keyboard focus obtainable";
    const string UiaKey = "UI Automation on the same desktop finds window and invokes button";
    static readonly string[] Keys = [WindowKey, DeviceKey, ReadKey, PrintKey, FocusKey, UiaKey];
    static readonly WndProc WndProcValue = OnWindowMessage;
    static bool buttonInvoked;

    static int Main(string[] args)
    {
        string? output = null;
        bool child = false;
        long uiaWindow = 0;
        for (int i = 0; i < args.Length; i++)
        {
            if (args[i] == "--out" && i + 1 < args.Length) output = args[++i];
            else if (args[i] == "--child") child = true;
            else if (args[i] == "--uia" && i + 1 < args.Length) uiaWindow = long.Parse(args[++i]);
            else { Console.Error.WriteLine($"Unknown argument: {args[i]}"); return 2; }
        }
        if (string.IsNullOrWhiteSpace(output)) { Console.Error.WriteLine("Usage: HiddenDesktop --out <json path>"); return 2; }
        if (uiaWindow != 0) return RunUiaClient(uiaWindow, output);
        return child ? RunChild(output) : RunParent(output);
    }

    static Report NewReport()
    {
        var r = new Report();
        foreach (var key in Keys) r.Unknown(key, "not reached");
        return r;
    }
    static void Save(string path, Report report) => File.WriteAllText(path, JsonSerializer.Serialize(report, new JsonSerializerOptions { WriteIndented = true }));

    static int RunParent(string output)
    {
        var report = NewReport();
        string childPath = output + "." + Guid.NewGuid().ToString("N") + ".child.json";
        IntPtr desktop = IntPtr.Zero;
        PROCESS_INFORMATION process = default;
        bool writeFailed = false;
        var clock = Stopwatch.StartNew();
        try
        {
            var name = "OpenPainter-W0-" + Guid.NewGuid().ToString("N");
            desktop = CreateDesktop(name, null, IntPtr.Zero, 0, 0x10000000, IntPtr.Zero);
            if (desktop == IntPtr.Zero) throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error(), "CreateDesktop failed");
            string exe = Environment.ProcessPath ?? throw new InvalidOperationException("Process path is unknown");
            var command = new StringBuilder($"\"{exe}\" --child --out \"{childPath}\"");
            var startup = new STARTUPINFO { cb = Marshal.SizeOf<STARTUPINFO>(), lpDesktop = name };
            if (!CreateProcess(exe, command, IntPtr.Zero, IntPtr.Zero, false, 0, IntPtr.Zero, null, ref startup, out process))
                throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error(), "CreateProcess failed");
            while (clock.Elapsed < TimeSpan.FromSeconds(50) && !File.Exists(childPath))
            {
                if (WaitForSingleObject(process.hProcess, 0) == 0) break;
                Thread.Sleep(100);
            }
            if (File.Exists(childPath))
            {
                report = JsonSerializer.Deserialize<Report>(File.ReadAllText(childPath)) ?? throw new InvalidDataException("Child report is empty");
                string uiaPath = output + "." + Guid.NewGuid().ToString("N") + ".uia.txt";
                try
                {
                    var uiaCommand = new StringBuilder($"\"{exe}\" --uia {report.windowHandle} --out \"{uiaPath}\"");
                    var uiaStartup = new STARTUPINFO { cb = Marshal.SizeOf<STARTUPINFO>(), lpDesktop = name };
                    if (!CreateProcess(exe, uiaCommand, IntPtr.Zero, IntPtr.Zero, false, 0, IntPtr.Zero, null, ref uiaStartup, out var uia))
                        throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error(), "CreateProcess for the UI Automation client failed");
                    try
                    {
                        var left = Math.Max(0, (55 - clock.Elapsed.TotalSeconds) * 1000);
                        if (WaitForSingleObject(uia.hProcess, (uint)left) != 0)
                        {
                            if (!TerminateProcess(uia.hProcess, 1)) throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error(), "TerminateProcess failed");
                            report.Unknown(UiaKey, "client hit the time limit");
                        }
                        else if (!GetExitCodeProcess(uia.hProcess, out uint uiaCode)) report.Set(UiaKey, false, $"GetExitCodeProcess failed: {Marshal.GetLastWin32Error()}");
                        else report.Set(UiaKey, uiaCode == 0, File.Exists(uiaPath) ? File.ReadAllText(uiaPath) : $"client exit {uiaCode}, no message");
                    }
                    finally { CloseHandle(uia.hThread); CloseHandle(uia.hProcess); }
                }
                catch (Exception error) { report.Set(UiaKey, false, error.GetBaseException().Message); }
                if (File.Exists(uiaPath)) File.Delete(uiaPath);
                if (report.windowHandle != 0) PostMessage(new IntPtr(report.windowHandle), 0x0010, IntPtr.Zero, IntPtr.Zero);
            }
            else report.Set(WindowKey, false, "child did not write a report within 50 seconds");
            if (WaitForSingleObject(process.hProcess, (uint)Math.Max(0, (60 - clock.Elapsed.TotalSeconds) * 1000)) != 0)
            {
                if (!TerminateProcess(process.hProcess, 1)) throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error(), "TerminateProcess failed");
                foreach (var key in Keys.Where(k => report.checks[k].status == "could not test")) report.Unknown(key, "60 second timeout");
            }
            else if (report.checks[UiaKey].status == "pass")
            {
                if (!GetExitCodeProcess(process.hProcess, out uint code)) report.Set(UiaKey, false, $"GetExitCodeProcess failed: {Marshal.GetLastWin32Error()}");
                else if (code != 0) report.Set(UiaKey, false, $"child did not confirm button invocation (exit {code})");
            }
        }
        catch (Exception error)
        {
            foreach (var key in Keys.Where(k => report.checks[k].status == "could not test")) report.Unknown(key, error.Message);
        }
        finally
        {
            if (process.hProcess != IntPtr.Zero)
            {
                if (WaitForSingleObject(process.hProcess, 0) != 0 && !TerminateProcess(process.hProcess, 1))
                    report.Set(WindowKey, false, $"Could not stop child: {Marshal.GetLastWin32Error()}");
                CloseHandle(process.hThread);
                CloseHandle(process.hProcess);
            }
            if (desktop != IntPtr.Zero && !CloseDesktop(desktop)) report.Set(WindowKey, false, $"CloseDesktop failed: {Marshal.GetLastWin32Error()}");
            try { Save(output, report); }
            catch (Exception error) { Console.Error.WriteLine($"Cannot write report: {error}"); writeFailed = true; }
        }
        return !writeFailed && report.checks.Values.All(x => x.status == "pass") ? 0 : 1;
    }

    static int RunUiaClient(long handle, string output)
    {
        try { InvokeButton(handle); return 0; }
        catch (Exception error) { File.WriteAllText(output, error.GetBaseException().ToString()); return 1; }
    }

    static void InvokeButton(long handle)
    {
        var window = AutomationElement.FromHandle(new IntPtr(handle));
        if (window == null) throw new InvalidOperationException("UI Automation found no window");
        var button = window.FindFirst(TreeScope.Descendants,
            new PropertyCondition(AutomationElement.ControlTypeProperty, ControlType.Button));
        if (button == null) throw new InvalidOperationException("UI Automation found no button");
        if (!button.TryGetCurrentPattern(InvokePattern.Pattern, out var pattern))
            throw new InvalidOperationException("Button has no InvokePattern");
        ((InvokePattern)pattern).Invoke();
    }

    static int RunChild(string output)
    {
        var report = NewReport();
        IntPtr window = IntPtr.Zero;
        try
        {
            var klass = "OpenPainterHiddenProbe" + Guid.NewGuid().ToString("N");
            var wc = new WNDCLASS { lpfnWndProc = WndProcValue, lpszClassName = klass, hInstance = GetModuleHandle(null) };
            if (RegisterClass(ref wc) == 0) throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error(), "RegisterClass failed");
            window = CreateWindowEx(0, klass, "OpenPainter W0 Hidden Probe", 0x10CF0000,
                10, 10, 160, 160, IntPtr.Zero, IntPtr.Zero, wc.hInstance, IntPtr.Zero);
            if (window == IntPtr.Zero) throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error(), "CreateWindowEx failed");
            report.windowHandle = window.ToInt64();
            report.Set(WindowKey, true);
            var button = CreateWindowEx(0, "BUTTON", "Invoke", 0x50010000, 10, 10, 80, 28,
                window, new IntPtr(1001), wc.hInstance, IntPtr.Zero);
            if (button == IntPtr.Zero) report.Set(UiaKey, false, $"Button creation failed: {Marshal.GetLastWin32Error()}");
            else report.buttonHandle = button.ToInt64();
            ShowWindow(window, 5);
            UpdateWindow(window);
            try { DrawAndRead(window, report); } catch (Exception error) { report.Set(DeviceKey, false, error.Message); report.Set(ReadKey, false, error.Message); }
            try
            {
                if (button == IntPtr.Zero) throw new InvalidOperationException("button was not created");
                SetFocus(button);
                report.Set(FocusKey, GetFocus() == button, GetFocus() == button ? null : "GetFocus did not return the button");
            }
            catch (Exception error) { report.Set(FocusKey, false, error.Message); }
            Save(output, report);
            var end = Stopwatch.StartNew();
            while (end.Elapsed < TimeSpan.FromSeconds(55))
            {
                while (PeekMessage(out var msg, IntPtr.Zero, 0, 0, 1))
                {
                    if (msg.message == 0x0012) return buttonInvoked ? 0 : 1;
                    TranslateMessage(ref msg);
                    DispatchMessage(ref msg);
                }
                Thread.Sleep(20);
            }
            return 1;
        }
        catch (Exception error)
        {
            report.Set(WindowKey, false, error.Message);
            try { Save(output, report); } catch (Exception writeError) { Console.Error.WriteLine(writeError); }
            return 1;
        }
        finally { if (window != IntPtr.Zero) DestroyWindow(window); }
    }

    // A real D3D12 flip swap chain, the same kind wgpu makes on DX12. It stays alive while PrintWindow runs.
    static void DrawAndRead(IntPtr window, Report report)
    {
        using var factory = DXGI.CreateDXGIFactory2<IDXGIFactory4>(false);
        D3D12.D3D12CreateDevice(null, FeatureLevel.Level_11_0, out ID3D12Device? created).CheckError();
        using var device = created!;
        using var queue = device.CreateCommandQueue(new CommandQueueDescription(CommandListType.Direct));
        var desc = new SwapChainDescription1
        {
            Width = 128, Height = 128, Format = Format.R8G8B8A8_UNorm, BufferCount = 2,
            BufferUsage = Usage.RenderTargetOutput, SampleDescription = new SampleDescription(1, 0),
            SwapEffect = SwapEffect.FlipDiscard, Scaling = Scaling.Stretch
        };
        using var chain1 = factory.CreateSwapChainForHwnd(queue, window, desc);
        using var chain = chain1.QueryInterface<IDXGISwapChain3>();
        using var back = chain.GetBuffer<ID3D12Resource>(chain.CurrentBackBufferIndex);
        using var heap = device.CreateDescriptorHeap(new DescriptorHeapDescription(DescriptorHeapType.RenderTargetView, 1));
        var target = heap.GetCPUDescriptorHandleForHeapStart();
        device.CreateRenderTargetView(back, null, target);
        using var allocator = device.CreateCommandAllocator(CommandListType.Direct);
        using var list = device.CreateCommandList<ID3D12GraphicsCommandList>(CommandListType.Direct, allocator);
        const uint rowPitch = 512;
        using var readback = device.CreateCommittedResource(new HeapProperties(HeapType.Readback), HeapFlags.None,
            ResourceDescription.Buffer(rowPitch * 128), ResourceStates.CopyDest);
        var footprint = new PlacedSubresourceFootPrint
        {
            Footprint = new SubresourceFootPrint(Format.R8G8B8A8_UNorm, 128, 128, 1, rowPitch)
        };
        list.ResourceBarrierTransition(back, ResourceStates.Present, ResourceStates.RenderTarget);
        list.ClearRenderTargetView(target, new Color4(1, 0, 0, 1));
        list.ResourceBarrierTransition(back, ResourceStates.RenderTarget, ResourceStates.CopySource);
        list.CopyTextureRegion(new TextureCopyLocation(readback, footprint), 0, 0, 0, new TextureCopyLocation(back, 0));
        list.ResourceBarrierTransition(back, ResourceStates.CopySource, ResourceStates.Present);
        list.Close();
        queue.ExecuteCommandList(list);
        using var fence = device.CreateFence(0);
        using var done = new AutoResetEvent(false);
        queue.Signal(fence, 1).CheckError();
        fence.SetEventOnCompletion(1, done).CheckError();
        if (!done.WaitOne(10000)) throw new TimeoutException("GPU did not finish in 10 seconds");
        var rgba = new byte[4];
        unsafe
        {
            byte* data = null;
            readback.Map(0, &data).CheckError();
            Marshal.Copy((IntPtr)(data + rowPitch * 64 + 4 * 64), rgba, 0, 4);
            readback.Unmap(0);
        }
        report.Set(ReadKey, rgba[0] == 255 && rgba[1] == 0 && rgba[2] == 0, $"pixel = {string.Join(',', rgba)}");
        chain.Present(0, PresentFlags.None).CheckError();
        report.Set(DeviceKey, true);
        Thread.Sleep(300);
        try { var seen = Capture(window); report.Set(PrintKey, seen, "PrintWindow returned no red pixel from the swap chain"); }
        catch (Exception error) { report.Set(PrintKey, false, error.Message); }
    }

    // Flag 2 is PW_RENDERFULLCONTENT, which includes GPU content. The button is not red, so red can only come from the swap chain.
    static bool Capture(IntPtr window)
    {
        var screen = GetDC(window);
        var dc = CreateCompatibleDC(screen);
        var bitmap = CreateCompatibleBitmap(screen, 160, 160);
        if (dc == IntPtr.Zero || bitmap == IntPtr.Zero) throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error(), "capture setup failed");
        var old = SelectObject(dc, bitmap);
        try
        {
            if (!PrintWindow(window, dc, 2)) throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error(), "PrintWindow failed");
            for (int y = 0; y < 160; y += 4) for (int x = 0; x < 160; x += 4)
                if (GetPixel(dc, x, y) == 0x000000FF) return true;
            return false;
        }
        finally { SelectObject(dc, old); DeleteObject(bitmap); DeleteDC(dc); ReleaseDC(window, screen); }
    }

    static IntPtr OnWindowMessage(IntPtr hwnd, uint msg, IntPtr wp, IntPtr lp)
    {
        if (msg == 0x0111 && ((long)wp & 0xffff) == 1001) { buttonInvoked = true; return IntPtr.Zero; }
        if (msg == 0x0010) { DestroyWindow(hwnd); return IntPtr.Zero; }
        if (msg == 0x0002) { PostQuitMessage(0); return IntPtr.Zero; }
        return DefWindowProc(hwnd, msg, wp, lp);
    }

    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)] struct WNDCLASS
    { public uint style; public WndProc lpfnWndProc; public int cbClsExtra, cbWndExtra; public IntPtr hInstance, hIcon, hCursor, hbrBackground; [MarshalAs(UnmanagedType.LPWStr)] public string? lpszMenuName, lpszClassName; }
    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)] struct STARTUPINFO
    { public int cb; public string? lpReserved, lpDesktop, lpTitle; public int dwX, dwY, dwXSize, dwYSize, dwXCountChars, dwYCountChars, dwFillAttribute, dwFlags; public short wShowWindow, cbReserved2; public IntPtr lpReserved2, hStdInput, hStdOutput, hStdError; }
    [StructLayout(LayoutKind.Sequential)] struct PROCESS_INFORMATION { public IntPtr hProcess, hThread; public int dwProcessId, dwThreadId; }
    [StructLayout(LayoutKind.Sequential)] struct MSG { public IntPtr hwnd; public uint message; public IntPtr wParam, lParam; public uint time; public int x, y; }
    delegate IntPtr WndProc(IntPtr hwnd, uint msg, IntPtr wp, IntPtr lp);
    [DllImport("user32.dll", CharSet = CharSet.Unicode, SetLastError = true)] static extern IntPtr CreateDesktop(string name, string? device, IntPtr devmode, uint flags, uint access, IntPtr security);
    [DllImport("user32.dll", SetLastError = true)] static extern bool CloseDesktop(IntPtr desktop);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)] static extern bool CreateProcess(string app, StringBuilder command, IntPtr processAttributes, IntPtr threadAttributes, bool inherit, uint flags, IntPtr environment, string? currentDirectory, ref STARTUPINFO startup, out PROCESS_INFORMATION process);
    [DllImport("kernel32.dll", SetLastError = true)] static extern uint WaitForSingleObject(IntPtr handle, uint timeout);
    [DllImport("kernel32.dll", SetLastError = true)] static extern bool TerminateProcess(IntPtr handle, uint exitCode);
    [DllImport("kernel32.dll", SetLastError = true)] static extern bool GetExitCodeProcess(IntPtr handle, out uint exitCode);
    [DllImport("kernel32.dll", SetLastError = true)] static extern bool CloseHandle(IntPtr handle);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode)] static extern IntPtr GetModuleHandle(string? name);
    [DllImport("user32.dll", CharSet = CharSet.Unicode, SetLastError = true)] static extern ushort RegisterClass(ref WNDCLASS wc);
    [DllImport("user32.dll", CharSet = CharSet.Unicode, SetLastError = true)] static extern IntPtr CreateWindowEx(uint exStyle, string klass, string title, uint style, int x, int y, int width, int height, IntPtr parent, IntPtr menu, IntPtr instance, IntPtr param);
    [DllImport("user32.dll")] static extern bool ShowWindow(IntPtr window, int command);
    [DllImport("user32.dll")] static extern bool UpdateWindow(IntPtr window);
    [DllImport("user32.dll")] static extern IntPtr SetFocus(IntPtr window);
    [DllImport("user32.dll")] static extern IntPtr GetFocus();
    [DllImport("user32.dll")] static extern bool DestroyWindow(IntPtr window);
    [DllImport("user32.dll")] static extern bool PostMessage(IntPtr window, uint message, IntPtr wp, IntPtr lp);
    [DllImport("user32.dll")] static extern bool PeekMessage(out MSG msg, IntPtr window, uint min, uint max, uint remove);
    [DllImport("user32.dll")] static extern bool TranslateMessage(ref MSG msg);
    [DllImport("user32.dll")] static extern IntPtr DispatchMessage(ref MSG msg);
    [DllImport("user32.dll")] static extern void PostQuitMessage(int code);
    [DllImport("user32.dll")] static extern IntPtr DefWindowProc(IntPtr window, uint message, IntPtr wp, IntPtr lp);
    [DllImport("user32.dll")] static extern IntPtr GetDC(IntPtr window);
    [DllImport("user32.dll")] static extern int ReleaseDC(IntPtr window, IntPtr dc);
    [DllImport("gdi32.dll")] static extern IntPtr CreateCompatibleDC(IntPtr dc);
    [DllImport("gdi32.dll")] static extern IntPtr CreateCompatibleBitmap(IntPtr dc, int width, int height);
    [DllImport("gdi32.dll")] static extern IntPtr SelectObject(IntPtr dc, IntPtr bitmap);
    [DllImport("gdi32.dll")] static extern bool DeleteObject(IntPtr bitmap);
    [DllImport("gdi32.dll")] static extern bool DeleteDC(IntPtr dc);
    [DllImport("gdi32.dll")] static extern uint GetPixel(IntPtr dc, int x, int y);
    [DllImport("user32.dll")] static extern bool PrintWindow(IntPtr window, IntPtr dc, uint flags);
}
