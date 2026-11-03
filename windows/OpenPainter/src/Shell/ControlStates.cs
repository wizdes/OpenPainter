// Hover, press and focus for every control type, in light, dark and high contrast. Fluent's
// templates read their state colors from named resources, so each window sets those names from its
// palette. Our own drawn controls take the same colors from here. Text stays Ink in every state, so
// the contrast test covers each fill.
using Avalonia;
using Avalonia.Controls;
using Avalonia.Controls.Templates;
using Avalonia.Media;
using Avalonia.Styling;
using UiLook;

namespace OpenPainter.Shell;

public static class ControlStates
{
    const double HoverShare = .10, PressShare = .18, ContrastHoverShare = .25, ContrastPressShare = .32;

    /// The palette's name in a Windows high-contrast session.
    public const string ContrastName = "high-contrast";

    public static Color Blend(Color under, Color over, double share)
    {
        byte Mix(byte a, byte b) => (byte)Math.Round(a + (b - a) * share, MidpointRounding.AwayFromZero);
        return Color.FromRgb(Mix(under.R, over.R), Mix(under.G, over.G), Mix(under.B, over.B));
    }

    static double Share(Palette p, double usual, double contrast) => p.Name == ContrastName ? contrast : usual;

    /// A control's fill under the pointer, over the surface it sits on.
    public static Color Hover(Palette p, Color surface) => Blend(surface, p.Ink, Share(p, HoverShare, ContrastHoverShare));

    /// A control's fill while pressed.
    public static Color Pressed(Palette p, Color surface) => Blend(surface, p.Ink, Share(p, PressShare, ContrastPressShare));

    /// What each state puts text on, for the contrast test.
    public static IEnumerable<(string Name, Color Front, Color Back)> Pairs(Palette p)
    {
        foreach (var (name, surface) in new[] { ("panel", p.Panel), ("options row", p.Options), ("strip", p.Chrome), ("title row", p.TitleBar), ("button", p.Control), ("field", p.Field) })
        {
            yield return ($"text on hover over {name}", p.Ink, Hover(p, surface));
            yield return ($"text on press over {name}", p.Ink, Pressed(p, surface));
        }
        yield return ("text on a selected row", p.Ink, p.Wash);
        yield return ("muted text on a panel", p.Muted, p.Panel);
        yield return ("muted text on the title row", p.Muted, p.TitleBar);
        yield return ("menu text on a menu", p.Ink, p.Panel);
    }

