// The shared look values for OpenPainter on Windows. W7 reuses this file.
// Colors are the Mac app's Theme.swift tokens, read from
// mac/OpenPainterKit/Sources/Shell/Theme.swift. Colors marked "from the shot" come from
// real Mac app shots, because macOS draws those surfaces itself.
// Light and dark differ only by the two palettes here. Scene code never asks which is on.
using Avalonia.Media;

namespace UiLook;

public sealed record Palette(
    string Name,
    Color Chrome,        // Strip and status bar.
    Color Options,       // Tool options row.
    Color Panel,         // Panel bodies and dialogs.
    Color Field,         // Text fields. Dark value from the shot: Theme.swift says 262B32, the shot draws 171717.
    Color Ink,           // Main text and icons.
    Color Muted,         // Labels and secondary text.
    Color Disabled,      // Commands that are off. From the shot.
    Color Line,          // Thin lines between rows and bars.
    Color PanelLine,     // Edges of swatches, wheel and slider tracks.
    Color Stage,         // Behind the image. From the shot.
    Color TitleBar,      // Window title row and panel title rows. From the shot.
    Color Accent,        // Selected things.
    Color Wash,          // Fill of selected rows and buttons.
    Color Control,       // Fill of pop-up buttons and plain buttons. From the shot.
    Color ControlEdge,   // Edge of text fields. From the shot.
    Color Plot,          // Fill of the Curves graph. Theme.swift's field value.
    Color Knob,          // Slider knob fill: white, as on the Mac.
    Color KnobRing,      // Slider knob edge. The Mac uses a shadow, which Fluent's knob does not have.
    Color SliderValue,   // Slider track left of the knob. Separate from panel lines.
    Color DisabledText,  // Off text in lists, such as History's Redo. Disabled is for faint toolbar icons.
    Color FieldBottom,   // The stronger bottom line on text fields, as Windows 11 draws it.
    Color PanelEdge,     // Panel and dialog outlines, so dark panels do not merge into the stage.
    Color OnAccent,      // Text on an accent fill, such as OK.
    Color Handle);       // Text box resize handle fill. From the shot.

public static class Tokens
{
    static Color C(uint rgb) => Color.FromRgb((byte)(rgb >> 16), (byte)(rgb >> 8), (byte)rgb);

    public static readonly Palette Light = new(
        "light",
        Chrome: C(0xF5F5F6), Options: C(0xE9EBED), Panel: C(0xF9F9FA), Field: C(0xFFFFFF),
        Ink: C(0x202329), Muted: C(0x5C626C), Disabled: C(0xA3A8AF), Line: C(0xCDD1D6), PanelLine: C(0x9DA5AD),
        Stage: C(0xFFFFFF), TitleBar: C(0xFFFFFF), Accent: C(0x1973C8), Wash: C(0xDCECFF),
        Control: C(0xDDDFE1), ControlEdge: C(0xB9BEC4), Plot: C(0xFFFFFF), Knob: C(0xFFFFFF), KnobRing: C(0x7C848E), SliderValue: C(0x848C95),
        DisabledText: C(0x6B717A), FieldBottom: C(0x7C848E), PanelEdge: C(0xCDD1D6), OnAccent: C(0xFFFFFF), Handle: C(0x0A84FF));

    public static readonly Palette Dark = new(
        "dark",
        Chrome: C(0x353A42), Options: C(0x2E333B), Panel: C(0x363B43), Field: C(0x171717),
        Ink: C(0xEEF0F3), Muted: C(0xBFC5CE), Disabled: C(0x7B818B), Line: C(0x515862), PanelLine: C(0x8E98A3),
        Stage: C(0x1E1E1E), TitleBar: C(0x1E1E1E), Accent: C(0x76B8FF), Wash: C(0x244967),
        Control: C(0x41464D), ControlEdge: C(0x737A85), Plot: C(0x262B32), Knob: C(0xFFFFFF), KnobRing: C(0x9AA3AE), SliderValue: C(0xB4BBC4),
        DisabledText: C(0xB0B6BF), FieldBottom: C(0xA3ABB5), PanelEdge: C(0x5F6671), OnAccent: C(0x101D2B), Handle: C(0x0A84FF));

