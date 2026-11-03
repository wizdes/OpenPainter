using Avalonia;
using Avalonia.Headless;
using Avalonia.Headless.XUnit;
using OpenPainter.Shell;

[assembly: AvaloniaTestApplication(typeof(OpenPainter.Tests.TestApp))]

namespace OpenPainter.Tests;

public sealed class TestApp
{
    public static AppBuilder BuildAvaloniaApp() => AppBuilder.Configure<App>()
        .UseSkia().UseHeadless(new AvaloniaHeadlessPlatformOptions { UseHeadlessDrawing = false });
}

static class Repo
{
    public static string Root { get; } = Find();

    static string Find()
    {
        for (var dir = new DirectoryInfo(AppContext.BaseDirectory); dir is not null; dir = dir.Parent)
            if (Directory.Exists(Path.Combine(dir.FullName, "windows", "OpenPainter")) && Directory.Exists(Path.Combine(dir.FullName, "engine")))
                return dir.FullName;
        throw new DirectoryNotFoundException($"no repository above {AppContext.BaseDirectory}");
    }

    public static string File(string relative) => Path.Combine(Root, relative.Replace('/', Path.DirectorySeparatorChar));
    public static string Read(string relative) => System.IO.File.ReadAllText(File(relative)).Replace("\r\n", "\n");
}

static class Shells
{
    public static StartOptions NoOptions => new(null, null, null, null);

    public static MainWindow Open(string? theme = null, double width = 1280, double height = 832)
    {
        var window = new MainWindow(NoOptions with { Theme = theme }, headless: true) { Width = width, Height = height };
        window.Show();
        return window;
    }
}
