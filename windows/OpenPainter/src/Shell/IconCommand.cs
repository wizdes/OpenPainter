// A real button that draws W6's icon target. Ui.IconButton draws the look: the icon, and
// the pressed wash when `On`. Fluent draws hover and press until W8 settles those states.
using Avalonia;
using Avalonia.Automation;
using Avalonia.Controls;
using Avalonia.Media;
using UiLook;

namespace OpenPainter.Shell;

public sealed class IconCommand : Button
{
    readonly Palette _palette;
    readonly string _icon;
    readonly double _target, _iconSize;
    readonly Color? _ink;
    bool _on;

    protected override Type StyleKeyOverride => typeof(Button);

    public IconCommand(Palette palette, string icon, string name, string? tooltip = null,
        double target = Tokens.CommandTarget, double iconSize = Tokens.IconSize, bool on = false, bool enabled = true, Color? ink = null)
    {
        _palette = palette;
        _ink = ink;
        _icon = icon;
        _target = target;
        _iconSize = iconSize;
        _on = on;
        Width = target;
        Height = target;
        MinWidth = 0;
        MinHeight = 0;
        Padding = new Thickness(0);
        Background = Brushes.Transparent;
        BorderThickness = new Thickness(0);
        CornerRadius = new CornerRadius(Tokens.ControlCorner);
        VerticalAlignment = Avalonia.Layout.VerticalAlignment.Center;
        Resources["ButtonBackgroundDisabled"] = Brushes.Transparent;
        Resources["ButtonBorderBrushDisabled"] = Brushes.Transparent;
        AutomationProperties.SetName(this, name);
        ToolTip.SetTip(this, tooltip ?? name);
        IsEnabled = enabled;
        Redraw();
    }

    public string Icon => _icon;

    public bool On
    {
        get => _on;
        set
        {
            if (_on == value) return;
            _on = value;
            Redraw();
        }
    }

    protected override void OnPropertyChanged(AvaloniaPropertyChangedEventArgs change)
    {
        base.OnPropertyChanged(change);
        if (change.Property == IsEnabledProperty) Redraw();
    }

    void Redraw() => Content = Ui.IconButton(_palette, _icon, IsEnabled ? _ink ?? _palette.Ink : _palette.Disabled, _target, _iconSize, _on);
}
