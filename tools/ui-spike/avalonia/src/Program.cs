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
    public static AppBuilder BuildAvaloniaApp() => WithMode(AppBuilder.Configure<App>().UsePlatformDetect().LogToTrace());
    // SPIKE_AV_MODE tries Avalonia's other drawing routes, to see whether the UI frame rate follows them.
    static AppBuilder WithMode(AppBuilder builder) => Environment.GetEnvironmentVariable("SPIKE_AV_MODE") switch {
        null or "" => builder,
        "software" => builder.With(new Win32PlatformOptions { RenderingMode = [Win32RenderingMode.Software] }),
        "wgl" => builder.With(new Win32PlatformOptions { RenderingMode = [Win32RenderingMode.Wgl] }),
        "redirection" => builder.With(new Win32PlatformOptions { CompositionMode = [Win32CompositionMode.RedirectionSurface] }),
        "dcomp" => builder.With(new Win32PlatformOptions { CompositionMode = [Win32CompositionMode.DirectComposition] }),
        var other => throw new ArgumentException($"SPIKE_AV_MODE: unknown mode '{other}'"),
    };
}
