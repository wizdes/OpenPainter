using Avalonia;
using Avalonia.Controls.ApplicationLifetimes;
using Avalonia.Themes.Fluent;

namespace SpikeAvalonia;

public sealed class App : Application {
    public static string? MeasurePath;
    public override void Initialize() { Styles.Add(new FluentTheme()); }
    public override void OnFrameworkInitializationCompleted() {
        if (ApplicationLifetime is IClassicDesktopStyleApplicationLifetime desktop) desktop.MainWindow = new Shell(MeasurePath);
        base.OnFrameworkInitializationCompleted();
    }
}
public static class Program {
    public static long StartTimestamp { get; private set; }
    [STAThread]
    public static int Main(string[] args) {
        StartTimestamp = System.Diagnostics.Stopwatch.GetTimestamp();
        if (args.Length != 0 && (args.Length != 2 || args[0] != "--measure")) { Console.Error.WriteLine("ARGUMENTS: use --measure <out.json>"); return 2; }
        App.MeasurePath = args.Length == 2 ? args[1] : null;
        try { BuildAvaloniaApp().StartWithClassicDesktopLifetime(args); return 0; }
        catch (Exception e) { Console.Error.WriteLine($"AVALONIA_SHELL: {e}"); return 1; }
    }
    public static AppBuilder BuildAvaloniaApp() => AppBuilder.Configure<App>().UsePlatformDetect().LogToTrace();
}
