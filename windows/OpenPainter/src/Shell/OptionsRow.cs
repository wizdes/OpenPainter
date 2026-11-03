// The 36 pixel row: the command buttons, fixed at the start (CommandBar.swift), then the
// tool picker and the current tool's options (OptionsBarView.swift). The options scroll
// sideways. When they do not fit, a button at the row's end opens the groups that are not
// whole in view. W6 decided this; the Mac only scrolls.
using Avalonia;
using Avalonia.Automation;
using Avalonia.Controls;
using Avalonia.Controls.Primitives;
using Avalonia.Controls.Templates;
using Avalonia.Input;
using Avalonia.Interactivity;
using Avalonia.Layout;
using Avalonia.Media;
using OpenPainter.Catalog;
using UiLook;

namespace OpenPainter.Shell;

/// One button of the command row, or one item of its More menu. Words and shortcuts come
/// from the catalog's menu item at `MenuPath`.
public sealed record BarCommand(string Key, string Icon, string Title, string? MenuPath)
{
    public string? Shortcut { get; init; }
    public string Action { get; init; } = "";
    public string Tooltip => Shortcut is null ? Title : $"{Title} ({Shortcut})";
}

public static class CommandBar
{
    public const string WandAction = "tool:magicWand";

    /// CommandBar.barCommands: the seven buttons, in groups of 3, 2 and 2.
    public static IReadOnlyList<IReadOnlyList<BarCommand>> Bar(IReadOnlyList<MenuEntry> menus, IReadOnlyList<CatalogTool> tools)
    {
        var wand = tools.First(t => t.Id == ToolState.MagicWand);
        return
        [
            [From(menus, "new", "cmd-new", "New", "File > New…"), From(menus, "open", "cmd-open", "Open", "File > Open…"), From(menus, "save", "cmd-save", "Save", "File > Save")],
            [From(menus, "undo", "cmd-undo", "Undo", "Edit > Undo"), From(menus, "redo", "cmd-redo", "Redo", "Edit > Redo")],
            [
                From(menus, "crop", "cmd-crop", "Crop to Selection", "Image > Crop to Selection"),
                new BarCommand("wand", wand.Icon ?? throw new InvalidDataException("CATALOG: the Magic Wand has no icon"), wand.Label, null)
                {
                    Shortcut = wand.Key.ToUpperInvariant(),
                    Action = WandAction,
                },
            ],
        ];
    }

    /// CommandBar.moreCommands: the More menu before the panel toggles. Null is a separator.
    public static IReadOnlyList<BarCommand?> More(IReadOnlyList<MenuEntry> menus) =>
    [
        new BarCommand("print", "cmd-print", "Print", null),
        null,
        From(menus, "cut", "cmd-cut", "Cut", "Edit > Cut"),
        From(menus, "copy", "cmd-copy", "Copy", "Edit > Copy"),
        From(menus, "paste", "cmd-paste", "Paste", "Edit > Paste"),
        null,
        From(menus, "deselect", "cmd-deselect", "Deselect", "Edit > Deselect"),
        null,
        From(menus, "grid", "view-pixel-grid", "Pixel Grid", "View > Pixel Grid"),
        From(menus, "rulers", "view-rulers", "Rulers", "View > Rulers"),
    ];

    /// The whole More menu as menu rows: its commands, then one toggle per panel.
    public static IReadOnlyList<MenuEntry> MoreEntries(IReadOnlyList<MenuEntry> menus, IReadOnlyList<CatalogPanel> panels)
    {
        var rows = new List<MenuEntry>();
        MenuEntry Separator() => new(EntryKind.Separator, "More commands > ---", "", null, "", null, []);
        foreach (var command in More(menus))
        {
            rows.Add(command is null
                ? Separator()
                : new MenuEntry(EntryKind.Item, $"More commands > {command.Title}", command.Title, command.Shortcut, command.Action, command.Icon, []));
        }
        rows.Add(Separator());
        foreach (var panel in panels)
            rows.Add(new MenuEntry(EntryKind.Item, $"More commands > {panel.Title}", panel.Title, panel.Shortcut, $"panel:{panel.Id}", panel.Icon, []));
        return rows;
    }

