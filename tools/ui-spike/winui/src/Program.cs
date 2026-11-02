using Microsoft.UI.Xaml;

namespace SpikeWinUI;

public static class Program {
    public static long StartTimestamp { get; private set; }
    [STAThread]
    public static int Main(string[] args) {
        StartTimestamp = System.Diagnostics.Stopwatch.GetTimestamp();
        if (args.Length != 0 && (args.Length != 2 || args[0] != "--measure")) { Console.Error.WriteLine("ARGUMENTS: use --measure <out.json>"); return 2; }
        App.MeasurePath = args.Length == 2 ? args[1] : null;
        try {
            // The same start-up that the XAML compiler's generated Main does.
            WinRT.ComWrappersSupport.InitializeComWrappers();
            Application.Start(_ => {
                SynchronizationContext.SetSynchronizationContext(new Microsoft.UI.Dispatching.DispatcherQueueSynchronizationContext(Microsoft.UI.Dispatching.DispatcherQueue.GetForCurrentThread()));
                new App();
            });
            return 0;
        }
        catch (Exception e) { Console.Error.WriteLine($"WINUI_SHELL: {e}"); return 1; }
    }
}
public sealed partial class App : Application {
    public static string? MeasurePath;
    public App() {
        InitializeComponent();
        UnhandledException += (_, e) => {
            var text = $"WINUI_UNHANDLED: {e.Exception.HResult:X8} {e.Message} {e.Exception}";
            Console.Error.WriteLine(text);
            if (MeasurePath is not null) File.WriteAllText(MeasurePath + ".error.txt", text);
        };
    }
    protected override void OnLaunched(Microsoft.UI.Xaml.LaunchActivatedEventArgs args) {
        new Shell(MeasurePath).Activate();
    }
}
