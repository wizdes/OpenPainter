// The Layers panel: mac/OpenPainterKit/Sources/Shell/LayersPanel.swift. One 49 pixel row per
// layer, top layer first, with the engine's picture of it, the name, an eye and the active
// row's edge and diamond. The action row has Add, Delete, Duplicate, Merge Down, Move Up, Move
// Down and Properties. Rows reorder by drag. Every change is an engine command, and choosing
// a layer adds no history step.
using Avalonia;
using Avalonia.Automation;
using Avalonia.Controls;
using Avalonia.Input;
using Avalonia.Layout;
using Avalonia.Media;
using Avalonia.Media.Imaging;
using OpenPainter.Bridge;
using UiLook;

namespace OpenPainter.Shell;

/// A picture on a checkerboard, fitted in its box, with a 1 pixel edge: LayerThumbView.
public sealed class LayerThumb : Control
{
    const double Cell = 4;
    readonly Palette _palette;
    readonly Size _pixels;
    Bitmap? _image;

    public LayerThumb(Palette palette, Size pixels)
    {
        _palette = palette;
        _pixels = pixels;
        IsHitTestVisible = false;
    }

    public Bitmap? Image
    {
        get => _image;
        set
        {
            _image = value;
            InvalidateVisual();
        }
    }

    /// The picture's rectangle inside the bounds.
    public Rect ImageRect
    {
        get
        {
            var s = Math.Min(Bounds.Width / _pixels.Width, Bounds.Height / _pixels.Height);
            var (w, h) = (Math.Max(Math.Round(_pixels.Width * s), 1), Math.Max(Math.Round(_pixels.Height * s), 1));
            return new Rect(Math.Round((Bounds.Width - w) / 2), Math.Round((Bounds.Height - h) / 2), w, h);
        }
    }

    public override void Render(DrawingContext context)
    {
        var r = ImageRect;
        // The checkerboard is content, as on the canvas, so it does not follow the theme.
        context.FillRectangle(Ui.B(Tokens.CheckerLight), r);
        var dark = Ui.B(Tokens.CheckerDark);
        var row = 0;
        for (var y = r.Top; y < r.Bottom; y += Cell, row++)
            for (var x = r.Left + (row % 2 == 0 ? 0 : Cell); x < r.Right; x += 2 * Cell)
                context.FillRectangle(dark, new Rect(x, y, Math.Min(Cell, r.Right - x), Math.Min(Cell, r.Bottom - y)));
        if (_image is not null)
        {
            using (context.PushRenderOptions(new RenderOptions { BitmapInterpolationMode = BitmapInterpolationMode.HighQuality }))
                context.DrawImage(_image, new Rect(_image.Size), r);
        }
        context.DrawRectangle(new Pen(Ui.B(_palette.Line)), r.Inflate(.5));
    }
}

/// One layer: picture, name, a detail line when it is hidden or not plain Normal at 100%, and
/// the eye. Dragging the row reorders layers.
public sealed class LayerRow : PanelRow
{
    const double DragStart = 3;

    // LayerRow.layout gives text x 56 and 114 wide in a 226 pixel row. This row is 224, inside the
    // window's outline, so its right side gives back those 2 pixels.
    const double TextLeft = 56, TextRight = 54;
    readonly LayersPanel _owner;

    public LayerInfo Layer { get; }
    public LayerThumb Thumb { get; }
    public IconCommand Eye { get; }
    public bool IsDragging { get; private set; }

    public override Control? InnerControl => Eye;

    public LayerRow(Palette p, LayersPanel owner, int index, LayerInfo layer, Size pixels, bool current, bool busy)
        : base(p, index, layer.Name, Detail(layer), Tokens.LayerRowHeight, current, false, TextLeft, TextRight)
    {
        _owner = owner;
        Layer = layer;
        Thumb = new LayerThumb(p, pixels)
        {
            Width = 40,
            Height = 40,
            HorizontalAlignment = HorizontalAlignment.Left,
            VerticalAlignment = VerticalAlignment.Center,
            Margin = new Thickness(8, 0, 0, 0),
        };
        Children.Add(Thumb);
        Eye = new IconCommand(p, layer.Visible ? "layer-visible" : "layer-hidden", layer.Visible ? $"Hide {layer.Name}" : $"Show {layer.Name}",
            layer.Visible ? "Visible: click to hide" : "Hidden: click to show", Tokens.PanelTarget, Tokens.IconSize, ink: layer.Visible ? p.Ink : p.Muted)
        {
            HorizontalAlignment = HorizontalAlignment.Right,
            VerticalAlignment = VerticalAlignment.Center,
            Margin = new Thickness(0, 0, 28, 0),
            IsEnabled = !busy,
        };
        AutomationProperties.SetItemStatus(Eye, layer.Visible ? "visible" : "hidden");
        AutomationProperties.SetItemStatus(this, layer.Visible ? "visible" : "hidden");
        AutomationProperties.SetHelpText(this, "Drag to reorder. Double-click for Layer Properties.");
        Children.Add(Eye);
    }