    static BarCommand From(IReadOnlyList<MenuEntry> menus, string key, string icon, string title, string path)
    {
        var entry = Find(menus, path) ?? throw new InvalidDataException($"CATALOG: no menu item {path} for the {key} button");
        return new BarCommand(key, icon, title, path) { Shortcut = entry.Key, Action = entry.Action };
    }

    static MenuEntry? Find(IEnumerable<MenuEntry> entries, string path)
    {
        foreach (var entry in entries)
        {
            if (entry.Path == path) return entry;
            if (Find(entry.Items, path) is { } inner) return inner;
        }
        return null;
    }
}

public sealed class OptionsRow : Grid
{
    const double WheelPoints = 48;
    const double GroupEdge = 12;   // OptionsBarView's stack insets.

    readonly Palette _palette;
    readonly ToolState _tools;
    readonly StackPanel _commands;
    readonly Grid _optionsCell = new() { Name = "OptionsCell", ClipToBounds = true };
    readonly StackPanel _groups = new() { Name = "OptionGroups", Orientation = Orientation.Horizontal, Spacing = Tokens.OptionGroupGap, Margin = new Thickness(GroupEdge, 0), Height = Tokens.OptionsHeight - 1 };
    readonly Border _leftCover, _rightCover;
    readonly StackPanel _moreContent = new() { Name = "MoreContent", Spacing = Tokens.OptionItemGap, Margin = new Thickness(12) };
    readonly List<(int Index, Control Group)> _moved = [];

    public IReadOnlyDictionary<string, IconCommand> Commands { get; }
    public IconCommand MoreCommands { get; }
    public ContextMenu MoreMenu { get; }
    public ComboBox ToolPicker { get; }
    public ScrollViewer Scroller { get; }
    public IconCommand MoreOptions { get; }
    public Popup MorePopup { get; }
    public double RightCover => _rightCover.Width;
    public double LeftCover => _leftCover.Width;
    public IReadOnlyList<Control> Groups => _groups.Children.ToList();
    public Panel MoreGroups => _moreContent;

