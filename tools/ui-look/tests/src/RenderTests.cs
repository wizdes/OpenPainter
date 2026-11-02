// Checks that every scene draws by the headless renderer, that each panel is in its place
// with content in it, and that token colors keep their contrast. No window opens on the screen.
using Avalonia;
using Avalonia.Headless;
using Avalonia.Headless.XUnit;
using Avalonia.Media;
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

/// Reads known places in a rendered scene. Positions are in layout pixels, relative to the window.
public sealed unsafe class Picture
{
    readonly byte[] _pixels;
    readonly int _stride;
    readonly PixelSize _size;

    public Picture(Bitmap bitmap)
    {
        _size = bitmap.PixelSize;
        _stride = _size.Width * 4;
        _pixels = new byte[_stride * _size.Height];
        fixed (byte* p = _pixels) bitmap.CopyPixels(new PixelRect(_size), (nint)p, _pixels.Length, _stride);
    }

    public Color AtPixel(int x, int y)
    {
        var i = y * _stride + x * 4;
        return Color.FromArgb(_pixels[i + 3], _pixels[i + 2], _pixels[i + 1], _pixels[i]);
    }

    /// A point given in layout pixels from the window's top left corner.
    public Color AtWindow(double x, double y) => AtPixel((int)((x + Tokens.ShotMargin) * Renderer.Scale), (int)((y + Tokens.ShotMargin) * Renderer.Scale));

    /// How many pixels in a rectangle are close to a color.
    public int CountNear(Rect r, Color c)
    {
        var count = 0;
        for (var y = (int)((r.Y + Tokens.ShotMargin) * Renderer.Scale); y < (r.Bottom + Tokens.ShotMargin) * Renderer.Scale; y++)
            for (var x = (int)((r.X + Tokens.ShotMargin) * Renderer.Scale); x < (r.Right + Tokens.ShotMargin) * Renderer.Scale; x++)
                if (Near(AtPixel(x, y), c, 40)) count++;
        return count;
    }

    public static bool Near(Color a, Color b, int tolerance = 3) =>
        Math.Abs(a.R - b.R) <= tolerance && Math.Abs(a.G - b.G) <= tolerance && Math.Abs(a.B - b.B) <= tolerance;

    public int DistinctColors()
    {
        var seen = new HashSet<uint>();
        for (var i = 0; i < _pixels.Length; i += 4) seen.Add(BitConverter.ToUInt32(_pixels, i));
        return seen.Count;
    }
}

public static class Regions
{
    const double StageTop = Tokens.TitleHeight + Tokens.OptionsHeight + Tokens.StripHeight;
    const double StageBottom = Tokens.WindowHeight - Tokens.StatusHeight;
    const double RightX = Tokens.WindowWidth - Tokens.PanelClearance - Tokens.ListPanelWidth;
    const double LeftX = Tokens.PanelClearance;
    const double TopY = StageTop + Tokens.PanelClearance;
    const double HistoryBottom = TopY + Tokens.ListPanelHeight;
    const double LayersY = StageBottom - Tokens.PanelClearance - Tokens.ListPanelHeight;
    const double ColorsY = StageBottom - Tokens.PanelClearance - Tokens.ColorsHeight;

    /// Each panel: an empty spot that must show the panel's fill, and its body, which must hold ink.
    static readonly (string Panel, double X, double Y, Rect Body)[] Panels =
    [
        ("Tools", LeftX + 67 - 12, TopY + 24 + 5 + 9 * 26 + 12, new Rect(LeftX + 4, TopY + 26, Tokens.ToolsWidth - 8, Tokens.ToolsHeight - 30)),
        ("History", RightX + 113, HistoryBottom - 14, new Rect(RightX + 4, TopY + 26, 100, 30)),
        ("Layers", RightX + 218, LayersY + Tokens.ListPanelHeight - 14, new Rect(RightX + 4, LayersY + Tokens.ListPanelHeight - 26, 200, 24)),
        ("Colors", LeftX + 120, ColorsY + 24 + 5, new Rect(LeftX + 60, ColorsY + 30, 30, 60)),
    ];

    public static List<string> Problems(Picture pic, Palette p, Scene s)
    {
        var problems = new List<string>();
        foreach (var (panel, x, y, body) in Panels)
        {
            var got = pic.AtWindow(x, y);
            if (!Picture.Near(got, p.Panel)) problems.Add($"{s.Name}-{p.Name}: {panel} panel: at ({x}, {y}) expected {p.Panel}, got {got}");
            var ink = pic.CountNear(body, p.Ink);
            if (ink < 20) problems.Add($"{s.Name}-{p.Name}: {panel} panel: only {ink} ink pixels in its body");
        }
        Color Dim(Color c) => s.Dialog == DialogKind.None ? c : Blend(c);
        foreach (var (name, x, y, c) in new[]
        {
            ("title row", 620.0, 4.0, p.TitleBar),
            ("options row", 640.0, Tokens.TitleHeight + 3, p.Options),
            ("image strip", 640.0, Tokens.TitleHeight + Tokens.OptionsHeight + 30, p.Chrome),
            ("status bar", 620.0, Tokens.WindowHeight - 3, p.Chrome),
            ("stage", 640.0, StageTop + 20, p.Stage),
        })
        {
            var got = pic.AtWindow(x, y);
            if (!Picture.Near(got, Dim(c), 4)) problems.Add($"{s.Name}-{p.Name}: {name}: expected {Dim(c)}, got {got}");
        }
        if (s.Dialog != DialogKind.None)
        {
            var got = pic.AtWindow(440, s.Dialog == DialogKind.Curves ? 420 : 440);
            if (!Picture.Near(got, p.Panel)) problems.Add($"{s.Name}-{p.Name}: dialog body missing, got {got}");
            var title = pic.AtWindow(600, s.Dialog == DialogKind.Curves ? 210 : 315);
            if (!Picture.Near(title, p.TitleBar)) problems.Add($"{s.Name}-{p.Name}: dialog title row missing, got {title}");
        }
        return problems;
    }

