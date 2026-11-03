// The title row: the menus, the title and the caption buttons, in one 32 pixel row.
//
// The row's background has the title bar role, so Windows moves the window from
// it and snaps it on a double click. Each top menu has the user role, so it gets
// clicks. Everything else in the row passes through to the background.
using Avalonia;
using Avalonia.Automation;
using Avalonia.Controls;
using Avalonia.Controls.Chrome;
using Avalonia.Controls.Primitives;
using Avalonia.Controls.Templates;
using Avalonia.Input;
using Avalonia.Layout;
using Avalonia.Media;
using OpenPainter.Catalog;
using UiLook;

namespace OpenPainter.Shell;

public sealed class TitleRow : Grid
{
    readonly Palette _palette;
    readonly TextBlock _title;
    readonly Border _dirtyDot;

    public TitleMenu Menus { get; }
    public CaptionButton MinimizeButton { get; }
    public CaptionButton MaximizeButton { get; }
    public CaptionButton CloseButton { get; }
    public string TitleText => _title.Text ?? "";
    public bool IsModifiedShown => _dirtyDot.IsVisible;
    public IBrush? TitleBrush => _title.Foreground;

    public TitleRow(Palette palette, IReadOnlyList<MenuEntry> menus, IReadOnlyDictionary<string, Action> actions)
    {
        _palette = palette;
        Name = "TitleRow";
        Height = Tokens.TitleHeight;
        Background = Ui.B(palette.TitleBar);
        ColumnDefinitions = new ColumnDefinitions("Auto,*,Auto");
        WindowDecorationProperties.SetElementRole(this, WindowDecorationsElementRole.TitleBar);

        Menus = (TitleMenu)MenuBuilder.Build(menus, palette, actions);
        Menus.VerticalAlignment = VerticalAlignment.Center;
        Menus.Margin = new Thickness(6, 0, 0, 0);
        Children.Add(Menus);

        _title = Ui.Label("", palette.Muted, weight: FontWeight.SemiBold);
        _dirtyDot = new Border
        {
            Width = 6,
            Height = 6,
            CornerRadius = new CornerRadius(3),
            Background = Ui.B(palette.Muted),
            VerticalAlignment = VerticalAlignment.Center,
            IsVisible = false,
        };
        var title = Ui.Row(6);
        title.Name = "TitleText";
        title.HorizontalAlignment = HorizontalAlignment.Center;
        title.IsHitTestVisible = false;
        title.Children.Add(_dirtyDot);
        title.Children.Add(_title);
        SetColumn(title, 1);
        Children.Add(title);

        MinimizeButton = new CaptionButton(CaptionKind.Minimize, palette);
        MaximizeButton = new CaptionButton(CaptionKind.Maximize, palette);
        CloseButton = new CaptionButton(CaptionKind.Close, palette);
        var caption = new StackPanel { Orientation = Orientation.Horizontal, Name = "CaptionButtons" };
        caption.Children.Add(MinimizeButton);
        caption.Children.Add(MaximizeButton);
        caption.Children.Add(CloseButton);
        SetColumn(caption, 2);
        Children.Add(caption);
    }

    /// The text of the title row. W6 decision: it includes the app name.
    public static string TitleFor(string? document) =>
        string.IsNullOrEmpty(document) ? MainWindow.AppName : $"{document} - {MainWindow.AppName}";

    public void SetTitle(string text, bool dirty)
    {
        _title.Text = text;
        _dirtyDot.IsVisible = dirty;
        AutomationProperties.SetName(_title, text);
    }

    /// An inactive window draws its title and glyphs fainter, as Windows 11 does.
    public void SetActive(bool active)
    {
        var text = active ? _palette.Muted : _palette.Disabled;
        _title.Foreground = Ui.B(text);
        _dirtyDot.Background = Ui.B(text);
        foreach (var item in Menus.Items.OfType<MenuItem>()) item.Foreground = Ui.B(active ? _palette.Ink : _palette.Muted);
        MinimizeButton.Active = active;
        MaximizeButton.Active = active;
        CloseButton.Active = active;
    }
}

/// A menu item that shows its shortcut in the catalog's words. Avalonia's own text for a
/// gesture names the Shift glyph of some keys, so it would print Ctrl+" for the ' key.
public sealed class CatalogMenuItem : MenuItem
{
    public const string GesturePart = "PART_InputGestureText";
    public const string IconPart = "PART_IconPresenter";

    protected override Type StyleKeyOverride => typeof(MenuItem);

    public MenuEntry Entry { get; }
    public CatalogMenuItem(MenuEntry entry) => Entry = entry;

    protected override void OnApplyTemplate(TemplateAppliedEventArgs e)
    {
        base.OnApplyTemplate(e);
        // Fluent's icon box is 16, which would scale the 20 pixel art down and blur it.
        var box = e.NameScope.Find<Control>(IconPart)
            ?? throw new InvalidOperationException($"MENU: Fluent's MenuItem template has no {IconPart}");
        box.Width = Tokens.IconSize;
        box.Height = Tokens.IconSize;
        if (Entry.Key is null) return;
        var text = e.NameScope.Find<TextBlock>(GesturePart)
            ?? throw new InvalidOperationException($"MENU: Fluent's MenuItem template has no {GesturePart}");
        text.Text = Entry.Key;
    }
}

