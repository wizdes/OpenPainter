// dotnet run --project tools/ui-look/app -- --out <folder>
// Writes the 12 look PNGs by the headless Skia renderer. Opens no window on the screen.
using Avalonia;
using Avalonia.Headless;
using Avalonia.Themes.Fluent;

namespace UiLook;

public sealed class App : Application
{
    public override void Initialize() => Styles.Add(new FluentTheme());
}

public static class Program
{
    public static AppBuilder BuildAvaloniaApp() => AppBuilder.Configure<App>()
        .UseSkia()
        .UseHeadless(new AvaloniaHeadlessPlatformOptions { UseHeadlessDrawing = false });

    public static int Main(string[] args)
    {
        var sheet = Array.IndexOf(args, "--icon-sheet");
        if (sheet >= 0 && sheet + 1 < args.Length)
        {
            BuildAvaloniaApp().SetupWithoutStarting();
            if (sheet + 2 < args.Length) Icons.Folder = args[sheet + 2];   // Optional: another folder of icons.
            Renderer.RenderIconSheet(args[sheet + 1]);
            return 0;
        }
        var wand = Array.IndexOf(args, "--magic-wand-check");
        if (wand >= 0 && wand + 1 < args.Length)
        {
            BuildAvaloniaApp().SetupWithoutStarting();
            Renderer.RenderMagicWandCheck(args[wand + 1]);
            return 0;
        }
        var at = Array.IndexOf(args, "--out");
        if (at < 0 || at + 1 >= args.Length)
        {
            Console.Error.WriteLine("usage: UiLook --out <folder> | --icon-sheet <file.png> [icon folder] | --magic-wand-check <file.png>");
            return 2;
        }
        BuildAvaloniaApp().SetupWithoutStarting();
        foreach (var file in Renderer.RenderAll(args[at + 1])) Console.WriteLine(file);
        return 0;
    }
}
