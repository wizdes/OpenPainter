using Avalonia;
using Avalonia.Controls;
using Avalonia.Headless;
using Avalonia.Headless.XUnit;
using Avalonia.Input;
using Avalonia.Layout;
using System.Runtime.InteropServices;
using Xunit;

[assembly: AvaloniaTestApplication(typeof(HeadlessAvalonia.TestAppBuilder))]

namespace HeadlessAvalonia;

public class TestAppBuilder
{
    public static AppBuilder BuildAvaloniaApp() => AppBuilder.Configure<Application>()
        .UseSkia()
        .UseHeadless(new AvaloniaHeadlessPlatformOptions { UseHeadlessDrawing = false });
}

public class Probe
{
    [AvaloniaFact]
    public void ClickTypeAndRender()
    {
        var box = new TextBox { Width = 160, Height = 36 };
        var button = new Button { Content = "Apply", Width = 100, Height = 36 };
        var result = new TextBlock { Text = "Waiting", Height = 36 };
        button.Click += (_, _) => result.Text = box.Text;
        var window = new Window
        {
            Width = 240,
            Height = 180,
            Content = new StackPanel { Children = { box, button, result } }
        };
        window.Show();
        var before = window.CaptureRenderedFrame();
        Assert.NotNull(before);
        box.Focus();
        window.KeyTextInput("hello");
        Assert.Equal("hello", box.Text);
        var click = button.TranslatePoint(new Point(5, 5), window)!.Value;
        window.MouseDown(click, MouseButton.Left);
        window.MouseUp(click, MouseButton.Left);
        Assert.Equal("hello", result.Text);
        var after = window.CaptureRenderedFrame();
        Assert.NotNull(after);
        static byte[] Pixels(Avalonia.Media.Imaging.WriteableBitmap image)
        {
            using var frame = image.Lock();
            var pixels = new byte[frame.RowBytes * frame.Size.Height];
            Marshal.Copy(frame.Address, pixels, 0, pixels.Length);
            return pixels;
        }
        Assert.False(Pixels(before!).SequenceEqual(Pixels(after!)));
        window.Close();
    }
}