    /// Sets Fluent's state resources on a window. Popups read them through their owner.
    public static void Apply(IResourceDictionary r, Palette p)
    {
        void Set(string key, Color color) => r[key] = new SolidColorBrush(color);

        // Buttons, and their cousins. A button's own surface differs by kind, so the state fill
        // takes the plain button's fill as the base. An icon button then shows the same wash.
        var hover = Hover(p, p.Control);
        var press = Pressed(p, p.Control);
        foreach (var kind in new[] { "Button", "RepeatButton", "ToggleButton" })
        {
            Set($"{kind}BackgroundPointerOver", hover);
            Set($"{kind}BackgroundPressed", press);
            Set($"{kind}BorderBrushPointerOver", p.FieldBottom);
            Set($"{kind}BorderBrushPressed", p.Ink);
            Set($"{kind}ForegroundPointerOver", p.Ink);
            Set($"{kind}ForegroundPressed", p.Ink);
        }
        Set("ToggleButtonBackgroundChecked", p.Wash);
        Set("ToggleButtonBackgroundCheckedPointerOver", Hover(p, p.Wash));
        Set("ToggleButtonBackgroundCheckedPressed", Pressed(p, p.Wash));
        Set("ToggleButtonBorderBrushChecked", p.Accent);
        Set("ToggleButtonBorderBrushCheckedPointerOver", p.Accent);
        Set("ToggleButtonBorderBrushCheckedPressed", p.Ink);
        Set("ToggleButtonForegroundChecked", p.Ink);
        Set("ToggleButtonForegroundCheckedPointerOver", p.Ink);
        Set("ToggleButtonForegroundCheckedPressed", p.Ink);

        // Text fields. Focus shows as an accent edge.
        Set("TextControlForeground", p.Ink);
        Set("TextControlForegroundPointerOver", p.Ink);
        Set("TextControlForegroundFocused", p.Ink);
        Set("TextControlBackground", p.Field);
        Set("TextControlBackgroundPointerOver", Hover(p, p.Field));
        Set("TextControlBackgroundFocused", p.Field);
        Set("TextControlBorderBrushPointerOver", p.FieldBottom);
        Set("TextControlBorderBrushFocused", p.Accent);
        Set("TextControlSelectionHighlightColor", p.Accent);
        Set("TextOnAccentFillColorSelectedTextBrush", p.OnAccent);

        // Combo boxes and their lists.
        Set("ComboBoxForeground", p.Ink);
        Set("ComboBoxForegroundFocused", p.Ink);
        Set("ComboBoxForegroundFocusedPressed", p.Ink);
        Set("ComboBoxBackgroundPointerOver", hover);
        Set("ComboBoxBackgroundPressed", press);
        Set("ComboBoxBorderBrushPointerOver", p.FieldBottom);
        Set("ComboBoxBorderBrushPressed", p.Ink);
        Set("ComboBoxBackgroundBorderBrushFocused", p.Accent);
        Set("ComboBoxDropDownBackground", p.Panel);
        Set("ComboBoxDropDownGlyphForeground", p.Ink);
        Set("ComboBoxDropDownGlyphForegroundFocused", p.Ink);
        Set("ComboBoxDropDownGlyphForegroundFocusedPressed", p.Ink);
        Set("ComboBoxItemForeground", p.Ink);
        Set("ComboBoxItemBackgroundPointerOver", Hover(p, p.Panel));
        Set("ComboBoxItemBackgroundPressed", Pressed(p, p.Panel));
        Set("ComboBoxItemBackgroundSelected", p.Wash);
        Set("ComboBoxItemBackgroundSelectedPointerOver", Hover(p, p.Wash));
        Set("ComboBoxItemBackgroundSelectedPressed", Pressed(p, p.Wash));
        foreach (var key in new[] { "ComboBoxItemForegroundPointerOver", "ComboBoxItemForegroundPressed", "ComboBoxItemForegroundSelected", "ComboBoxItemForegroundSelectedPointerOver", "ComboBoxItemForegroundSelectedPressed" })
            Set(key, p.Ink);

        // Check boxes.
        foreach (var state in new[] { "Unchecked", "Checked", "Indeterminate" })
        {
            var on = state != "Unchecked";
            foreach (var (name, base_) in new[] { ("PointerOver", Hover(p, on ? p.Accent : p.Panel)), ("Pressed", Pressed(p, on ? p.Accent : p.Panel)) })
            {
                Set($"CheckBoxForeground{state}{name}", p.Ink);
                Set($"CheckBoxCheckBackgroundFill{state}{name}", base_);
                Set($"CheckBoxCheckBackgroundStroke{state}{name}", on ? p.Accent : p.Ink);
                Set($"CheckBoxCheckGlyphForeground{state}{name}", p.OnAccent);
            }
        }

        // Menus, top row and lists.
        Set("MenuFlyoutPresenterBackground", p.Panel);
        Set("MenuFlyoutItemForeground", p.Ink);
        Set("MenuFlyoutItemForegroundPointerOver", p.Ink);
        Set("MenuFlyoutItemForegroundPressed", p.Ink);
        Set("MenuFlyoutItemForegroundDisabled", p.DisabledText);
        Set("MenuFlyoutItemBackgroundPointerOver", Hover(p, p.Panel));
        Set("MenuFlyoutItemBackgroundPressed", Pressed(p, p.Panel));
        Set("MenuFlyoutItemKeyboardAcceleratorTextForeground", p.Muted);
        Set("MenuFlyoutItemKeyboardAcceleratorTextForegroundPointerOver", p.Ink);
        Set("MenuFlyoutItemKeyboardAcceleratorTextForegroundPressed", p.Ink);
        Set("MenuFlyoutItemKeyboardAcceleratorTextForegroundDisabled", p.DisabledText);
        Set("MenuFlyoutSubItemChevronPointerOver", p.Ink);
        Set("MenuFlyoutSubItemChevronPressed", p.Ink);

        // Fluent's sliders in the dialogs and the options row.
        Set("SliderThumbBackgroundPointerOver", p.Accent);
        Set("SliderThumbBackgroundPressed", p.Ink);
        Set("SliderTrackValueFillPointerOver", p.Accent);
        Set("SliderTrackValueFillPressed", p.Ink);
        Set("SliderTrackFillPointerOver", p.PanelLine);
        Set("SliderTrackFillPressed", p.PanelLine);

        // Scroll bars, tool tips.
        Set("ScrollBarThumbFillPointerOver", p.Muted);
        Set("ScrollBarThumbFillPressed", p.Ink);
        Set("ToolTipBackground", p.Panel);
        Set("ToolTipForeground", p.Ink);
    }

    /// The keyboard focus mark: an outer black line and an inner white line, the way Windows 11 draws
    /// one, so it shows on any surface in any theme, high contrast included. The same template
    /// serves all controls, and Avalonia shows it only for keyboard focus.
    static readonly FuncTemplate<Control> Ring = new(() => new Border
    {
        BorderBrush = Brushes.Black,
        BorderThickness = new Thickness(1),
        CornerRadius = new CornerRadius(Tokens.ControlCorner),
        IsHitTestVisible = false,
        Child = new Border
        {
            BorderBrush = Brushes.White,
            BorderThickness = new Thickness(1),
            CornerRadius = new CornerRadius(Tokens.ControlCorner - 1),
        },
    });

    /// The app's styles for focus: the ring on all controls. A text field shows focus by its edge,
    /// and a menu row by its fill, so neither takes the ring.
    public static Styles FocusStyles() =>
    [
        new Style(x => x.Is<Control>()) { Setters = { new Setter(Control.FocusAdornerProperty, Ring) } },
        new Style(x => x.Is<TextBox>()) { Setters = { new Setter(Control.FocusAdornerProperty, null) } },
        new Style(x => x.Is<MenuItem>()) { Setters = { new Setter(Control.FocusAdornerProperty, null) } },
    ];
}