    static Color Blend(Color c)
    {
        var a = Tokens.Scrim.A / 255.0;
        return Color.FromRgb((byte)(c.R * (1 - a)), (byte)(c.G * (1 - a)), (byte)(c.B * (1 - a)));
    }
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
                var colors = new Picture(bitmap).DistinctColors();
                Assert.True(colors > 500, $"{file} has only {colors} colors, so it looks blank");
            }
        }
        finally
        {
            if (Directory.Exists(folder)) Directory.Delete(folder, recursive: true);
        }
    }

    [AvaloniaFact]
    public void EveryPanelAndBarIsInItsPlaceInEveryScene()
    {
        var problems = new List<string>();
        foreach (var p in new[] { Tokens.Light, Tokens.Dark })
            foreach (var s in Scenes.WindowScenes)
            {
                using var bitmap = Renderer.DrawScene(p, s);
                problems.AddRange(Regions.Problems(new Picture(bitmap), p, s));
            }
        Assert.True(problems.Count == 0, string.Join("\n", problems));
    }

    [AvaloniaTheory]
    [InlineData("Tools")]
    [InlineData("History")]
    [InlineData("Layers")]
    [InlineData("Colors")]
    public void AMissingPanelFails(string panel)
    {
        using var bitmap = Renderer.DrawScene(Tokens.Light, Scenes.Main, omitPanel: panel);
        var problems = Regions.Problems(new Picture(bitmap), Tokens.Light, Scenes.Main);
        Assert.Contains(problems, m => m.Contains(panel + " panel", StringComparison.Ordinal));
    }

    [AvaloniaFact]
    public void TheExpandedColorsPanelHasItsSliders()
    {
        foreach (var p in new[] { Tokens.Light, Tokens.Dark })
        {
            using var bitmap = Renderer.DrawColorsExpanded(p);
            var pic = new Picture(bitmap);
            Assert.True(Picture.Near(pic.AtPixel(64 + 280, 64 + 216), p.Panel), "expanded panel body");
            Assert.True(pic.DistinctColors() > 500);
        }
    }

    [AvaloniaFact]
    public void TheMagicWandCheckShowsTheDotOutsideTheIconAndThePressedButton()
    {
        Assert.False(Panels.ToolIconBox.Intersects(Panels.ToolDotBox), "The selected dot overlaps the icon box.");
        var p = Tokens.Light;
        using var bitmap = Renderer.DrawScene(p, Scenes.MagicWandCheck);
        var pic = new Picture(bitmap);
        // The wand is the seventh tool: row 3, column 1. The panel's grid starts 17 in and 29 down.
        var cellX = Tokens.PanelClearance + 17;
        var cellY = Tokens.TitleHeight + Tokens.OptionsHeight + Tokens.StripHeight + Tokens.PanelClearance + 29 + 3 * 26;
        var dot = pic.AtWindow(cellX + Panels.ToolDotBox.Center.X, cellY + Panels.ToolDotBox.Center.Y);
        Assert.True(Picture.Near(dot, p.Accent, 8), $"The selected dot is not there: {dot}");
        var wash = pic.AtWindow(cellX + 1, cellY + 23);
        Assert.True(Picture.Near(wash, p.Accent, 8) || Picture.Near(pic.AtWindow(cellX + 3, cellY + 23), p.Wash, 8), "The selected cell has no wash.");
        // The pressed wand button in the options row: its seventh command, with wash behind it.
        var buttonX = 12 + 3 * 28 + 2 * 6 + 12 + 2 * 28 + 6 + 12 + 28 + 6;
        var fill = pic.AtWindow(buttonX + 4, Tokens.TitleHeight + 7);
        Assert.True(Picture.Near(fill, p.Wash, 8), $"The wand button is not pressed: {fill}");
    }

    [Fact]
    public void TokenColorsKeepTheirContrast()
    {
        var problems = new List<string>();
        foreach (var p in new[] { Tokens.Light, Tokens.Dark })
            foreach (var (name, front, back, minimum) in Tokens.ContrastPairs(p))
            {
                var ratio = Tokens.Contrast(front, back);
                if (ratio < minimum) problems.Add($"{p.Name}: {name} is {ratio:F2} to 1, the minimum is {minimum}");
            }
        Assert.True(problems.Count == 0, string.Join("\n", problems));
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
