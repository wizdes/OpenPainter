// The Tools panel: ToolsPanelView and ToolCell in mac/OpenPainterKit/Sources/Shell/PanelViews.swift.
// Two columns of 24 pixel targets with 2 pixel gaps, in the Mac's order. The selected tool has a
// 2 pixel edge and a dot. The list scrolls when the panel is short. Every tool can be selected,
// as on the Mac, and the ones with no canvas action until W9 say so in the hint.
using Avalonia;
using Avalonia.Automation;
using Avalonia.Controls;
using Avalonia.Controls.Primitives;
using Avalonia.Layout;
using Avalonia.Media;
using OpenPainter.Catalog;
using UiLook;

namespace OpenPainter.Shell;

public sealed class ToolCell : Button
{
    protected override Type StyleKeyOverride => typeof(Button);

    readonly Border _wash, _edge, _dot;

    public CatalogTool Tool { get; }

    public ToolCell(Palette p, CatalogTool tool)
    {
        Tool = tool;
        Width = Tokens.PanelTarget;
        Height = Tokens.PanelTarget;
        MinWidth = 0;
        MinHeight = 0;
        Padding = new Thickness(0);
        Background = Brushes.Transparent;
        BorderThickness = new Thickness(0);
        CornerRadius = new CornerRadius(3);
        HorizontalContentAlignment = HorizontalAlignment.Stretch;
        VerticalContentAlignment = VerticalAlignment.Stretch;
        AutomationProperties.SetName(this, tool.Label);
        ToolTip.SetTip(this, $"{tool.Label} ({tool.Key.ToUpperInvariant()})");
        _wash = new Border { Background = Ui.B(p.Wash), CornerRadius = new CornerRadius(3), IsVisible = false };
        _edge = new Border { Background = Ui.B(p.Accent), Width = 2, HorizontalAlignment = HorizontalAlignment.Left, IsVisible = false };
        // The dot sits outside the icon box: W6's Panels.ToolDotBox.
        _dot = new Border
        {
            Background = Ui.B(p.Accent),
            CornerRadius = new CornerRadius(2),
            Width = 4,
            Height = 4,
            HorizontalAlignment = HorizontalAlignment.Left,
            VerticalAlignment = VerticalAlignment.Top,
            Margin = new Thickness(22, 0, 0, 0),
            IsVisible = false,
        };
        var icon = tool.Icon ?? throw new InvalidDataException($"CATALOG: the tool {tool.Id} has no icon");
        Content = new Grid
        {
            Children =
            {
                _wash,
                _edge,
                _dot,
                new IconView(icon, p.Ink) { HorizontalAlignment = HorizontalAlignment.Center, VerticalAlignment = VerticalAlignment.Center },
            },
        };
    }

    public bool IsOn
    {
        get => _wash.IsVisible;
        set
        {
            _wash.IsVisible = value;
            _edge.IsVisible = value;
            _dot.IsVisible = value;
            AutomationProperties.SetItemStatus(this, value ? "selected" : "");
        }
    }

    /// The icon's place: W6's Panels.ToolIconBox.
    public Control IconBox => ((Grid)Content!).Children.OfType<IconView>().Single();
}

public sealed class ToolsPanel : Grid
{
    const double Cell = Tokens.PanelTarget, Gap = 2, Inset = 5;

    readonly ToolState _tools;
    readonly Dictionary<string, ToolCell> _cells = [];

    /// The cells by tool id, in the Mac's order.
    public IReadOnlyDictionary<string, ToolCell> Cells => _cells;
    public ScrollViewer Scroller { get; }

    /// The content height that shows every tool with no empty space, less the outline's pixel
    /// under the panel's body, so a default panel has nothing to scroll.
    public static double FittingHeight(int tools)
    {
        var rows = (tools + 1) / 2;
        return rows * Cell + (rows - 1) * Gap + 2 * Inset - 1;
    }

    public ToolsPanel(Palette p, PanelContext context)
    {
        _tools = context.Tools;
        Background = Ui.B(p.Panel);
        var grid = new Avalonia.Controls.Canvas { Name = "ToolGrid", Height = FittingHeight(_tools.Tools.Count) };
        // The panel's window is 84 wide and this body starts after its 1 pixel outline.
        var x0 = Math.Round((Tokens.ToolsWidth - (2 * Cell + Gap)) / 2) - 1;
        for (var i = 0; i < _tools.Tools.Count; i++)
        {
            var cell = new ToolCell(p, _tools.Tools[i]);
            Avalonia.Controls.Canvas.SetLeft(cell, x0 + i % 2 * (Cell + Gap));
            Avalonia.Controls.Canvas.SetTop(cell, Inset + i / 2 * (Cell + Gap));
            var id = cell.Tool.Id;
            cell.Click += (_, _) => _tools.Current = id;
            _cells[id] = cell;
            grid.Children.Add(cell);
        }
        Scroller = new ScrollViewer
        {
            HorizontalScrollBarVisibility = ScrollBarVisibility.Disabled,
            VerticalScrollBarVisibility = ScrollBarVisibility.Auto,
            Content = grid,
        };
        Children.Add(Scroller);
        Sync();
        AttachedToVisualTree += (_, _) =>
        {
            _tools.Changed += Sync;
            Sync();
        };
        DetachedFromVisualTree += (_, _) => _tools.Changed -= Sync;
    }

    void Sync()
    {
        foreach (var (id, cell) in _cells) cell.IsOn = id == _tools.Current;
    }
}