/// The menu bar. Avalonia opens a window's main menu when Alt comes up, even after an Alt-click,
/// and `BlockOpen` is how the window's one answer about Alt stops that.
public sealed class TitleMenu : Menu
{
    protected override Type StyleKeyOverride => typeof(Menu);

    public Func<bool>? BlockOpen { get; set; }

    public override void Open()
    {
        if (BlockOpen?.Invoke() == true) return;
        base.Open();
    }
}

/// Makes the real menu controls from the Windows menus.
public static class MenuBuilder
{
    /// Items with an entry in `actions` work. All others are present and off.
    public static Menu Build(IReadOnlyList<MenuEntry> menus, Palette palette, IReadOnlyDictionary<string, Action> actions)
    {
        var bar = new TitleMenu { Name = "Menus", Background = Brushes.Transparent };
        // Windows reaches a menu bar with Alt or F10, not Tab: docs/windows/shortcuts-windows.md.
        KeyboardNavigation.SetIsTabStop(bar, false);
        foreach (var menu in menus)
        {
            var top = new MenuItem
            {
                Header = menu.Title,
                Name = "Menu" + menu.Title,
                FontSize = Tokens.FontBody,
                Foreground = Ui.B(palette.Ink),
                Padding = new Thickness(8, 5),
                MinHeight = 0,
                // AccessText underlines the letter while Alt is down. Header stays the plain word.
                HeaderTemplate = new FuncDataTemplate<string>((title, _) => new AccessText { Text = MenuLetters.Marked(title) }),
            };
            WindowDecorationProperties.SetElementRole(top, WindowDecorationsElementRole.User);
            KeyboardNavigation.SetIsTabStop(top, false);
            Fill(top.Items, menu.Items, palette, actions);
            top.SubmenuOpened += LoadOnce(top, palette);
            bar.Items.Add(top);
        }
        return bar;
    }

    /// Icons load the first time a menu opens, so they cost nothing at startup.
    static EventHandler<Avalonia.Interactivity.RoutedEventArgs> LoadOnce(MenuItem parent, Palette palette)
    {
        var loaded = false;
        return (_, e) =>
        {
            if (loaded || !ReferenceEquals(e.Source, parent)) return;
            loaded = true;
            LoadIcons(parent, palette);
        };
    }

    /// An item works when the actions name its path, or its action.
    public static bool TryAction(MenuEntry entry, IReadOnlyDictionary<string, Action> actions, out Action action) =>
        actions.TryGetValue(entry.Path, out action!) || actions.TryGetValue(entry.Action, out action!);

    public static void Fill(ItemCollection parent, IReadOnlyList<MenuEntry> entries, Palette palette, IReadOnlyDictionary<string, Action> actions)
    {
        foreach (var entry in entries)
        {
            if (entry.Kind == EntryKind.Separator)
            {
                parent.Add(new Separator());
                continue;
            }
            var item = new CatalogMenuItem(entry) { Header = entry.Title, FontSize = Tokens.FontBody };
            AutomationProperties.SetName(item, entry.Title);
            if (entry.Key is not null) item.InputGesture = WindowsKeys.Gesture(entry.Key);
            if (entry.Kind == EntryKind.Menu)
            {
                Fill(item.Items, entry.Items, palette, actions);
                item.SubmenuOpened += LoadOnce(item, palette);
            }
            else if (TryAction(entry, actions, out var action))
            {
                item.Click += (_, _) => action();
                item.PropertyChanged += (_, e) =>
                {
                    // An icon loaded while the item was on must not stay dark when it goes off.
                    if (e.Property == InputElement.IsEnabledProperty && item is { Entry.Icon: { } icon, Icon: not null })
                        item.Icon = new IconView(icon, item.IsEnabled ? palette.Ink : palette.Disabled);
                };
                if (entry.Action.StartsWith(PanelActionPrefix, StringComparison.Ordinal)) item.ToggleType = MenuItemToggleType.CheckBox;
            }
            else
            {
                item.IsEnabled = false;
            }
            parent.Add(item);
        }
    }

    public const string PanelActionPrefix = "panel:";

    public static void LoadIcons(MenuItem parent, Palette palette) => LoadIcons(parent.Items.OfType<MenuItem>(), palette);

    /// At 20, because 48 of the icon files have no 16 pixel art: 33 Fluent files and the 15 own-work files.
    public static void LoadIcons(IEnumerable<MenuItem> items, Palette palette)
    {
        foreach (var item in items)
            if (item is CatalogMenuItem { Entry.Icon: not null } catalogItem)
                item.Icon = new IconView(catalogItem.Entry.Icon, item.IsEnabled ? palette.Ink : palette.Disabled);
    }
}
