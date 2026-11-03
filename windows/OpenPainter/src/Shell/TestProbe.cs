// Test hooks that the harness in windows/harness reads from outside the process. They run
// only when an argument asks for them.
//
// --test-first-frame writes a file at the moment the engine has presented a frame at the
// asked zoom, so the harness can wait on that signal and not on a fixed time.
// --test-pan-trace drags the Pan tool along a fixed path by posting mouse messages to the
// canvas's own window, so the user's pointer does not move, and writes the frame times the
// engine stamped on its render thread.
using System.Diagnostics;
using System.Globalization;
using System.Runtime.InteropServices;
using System.Text;
using Avalonia;
using OpenPainter.Canvas;

namespace OpenPainter.Shell;

public static partial class TestProbe
{
    const uint WmMouseMove = 0x0200, WmLeftDown = 0x0201, WmLeftUp = 0x0202;
    const nint MkLeft = 1;
    const uint FirstFrameLimitMs = 120_000, NextFrameLimitMs = 10_000;
    const int PanMoveEveryMs = 4;       // 250 moves a second, more than any display here shows.
    const int SettleMs = 1000;          // The strip's preview and first layouts end before the count.
    const int LastMessagesMs = 200;     // The last posted messages reach the canvas.
    const int SampleEveryMs = 250;      // How often the trace reads the engine's view.

