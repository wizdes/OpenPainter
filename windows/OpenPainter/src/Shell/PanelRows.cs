// Rows for History and Layers, and the list above an action row: PanelRow and
// RowListPanelView in mac/OpenPainterKit/Sources/Shell/PanelViews.swift, drawn as W6's
// Panels.ListRow draws them. Tab reaches each row, Space or Enter picks it, and arrows move.
using Avalonia;
using Avalonia.Automation;
using Avalonia.Controls;
using Avalonia.Controls.Primitives;
using Avalonia.Input;
using Avalonia.Layout;
using Avalonia.Media;
using UiLook;

namespace OpenPainter.Shell;

/// What the panels work on, and whether a dialog holds them off.
public sealed class PanelContext(ToolState tools)
{
    public ToolState Tools { get; } = tools;

    public Editor? Editor { get; set; }

    /// A line for the status bar: palette reports.
    public Action<string>? Report { get; set; }

    /// A field took bad input: NSSound.beep on the Mac.
    public Action? Refused { get; set; }

    /// Asks the user for a palette file to load. Null means no file was chosen. Tests set this,
    /// because the system's file picker needs a real pointer.
    public Func<TopLevel, Task<string?>>? PickPaletteFile { get; set; }

    /// Commands are off while a dialog is up on the document.
    public bool Busy { get; set; }

    /// Opens Layer Properties for the layer at this index.
    public Action<int>? ShowLayerProperties { get; set; }

    /// A field finished with Enter, so the keyboard goes back to the canvas: ColorsPanel.swift's
    /// "Give the keyboard back to the document window".
    public Action? GiveKeyboardBack { get; set; }
}

/// One list row: a 2 pixel edge and a diamond when current, and an optional detail line.
public class PanelRow : Grid
{
    const double DetailFont = 11;

    protected readonly Palette Look;
    readonly Color _surface;
    readonly bool _current;
    Point _downAt;
    bool _down;

    public int Index { get; }
    public bool IsCurrent { get; }
    public bool Dimmed { get; }
    public TextBlock TitleLabel { get; }
    public TextBlock? DetailLabel { get; }

    /// A click, with the press's click count.
    public event Action<int, int>? Clicked;

    /// The press ended, or the row lost the pointer.
    public event Action? HoldEnded;

    /// A press is under way on this row, so the list must not replace it yet.
    public bool Held => _down;

    /// A control inside the row that can take the keyboard: the Layers row's eye.
    public virtual Control? InnerControl => null;

    public PanelRow(Palette p, int index, string title, string? detail, double height, bool current, bool dimmed, double textLeft, double textRight)
    {
        Look = p;
        Index = index;
        IsCurrent = current;
        Dimmed = dimmed;
        Height = height;
        _current = current;
        _surface = current ? p.Wash : p.Panel;
        Focusable = true;
        Background = current ? Ui.B(p.Wash) : Brushes.Transparent;
        ClipToBounds = true;
        TitleLabel = Ui.Label(title, dimmed ? p.Muted : p.Ink, weight: current ? Tokens.StrongWeight : FontWeight.Normal);
        TitleLabel.Height = 16;
        TitleLabel.VerticalAlignment = VerticalAlignment.Top;
        TitleLabel.Margin = new Thickness(textLeft, detail is null ? (height - 16) / 2 : height / 2 - 17, textRight, 0);
        TitleLabel.IsHitTestVisible = false;
        Children.Add(TitleLabel);
        if (detail is not null)
        {
            // PanelRow.detailLabel: a size smaller than the title, which is what fits it on the Mac.
            DetailLabel = Ui.Label(detail, p.Muted, DetailFont);
            DetailLabel.Height = 16;
            DetailLabel.VerticalAlignment = VerticalAlignment.Top;
            DetailLabel.Margin = new Thickness(textLeft, height / 2 + 1, textRight, 0);
            DetailLabel.IsHitTestVisible = false;
            Children.Add(DetailLabel);
        }
        if (current)
        {
            Children.Add(new Border { Width = 2, Background = Ui.B(p.Accent), HorizontalAlignment = HorizontalAlignment.Left, IsHitTestVisible = false });
            Children.Add(new Drawn((c, size) =>
            {
                var (x, y) = (size.Width - 12, size.Height / 2);
                c.DrawGeometry(Ui.B(p.Accent), null, new PolylineGeometry(new[] { new Point(x, y - 4), new Point(x + 4, y), new Point(x, y + 4), new Point(x - 4, y) }, true));
            }));
        }
        Children.Add(new Border { Height = 1, Background = Ui.B(p.Line), VerticalAlignment = VerticalAlignment.Bottom, IsHitTestVisible = false });
        AutomationProperties.SetName(this, title);
        PointerPressed += OnPressed;
        PointerMoved += OnMoved;
        PointerReleased += OnReleased;
        PointerCaptureLost += (_, _) =>
        {
            if (!_down) return;
            _down = false;
            ShowState();
            CaptureLost();
            HoldEnded?.Invoke();
        };
    }

    int _clicks;

    void OnPressed(object? sender, PointerPressedEventArgs e)
    {
        if (!e.GetCurrentPoint(this).Properties.IsLeftButtonPressed) return;
        _down = true;
        _clicks = e.ClickCount;
        _downAt = e.GetPosition(this);
        ShowState();
        Pressed(e);
    }

