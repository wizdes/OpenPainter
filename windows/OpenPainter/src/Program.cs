using System.Runtime.InteropServices;
using Avalonia;
using Avalonia.Controls.ApplicationLifetimes;
using Avalonia.Themes.Fluent;
using Avalonia.Threading;
using OpenPainter.Shell;

namespace OpenPainter;

public sealed class App : Application
{
    public static StartOptions Options { get; set; } = new(null, null, null, null);
    public static int ExitCode { get; set; }

    public override void Initialize() => Styles.Add(new FluentTheme());

    /// The app ends when its main window closes. Avalonia's default waits for every window,
    /// and a hidden panel is one that nothing would ever close.
    public static void ConfigureLifetime(IClassicDesktopStyleApplicationLifetime desktop) =>
        desktop.ShutdownMode = Avalonia.Controls.ShutdownMode.OnMainWindowClose;

    public override void OnFrameworkInitializationCompleted()
    {
        if (ApplicationLifetime is IClassicDesktopStyleApplicationLifetime desktop)
        {
            ConfigureLifetime(desktop);
            Fatal.Shutdown = () => desktop.Shutdown(1);
            Fatal.TestPlace = Options.TestThrow;
            Dispatcher.UIThread.UnhandledException += (_, e) =>
            {
                e.Handled = true;
                Fatal.Raise(e.Exception);
            };
            var window = new MainWindow(Options);
            if (Options.TestOpenMenu is { } menu)
                window.Opened += (_, _) => DispatcherTimer.RunOnce(() => window.TitleBar.Menus.Items.OfType<Avalonia.Controls.MenuItem>().Single(m => (string?)m.Header == menu).Open(), TimeSpan.FromMilliseconds(1000));
            if (Options.TestOpenMore) window.Opened += (_, _) => DispatcherTimer.RunOnce(window.Options.OpenMore, TimeSpan.FromMilliseconds(1000));
            if (Options.ExitAfterMs is { } ms) TestReport.Arrange(window, ms, Options.ReportPath, code => ExitCode = code);
            TestProbe.Arrange(window, Options, code => ExitCode = code);
            desktop.MainWindow = window;
        }
        base.OnFrameworkInitializationCompleted();
    }
}

public static partial class Program
{
    [LibraryImport("user32.dll", EntryPoint = "MessageBoxW", StringMarshalling = StringMarshalling.Utf16)]
    private static partial int MessageBox(nint owner, string text, string caption, uint type);

    /// Shows errors at once. A hooked test run ends by itself, so it must not wait on a dialog.
    public static void ShowFatal(Exception e)
    {
        const uint ErrorIcon = 0x10;
        Console.Error.WriteLine($"OPENPAINTER: {e}");
        if (App.Options.ExitAfterMs is null) MessageBox(0, e.Message, MainWindow.AppName, ErrorIcon);
    }

    [STAThread]
    public static int Main(string[] args)
    {
        try
        {
            // A bad argument must not open a dialog in a run that asked to end by itself.
            if (args.Contains("--exit-after-ms")) App.Options = App.Options with { ExitAfterMs = 1 };
            App.Options = StartOptions.Parse(args);
            BuildAvaloniaApp().StartWithClassicDesktopLifetime(args);
            return App.ExitCode;
        }
        catch (Exception e)
        {
            ShowFatal(e);
            return 1;
        }
    }

    public static AppBuilder BuildAvaloniaApp() => AppBuilder.Configure<App>().UsePlatformDetect().LogToTrace();
}
