// Checks that every scene draws by the headless renderer. No window opens on the screen.
using Avalonia;
using Avalonia.Headless;
using Avalonia.Headless.XUnit;
using Avalonia.Media.Imaging;
using UiLook;
using Xunit;

[assembly: AvaloniaTestApplication(typeof(UiLookTests.TestApp))]

namespace UiLookTests;

public sealed class TestApp
{
    public static AppBuilder BuildAvaloniaApp() => AppBuilder.Configure<App>()
        .UseSkia()
        .UseHeadless(new AvaloniaHeadlessPlatformOptions { UseHeadlessDrawing = false });
}

public sealed class RenderTests
{
    [AvaloniaFact]
    public void EveryRenderIsWrittenAtItsSizeAndIsNotBlank()
    {
        var folder = Path.Combine(Path.GetTempPath(), "ui-look-tests-" + Guid.NewGuid().ToString("N"));
        try
        {
            var written = Renderer.RenderAll(folder);
            var expected = Renderer.Outputs().ToList();
            Assert.Equal(12, expected.Count);
            Assert.Equal(12, written.Count);
            foreach (var (file, width, height) in expected)
            {
                var path = Path.Combine(folder, file);
                Assert.True(File.Exists(path), $"{file} was not written");
                using var bitmap = new Bitmap(path);
                Assert.Equal(new PixelSize(width, height), bitmap.PixelSize);
                var colors = DistinctColors(bitmap);
                Assert.True(colors > 500, $"{file} has only {colors} colors, so it looks blank");
            }
        }
        finally
        {
            if (Directory.Exists(folder)) Directory.Delete(folder, recursive: true);
        }
    }

    static unsafe int DistinctColors(Bitmap bitmap)
    {
        var size = bitmap.PixelSize;
        var stride = size.Width * 4;
        var pixels = new byte[stride * size.Height];
        fixed (byte* p = pixels) bitmap.CopyPixels(new PixelRect(size), (nint)p, pixels.Length, stride);
        var seen = new HashSet<uint>();
        for (var i = 0; i < pixels.Length; i += 4)
            seen.Add(BitConverter.ToUInt32(pixels, i));
        return seen.Count;
    }

    [AvaloniaFact]
    public void TheLoaderDrawsEveryIcon()
    {
        var files = Directory.GetFiles(Icons.Folder, "*.svg");
        Assert.True(files.Length > 100, $"Only {files.Length} icons were copied to the output folder.");
        foreach (var file in files)
        {
            var art = Icons.Read(file);
            Assert.True(art.ViewBox == 20, $"{file}: view box is {art.ViewBox}");
            Assert.All(art.Parts, part => Assert.True(part.Geometry.Bounds.Width > 0 || part.Geometry.Bounds.Height > 0, $"{file} has an empty path"));
        }
    }

    [Fact]
    public void TheSceneCodeNeverAsksWhichThemeIsOn()
    {
        // Light and dark differ only by the tokens. Only Renderer tells Fluent the variant.
        var repo = new DirectoryInfo(AppContext.BaseDirectory);
        while (repo is not null && !File.Exists(Path.Combine(repo.FullName, "tools", "ui-look", "app", "UiLook.csproj"))) repo = repo.Parent;
        Assert.NotNull(repo);
        foreach (var name in new[] { "Shell.cs", "Panels.cs", "Dialogs.cs", "Scenes.cs", "Ui.cs" })
        {
            var text = File.ReadAllText(Path.Combine(repo.FullName, "tools", "ui-look", "app", "src", name));
            Assert.DoesNotContain("ThemeVariant", text);
            Assert.DoesNotContain("Tokens.Dark", text);
            Assert.DoesNotContain("Tokens.Light", text);
        }
    }
}
