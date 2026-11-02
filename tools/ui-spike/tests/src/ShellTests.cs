using Avalonia;
using Avalonia.Controls;
using Avalonia.Headless;
using Avalonia.Headless.XUnit;
using Avalonia.Input;
using SpikeAvalonia;
using Xunit;

[assembly: AvaloniaTestApplication(typeof(SpikeTests.TestApp))]
namespace SpikeTests;

public sealed class TestApp {
    public static AppBuilder BuildAvaloniaApp() => AppBuilder.Configure<SpikeAvalonia.App>()
        .UseSkia().UseHeadless(new AvaloniaHeadlessPlatformOptions { UseHeadlessDrawing = false });
}

public sealed class ShellTests {
    [AvaloniaFact]
    public void MenuDialogNumberAndCanvasInput() {
        var shell = new Shell(headless: true); shell.Show();
        var menuAt = shell.MenuButton.TranslatePoint(new Point(5, 5), shell)!.Value;
        shell.MouseDown(menuAt, MouseButton.Left); shell.MouseUp(menuAt, MouseButton.Left);
        Assert.Contains("dialog:open", shell.EventLog);
        var dialog = shell.OwnedWindows.Single(w => w.Title == "Test dialog");
        var number = ((StackPanel)dialog.Content!).Children.OfType<NumericUpDown>().Single();
        number.Focus(); dialog.KeyTextInput("42");
        Assert.Contains("42", number.Text ?? "");
        shell.CanvasInput.Focus();
        shell.KeyTextInput("é");
        var point = shell.CanvasInput.TranslatePoint(new Point(20, 20), shell)!.Value;
        shell.MouseDown(point, MouseButton.Left); shell.MouseUp(point, MouseButton.Left);
        Assert.Contains("focus:canvas", shell.EventLog);
        Assert.Contains("text:é", shell.EventLog);
        Assert.Contains(shell.EventLog, e => e.StartsWith("pointer:down:"));
        shell.PanelButton.Focus();
        shell.KeyPress(Key.Tab, RawInputModifiers.None, PhysicalKey.Tab, null);
        Assert.True(shell.CanvasInput.IsFocused);
        shell.KeyPress(Key.Tab, RawInputModifiers.None, PhysicalKey.Tab, null);
        Assert.True(shell.MenuButton.IsFocused);
        dialog.Close(); shell.Close();
    }
}