    public OptionsRow(Palette palette, ToolState tools, IReadOnlyList<MenuEntry> menus, IReadOnlyList<CatalogPanel> panels, IReadOnlyDictionary<string, Action> actions)
    {
        _palette = palette;
        _tools = tools;
        Name = "OptionsRow";
        Height = Tokens.OptionsHeight;
        Background = Ui.B(palette.Options);
        ClipToBounds = true;
        ColumnDefinitions = new ColumnDefinitions("Auto,*,Auto");
        var line = new Border { Height = 1, Background = Ui.B(palette.Line), VerticalAlignment = VerticalAlignment.Bottom };
        SetColumnSpan(line, 3);
        Children.Add(line);

        var (commands, buttons, more) = BuildCommands(menus, tools, actions);
        _commands = commands;
        Commands = buttons;
        MoreCommands = more;
        Children.Add(_commands);

        MoreMenu = new ContextMenu { Placement = PlacementMode.BottomEdgeAlignedLeft, Name = "MoreMenu" };
        MenuBuilder.Fill(MoreMenu.Items, CommandBar.MoreEntries(menus, panels), palette, actions);
        var iconsLoaded = false;
        MoreMenu.Opening += (_, _) =>
        {
            if (iconsLoaded) return;
            iconsLoaded = true;
            MenuBuilder.LoadIcons(MoreMenu.Items.OfType<MenuItem>(), palette);
        };
        MoreCommands.Click += (_, _) => MoreMenu.Open(MoreCommands);

        ToolPicker = BuildPicker();
        Scroller = new ScrollViewer
        {
            Name = "OptionsScroller",
            HorizontalScrollBarVisibility = ScrollBarVisibility.Hidden,
            VerticalScrollBarVisibility = ScrollBarVisibility.Disabled,
            Content = _groups,
            Margin = new Thickness(0, 0, 0, 1),
        };
        _optionsCell.Children.Add(Scroller);
        _leftCover = new Border { Name = "LeftCover", Background = Ui.B(palette.Options), Width = 0, HorizontalAlignment = HorizontalAlignment.Left, Margin = new Thickness(0, 0, 0, 1) };
        _rightCover = new Border { Name = "RightCover", Background = Ui.B(palette.Options), Width = 0, HorizontalAlignment = HorizontalAlignment.Right, Margin = new Thickness(0, 0, 0, 1) };
        _optionsCell.Children.Add(_leftCover);
        _optionsCell.Children.Add(_rightCover);
        SetColumn(_optionsCell, 1);
        Children.Add(_optionsCell);

        MoreOptions = new IconCommand(palette, "opt-overflow", "More options", target: Tokens.CommandTarget, iconSize: Tokens.SmallIconSize) { IsVisible = false };
        var moreCell = new Grid { Width = Tokens.OverflowWidth, Margin = new Thickness(0, 0, 0, 1), Background = Ui.B(palette.Options), Name = "MoreCell", IsVisible = false };
        moreCell.Children.Add(new Border { Width = 1, Height = 20, Background = Ui.B(palette.Line), HorizontalAlignment = HorizontalAlignment.Left, VerticalAlignment = VerticalAlignment.Center });
        MoreOptions.IsVisible = true;
        MoreOptions.HorizontalAlignment = HorizontalAlignment.Center;
        moreCell.Children.Add(MoreOptions);
        SetColumn(moreCell, 2);
        Children.Add(moreCell);

        MorePopup = new Popup
        {
            Name = "MorePopup",
            PlacementTarget = MoreOptions,
            Placement = PlacementMode.BottomEdgeAlignedRight,
            IsLightDismissEnabled = true,
            Child = new Border
            {
                Background = Ui.B(palette.Options),
                BorderBrush = Ui.B(palette.PanelEdge),
                BorderThickness = new Thickness(1),
                CornerRadius = new CornerRadius(Tokens.ControlCorner),
                Child = _moreContent,
            },
        };
        Children.Add(MorePopup);
        MoreOptions.Click += (_, _) => { if (MorePopup.IsOpen) CloseMore(); else OpenMore(); };
        MorePopup.Closed += (_, _) => PutBackMoved();

        ShowTool();
        tools.Changed += OnToolChanged;
        DetachedFromVisualTree += (_, _) => tools.Changed -= OnToolChanged;
        Scroller.ScrollChanged += (_, _) => UpdateFit();
        SizeChanged += (_, _) => UpdateFit();
        _groups.SizeChanged += (_, _) => UpdateFit();
        LayoutUpdated += (_, _) => UpdateFit();
        // The vertical wheel moves the row sideways. Tunnel, so a control under the pointer does not take it first.
        AddHandler(PointerWheelChangedEvent, OnWheel, RoutingStrategies.Tunnel);
    }

