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
    Color Knob,          // Slider knob. White on the Mac with a shadow; see docs/windows/w6-look.md.
    Color Handle);       // Text box resize handle fill. From the shot.

public static class Tokens
{
    static Color C(uint rgb) => Color.FromRgb((byte)(rgb >> 16), (byte)(rgb >> 8), (byte)rgb);

    public static readonly Palette Light = new(
        "light",
        Chrome: C(0xF5F5F6), Options: C(0xE9EBED), Panel: C(0xF9F9FA), Field: C(0xFFFFFF),
        Ink: C(0x202329), Muted: C(0x5C626C), Disabled: C(0xA3A8AF), Line: C(0xCDD1D6), PanelLine: C(0x9DA5AD),
        Stage: C(0xFFFFFF), TitleBar: C(0xFFFFFF), Accent: C(0x1973C8), Wash: C(0xDCECFF),
        Control: C(0xDDDFE1), ControlEdge: C(0xCDD1D6), Plot: C(0xFFFFFF), Knob: C(0x9DA5AD), Handle: C(0x0A84FF));

    public static readonly Palette Dark = new(
        "dark",
        Chrome: C(0x353A42), Options: C(0x2E333B), Panel: C(0x363B43), Field: C(0x171717),
        Ink: C(0xEEF0F3), Muted: C(0xBFC5CE), Disabled: C(0x7B818B), Line: C(0x515862), PanelLine: C(0x8E98A3),
        Stage: C(0x1E1E1E), TitleBar: C(0x1E1E1E), Accent: C(0x76B8FF), Wash: C(0x244967),
        Control: C(0x41464D), ControlEdge: C(0x515862), Plot: C(0x262B32), Knob: C(0xFFFFFF), Handle: C(0x0A84FF));

    /// Fixed colors that do not follow the theme.
    public static readonly Color Scrim = Color.FromArgb(0x33, 0, 0, 0);   // Window dim under a dialog, 20% black, from the shot.
    public static readonly Color Shadow = Color.FromArgb(0x4D, 0, 0, 0);
    public static readonly Color CheckerLight = C(0xFFFFFF);
    public static readonly Color CheckerDark = C(0xCCCCCC);
    public static readonly Color TextOnCanvas = C(0xFFFFFF);

    // Fonts. Segoe UI takes the place of the Mac system font.
    public const string FontFamily = "Segoe UI";
    public const double FontBody = 12;
    public const double FontSmall = 11;
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

    // Corners. Windows 11 uses 8 for windows and 4 for controls. The Mac shots use about 10 and 5.
    public const double WindowCorner = 8, PanelCorner = 8, DialogCorner = 8, ControlCorner = 4;

    // Spacing.
    public const double OptionGroupGap = 24, OptionItemGap = 8, CommandGap = 6, CommandGroupGap = 12;
}