    /// LayerRow.detail.
    public static string? Detail(LayerInfo l)
    {
        var parts = new List<string>();
        if (!l.Visible) parts.Add("Hidden");
        if (l.Blend != "normal") parts.Add(LayerBlend.Title(l.Blend));
        if (l.Opacity != 255) parts.Add($"{(int)Math.Round(l.Opacity * 100.0 / 255, MidpointRounding.AwayFromZero)}%");
        return parts.Count == 0 ? null : string.Join(" · ", parts);
    }

    protected override void Pressed(PointerPressedEventArgs e)
    {
        IsDragging = false;
        e.Pointer.Capture(this);
    }

    protected override void Dragged(PointerEventArgs e, Point downAt)
    {
        var at = e.GetPosition(this);
        if (!IsDragging && Math.Sqrt(Math.Pow(at.X - downAt.X, 2) + Math.Pow(at.Y - downAt.Y, 2)) < DragStart) return;
        IsDragging = true;
        _owner.DragMoved(e.GetPosition(_owner.Overlay));
    }

    protected override void CaptureLost()
    {
        if (!IsDragging) return;
        IsDragging = false;
        _owner.DragCancelled();
    }

    protected override bool Released(PointerReleasedEventArgs e)
    {
        e.Pointer.Capture(null);
        if (!IsDragging) return false;
        IsDragging = false;
        _owner.DragEnded(this, e.GetPosition(_owner.Overlay));
        return true;
    }
}

public sealed class LayersPanel : RowListPanel
{
    /// The buttons in the Mac's order: engine command, icon, name, and the key its tip shows.
    public static readonly (string Id, string Icon, string Label, string? Key)[] Items =
    [
        ("layer.add", "layer-add", "Add New Layer", "Ctrl+Shift+N"),
        ("layer.delete", "layer-delete", "Delete Layer", "Ctrl+Shift+Delete"),
        ("layer.duplicate", "layer-duplicate", "Duplicate Layer", "Ctrl+Shift+D"),
        ("layer.merge_down", "layer-merge-down", "Merge Layer Down", null),
        ("layer.move_up", "arrow-up", "Move Layer Up", null),
        ("layer.move_down", "arrow-down", "Move Layer Down", null),
        ("layer.properties", "layer-properties", "Layer Properties", "F4"),
    ];

    readonly PanelContext _context;
    readonly Dictionary<string, IconCommand> _buttons = [];
    LayerThumbnails? _watching;

    /// The insertion line shown while a row is dragged.
    public Border DropLine { get; }
    public int? DropGap { get; private set; }

    /// Buttons by engine command id.
    public IReadOnlyDictionary<string, IconCommand> Buttons => _buttons;
    public IReadOnlyList<LayerRow> LayerRows => Rows.OfType<LayerRow>().ToList();

    public LayersPanel(Palette p, PanelContext context) : base(p, Tokens.LayerRowHeight)
    {
        _context = context;
        var row = Ui.Row(4);
        foreach (var (id, icon, label, key) in Items)
        {
            var button = new IconCommand(p, icon, label, key is null ? label : $"{label} ({key})", Tokens.PanelTarget, Tokens.IconSize);
            button.Click += (_, _) => Act(id);
            _buttons[id] = button;
            row.Children.Add(button);
        }
        Actions.Children.Add(row);
        DropLine = new Border { Height = 2, Background = Ui.B(p.Accent), IsVisible = false };
        Overlay.Children.Add(DropLine);
        Reload();
    }

    /// A theme change replaces this panel, so the old one must stop listening.
    protected override void OnDetachedFromVisualTree(Avalonia.VisualTreeAttachmentEventArgs e)
    {
        base.OnDetachedFromVisualTree(e);
        if (_watching is not null) _watching.Ready -= ThumbnailReady;
        _watching = null;
    }