    (StackPanel, Dictionary<string, IconCommand>, IconCommand) BuildCommands(IReadOnlyList<MenuEntry> menus, ToolState tools, IReadOnlyDictionary<string, Action> actions)
    {
        var row = Ui.Row();
        row.Name = "CommandButtons";
        row.Margin = new Thickness(12, 0, 0, 1);
        var buttons = new Dictionary<string, IconCommand>();
        var groups = CommandBar.Bar(menus, tools.Tools);
        foreach (var group in groups)
        {
            for (var i = 0; i < group.Count; i++)
            {
                var c = group[i];
                var on = actions.TryGetValue(c.Action, out var action);
                var button = new IconCommand(_palette, c.Icon, c.Title, c.Tooltip, enabled: on)
                {
                    Margin = new Thickness(0, 0, i == group.Count - 1 ? Tokens.CommandGroupGap : Tokens.CommandGap, 0),
                    Name = "Command" + c.Key,
                };
                if (action is not null) button.Click += (_, _) => action();
                buttons[c.Key] = button;
                row.Children.Add(button);
            }
        }
        // More sits with the seven, closer than a group gap.
        if (row.Children[^1] is Control last) last.Margin = new Thickness(0, 0, Tokens.CommandGap, 0);
        var more = new IconCommand(_palette, "cmd-more", "More commands") { Margin = new Thickness(0, 0, Tokens.CommandGroupGap, 0), Name = "CommandMore" };
        row.Children.Add(more);
        row.Children.Add(new Border { Width = 1, Height = 20, Background = Ui.B(_palette.Line), VerticalAlignment = VerticalAlignment.Center });
        return (row, buttons, more);
    }

    ComboBox BuildPicker()
    {
        var palette = _palette;
        var picker = Ui.Choice(palette, _tools.Tools[0], OptionRows.PickerWidth, padLeft: 8);
        picker.Name = "ToolPicker";
        AutomationProperties.SetName(picker, "Tool");
        foreach (var tool in _tools.Tools.Skip(1)) picker.Items.Add(tool);
        picker.ItemTemplate = new FuncDataTemplate<CatalogTool>((tool, _) =>
        {
            var row = Ui.Row(6);
            if (tool is null) return row;
            if (tool.Icon is { } icon) row.Children.Add(new IconView(icon, palette.Ink));
            row.Children.Add(Ui.Label(tool.Label, palette.Ink));
            return row;
        });
        picker.SelectedItem = _tools.CurrentTool;
        picker.SelectionChanged += (_, _) =>
        {
            if (picker.SelectedItem is CatalogTool tool) _tools.Current = tool.Id;
        };
        return picker;
    }

    void OnToolChanged()
    {
        if (!ReferenceEquals(ToolPicker.SelectedItem, _tools.CurrentTool)) ToolPicker.SelectedItem = _tools.CurrentTool;
        ShowTool();
    }

    /// Rebuilds the row for the current tool, and shows it from the start.
    void ShowTool()
    {
        CloseMore();
        // The picker stays in place, because its own selection change can land here.
        while (_groups.Children.Count > 1) _groups.Children.RemoveAt(1);
        if (_groups.Children.Count == 0) _groups.Children.Add(ToolPicker);
        foreach (var group in OptionRows.For(_tools.Current, _tools.CurrentTool.Options))
            _groups.Children.Add(BuildGroup(group));
        Scroller.Offset = default;
        if (Commands.TryGetValue("wand", out var wand)) wand.On = _tools.Current == ToolState.MagicWand;
    }

    Control BuildGroup(IEnumerable<OptionItem> items)
    {
        var p = _palette;
        var row = Ui.Row(Tokens.OptionItemGap);
        foreach (var item in items)
        {
            row.Children.Add(item switch
            {
                OptionLabel l => Ui.Label(l.Text, p.Muted),
                OptionField f => Named(Ui.Field(p, f.Text, 52, Tokens.OptionHeight), f.Name),
                OptionChoice c => BuildChoice(c),
                OptionCheck c => BuildCheck(c),
                // Not working until W8, so present and off.
                OptionStep s => new IconCommand(p, s.Icon, s.Name, iconSize: Tokens.SmallIconSize, enabled: false),
                OptionToggle t => new IconCommand(p, t.Icon, t.Name, on: t.On),
                OptionSlide s => Named(Ui.Slide(p, s.Min, s.Max, s.Value, 96), s.Name),
                OptionPush b => BuildPush(b),
                _ => throw new InvalidOperationException($"OPTIONS: unknown row item {item}"),
            });
        }
        return row;
    }

