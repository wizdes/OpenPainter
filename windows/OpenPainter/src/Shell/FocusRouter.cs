// Who gets a key: mac/OpenPainterKit/Sources/Shell/FocusRouter.swift. The commands go to the
// document unless a text field or a dialog has focus. With a text field focused, the field keeps
// its own editing keys and the other menu keys still work, as a Mac menu's do. With a dialog up,
// the document gets nothing.
using Avalonia.Controls;
using OpenPainter.Catalog;
using UiLook;

namespace OpenPainter.Shell;

public enum FocusTarget
{
    /// The active document gets the command.
    Canvas,
    /// A text field is editing, and it gets the command.
    TextField,
    /// A dialog is up, and the document gets nothing.
    Dialog,
}

/// Where a key press came from.
public enum KeySource
{
    /// The canvas: the native child, or its focus stop in a headless window.
    Canvas,
    /// A control in the main window other than the canvas.
    Window,
    /// A floating panel.
    Panel,
}

public static class FocusRouter
{
    /// The keys a focused text field keeps. The Mac's list is Select All, Delete, Escape, Enter,
    /// Undo and Redo. The rest are the field's own on the Mac too, through the responder chain.
    public static readonly IReadOnlySet<string> FieldKeys = new HashSet<string>(StringComparer.Ordinal)
    {
        "Ctrl+A", "Delete", "Shift+Delete", "Backspace", "Esc", "Enter", "Ctrl+Z", "Ctrl+Shift+Z", "Ctrl+Y", "Ctrl+X", "Ctrl+C", "Ctrl+V",
    };

    /// FocusRouter.target(in:). `focused` is what has keyboard focus in the window the key came from.
    public static FocusTarget Target(bool dialogUp, object? focused)
    {
        if (dialogUp) return FocusTarget.Dialog;
        return IsText(focused) ? FocusTarget.TextField : FocusTarget.Canvas;
    }

    /// FocusRouter.isText: an editable field.
    public static bool IsText(object? focused) => focused is TextBox { IsReadOnly: false };

    /// True when the document's side may act on this key. Letter keys count as the field's own too.
    public static bool DocumentGets(FocusTarget target, string words) => target switch
    {
        FocusTarget.Dialog => false,
        FocusTarget.TextField => !FieldKeys.Contains(words) && !IsPlainTyping(words),
        _ => true,
    };

    /// A key with no Ctrl or Alt types into a field: letters, digits and punctuation, with or without Shift.
    public static bool IsPlainTyping(string words)
    {
        var key = words.StartsWith("Shift+", StringComparison.Ordinal) ? words["Shift+".Length..] : words;
        return key.Length == 1;
    }

    /// True when the canvas's own keys may run: tool letters, X, C, D, [ and ], Enter. A control in
    /// the main window keeps them, so a list can use its own letters. On the Mac a click in a panel
    /// leaves the keyboard with the canvas, so a panel with no text field passes them on.
    public static bool CanvasKeysRun(FocusTarget target, KeySource source) => target == FocusTarget.Canvas && source != KeySource.Window;
}

/// The keys that work on the canvas and are not menu items: KeyboardShortcuts.canvas in
/// KeyboardShortcuts.swift, with EditorWindowController.canvasShortcut and colorKey.
public static class CanvasKeys
{
    /// Runs one key. True means it was used.
    public static bool Run(ToolState tools, Editor? editor, string words, bool repeat)
    {
        switch (words)
        {
            case "[": Width(tools, -1); return true;
            case "]": Width(tools, 1); return true;
            case "Ctrl+[": Width(tools, -5); return true;
            case "Ctrl+]": Width(tools, 5); return true;
            case "Enter":
                // EditorWindowController.canvasKey. W9 adds finishing a crop, a move or a fill.
                editor?.ClearMessage();
                return true;
            // Only one image can be open until W10. The key must still stay on the canvas.
            case "Ctrl+Tab" or "Ctrl+Shift+Tab": return true;
        }
        if (repeat) return false;
        var reverse = words.StartsWith("Shift+", StringComparison.Ordinal);
        var key = reverse ? words["Shift+".Length..] : words;
        if (key.Length != 1 || key[0] is < 'A' or > 'Z') return false;
        if (!reverse)
        {
            switch (key[0])
            {
                case 'X': tools.SwapColors(); return true;
                case 'C': tools.ToggleActiveSlot(); return true;
                case 'D': tools.ResetColors(); return true;
            }
        }
        if (tools.ToolForKey(key[0], reverse) is not { } tool) return false;
        tools.Current = tool;
        return true;
    }

    static void Width(ToolState tools, double by)
    {
        var width = Math.Round(tools.Settings.BrushWidth + by, MidpointRounding.AwayFromZero);
        tools.Settings = tools.Settings with { BrushWidth = Math.Clamp(width, ToolSettings.LeastWidth, ToolSettings.MostWidth) };
    }
}

/// What Alt does, in one small machine, so each window and the native canvas share one answer.
/// A clean tap opens the menu bar on release. Anything between the press and the release ends the
/// tap, because here Alt is also a modifier: Alt-click on the Colors wheel keeps the spoke, and
/// Alt-click with the Zoom tool zooms out.
public sealed class AltGesture
{
    bool _held, _armed;

    /// True from a spoiled tap until the next Alt press.
    public bool Spoiled { get; private set; }

    /// Alt went down. A held key repeats, and a repeat does not start a new tap.
    public void Press(bool otherModifier = false)
    {
        if (_held) return;
        _held = true;
        _armed = !otherModifier;
        Spoiled = otherModifier;
    }

    /// Another key or a pointer press came while Alt was down.
    public void Spoil()
    {
        if (!_held) return;
        _armed = false;
        Spoiled = true;
    }

    /// Alt came up. True means this was a clean tap.
    public bool Release()
    {
        var tap = _held && _armed;
        _held = false;
        _armed = false;
        return tap;
    }

    /// The window lost the keyboard, so the release may never come here.
    public void Forget()
    {
        _held = false;
        _armed = false;
    }
}

/// The access letters of the top menus, a choice recorded in docs/windows/shortcuts-windows.md.
/// The Mac's menus have none.
public static class MenuLetters
{
    public static readonly IReadOnlyDictionary<string, char> Letters = new Dictionary<string, char>
    {
        ["File"] = 'F', ["Edit"] = 'E', ["View"] = 'V', ["Image"] = 'I', ["Layers"] = 'L',
        ["Adjustments"] = 'A', ["Effects"] = 'C', ["Window"] = 'W', ["Help"] = 'H',
    };

    /// The menu title with `_` before its letter, for AccessText, which underlines it.
    public static string Marked(string title)
    {
        if (!Letters.TryGetValue(title, out var letter)) return title;
        var at = title.IndexOf(letter.ToString(), StringComparison.OrdinalIgnoreCase);
        return at < 0 ? title : title.Insert(at, "_");
    }

    /// The title for a letter pressed with Alt, or null.
    public static string? Title(char letter)
    {
        foreach (var (title, key) in Letters)
            if (key == char.ToUpperInvariant(letter)) return title;
        return null;
    }
}