    [LibraryImport("user32.dll", EntryPoint = "PostMessageW", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static partial bool PostMessage(nint hwnd, uint message, nint w, nint l);
    [LibraryImport("winmm.dll")] private static partial uint timeBeginPeriod(uint ms);
    [LibraryImport("winmm.dll")] private static partial uint timeEndPeriod(uint ms);

    public static void Arrange(MainWindow window, StartOptions options, Action<int> setExitCode)
    {
        if (options.TestZoom is null && options.FirstFramePath is null && options.PanTracePath is null) return;
        window.ImageReady += () => _ = Run(window, options, setExitCode);
    }

    static string Text(double value) => value.ToString("R", CultureInfo.InvariantCulture);

    static void WriteAtOnce(string path, string text)
    {
        var partial = path + ".part";
        File.WriteAllText(partial, text);
        File.Move(partial, path, overwrite: true);
    }

    /// The window and view facts that the harness needs to find a document pixel on screen.
    static string ViewFacts(MainWindow window)
    {
        var session = window.Session ?? throw new InvalidOperationException("TEST_PROBE: no session");
        var native = window.Native ?? throw new InvalidOperationException("TEST_PROBE: no canvas");
        var main = window.TryGetPlatformHandle()?.Handle ?? throw new InvalidOperationException("TEST_PROBE: no window handle");
        var info = session.Info();
        var v = window.View.View;
        var stage = window.Palette.Stage;
        return $"\"main_hwnd\": {main}, \"canvas_hwnd\": {native.Hwnd}, " +
            $"\"zoom\": {Text(info.zoom)}, \"origin_x\": {Text(info.origin_x)}, \"origin_y\": {Text(info.origin_y)}, \"scale\": {Text(info.scale)}, " +
            $"\"document_width\": {Text(v.DocumentSize.Width)}, \"document_height\": {Text(v.DocumentSize.Height)}, " +
            $"\"view_width\": {Text(v.ViewSize.Width)}, \"view_height\": {Text(v.ViewSize.Height)}, " +
            $"\"drawable_width\": {info.drawable_width}, \"drawable_height\": {info.drawable_height}, " +
            $"\"frames\": {info.frames}, \"frame_errors\": {info.frame_errors}, \"last_error\": {info.last_error}, " +
            $"\"validation_errors\": {session.Counters().validation_errors}, " +
            $"\"has_display_table\": {info.has_display_table}, \"display_icc_len\": {info.display_icc_len}, \"doc_uploads\": {info.doc_uploads}, " +
            $"\"stage\": [{stage.R}, {stage.G}, {stage.B}], \"theme\": \"{window.ActualThemeVariant}\", " +
            $"\"client_width\": {Text(window.ClientSize.Width)}, \"client_height\": {Text(window.ClientSize.Height)}, " +
            $"\"status_bottom\": {Text(window.Status.TranslatePoint(new Point(0, window.Status.Bounds.Height), window)?.Y ?? double.MaxValue)}";
    }

    /// Runs on the UI thread, and each long wait runs on another thread.
    static async Task Run(MainWindow window, StartOptions options, Action<int> setExitCode)
    {
        try
        {
            var session = window.Session ?? throw new InvalidOperationException("TEST_PROBE: no session");
            var started = Process.GetCurrentProcess().StartTime;
            double SinceStart() => (DateTime.Now - started).TotalMilliseconds;
            var attached = SinceStart();
            if (!await Task.Run(() => session.WaitFrames(1, FirstFrameLimitMs)))
                throw new InvalidOperationException($"no frame in {FirstFrameLimitMs} ms: frame errors {session.Info().frame_errors}, last {session.Info().last_error}");
            var first = SinceStart();
            await window.CanvasShown;

            if (options.TestZoom is { } zoom)
            {
                var before = session.Info().frames;
                window.View.SetZoom(zoom);
                if (!await Task.Run(() => session.WaitFrames(before + 1, NextFrameLimitMs)))
                    throw new InvalidOperationException($"no frame in {NextFrameLimitMs} ms after the zoom changed");
            }

            if (options.FirstFramePath is { } path)
                WriteAtOnce(path, $"{{ \"attached_ms\": {Text(attached)}, \"first_frame_ms\": {Text(first)}, \"signal_ms\": {Text(SinceStart())}, {ViewFacts(window)} }}\n");

            if (options.PanTracePath is { } tracePath)
            {
                // The window stays open, so the harness can read what it shows after the trace.
                await PanTraceRun(window, options.PanSeconds, tracePath);
            }
        }
        catch (Exception e)
        {
            // A failed hook must end the run with its name, not leave a window waiting.
            Console.Error.WriteLine($"TEST_PROBE: {e.Message}");
            setExitCode(1);
            window.Close();
        }
    }

    static async Task PanTraceRun(MainWindow window, double seconds, string path)
    {
        var session = window.Session ?? throw new InvalidOperationException("TEST_PROBE: no session");
        var hwnd = window.Native?.Hwnd ?? throw new InvalidOperationException("TEST_PROBE: no canvas");
        var view = window.View;
        window.Tools.Current = ToolState.Pan;
        WindowLimits.RaiseWithoutFocus(window.TryGetPlatformHandle()?.Handle ?? throw new InvalidOperationException("no window handle"));
        await Task.Delay(SettleMs);

        var v = view.View;
        var trace = PanTrace.For(v.DocumentSize.Width, v.DocumentSize.Height, v.ViewSize.Width, v.ViewSize.Height, v.Zoom, window.RenderScaling);
        var viewChanges = 0L;
        void CountChange() => viewChanges++;
        view.Changed += CountChange;
        session.TakeFrameTimes();
        var framesBefore = session.Info().frames;
        var errorsBefore = session.Info().frame_errors;
        var originStart = v.Origin;

        var drive = await Task.Run(() => DriveTrace(session, hwnd, trace, seconds, framesBefore));
        await Task.Delay(LastMessagesMs);
        var times = session.TakeFrameTimes();
        view.Changed -= CountChange;

        var info = session.Info();
        var list = new StringBuilder();
        foreach (var time in times) list.Append(list.Length == 0 ? "" : ", ").Append(Text(time));
        var samples = string.Join(", ", drive.Samples.Select(s => $"[{Text(s.Seconds)}, {s.X}, {s.Y}, {Text(s.OriginX)}, {Text(s.OriginY)}, {s.Frames}]"));
        // The frame log has its own clock, so the first frame ties it to this one. A run that
        // never saw a frame reports the whole time as lost at both ends.
        var startGap = drive.FirstFrameMs is { } firstAt ? firstAt - drive.FirstMoveMs : drive.EndMs - drive.FirstMoveMs;
        var endGap = drive.FirstFrameMs is { } seenAt && times.Length != 0 ? drive.EndMs - seenAt - (times[^1] - times[0]) : drive.EndMs - drive.FirstMoveMs;
        WriteAtOnce(path, $"{{ \"seconds\": {Text(seconds)}, \"moves_posted\": {drive.Moves}, \"view_changes\": {viewChanges}, " +
            $"\"start_gap_ms\": {Text(startGap)}, \"end_gap_ms\": {Text(endGap)}, \"view_samples\": [{samples}], " +
            $"\"trace\": {{ \"start_x\": {trace.StartX}, \"start_y\": {trace.StartY}, \"reach_x\": {trace.ReachX}, \"reach_y\": {trace.ReachY} }}, " +
            $"\"origin_start\": [{Text(originStart.X)}, {Text(originStart.Y)}], \"origin_end\": [{Text(info.origin_x)}, {Text(info.origin_y)}], " +
            $"\"frames_counted\": {info.frames - framesBefore}, \"frame_errors_during\": {info.frame_errors - errorsBefore}, " +
            $"{ViewFacts(window)}, \"frame_times_ms\": [{list}] }}\n");
    }

    static nint PointLparam(int x, int y) => (nint)(((long)(ushort)(short)y << 16) | (ushort)(short)x);

    static void Post(nint hwnd, uint message, nint w, nint l)
    {
        if (!PostMessage(hwnd, message, w, l)) throw new InvalidOperationException($"PostMessage {message}: {Marshal.GetLastPInvokeError()}");
    }

    sealed record DriveResult(long Moves, double FirstMoveMs, double? FirstFrameMs, double EndMs, List<(double Seconds, int X, int Y, double OriginX, double OriginY, ulong Frames)> Samples);

    /// Posts the drag from its own thread. A late wake makes one bigger step, as a real
    /// pointer does. It never waits on the UI thread, so a slow UI shows up in the counts.
    /// It reads the engine's view now and then, which takes the engine's short lock and no UI.
    static DriveResult DriveTrace(CanvasSession session, nint hwnd, PanTrace trace, double seconds, ulong framesBefore)
    {
        if (timeBeginPeriod(1) != 0) throw new InvalidOperationException("timeBeginPeriod refused 1 ms");
        try
        {
            var clock = Stopwatch.StartNew();
            var (lastX, lastY) = trace.At(0);
            Post(hwnd, WmLeftDown, MkLeft, PointLparam(lastX, lastY));
            long moves = 0;
            double? firstMove = null, firstFrame = null;
            var nextSample = 0.0;
            var samples = new List<(double, int, int, double, double, ulong)>();
            while (clock.Elapsed.TotalSeconds < seconds)
            {
                Thread.Sleep(PanMoveEveryMs);
                var (x, y) = trace.At(clock.Elapsed.TotalSeconds);
                if (x != lastX || y != lastY)
                {
                    (lastX, lastY) = (x, y);
                    Post(hwnd, WmMouseMove, MkLeft, PointLparam(x, y));
                    moves++;
                    firstMove ??= clock.Elapsed.TotalMilliseconds;
                }
                var now = clock.Elapsed.TotalMilliseconds;
                if (firstFrame is null && firstMove is not null && session.Info().frames > framesBefore) firstFrame = clock.Elapsed.TotalMilliseconds;
                if (now >= nextSample)
                {
                    var info = session.Info();
                    samples.Add((now / 1000, lastX, lastY, info.origin_x, info.origin_y, info.frames));
                    nextSample = now + SampleEveryMs;
                }
            }
            var end = clock.Elapsed.TotalMilliseconds;
            Post(hwnd, WmLeftUp, 0, PointLparam(lastX, lastY));
            return new DriveResult(moves, firstMove ?? end, firstFrame, end, samples);
        }
        finally
        {
            if (timeEndPeriod(1) != 0) Console.Error.WriteLine("TEST_PROBE: timeEndPeriod refused 1 ms");
        }
    }
}
