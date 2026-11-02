// Draws every scene to a PNG with Avalonia's headless platform and the Skia renderer.
// No window opens on the screen.
using Avalonia;
using Avalonia.Controls;
using Avalonia.Media.Imaging;
using Avalonia.Platform;
using Avalonia.Styling;
using Avalonia.Threading;

namespace UiLook;

public static class Renderer
{
    public const int Scale = 2;

    public static string FixturePath => Path.Combine(AppContext.BaseDirectory, "assets", "plasma_alpha_512x384.png");

    /// Every output file name with its expected pixel size.
    public static IEnumerable<(string File, int Width, int Height)> Outputs()
    {
        foreach (var variant in new[] { Tokens.Light.Name, Tokens.Dark.Name })
        {
            foreach (var s in Scenes.WindowScenes)
                yield return ($"{s.Name}-{variant}.png", (int)Tokens.ShotWidth * Scale, (int)Tokens.ShotHeight * Scale);
            yield return ($"{Scenes.ColorsExpanded}-{variant}.png", 616, 1068);
        }
    }

    public static IReadOnlyList<string> RenderAll(string folder)
    {
        Directory.CreateDirectory(folder);
        var written = new List<string>();
        using var image = new Bitmap(FixturePath);
        foreach (var p in new[] { Tokens.Light, Tokens.Dark })
        {
            foreach (var s in Scenes.WindowScenes)
            {
                var canvas = CanvasBitmap(image, s.Canvas);
                var file = Path.Combine(folder, $"{s.Name}-{p.Name}.png");
                Save(Shell.Build(p, s, image, canvas), p, (int)Tokens.ShotWidth, (int)Tokens.ShotHeight, file);
                written.Add(file);
            }
            var panel = Path.Combine(folder, $"{Scenes.ColorsExpanded}-{p.Name}.png");
            Save(Shell.ColorsExpanded(p), p, 308, 534, panel);
            written.Add(panel);
        }
        return written;
    }

    /// Draws one scene in memory, for tests. `omitPanel` leaves out a panel by title.
    public static RenderTargetBitmap DrawScene(Palette p, Scene s, string? omitPanel = null)
    {
        using var image = new Bitmap(FixturePath);
        return Draw(Shell.Build(p, s, image, CanvasBitmap(image, s.Canvas), omitPanel), p, (int)Tokens.ShotWidth, (int)Tokens.ShotHeight);
    }

    public static RenderTargetBitmap DrawColorsExpanded(Palette p) => Draw(Shell.ColorsExpanded(p), p, 308, 534);

    /// Writes the Magic Wand check scene: the selected dot and the pressed wand button.
    public static void RenderMagicWandCheck(string file)
    {
        using var bitmap = DrawScene(Tokens.Light, Scenes.MagicWandCheck);
        bitmap.Save(file, PngBitmapEncoderOptions.Default);
    }

    /// Every icon with its file name on one sheet, for looking them over.
    public static void RenderIconSheet(string file)
    {
        var names = Directory.GetFiles(Icons.Folder, "*.svg").Select(Path.GetFileNameWithoutExtension).OfType<string>().Order().ToArray();
        const int columns = 11, cellW = 120, cellH = 60;
        var rows = (names.Length + columns - 1) / columns;
        var p = Tokens.Light;
        var canvas = new Canvas { Width = columns * cellW, Height = rows * cellH, Background = Ui.B(p.Panel) };
        for (var i = 0; i < names.Length; i++)
        {
            var x = i % columns * cellW;
            var y = i / columns * cellH;
            Ui.At(canvas, new IconView(names[i], p.Ink), x + 50, y + 8);
            var label = Ui.Label(names[i], p.Muted, 9);
            label.TextAlignment = Avalonia.Media.TextAlignment.Center;
            Ui.At(canvas, label, x, y + 34, cellW, 14);
        }
        Save(canvas, p, columns * cellW, rows * cellH, file);
    }

    static void Save(Control root, Palette p, int width, int height, string file)
    {
        using var bitmap = Draw(root, p, width, height);
        bitmap.Save(file, PngBitmapEncoderOptions.Default);
    }

    static RenderTargetBitmap Draw(Control root, Palette p, int width, int height)
    {
        var window = new Window
        {
            Width = width,
            Height = height,
            Content = root,
            Background = null,
            FontFamily = new Avalonia.Media.FontFamily(Tokens.FontFamily),
            RequestedThemeVariant = p == Tokens.Dark ? ThemeVariant.Dark : ThemeVariant.Light,
        };
        window.Show();
        Dispatcher.UIThread.RunJobs();
        root.Measure(new Size(width, height));
        root.Arrange(new Rect(0, 0, width, height));
        window.UpdateLayout();
        Dispatcher.UIThread.RunJobs();
        var bitmap = new RenderTargetBitmap(new PixelSize(width * Scale, height * Scale), new Vector(96 * Scale, 96 * Scale));
        bitmap.Render(root);
        window.Close();
        Dispatcher.UIThread.RunJobs();
        return bitmap;
    }

    /// The image the stage shows. The dialog scenes tint it to stand in for the engine's
    /// live preview: this tool has no engine, so the math is a plain approximation.
    static Bitmap CanvasBitmap(Bitmap source, CanvasFx fx)
    {
        if (fx is not (CanvasFx.CurvesPreview or CanvasFx.HueSaturationPreview)) return source;
        var size = source.PixelSize;
        var pixels = new byte[size.Width * size.Height * 4];
        unsafe
        {
            fixed (byte* ptr = pixels)
                source.CopyPixels(new PixelRect(size), (nint)ptr, pixels.Length, size.Width * 4);
        }
        for (var i = 0; i < pixels.Length; i += 4)
        {
            double a = pixels[i + 3] / 255.0;
            if (a <= 0) continue;
            double b = pixels[i] / 255.0 / a, g = pixels[i + 1] / 255.0 / a, r = pixels[i + 2] / 255.0 / a;
            r = Curve(r); g = Curve(g); b = Curve(b);
            if (fx == CanvasFx.HueSaturationPreview)
            {
                var (h, s, v) = Ui.ToHsv(Avalonia.Media.Color.FromRgb(Ui.To8(r), Ui.To8(g), Ui.To8(b)));
                var c = Ui.Hsv(h - 22, Math.Min(s * 1.3, 1), v);
                r = c.R / 255.0; g = c.G / 255.0; b = c.B / 255.0;
            }
            pixels[i] = Ui.To8(b * a);
            pixels[i + 1] = Ui.To8(g * a);
            pixels[i + 2] = Ui.To8(r * a);
        }
        var result = new WriteableBitmap(size, new Vector(96, 96), PixelFormat.Bgra8888, AlphaFormat.Premul);
        using (var buffer = result.Lock())
            System.Runtime.InteropServices.Marshal.Copy(pixels, 0, buffer.Address, pixels.Length);
        return result;
    }

    /// The S-shaped curve from the Curves shot, close enough for a stand-in preview.
    static double Curve(double t) => t + (t * t * (3 - 2 * t) - t) * .35;
}