    public override void Reload()
    {
        if (WaitForRelease()) return;
        if (_context.Editor is not { } e)
        {
            SetRows([]);
            foreach (var button in _buttons.Values) button.IsEnabled = false;
            return;
        }
        if (!ReferenceEquals(_watching, e.Thumbnails))
        {
            if (_watching is not null) _watching.Ready -= ThumbnailReady;
            _watching = e.Thumbnails;
            _watching.Ready += ThumbnailReady;
            _watching.IsShown = image => LayerRows.Any(r => ReferenceEquals(r.Thumb.Image, image));
        }
        var layers = e.Info.Layers;
        var size = new Size(Math.Max(e.Info.Width, 1), Math.Max(e.Info.Height, 1));
        var rows = new List<PanelRow>();
        // Top layer first, like paint.net.
        for (var i = layers.Count - 1; i >= 0; i--)
        {
            var row = new LayerRow(Look, this, i, layers[i], size, i == e.Info.ActiveLayer, _context.Busy);
            row.Thumb.Image = e.Thumbnails.Image(layers[i]);
            row.Clicked += OnRowClicked;
            var index = i;
            row.Eye.Click += (_, _) => ToggleVisible(index);
            rows.Add(row);
        }
        SetRows(rows);
        foreach (var (id, button) in _buttons)
            button.IsEnabled = !_context.Busy && (id == "layer.properties" ? layers.Count != 0 : e.IsEnabled(id));
        ScrollToCurrent();
    }

    void OnRowClicked(int index, int clicks)
    {
        if (clicks >= 2)
        {
            Select(index);
            _context.ShowLayerProperties?.Invoke(index);
        }
        else Select(index);
    }

    void ThumbnailReady(long layer, long revision)
    {
        if (_context.Editor is not { } e) return;
        foreach (var row in LayerRows)
            if (row.Layer.Id == layer && row.Layer.Revision == revision) row.Thumb.Image = e.Thumbnails.Cached(layer, revision);
    }

    public void Select(int index)
    {
        if (_context.Busy || _context.Editor is not { } e || index == e.Info.ActiveLayer) return;
        e.Run("layer.set_active", EngineParams.Of(("index", index)));
    }

    public void ToggleVisible(int index)
    {
        if (_context.Busy || _context.Editor is not { } e || index >= e.Info.Layers.Count) return;
        e.Run("layer.set_visible", EngineParams.Of(("index", index), ("visible", !e.Info.Layers[index].Visible)));
    }

    public void Act(string id)
    {
        if (_context.Busy || _context.Editor is not { } e) return;
        switch (id)
        {
            case "layer.add":
                e.Run(id, EngineParams.Of(("name", $"Layer {e.Info.Layers.Count + 1}")));
                break;
            case "layer.properties":
                _context.ShowLayerProperties?.Invoke(e.Info.ActiveLayer);
                break;
            default:
                e.Run(id);
                break;
        }
    }

    /// The gap (0 is above the top row) nearest to a point in the list.
    public int Gap(Point at) => Math.Clamp((int)Math.Round(at.Y / RowHeight, MidpointRounding.AwayFromZero), 0, Rows.Count);

    public void DragMoved(Point at)
    {
        if (_context.Busy) return;
        var gap = Gap(at);
        DropGap = gap;
        Avalonia.Controls.Canvas.SetTop(DropLine, gap * RowHeight - 1);
        DropLine.Width = Overlay.Bounds.Width;
        DropLine.IsVisible = true;
    }

    /// Drops the row at the gap under the point: one `layer.move` step, or nothing when the row
    /// would land where it is.
    public void DragEnded(LayerRow row, Point at)
    {
        DropLine.IsVisible = false;
        DropGap = null;
        if (!_context.Busy && _context.Editor is { } e && DropIndex(row.Index, Gap(at), e.Info.Layers.Count) is { } to)
            e.Run("layer.move", EngineParams.Of(("from", row.Index), ("to", to)));
    }

    /// The row lost the pointer in mid drag, so the drag ends where it began.
    public void DragCancelled()
    {
        DropLine.IsVisible = false;
        DropGap = null;
    }

    /// Rows are top layer first. Moving layer `from` into display gap `gap` gives the layer
    /// index it lands at, or null for no move.
    public static int? DropIndex(int from, int gap, int count)
    {
        var fromRow = count - 1 - from;
        var toRow = gap > fromRow ? gap - 1 : gap;
        var to = count - 1 - Math.Clamp(toRow, 0, count - 1);
        return to == from ? null : to;
    }
}
