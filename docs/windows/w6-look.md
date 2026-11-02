# W6: Windows look and icons

W6 draws one Windows version of the Mac window with Avalonia 12 and gives every Mac icon a Windows icon with a license record. The Mac app is the reference. This file lists each change from the Mac and why, then each place where the picture still differs.

The builder wrote this file. Root adds the critique findings and decisions.

All pictures come from Avalonia's headless platform with the Skia renderer. No real window has been checked.

## How to compare

The renders have the same size and window position as the Mac shots. The picture is 1376 by 928 layout pixels at 2x, the window is 1280 by 832, and the margin of 48 is transparent. `colors-expanded` is 616 by 1068 pixels, the size of the Mac's panel shot. The panels, the canvas, the status bar and the dialogs sit at the Mac's positions, to within about 1 layout pixel by eye. I did not measure that with a tool.

## Changes the brief asked for

| Change | Why |
| --- | --- |
| Caption buttons on the right, in place of the three dots | Windows puts minimize, maximize and close there. Each is 46 by 32, the Windows 11 size. |
| Menu bar inside the window, in the Mac's order: File, Edit, View, Image, Layers, Adjustments, Effects, Window, Help | Windows has no system menu bar. |
| Segoe UI in place of the Mac font | It is the Windows system font. |
| Ctrl and Alt in place of Command and Option | Windows keyboards have these keys. The visible cases are the Ellipse Select hint and the Curves hint. |
| OK before Cancel | This is the Windows order. The two buttons swap places and keep the Mac's positions. |
| Panel close on the right, as a small X | Windows puts close on the right. The Mac's grey dot would mean nothing here. |
| The Mac's colors kept | See the next section for the exceptions, each with its reason. |

## Other changes I found I needed

| Change | Why |
| --- | --- |
| The menu bar shares the title row, so the title row holds menus, title and caption buttons | A separate row would push every panel and the canvas down by about 28 pixels. This way everything below stays at the Mac's position. Visual Studio Code uses the same layout. Root or the user may prefer a separate row. |
| The Mac's OpenPainter menu is gone | Windows apps have no app menu. W7 places its items: About under Help, Settings and Exit under File or Edit. |
| The title is centered, with no document icon | Windows has no document icon in the title bar. The Mac's left side now holds the menus. |
| A small dot before the title means unsaved changes | The Mac shows this as a dot inside its close button. The Windows close button cannot show it. |
| The font pop-up says Segoe UI, not Avenir Next | Avenir Next is not installed on Windows, and the canvas text is Segoe UI bold as the brief says. |
| Pop-up buttons show one down chevron, not an up and down pair | This is Fluent's ComboBox and the Windows habit. |
| Window title and panel titles use the `muted` color | The Mac draws them in BFBFBF on white, which measures 1.8 to 1. The `muted` color measures 6.1 to 1 in light and 9.6 to 1 in dark. |
| Corners of 8 on windows, panels and dialogs, and 4 on controls | These are the Windows 11 values. The Mac shots use about 10 and 5. |
| Slider tracks are thin, with a round knob | This is Fluent's Slider. The Mac's track is thicker and its knob is a wide pill. |
| The slider knob in light mode is grey, 9DA5AD | The Mac's white knob stands out only through its shadow, and Fluent's knob has no shadow. A white one nearly vanished on the panel. Grey measures 2.4 to 1 against the panel. Dark mode keeps white. |
| Control heights: options fields 26, dialog buttons 26, plain buttons and fields 24 | The Mac's frames are 28 and 24, but it draws inside them with space around. These numbers match what the Mac draws, closely enough by eye. |
| Icons draw at 20 layout pixels nearly everywhere | SF Symbols at 13 to 18 points have about the same ink as a Fluent 20 px icon, which has about 2 pixels of space inside it. |
| Three smaller sizes: 16 for History's Undo and Redo and for the brush width minus and plus, 12 for the panel close X | At 20 these looked too large next to the Mac's smaller symbols. The art is still the 20 px file, so at 100% display scale they may look slightly soft. A real window check should look at this. |

