// The minimize, maximize and close buttons of the custom title row.
//
// Each has a role that Avalonia's Win32 window turns into the real system answer
// to WM_NCHITTEST: HTMINBUTTON, HTMAXBUTTON or HTCLOSE. HTMAXBUTTON is what makes
// Windows 11 show Snap Layouts. Avalonia passes the pointer to these controls as
// normal input, so hover, press and click run here.
using Avalonia;
using Avalonia.Automation;
using Avalonia.Controls;
using Avalonia.Controls.Chrome;
using Avalonia.Input;
using Avalonia.Media;
using UiLook;

namespace OpenPainter.Shell;

public enum CaptionKind { Minimize, Maximize, Close }

/// Colors Windows 11 draws on caption buttons. These are not Mac colors, so they are not tokens.
public static class CaptionLook
{
    public static readonly Color CloseHover = Color.FromRgb(0xC4, 0x2B, 0x1C);
    public static readonly Color ClosePressed = Color.FromRgb(0xC8, 0x3C, 0x31);
    public static readonly Color OnClose = Colors.White;

    /// The fill behind the glyph. Transparent when the button is at rest.
    public static Color Fill(CaptionKind kind, bool hover, bool pressed, Palette p)
    {
        if (!hover && !pressed) return Colors.Transparent;
        if (kind == CaptionKind.Close) return pressed ? ClosePressed : CloseHover;
        return Color.FromArgb(pressed ? (byte)0x0F : (byte)0x17, p.Ink.R, p.Ink.G, p.Ink.B);
    }

    /// The glyph's color. An inactive window draws it fainter, and a red button draws it white.
    public static Color Glyph(CaptionKind kind, bool hover, bool pressed, bool active, Palette p)
    {
        if (kind == CaptionKind.Close && (hover || pressed)) return OnClose;
        return active ? p.Ink : p.Disabled;
    }

    public static WindowDecorationsElementRole Role(CaptionKind kind) => kind switch
    {
        CaptionKind.Minimize => WindowDecorationsElementRole.MinimizeButton,
        CaptionKind.Maximize => WindowDecorationsElementRole.MaximizeButton,
        _ => WindowDecorationsElementRole.CloseButton,
    };
}

public sealed class CaptionButton : Button
{
    const double GlyphSize = 10;
    readonly Palette _palette;
    bool _active = true;
    bool _maximized;

    static CaptionButton() => AffectsRender<CaptionButton>(IsPointerOverProperty, IsPressedProperty);

    protected override Type StyleKeyOverride => typeof(CaptionButton);

    public CaptionKind Kind { get; }

    public CaptionButton(CaptionKind kind, Palette palette)
    {
        Kind = kind;
        _palette = palette;
        Width = Tokens.CaptionButtonWidth;
        Height = Tokens.TitleHeight;
        Focusable = false;
        Background = Brushes.Transparent;
        WindowDecorationProperties.SetElementRole(this, CaptionLook.Role(kind));
        AutomationProperties.SetName(this, kind switch
        {
            CaptionKind.Minimize => "Minimize",
            CaptionKind.Maximize => "Maximize",
            _ => "Close",
        });
    }

    public bool Active
    {
        get => _active;
        set { _active = value; InvalidateVisual(); }
    }

    /// True shows the restore glyph, and the button's name says Restore.
    public bool Maximized
    {
        get => _maximized;
        set
        {
            _maximized = value;
            if (Kind == CaptionKind.Maximize) AutomationProperties.SetName(this, value ? "Restore" : "Maximize");
            InvalidateVisual();
        }
    }

    public Color CurrentFill => CaptionLook.Fill(Kind, IsPointerOver, IsPressed, _palette);
    public Color CurrentGlyph => CaptionLook.Glyph(Kind, IsPointerOver, IsPressed, _active, _palette);

    public override void Render(DrawingContext context)
    {
        context.FillRectangle(new SolidColorBrush(CurrentFill), new Rect(Bounds.Size));
        // Whole pixels, so 1 pixel lines stay sharp at 100%.
        var left = Math.Round((Bounds.Width - GlyphSize) / 2);
        var top = Math.Round((Bounds.Height - GlyphSize) / 2);
        var brush = new SolidColorBrush(CurrentGlyph);
        var pen = new Pen(brush, 1);
        switch (Kind)
        {
            case CaptionKind.Minimize:
                context.FillRectangle(brush, new Rect(left, top + 5, GlyphSize, 1));
                break;
            case CaptionKind.Maximize when _maximized:
                context.DrawRectangle(pen, new Rect(left + .5, top + 2.5, 7, 7), 1);
                // The back window's two visible edges.
                context.DrawLine(pen, new Point(left + 2.5, top + 2), new Point(left + 2.5, top + .5));
                context.DrawLine(pen, new Point(left + 2.5, top + .5), new Point(left + 9.5, top + .5));
                context.DrawLine(pen, new Point(left + 9.5, top + .5), new Point(left + 9.5, top + 7.5));
                context.DrawLine(pen, new Point(left + 9.5, top + 7.5), new Point(left + 8, top + 7.5));
                break;
            case CaptionKind.Maximize:
                context.DrawRectangle(pen, new Rect(left + .5, top + .5, 9, 9), 1);
                break;
            default:
                context.DrawLine(pen, new Point(left, top), new Point(left + GlyphSize, top + GlyphSize));
                context.DrawLine(pen, new Point(left + GlyphSize, top), new Point(left, top + GlyphSize));
                break;
        }
    }
}