    static T Named<T>(T control, string name) where T : Control
    {
        AutomationProperties.SetName(control, name);
        return control;
    }

    Control BuildChoice(OptionChoice c)
    {
        var box = Ui.Choice(_palette, c.Items[0], c.Width, c.Name == "Font size" ? Tokens.OptionHeight : Tokens.ChoiceHeight, c.Name == "Font size" ? 8 : 10);
        foreach (var item in c.Items.Skip(1)) box.Items.Add(item);
        box.SelectedIndex = c.Selected;
        if (c.Name == "Font size") box.Background = Ui.B(_palette.Field);
        return Named(box, c.Name);
    }

    Control BuildCheck(OptionCheck c)
    {
        var check = Ui.Check(_palette, c.Text);
        check.IsChecked = c.On;
        return Named(check, c.Name);
    }

    Control BuildPush(OptionPush b)
    {
        var push = Ui.Push(_palette, b.Text, b.Width);
        push.IsEnabled = false;   // No session can be open until W8.
        return Named(push, b.Name);
    }

    IReadOnlyList<(double Left, double Right)> GroupSpans()
    {
        var left = _groups.Margin.Left;
        return _groups.Children.Select(g => (g.Bounds.X + left, g.Bounds.Right + left)).ToList();
    }

    /// The room for options when the "more" button is hidden.
    double Room => Bounds.Width - _commands.Bounds.Width - _commands.Margin.Left - _commands.Margin.Right;

    void UpdateFit()
    {
        var more = MoreOptions.Parent as Control ?? throw new InvalidOperationException("OPTIONS: the more button has no cell");
        if (!MorePopup.IsOpen)
        {
            var content = _groups.Bounds.Width + _groups.Margin.Left + _groups.Margin.Right;
            var overflows = OptionsFit.Overflows(content, Room);
            if (more.IsVisible != overflows) more.IsVisible = overflows;
        }
        var (left, right) = OptionsFit.Covers(GroupSpans(), Scroller.Offset.X, Scroller.Viewport.Width);
        if (_leftCover.Width != left) _leftCover.Width = left;
        if (_rightCover.Width != right) _rightCover.Width = right;
    }

    /// The groups that are not whole in view right now.
    public IReadOnlyList<int> HiddenGroups() => OptionsFit.Hidden(GroupSpans(), Scroller.Offset.X, Scroller.Viewport.Width);

    /// Moves the hidden groups into the popup. A blank of the same width holds each place,
    /// so the row does not shift while the popup is open.
    public void OpenMore()
    {
        if (MorePopup.IsOpen) return;
        foreach (var index in HiddenGroups())
        {
            var group = _groups.Children[index];
            var blank = new Border { Width = group.Bounds.Width, Height = group.Bounds.Height };
            _groups.Children[index] = blank;
            _moved.Add((index, group));
            group.HorizontalAlignment = HorizontalAlignment.Left;
            _moreContent.Children.Add(group);
        }
        MorePopup.IsOpen = true;
    }

    public void CloseMore()
    {
        MorePopup.IsOpen = false;
        PutBackMoved();
    }

    void PutBackMoved()
    {
        foreach (var (index, group) in _moved)
        {
            _moreContent.Children.Remove(group);
            _groups.Children[index] = group;
        }
        _moved.Clear();
    }

    void OnWheel(object? sender, PointerWheelEventArgs e)
    {
        if (!_optionsCell.IsPointerOver) return;
        var delta = e.Delta.Y != 0 ? e.Delta.Y : -e.Delta.X;
        ScrollBy(-delta * WheelPoints);
        e.Handled = true;
    }

    public void ScrollBy(double points)
    {
        var max = Math.Max(0, Scroller.Extent.Width - Scroller.Viewport.Width);
        Scroller.Offset = new Vector(Math.Clamp(Scroller.Offset.X + points, 0, max), 0);
    }
}
