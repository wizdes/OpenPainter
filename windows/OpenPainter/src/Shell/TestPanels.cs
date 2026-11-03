// A test hook for a real window: --test-panels <folder> builds a layer stack through the panels
// and steps through Layer Properties, one stage at a time, so the harness can look at each from
// outside. A headless test cannot see the window order, the title row's hit test or real pixels.
// The app writes ready-<n>.json, and the harness answers with go-<n>.
using System.Globalization;
using System.Runtime.InteropServices;
using Avalonia;
using Avalonia.Threading;
using OpenPainter.Bridge;

namespace OpenPainter.Shell;

public static partial class TestPanels
{
    const int PollMs = 20, LimitMs = 120_000;
    const uint WmNcHitTest = 0x0084, GwOwner = 4;
    const uint NextFrameLimitMs = 10_000;

    [LibraryImport("user32.dll", EntryPoint = "SendMessageW")] private static partial nint SendMessage(nint hwnd, uint message, nint w, nint l);
    [LibraryImport("user32.dll")] private static partial nint GetWindow(nint hwnd, uint command);

    public static void Arrange(MainWindow window, StartOptions options, Action<int> setExitCode)
    {
        if (options.TestPanelsFolder is not { } folder) return;
        window.ImageReady += () => _ = Run(window, folder, setExitCode);
    }

    static nint Handle(Avalonia.Controls.Window window) => window.TryGetPlatformHandle()?.Handle ?? throw new InvalidOperationException("TEST_PANELS: no window handle");

    static void Ready(string folder, int stage, string facts)
    {
        var path = Path.Combine(folder, $"ready-{stage}.json");
        File.WriteAllText(path + ".part", $"{{ {facts} }}\n");
        File.Move(path + ".part", path);
    }

    static async Task WaitGo(string folder, int stage)
    {
        var path = Path.Combine(folder, $"go-{stage}");
        var waited = 0;
        while (!File.Exists(path))
        {
            await Task.Delay(PollMs);
            waited += PollMs;
            if (waited > LimitMs) throw new TimeoutException($"no {path} in {LimitMs} ms");
        }
    }

    /// The renders and refreshes settle on the UI thread, in short turns so input still runs.
    static async Task Settled(Editor editor)
    {
        var ticket = editor.Client.Dispatch(editor.Document, "doc.sync", EngineParams.None);
        if (await Task.Run(() => editor.Client.Wait(ticket, LimitMs)) is null) throw new TimeoutException("doc.sync did not end");
        await Task.Delay(PollMs);
        editor.Refresh();
        await Task.Delay(PollMs);
    }

    static ulong Frames(MainWindow window) => (window.Session ?? throw new InvalidOperationException("TEST_PANELS: no session")).Info().frames;

    /// Waits for a frame after the one counted before a change. The change may have shown already.
    static async Task FrameAfter(MainWindow window, ulong before)
    {
        var session = window.Session ?? throw new InvalidOperationException("TEST_PANELS: no session");
        if (!await Task.Run(() => session.WaitFrames(before + 1, NextFrameLimitMs))) throw new TimeoutException($"no frame in {NextFrameLimitMs} ms");
    }

    static string Num(double value) => value.ToString(CultureInfo.InvariantCulture);

    /// Asks Windows what a point on the dialog is, as a real pointer would find out.
    static long HitTest(nint hwnd, PixelPoint at) =>
        (long)SendMessage(hwnd, WmNcHitTest, 0, (nint)(((long)(ushort)(short)at.Y << 16) | (ushort)(short)at.X));

