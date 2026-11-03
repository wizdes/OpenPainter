// The History panel: mac/OpenPainterKit/Sources/Shell/HistoryPanel.swift. One 27 pixel row per
// step with its name and an icon for its command. The current step has an edge and a diamond.
// Undone steps stay readable in muted text with a redo arrow. A click jumps to that step.
// Undo and Redo sit in the action row.
using Avalonia;
using Avalonia.Automation;
using Avalonia.Controls;
using Avalonia.Layout;
using Avalonia.Media;
using UiLook;

namespace OpenPainter.Shell;

/// Icons and display names for history steps: HistoryStep in HistoryPanel.swift, with the file
/// each row of docs/windows/icons.toml names for that step.
public static class HistoryStep
{
    public static string Icon(string command) => command switch
    {
        "document.open" => "doc-image",
        "layer.add" => "layer-add",
        "layer.delete" => "layer-delete",
        "layer.duplicate" => "layer-duplicate",
        "layer.merge_down" => "layer-merge-down",
        "layer.move_up" => "arrow-up",
        "layer.move_down" => "arrow-down",
        "layer.move" => "layer-move",
        "layer.set_visible" => "layer-visible",
        "layer.properties" => "layer-properties",
        "selection.select_all" => "edit-select-all",
        "selection.deselect" => "cmd-deselect",
        "move.selection" => "tool-move-selection",
        "selection.rectangle" => "tool-rectangle-select",
        "selection.invert" => "edit-invert-selection",
        "selection.expand" => "step-selection",
        "selection.paste" => "edit-paste-replace",
        "selection.ellipse" => "tool-ellipse-select",
        "selection.lasso" => "tool-lasso",
        "selection.erase" => "edit-erase-selection",
        "selection.fill" => "edit-fill",
        "tool.stroke" => "tool-paintbrush",
        "Paintbrush" => "tool-paintbrush",
        "Eraser" => "tool-eraser",
        "Pencil" => "tool-pencil",
        "tool.magic_wand" => "tool-magic-wand",
        "tool.paint_bucket" => "tool-paint-bucket",
        "tool.text.finish" => "tool-text",
        "image.crop_to_selection" or "image.auto_crop" => "cmd-crop",
        "image.flip_horizontal" => "image-flip-horizontal",
        "image.flip_vertical" => "image-flip-vertical",
        "image.rotate_90_cw" => "image-rotate-right",
        "image.rotate_90_ccw" => "image-rotate-left",
        "image.rotate_180" => "image-rotate-180",
        "image.flatten" => "image-flatten",
        "image.resize" => "image-resize",
        "image.canvas_size" => "image-canvas-size",
        "image.assign_profile" or "image.convert_profile" => "image-color-profile",
        _ when command.StartsWith("effect.adjust.", StringComparison.Ordinal) || command == "image.invert_colors" => "step-adjustment",
        _ when command.StartsWith("effect.", StringComparison.Ordinal) => "step-effect",
        _ when command.StartsWith("move", StringComparison.Ordinal) || command.StartsWith("Move", StringComparison.Ordinal) => "tool-move-pixels",
        _ => "step-generic",
    };

    /// The engine labels a few steps with their command id. These show paint.net's names.
    public static string Title(string label) => label switch
    {
        "layer.add" => "Add New Layer",
        "layer.delete" => "Delete Layer",
        _ => label,
    };
}

/// A history row: the command icon on the left, and a redo arrow on an undone row.
public sealed class HistoryRow : PanelRow
{
    public bool Undone { get; }
    public IconView? RedoMark { get; }
    public IconView StepIcon { get; }

    /// Done, current or undone, as the Mac's accessibility value says it.
    public string StateWord => Undone ? "undone" : IsCurrent ? "current" : "done";

    public HistoryRow(Palette p, int index, string label, string command, bool undone, bool current)
        : base(p, index, HistoryStep.Title(label), null, Tokens.HistoryRowHeight, current, undone, 36, 24)
    {
        Undone = undone;
        StepIcon = new IconView(HistoryStep.Icon(command), undone ? p.Muted : p.Ink)
        {
            HorizontalAlignment = HorizontalAlignment.Left,
            VerticalAlignment = VerticalAlignment.Top,
            Margin = new Thickness(7, 3, 0, 0),
            IsHitTestVisible = false,
        };
        Children.Add(StepIcon);
        if (undone)
        {
            RedoMark = new IconView("cmd-redo", p.Muted, Tokens.SmallIconSize)
            {
                HorizontalAlignment = HorizontalAlignment.Right,
                VerticalAlignment = VerticalAlignment.Center,
                Margin = new Thickness(0, 0, 4, 0),
                IsHitTestVisible = false,
            };
            Children.Add(RedoMark);
        }
        AutomationProperties.SetItemStatus(this, StateWord);
    }
}