    /// The row's fill in hover and press, over its usual fill.
    void ShowState()
    {
        if (_down) Background = Ui.B(ControlStates.Pressed(Look, _surface));
        else if (IsPointerOver) Background = Ui.B(ControlStates.Hover(Look, _surface));
        else Background = _current ? Ui.B(Look.Wash) : Brushes.Transparent;
    }

    protected override void OnPropertyChanged(AvaloniaPropertyChangedEventArgs change)
    {
        base.OnPropertyChanged(change);
        if (change.Property == IsPointerOverProperty) ShowState();
    }

    /// KeyboardAccess in KeyboardShortcuts.swift: Space or Enter picks the row, and the up and down
    /// arrows move to the row above or below.
    protected override void OnKeyDown(KeyEventArgs e)
    {
        base.OnKeyDown(e);
        if (e.Handled || (e.KeyModifiers & (KeyModifiers.Control | KeyModifiers.Alt | KeyModifiers.Meta)) != 0) return;
        switch (e.Key)
        {
            case Key.Space or Key.Enter:
                Clicked?.Invoke(Index, 1);
                e.Handled = true;
                break;
            case Key.Up or Key.Down when Parent is Avalonia.Controls.Panel list:
                var rows = list.Children.OfType<PanelRow>().ToList();
                var at = rows.IndexOf(this) + (e.Key == Key.Up ? -1 : 1);
                if (at < 0 || at >= rows.Count) return;
                rows[at].Focus(NavigationMethod.Directional);
                rows[at].BringIntoView();
                e.Handled = true;
                break;
        }
    }

    void OnMoved(object? sender, PointerEventArgs e)
    {
        if (_down) Dragged(e, _downAt);
    }

    void OnReleased(object? sender, PointerReleasedEventArgs e)
    {
        if (!_down) return;
        _down = false;
        ShowState();
        // A release off the row is no click, as with any Windows button.
        var over = new Rect(Bounds.Size).Contains(e.GetPosition(this));
        if (!Released(e) && over) Clicked?.Invoke(Index, _clicks);
        HoldEnded?.Invoke();
    }

    protected virtual void Pressed(PointerPressedEventArgs e) { }
    protected virtual void Dragged(PointerEventArgs e, Point downAt) { }

    /// True when the press became something other than a click.
    protected virtual bool Released(PointerReleasedEventArgs e) => false;

    protected virtual void CaptureLost() { }
}

/// A scrolling list of rows above an action row.
public class RowListPanel : Grid
{
    readonly StackPanel _rows = new();
    bool _reloadWhenFree;

    protected Palette Look { get; }
    public double RowHeight { get; }
    public ScrollViewer Scroller { get; }
    public Avalonia.Controls.Canvas Overlay { get; } = new() { IsHitTestVisible = false };

    /// The action row. Each panel fills it.
    public Grid Actions { get; } = new() { Margin = new Thickness(8, 2) };

    public IReadOnlyList<PanelRow> Rows => _rows.Children.OfType<PanelRow>().ToList();

    public RowListPanel(Palette p, double rowHeight)
    {
        Look = p;
        RowHeight = rowHeight;
        Background = Ui.B(p.Panel);
        RowDefinitions = new RowDefinitions($"*,1,{Tokens.ActionRowHeight}");
        Scroller = new ScrollViewer
        {
            HorizontalScrollBarVisibility = ScrollBarVisibility.Disabled,
            VerticalScrollBarVisibility = ScrollBarVisibility.Auto,
            Content = new Grid { Children = { _rows, Overlay } },
        };
        Children.Add(Scroller);
        var line = new Border { Background = Ui.B(p.Line) };
        SetRow(line, 1);
        Children.Add(line);
        SetRow(Actions, 2);
        Children.Add(Actions);
    }

    protected void SetRows(IEnumerable<PanelRow> rows)
    {
        // A new list replaces the rows, and the keyboard must stay at the same place in it,
        // on the row or on the control inside it.
        var old = _rows.Children.OfType<PanelRow>().ToList();
        var inner = old.FindIndex(r => r.InnerControl?.IsFocused == true);
        var focused = inner >= 0 ? inner : old.FindIndex(r => r.IsFocused);
        _rows.Children.Clear();
        foreach (var row in rows)
        {
            row.HoldEnded += OnHoldEnded;
            _rows.Children.Add(row);
        }
        if (focused >= 0 && _rows.Children.Count > 0)
        {
            var row = (PanelRow)_rows.Children[Math.Min(focused, _rows.Children.Count - 1)];
            if (inner < 0 || row.InnerControl?.Focus(NavigationMethod.Directional) != true) row.Focus(NavigationMethod.Directional);
        }
    }

    /// True when a press is under way. A new list then waits for it, because the row under the
    /// pointer must stay to get its release. The Mac holds a list only during a drag, but
    /// AppKit sends a release to a view that has left its parent, and Avalonia does not.
    protected bool WaitForRelease()
    {
        if (!Rows.Any(r => r.Held)) return false;
        _reloadWhenFree = true;
        return true;
    }

    void OnHoldEnded()
    {
        if (!_reloadWhenFree) return;
        _reloadWhenFree = false;
        Avalonia.Threading.Dispatcher.UIThread.Post(Reload);
    }

    public virtual void Reload() { }

    protected void ScrollToCurrent()
    {
        if (Rows.FirstOrDefault(r => r.IsCurrent) is { } row) row.BringIntoView();
    }
}