## Colors that do not come from Theme.swift

The tokens in `tools/ui-look/app/src/Tokens.cs` copy `Theme.swift`, except for these, which macOS draws itself or which the shots show differently. I read them from pixels in the Mac shots.

| Token | Value | Why |
| --- | --- | --- |
| `Stage` | FFFFFF light, 1E1E1E dark | `Theme.swift` says 777777 and 252525, but the shots show these. The shots win, because they are the app as it runs. Root may want to ask why they differ. |
| `TitleBar` | FFFFFF light, 1E1E1E dark | macOS draws the title bar. |
| `Field` in dark | 171717 | `Theme.swift` says 262B32, but text fields in the shot draw 171717. |
| `Plot` | FFFFFF light, 262B32 dark | The Curves graph in the shot uses the `Theme.swift` field value. |
| `Control` | DDDFE1 light, 41464D dark | The fill of pop-up buttons and plain buttons, close to the shots' 2 samples. |
| `Disabled` | A3A8AF light, 7B818B dark | Picked by eye against the shots' dimmed commands. |
| `Handle` | 0A84FF | The Text tool's resize handle. |

## Where the picture still differs from the Mac

| Place | Difference | Why |
| --- | --- | --- |
| Canvas | Colors and shapes differ a little, most in light mode, where the Mac's blues are stronger | The brief stretches the 512 by 384 fixture. The Mac shot shows a 4096 by 3072 image. |
| Canvas in Curves and Hue / Saturation | The tint is close but not equal | The Mac shows the engine's live preview. This tool has no engine, so `Renderer.CanvasBitmap` uses plain math in its place. It is not the effect's real result. |
| Canvas text | Letter shapes differ | Segoe UI bold in place of Avenir Next. I set the size to 36.2 so the width matches the Mac's. |
| All text | Words are a little narrower | Segoe UI is narrower than the Mac font at the same size. Items that flow shift: "Opacity" starts about 16 layout pixels further left, and the status bar's "Selection" starts about 27 further right. |
| Options row | The Mac cuts off "Opac", while Windows shows all of "Opacity" | The same cause. The row still clips at the window edge, and "Blend" is cut off in the text scene. |
| Selection scene status hint | The Mac cuts its hint short with an ellipsis, and Windows shows all of it | The same cause. |
| Hex field and the Hue field | The Mac shows the text as selected, and these renders do not | Avalonia draws a selection only in a focused field, and these scenes give nothing focus. W7 gets this from real focus. |
| Every icon | Different drawings | Fluent icons in place of SF Symbols, by rule. The largest visible changes: the Paintbrush is a wall brush, Text shows two letters A, Recolor has a sparkle, Save is a floppy disk, and Move Selection is my own drawing of four arrows. |
| Half-filled circle in Curves and Hue / Saturation rows | Mine looks larger than the Mac's | It is my own 16 pixel disc in a 20 pixel box, wider than the Mac's symbol at that size. |
| Dashed outlines | The dash length is 2 and 2, and the Mac's looks slightly longer | I matched it by eye. W5's engine overlay draws the real one. |
| Dialogs | No title bar, as on the Mac | The Mac's dialog is a sheet with no title. A Windows dialog usually has one, with the effect's name and a close button. I kept the Mac's look because the brief says to, and W7 should decide. |
| Shadows | Window and panel shadows are close, not equal | Avalonia's box shadow is simpler than macOS's. |
| Status bar and layer text | The Mac's values look slightly heavier | The same font weight draws differently in each font. |

## Not checked

- A real window. Everything here comes from the headless renderer, so text may look different with Windows' own text drawing.
- Display scales other than 2x, in particular 100%.
- Hover, pressed and focus states. The scenes show only the resting state.
- High contrast mode.

## Findings from the critiques

Root adds these.