    static async Task Run(MainWindow window, string folder, Action<int> setExitCode)
    {
        try
        {
            await window.CanvasShown;
            var editor = window.Editor ?? throw new InvalidOperationException("TEST_PANELS: no document");
            Directory.CreateDirectory(folder);

            // A stack with a changed copy, a hidden layer and an undone step, through the panels' own commands.
            editor.Run("layer.duplicate");
            editor.Run("image.invert_colors");
            editor.Run("layer.add", EngineParams.Of(("name", "Layer 3")));
            editor.Run("layer.set_visible", EngineParams.Of(("index", 2), ("visible", false)));
            editor.Run("layer.properties", EngineParams.Of(("index", 2), ("name", "Sketch"), ("visible", false), ("blend", "multiply"), ("opacity", 128)));
            editor.Run("layer.set_active", EngineParams.Of(("index", 1)));
            editor.Run("selection.select_all");
            editor.Run("edit.undo");
            await Settled(editor);
            window.Tools.Primary = window.Tools.Palette.Colors[12];
            window.Tools.Secondary = new DocColor(0xAB, 0xCD, 0xEF);
            await Task.Delay(500);   // The layer pictures.
            var history = window.Panels.Panels[PanelLayout.History];
            var layers = window.Panels.Panels[PanelLayout.Layers];
            var toolsPanel = window.Panels.Panels[PanelLayout.Tools];
            var colorsPanel = window.Panels.Panels[PanelLayout.Colors];
            var swatch = colorsPanel.PointToScreen(default);
            var inSwatch = window.Panels.Colors.Swatches.PointToScreen(new Point(44, 44));
            var rows = string.Join(", ", window.Panels.Layers.LayerRows.Select(r => $"\"{r.TitleLabel.Text}\""));
            Ready(folder, 1, $"\"main_hwnd\": {Handle(window)}, \"canvas_hwnd\": {window.Native?.Hwnd}, \"history_hwnd\": {Handle(history)}, \"layers_hwnd\": {Handle(layers)}, " +
                $"\"tools_hwnd\": {Handle(toolsPanel)}, \"colors_hwnd\": {Handle(colorsPanel)}, \"secondary_x\": {inSwatch.X - swatch.X}, \"secondary_y\": {inSwatch.Y - swatch.Y}, " +
                $"\"history_rows\": {window.Panels.History.Rows.Count}, \"layer_rows\": [{rows}], \"thumbs\": {window.Panels.Layers.LayerRows.Count(r => r.Thumb.Image is not null)}");
            await WaitGo(folder, 1);

            // The dialog for the inverted copy.
            var dialog = window.ShowLayerProperties(1) ?? throw new InvalidOperationException("TEST_PANELS: the dialog did not open");
            await Task.Delay(500);
            var hwnd = Handle(dialog);
            var scale = dialog.RenderScaling;
            var title = dialog.TitleRow.PointToScreen(new Point(60, dialog.TitleRow.Bounds.Height / 2));
            var close = dialog.CloseButton.PointToScreen(new Point(dialog.CloseButton.Bounds.Width / 2, dialog.CloseButton.Bounds.Height / 2));
            Ready(folder, 2, $"\"dialog_hwnd\": {hwnd}, \"owner_hwnd\": {GetWindow(hwnd, GwOwner)}, \"title_hit\": {HitTest(hwnd, title)}, \"close_hit\": {HitTest(hwnd, close)}, " +
                $"\"position_x\": {dialog.Position.X}, \"position_y\": {dialog.Position.Y}, \"scale\": {scale.ToString(CultureInfo.InvariantCulture)}");
            await WaitGo(folder, 2);

            // The harness moved it. Hiding the copy shows through the live canvas.
            var moved = dialog.Position;
            var frames = Frames(window);
            dialog.Set(dialog.Current with { Visible = false });
            await Settled(editor);
            await FrameAfter(window, frames);
            Ready(folder, 3, $"\"moved_x\": {moved.X}, \"moved_y\": {moved.Y}, \"session_started\": {(dialog.SessionStarted ? "true" : "false")}");
            await WaitGo(folder, 3);

            // Cancel brings it back and adds no step.
            var steps = editor.Info.HistoryLabels.Count;
            frames = Frames(window);
            dialog.Cancel();
            await Settled(editor);
            await FrameAfter(window, frames);
            Ready(folder, 4, $"\"steps_before\": {steps}, \"steps_after\": {editor.Info.HistoryLabels.Count}, \"copy_visible\": {(editor.Info.Layers[1].Visible ? "true" : "false")}, \"dialog_gone\": {(window.Dialog is null ? "true" : "false")}");
            await WaitGo(folder, 4);

            // More resizes Colors and Tools. A Fixed Size row binds its fields.
            window.Panels.Colors.SetExpanded(true);
            window.Tools.Current = "rectangleSelect";
            window.Tools.Settings = window.Tools.Settings with { SelectStyle = SelectStyleChoice.FixedSize, SizeWidth = 320, SizeHeight = 200 };
            window.Unit = MeasurementUnit.Inches;
            await Task.Delay(500);
            Ready(folder, 5, $"\"colors_height\": {Num(colorsPanel.ClientSize.Height)}, \"tools_height\": {Num(toolsPanel.ClientSize.Height)}, \"cells\": {window.Panels.Colors.Swatchboard.Cells.Count}, \"size_text\": \"{window.Options.Option<Avalonia.Controls.TextBox>(OptionBinding.SizeWidth).Text}\", \"image_text\": \"{window.Status.ImageValue.Text}\"");
            await WaitGo(folder, 5);

            // Less brings both back. The Text row lists Windows' fonts.
            window.Panels.Colors.SetExpanded(false);
            window.Tools.Current = "text";
            window.Unit = MeasurementUnit.Pixels;
            await Task.Delay(500);
            Ready(folder, 6, $"\"colors_height\": {Num(colorsPanel.ClientSize.Height)}, \"tools_height\": {Num(toolsPanel.ClientSize.Height)}, \"cells\": {window.Panels.Colors.Swatchboard.Cells.Count}, \"fonts\": {window.Options.Option<Avalonia.Controls.ComboBox>(OptionBinding.Font).ItemCount}, \"profile\": \"{window.Status.ProfileValue.Text}\"");
            await WaitGo(folder, 6);
        }
        catch (Exception e)
        {
            // A failed hook must end the run with its name, not leave a window waiting.
            Console.Error.WriteLine($"TEST_PANELS: {e.Message}");
            setExitCode(1);
            Dispatcher.UIThread.Post(window.Close);
        }
    }
}
