# Classic v2 build spec

Classic is the chosen OpenPainter layout. The HTML is a visual build reference for a native macOS editor. One CSS pixel in the mockup stands for one logical point in the app. macOS owns the real traffic lights, title editing state, menus, and utility panel frames. The app supplies the commands and content.

## Window and layout

The minimum main window is 1100 × 792 pt. The mock desktop menu is 28 pt, with a 6 pt window margin. In the native app, the system menu bar is outside the window. The native window has a plain title bar and no toolbar row (the mockup's 52 pt toolbar band is gone; the command buttons moved into the options row), so the native stage is taller by the difference; panel and fit rules use the actual stage. The mock traffic lights are 12 pt circles with 8 pt gaps; the native app uses system controls. The options row is 36 pt, the image strip 64 pt, and the status bar 28 pt. The stage fills the remaining height. At 1280 × 800, the stage is 582 pt high in the mockup.

The document's fit rectangle is the stage minus the space occupied by visible panels and 16 pt on each exposed image side. With all panels visible, the left reserve is 272 pt and the right reserve is 254 pt, measured from the stage edges. Center the fitted image in that clear rectangle. Preserve its aspect ratio. The sample is 1200 × 760 pixels and draws no wider than 820 pt. Fit recomputes after a resize, panel move, or visibility change. At a user-selected zoom, keep the chosen zoom and allow panning; opening a panel must not silently change it. If a moved panel enters the image's fit area, recompute the clear rectangle. Keep all panel frames clear of each other and of the window edge.

The default panel layout uses 12 pt clearance from the stage edges and at least 12 pt between panels on each side. Tools is top-left, Colors bottom-left, History top-right, and Layers bottom-right. The [Tools](https://www.getpaint.net/doc/latest/ToolsWindow.html), [Colors](https://www.getpaint.net/doc/latest/ColorsWindow.html), [History](https://www.getpaint.net/doc/latest/HistoryWindow.html), and [Layers](https://www.getpaint.net/doc/latest/LayersWindow.html) pages document floating, movable, resettable utility windows. They do not state numeric default corner positions or a Tools width, so these positions and widths are OpenPainter choices. The narrow Tools strip follows the shape shown in the Tools page. Each panel has a 24 pt header and a 24 × 24 pt close target on the left. The title is 13 pt semibold. The header is a drag target except over controls. Closing a panel clears its check mark in the More menu.

| Panel | Default size | Minimum size | Content and behavior |
| --- | --- | --- | --- |
| Tools | 84 × 292 pt | 84 × 94 pt | Two columns of 24 × 24 pt targets, 2 pt gaps. A 2 pt edge and dot mark Paintbrush. The tool list scrolls when short. |
| Colors | 244 × 246 pt | 244 × 246 pt compact | Opens with More » and 32 palette colors. More expands to at most 470 pt or the available height; the palette scrolls. When expanded, Tools shortens to 94 pt and scrolls, leaving a 12 pt gap. |
| History | 226 × 245 pt | 226 × 160 pt | 27 pt rows scroll. Current row has a 2 pt edge and diamond. Undone rows keep secondary text color and use a redo arrow. Undo and Redo have 24 pt targets. |
| Layers | 226 × 245 pt | 226 × 160 pt | 49 pt rows scroll. Active row has a 2 pt edge and diamond. Eye or eye-slash visibility targets are 24 × 24 pt. The action row uses at least 24 pt targets. |

At 1280 × 800 with all panels visible, Tools ends at stage y=304 and Colors begins at y=324; History ends at y=257 and Layers begins at y=324. The fitted sample lies between the left and right panels with at least 16 pt clearance. At the minimum window height, the left stack has 12 + 292 + 12 + 246 + 12 = 574 pt, exactly the stage height. Keep panel positions as user preferences. A reset command restores these defaults.

## Top controls

The title shows only the document name. macOS supplies its Edited state for unsaved changes. There is no appearance switch or shortcut legend in the title bar, and there is no toolbar row. The command buttons lead the options row, on the same line as the tool picker, and use `ink`, `line`, `accent`, and `wash` tokens. Command targets are 28 × 28 pt with 18 pt line drawings. Targets have 6 pt gaps and command groups have 12 pt gaps. Exactly seven buttons show, in this order: New, Open, Save | Undo, Redo | Crop to Selection, Magic Wand. Then a More target (`ellipsis.circle`) opens a menu with every other command from the old toolbar: Print | Cut, Copy, Paste | Deselect | Pixel Grid, Rulers (check marks) | Tools, History, Layers, Colors (check marks, F5–F8 shown). A 1 pt `line` separator and 12 pt gaps divide the commands from the tool options. Each button and More item sends the same action as its main-menu command and takes its enabled state from the same validation (Crop is off with no selection, Undo with nothing to undo, everything with a sheet up). New and Open are always on. Print is not in v1 and stays off. Magic Wand picks that tool, as its Tools button does, and shows the pressed state (filled wash, outline, 2 pt bottom stroke) while it is the current tool. Icons have text labels for accessibility and tooltips with shortcuts. The command buttons never scroll or clip; only the tool options after them scroll at narrow widths.

After the command buttons, the full-width options row has a 28 pt high picker containing an 18 pt tool icon, Paintbrush, and a chevron. Brush width has a numeric input and separate 28 × 28 pt minus and plus targets. Antialiasing is one 28 pt control. Blend mode and Opacity are 28 pt high fields. Labels are 12 pt and sit 8 pt from their control; groups have 16 pt gaps. At narrow sizes the options row scrolls horizontally without wrapping or clipping controls. The current tool determines which options appear.

The image strip is 64 pt high. Each image uses a 56 × 40 pt preview, with a single-line 12 pt name below. Thumbnails have 8 pt gaps. The active item has a 2 pt outline and a dot. A modified image has an asterisk in primary text color. A 24 × 24 pt close target appears on hover and keyboard focus. Long names truncate with a tooltip; the strip scrolls when images exceed its width. Clicking switches images, dragging reorders them, and closing a modified image invokes the save-changes flow. These image-list behaviors follow the [Main Window page](https://www.getpaint.net/doc/latest/MainWindow.html).

## Colors and state

Primary `#CF684D` and secondary `#E9E7DC` are document colors. Their bytes stay identical in light and dark appearance; only the swatch frame changes. The primary slot has a notch. The compact Colors body has 10 pt side padding, a 52 pt wheel, 32 pt primary and secondary swatches, and an editable 72 × 24 pt hex field. The color wheel has a ring marker. The compact Colors panel shows 32 swatches in 8 columns. More » shows all 96 in a scrolling 8-column grid. Each swatch is a 24 × 24 pt target with 2 pt gaps. The chosen palette swatch has a 2 pt outline and check mark. The default is a saturated OpenPainter palette modeled on paint.net's 96-color palette, not a claim of byte-for-byte identity. The [palette documentation](https://www.getpaint.net/doc/latest/WorkingWithPalettes.html) states that a palette text file holds one `aarrggbb` eight-digit hex color per line; `aa` is alpha. It says the compact Colors window shows the first 32 colors and the expanded window shows all 96. Import and export should use that format. Palette clicks change the active document color, while right-click changes the inactive color, as the [Colors page](https://www.getpaint.net/doc/latest/ColorsWindow.html) describes.

All interactive targets are at least 24 × 24 pt. Selected tools, tabs, history steps, and layers have a 2 pt edge plus a dot or diamond. Undone History steps remain available and use `muted` text plus a redo arrow. They do not use lowered opacity. Rows use 12 pt text; secondary text is at least 11 pt. Panel titles are 13 pt semibold. The dark panel border is `#8E98A3`, which has 5.23:1 contrast against the dark stage. Draw a visible keyboard focus ring with `accent` and expose selected, pressed, hidden, and modified states to VoiceOver.

## Status and shortcuts

The 28 pt status bar shows selection size, cursor position, image size, units, an editable zoom value, Fit and 100% buttons, and a 96 pt wide zoom slider whose interaction area is 28 pt high. Items have 10 pt gaps and use `ink` for values and `muted` for labels. Fields and buttons are at least 24 pt high. The displayed zoom must equal the actual image scale; update it when Fit recomputes. On a tight window, hide the short tool hint before removing any of these values. Units opens a menu.

The Window menu lists Tools F5 / Option-Command-1, History F6 / Option-Command-2, Layers F7 / Option-Command-3, and Colors F8 / Option-Command-4. Both sets toggle the same panel state. F5–F8 follow the [paint.net keyboard commands](https://www.getpaint.net/doc/latest/KeyboardMouseCommands.html). The Option-Command set works on Mac laptops whose F-keys control media. The Window menu and the More menu show the same check marks. The mockup accepts `?menu=window` to show this menu.

## Appearance tokens

Contrast ratios use WCAG relative luminance, normal text over an opaque surface. Document swatches and palette colors are content; do not theme them. The mockup uses the system sans font.

| Token | Light | Dark | Use |
| --- | --- | --- | --- |
| `desktop` | `#CBD0D5` | `#252932` | Mock desktop only |
| `chrome` | `#F5F5F6` | `#353A42` | Toolbar, strip, status |
| `options` (`chrome2`) | `#E9EBED` | `#2E333B` | Options and panel headers |
| `panel` | `#F9F9FA` | `#363B43` | Opaque panel body |
| `field` | `#FFFFFF` | `#262B32` | Inputs and menu fields |
| `ink` | `#202329` | `#EEF0F3` | Primary text |
| `muted` | `#5C626C` | `#BFC5CE` | Secondary text |
| `line` | `#CDD1D6` | `#515862` | Internal dividers |
| `panel-line` | `#9DA5AD` | `#8E98A3` | Panel edge |
| `stage` | `#777777` | `#252525` | Area around the document |
| `accent` (`blue`) | `#1973C8` | `#76B8FF` | Focus and selection edge |
| `wash` (`blueWash`) | `#DCEcff` | `#244967` | Selected fill |
| `on-accent` | `#101D2B` | `#101D2B` | Antialiasing check |

Primary text is 14.45:1 / 10.02:1 on chrome and 14.96:1 / 9.87:1 on panels, light / dark. Secondary text is 5.64:1 / 6.59:1 on chrome and 5.84:1 / 6.49:1 on panels. The dark check ink is 8.17:1 on its blue fill. Selection edges and icons are backed by shape markers, so color alone does not carry state. Native disabled items may use the system disabled contrast treatment.

## Choices to check

- Verify the default panel corners and the narrow Tools width against a running paint.net build. Its cited window pages allow reset but do not give numeric defaults.
- Decide whether moved panels should snap to the 12 pt edge grid. The mockup shows default positions only.
- Decide whether panels should fade when the pointer leaves. The cited window pages do not describe panel fading, so it is a later option.
- Confirm how the Mac system Edited state appears under each supported macOS version with no toolbar.
- Check long translated labels, VoiceOver order, keyboard focus, image-strip drag and scroll, and native utility-panel behavior in the app. The HTML mockup does not implement editing commands or panel dragging.