/// A text button with a small icon: History's Undo and Redo.
public sealed class TextCommand : Button
{
    readonly Palette _palette;
    readonly string _icon, _text;
    readonly bool _iconLast;

    protected override Type StyleKeyOverride => typeof(Button);

    public TextCommand(Palette palette, string icon, string text, string tooltip, bool iconLast)
    {
        _palette = palette;
        _icon = icon;
        _text = text;
        _iconLast = iconLast;
        Height = Tokens.ButtonHeight;
        MinWidth = Tokens.PanelTarget;
        MinHeight = 0;
        Padding = new Thickness(0);
        Background = Brushes.Transparent;
        BorderThickness = new Thickness(0);
        VerticalAlignment = VerticalAlignment.Center;
        Resources["ButtonBackgroundDisabled"] = Brushes.Transparent;
        Resources["ButtonBorderBrushDisabled"] = Brushes.Transparent;
        AutomationProperties.SetName(this, text);
        ToolTip.SetTip(this, tooltip);
        Redraw();
    }

    protected override void OnPropertyChanged(AvaloniaPropertyChangedEventArgs change)
    {
        base.OnPropertyChanged(change);
        if (change.Property == IsEnabledProperty) Redraw();
    }

    void Redraw()
    {
        var color = IsEnabled ? _palette.Ink : _palette.DisabledText;
        var row = Ui.Row(4);
        var icon = new IconView(_icon, color, Tokens.SmallIconSize);
        var label = Ui.Label(_text, color);
        if (!_iconLast) row.Children.Add(icon);
        row.Children.Add(label);
        if (_iconLast) row.Children.Add(icon);
        Content = row;
    }
}

public sealed class HistoryPanel : RowListPanel
{
    readonly PanelContext _context;

    public TextCommand UndoButton { get; }
    public TextCommand RedoButton { get; }
    public IReadOnlyList<HistoryRow> HistoryRows => Rows.OfType<HistoryRow>().ToList();

    public HistoryPanel(Palette p, PanelContext context) : base(p, Tokens.HistoryRowHeight)
    {
        _context = context;
        UndoButton = new TextCommand(p, "cmd-undo", "Undo", "Undo (Ctrl+Z)", iconLast: false) { HorizontalAlignment = HorizontalAlignment.Left };
        RedoButton = new TextCommand(p, "cmd-redo", "Redo", "Redo (Ctrl+Shift+Z)", iconLast: true) { HorizontalAlignment = HorizontalAlignment.Right };
        UndoButton.Click += (_, _) => Undo();
        RedoButton.Click += (_, _) => Redo();
        Actions.Children.Add(UndoButton);
        Actions.Children.Add(RedoButton);
        Reload();
    }

    public override void Reload()
    {
        if (WaitForRelease()) return;
        if (_context.Editor is not { } e)
        {
            SetRows([]);
            UndoButton.IsEnabled = false;
            RedoButton.IsEnabled = false;
            return;
        }
        var current = e.Info.HistoryIndex;
        var rows = new List<PanelRow>();
        for (var i = 0; i < e.Info.HistoryLabels.Count; i++)
        {
            var command = i < e.Info.HistoryCommands.Count ? e.Info.HistoryCommands[i] : "";
            var row = new HistoryRow(Look, i, e.Info.HistoryLabels[i], command, undone: i > current, current: i == current);
            row.Clicked += (index, _) => Jump(index);
            rows.Add(row);
        }
        SetRows(rows);
        UndoButton.IsEnabled = !_context.Busy && current > 0;
        RedoButton.IsEnabled = !_context.Busy && current + 1 < e.Info.HistoryLabels.Count;
        ScrollToCurrent();
    }

    public void Jump(int index)
    {
        if (_context.Busy || _context.Editor is not { } e || index == e.Info.HistoryIndex) return;
        e.Run("history.jump", Bridge.EngineParams.Of(("index", index)));
    }

    void Undo()
    {
        if (!_context.Busy) _context.Editor?.Run("edit.undo");
    }

    void Redo()
    {
        if (!_context.Busy) _context.Editor?.Run("edit.redo");
    }
}