    /// Fixed colors that do not follow the theme.
    public static readonly Color Scrim = Color.FromArgb(0x33, 0, 0, 0);   // Window dim under a dialog, 20% black, from the shot.
    public static readonly Color Shadow = Color.FromArgb(0x4D, 0, 0, 0);
    public static readonly Color CheckerLight = C(0xFFFFFF);
    public static readonly Color CheckerDark = C(0xCCCCCC);
    public static readonly Color TextOnCanvas = C(0xFFFFFF);

    // Fonts. Segoe UI takes the place of the Mac system font.
    public const string FontFamily = "Segoe UI";
    public const double FontBody = 12;
    public const double FontSmall = 12;
    public const double SelectionDash = 4;            // Selection dash and gap, measured from the Mac shot.
    public static readonly FontWeight StrongWeight = FontWeight.Bold;   // Selected rows and strip names.
    public const double FontCanvasText = 36.2;     // "Spring in Holland" at 18% zoom.

    // Sizes in layout pixels. Names and numbers follow Theme.swift and PanelLayout.swift.
    public const double ShotWidth = 1376, ShotHeight = 928, ShotMargin = 48;
    public const double WindowWidth = 1280, WindowHeight = 832;
    public const double TitleHeight = 32, OptionsHeight = 36, StripHeight = 64, StatusHeight = 28;
    public const double CommandTarget = 28, PanelTarget = 24, IconSize = 20, SmallIconSize = 16;
    public const double PanelTitleHeight = 24, PanelClearance = 12;
    public const double ToolsWidth = 84, ToolsHeight = 292, ColorsWidth = 244, ColorsHeight = 246, ColorsExpandedHeight = 470;
    public const double ListPanelWidth = 226, ListPanelHeight = 245;
    public const double HistoryRowHeight = 27, LayerRowHeight = 49, ActionRowHeight = 28;
    public const double FieldHeight = 24, OptionHeight = 26, ChoiceHeight = 25, ButtonHeight = 24, DialogButtonHeight = 26;
    public const double ImageStripItemWidth = 104;
    public const double CaptionButtonWidth = 46;
    public const double DialogTitleHeight = 32;
    public const double ChoiceArrow = 8, KnobSize = 14, OverflowWidth = 30;

    // Corners. Windows 11 uses 8 for windows and 4 for controls. The Mac shots use about 10 and 5.
    public const double WindowCorner = 8, PanelCorner = 8, DialogCorner = 8, ControlCorner = 4;

    /// Pairs of colors that must stay readable, with the lowest allowed contrast. A test checks them.
    public static IEnumerable<(string Name, Color Front, Color Back, double Minimum)> ContrastPairs(Palette p)
    {
        yield return ("slider knob ring on panel", p.KnobRing, p.Panel, 3);
        yield return ("slider value fill on panel", p.SliderValue, p.Panel, 3);
        yield return ("disabled list text on panel", p.DisabledText, p.Panel, 4.5);
        yield return ("OK text on accent", p.OnAccent, p.Accent, 4.5);
        yield return ("dialog title on title row", p.Ink, p.TitleBar, 4.5);
        yield return ("button text on button", p.Ink, p.Control, 4.5);
        yield return ("button edge on panel", p.ControlEdge, p.Panel, 1.6);
        yield return ("field bottom line on field", p.FieldBottom, p.Field, 3);
        yield return ("panel outline on stage", p.PanelEdge, p.Stage, 1.45);
    }

    /// WCAG contrast ratio between two opaque colors.
    public static double Contrast(Color a, Color b)
    {
        static double L(Color c)
        {
            static double F(byte v) { var x = v / 255.0; return x <= .03928 ? x / 12.92 : Math.Pow((x + .055) / 1.055, 2.4); }
            return .2126 * F(c.R) + .7152 * F(c.G) + .0722 * F(c.B);
        }
        double la = L(a), lb = L(b);
        return (Math.Max(la, lb) + .05) / (Math.Min(la, lb) + .05);
    }

    // Spacing.
    public const double OptionGroupGap = 24, OptionItemGap = 8, CommandGap = 6, CommandGroupGap = 12;
}
