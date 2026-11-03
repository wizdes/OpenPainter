// Windows high contrast. The system names the colors, and the app must use them and nothing else
// for its surfaces, text and marks. Document pixels, swatches and the wheel keep their real colors,
// because they are content. A test hook can read a .theme file, so a run can check a real window
// with these colors and leave the user's system setting alone.
using System.Globalization;
using System.Runtime.InteropServices;
using Avalonia.Media;
using UiLook;

namespace OpenPainter.Shell;

/// The system colors the app reads. The names follow GetSysColor.
public sealed record SystemColors(Color Window, Color WindowText, Color Hotlight, Color GrayText, Color Highlight, Color HighlightText, Color ButtonFace, Color ButtonText);

public static partial class HighContrast
{
    const uint GetHighContrast = 0x0042;
    const uint HighContrastOn = 1;
    const int ColorWindow = 5, ColorWindowText = 8, ColorHighlight = 13, ColorHighlightText = 14, ColorButtonFace = 15, ColorGrayText = 17, ColorButtonText = 18, ColorHotlight = 26;

    [StructLayout(LayoutKind.Sequential)]
    struct Settings
    {
        public uint Size, Flags;
        public nint Scheme;
    }

    [LibraryImport("user32.dll", EntryPoint = "SystemParametersInfoW", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static partial bool SystemParameters(uint action, uint size, ref Settings value, uint ini);

    [LibraryImport("user32.dll")] private static partial uint GetSysColor(int index);

    /// The palette for these colors. Every surface is the system's window color and every line and
    /// word its text color. The accent is its highlight color. A selected row has no wash, because
    /// the system's text color only promises to read on the window color, and its edge and diamond
    /// mark it.
    public static Palette Palette(SystemColors s) => new(
        ControlStates.ContrastName,
        Chrome: s.Window, Options: s.Window, Panel: s.Window, Field: s.Window,
        Ink: s.WindowText, Muted: s.WindowText, Disabled: s.GrayText, Line: s.WindowText, PanelLine: s.WindowText,
        Stage: s.Window, TitleBar: s.Window, Accent: s.Highlight, Wash: s.Window,
        Control: s.Window, ControlEdge: s.WindowText, Plot: s.Window, Knob: s.Window, KnobRing: s.WindowText, SliderValue: s.WindowText,
        DisabledText: s.GrayText, FieldBottom: s.WindowText, PanelEdge: s.WindowText, OnAccent: s.HighlightText, Handle: s.Highlight);

    /// True while Windows is in a high-contrast session. It only reads the setting.
    public static bool IsOn()
    {
        var settings = new Settings { Size = (uint)Marshal.SizeOf<Settings>() };
        if (!SystemParameters(GetHighContrast, settings.Size, ref settings, 0))
            throw new InvalidOperationException($"HIGH_CONTRAST: SystemParametersInfo: {Marshal.GetLastPInvokeError()}");
        return (settings.Flags & HighContrastOn) != 0;
    }

    public static SystemColors FromSystem()
    {
        static Color Read(int index)
        {
            var bgr = GetSysColor(index);
            return Color.FromRgb((byte)bgr, (byte)(bgr >> 8), (byte)(bgr >> 16));
        }
        return new SystemColors(Read(ColorWindow), Read(ColorWindowText), Read(ColorHotlight), Read(ColorGrayText), Read(ColorHighlight), Read(ColorHighlightText), Read(ColorButtonFace), Read(ColorButtonText));
    }

    /// The colors in a Windows .theme file's `[Control Panel\Colors]` section. A missing color
    /// stops the run and names it.
    public static SystemColors FromThemeText(string text, string name)
    {
        var found = new Dictionary<string, Color>(StringComparer.OrdinalIgnoreCase);
        var inColors = false;
        foreach (var raw in text.Split('\n'))
        {
            var line = raw.Trim();
            if (line.StartsWith('['))
            {
                inColors = line.Equals(@"[Control Panel\Colors]", StringComparison.OrdinalIgnoreCase);
                continue;
            }
            var equals = line.IndexOf('=');
            if (!inColors || equals <= 0) continue;
            var parts = line[(equals + 1)..].Split(' ', StringSplitOptions.RemoveEmptyEntries);
            if (parts.Length != 3) continue;
            if (!byte.TryParse(parts[0], NumberStyles.None, CultureInfo.InvariantCulture, out var r)
                || !byte.TryParse(parts[1], NumberStyles.None, CultureInfo.InvariantCulture, out var g)
                || !byte.TryParse(parts[2], NumberStyles.None, CultureInfo.InvariantCulture, out var b)) continue;
            found[line[..equals]] = Color.FromRgb(r, g, b);
        }
        Color Need(string key) => found.TryGetValue(key, out var c) ? c : throw new InvalidDataException($"HIGH_CONTRAST: {name} has no {key} color");
        return new SystemColors(Need("Window"), Need("WindowText"), Need("HotTrackingColor"), Need("GrayText"), Need("Hilight"), Need("HilightText"), Need("ButtonFace"), Need("ButtonText"));
    }

    /// Whether these colors want Avalonia's dark variant for what they do not name.
    public static bool IsDark(SystemColors s) => Tokens.Contrast(s.Window, Colors.Black) < Tokens.Contrast(s.Window, Colors.White);
}
