# W6: Windows look and icons

W6 draws one Windows version of the Mac window with Avalonia 12 and gives every Mac icon a Windows icon with a license record. The Mac app is the reference. This file lists each change from the Mac and why, then each place where the picture still differs, then what this picture leaves for W7 and W8.

The builder wrote this file. Root adds the critique findings and decisions.

All pictures come from Avalonia's headless platform with the Skia renderer. No real window has been checked.

## How to compare

The renders have the same size and window position as the Mac shots. The picture is 1376 by 928 layout pixels at 2x, the window is 1280 by 832, and the margin of 48 is transparent. `colors-expanded` is 616 by 1068 pixels, the size of the Mac's panel shot. The panels, the canvas and the status bar sit at the Mac's positions, to within about 1 layout pixel by eye. I did not measure that with a tool. A test checks that each panel and bar is present at its place in every scene.

## Changes the brief asked for

| Change | Why |
| --- | --- |
| Caption buttons on the right, in place of the three dots | Windows puts minimize, maximize and close there. Each is 46 by 32, the Windows 11 size. |
| Menu bar inside the window, in the Mac's order: File, Edit, View, Image, Layers, Adjustments, Effects, Window, Help | Windows has no system menu bar. |
| Segoe UI in place of the Mac font | It is the Windows system font. |
| Ctrl and Alt in place of Command and Option | Windows keyboards have these keys. The visible cases are the Ellipse Select hint and the Curves hint. |
| OK before Cancel, with OK filled in the accent color | This is the Windows order and its way of marking the default button. |
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
| The font size is a combo box with an arrow, as on the Mac | It uses a field fill so it reads as editable. |
| Pop-up buttons show one small down chevron, not an up and down pair | This is Fluent's ComboBox and the Windows habit. Its arrow is 8 pixels, in proportion to 12 pixel text. |
| Dialogs have a title row with the effect's name and an X | The Mac's panel sets a title (`EffectDialog.swift:88`), and a Windows dialog shows it in a row that drags the window. The row adds 32 pixels, so each dialog starts 16 higher than on the Mac and ends 16 lower. |
| Window title and panel titles use the `muted` color | The Mac draws them in BFBFBF on white, which measures 1.8 to 1. The `muted` color measures 6.1 to 1 in light and 9.6 to 1 in dark. |
| All small text is 12 pixels, not 11 | Panel titles and the Curves hint were hard to read at 11 in Segoe UI. |
| Selected rows and strip names are bold | Segoe UI's semibold looks lighter than the Mac's semibold. |
| Corners of 8 on windows, panels and dialogs, and 4 on controls | These are the Windows 11 values. The Mac shots use about 10 and 5. |
| Slider tracks are thin, with a round white knob and a darker ring | This is close to Fluent's Slider. The Mac's track is thicker, and its knob is a wide pill with a shadow. A plain white knob nearly vanished on the panel, so the ring carries the edge, at 3.6 to 1 in light. |
| Buttons and pop-ups have a 1 pixel edge, and text fields have a stronger bottom line | Windows 11 draws controls this way. The tokens are `ControlEdge` and `FieldBottom`. |
| Panels and dialogs have a 1 pixel outline and a line under the title | Dark panels merged into the stage without them. |
| Off text in lists uses `DisabledText`, not `Disabled` | History's Redo must stay readable, at 4.5 to 1 or more. Faint toolbar icons keep `Disabled`. |
| The selected-tool dot sits outside the icon box | On the Mac it can overlap the icon, because SF Symbols draw smaller. `--magic-wand-check` draws the wand selected, and a test checks it. |
| A button with a double chevron at the end of the options row, in the Brush and Text scenes | The row clips there. The Mac scrolls sideways with no sign that there is more. **W7 must make this button open the rest of the options, and must also let the row scroll sideways.** |
| Control heights: options fields 26, dialog buttons 26, plain buttons and fields 24 | The Mac's frames are 28 and 24, but it draws inside them with space around. These numbers match what the Mac draws, closely enough by eye. |
| Icons draw at 20 layout pixels nearly everywhere | SF Symbols at 13 to 18 points have about the same ink as a Fluent 20 px icon, which has about 2 pixels of space inside it. |
| Smaller sizes: 16 for History's Undo and Redo, for the brush width minus and plus, for the dialog X and for the options button; 12 for the panel close X | At 20 these looked too large next to the Mac's smaller symbols. The art is still the 20 px file, so at 100% display scale they may look slightly soft. |

## Icon record

`docs/windows/icons.toml` has one row per use in the Mac app, not one per Mac symbol. The same Mac symbol can map to different Windows files. For example, the Mac's `drop` is the Paint Bucket tool and also Gaussian Blur. Here the tool shows a bucket and the effect shows a drop.

Each row records one meaning and the SHA-256 of its file. `windows/assets/icons/fluent-used.txt` lists every Fluent icon in use, with the hash of the unchanged package file. The tests fail on replaced art, a fake source name, a use with no row, and a scene that draws a different icon from the record.

## Colors that do not come from Theme.swift

The tokens in `tools/ui-look/app/src/Tokens.cs` copy `Theme.swift`, except for these, which macOS draws itself or which the shots show differently. I read the first group from pixels in the Mac shots.

| Token | Value | Why |
| --- | --- | --- |
| `Stage` | FFFFFF light, 1E1E1E dark | `Theme.swift` says 777777 and 252525, but the shots show these. The shots win, because they are the app as it runs. Root may want to ask why they differ. |
| `TitleBar` | FFFFFF light, 1E1E1E dark | macOS draws the title bar. |
| `Field` in dark | 171717 | `Theme.swift` says 262B32, but text fields in the shot draw 171717. |
| `Plot` | FFFFFF light, 262B32 dark | The Curves graph in the shot uses the `Theme.swift` field value. |
| `Control` | DDDFE1 light, 41464D dark | The fill of pop-up buttons and plain buttons, close to the shots' 2 samples. |
| `Disabled` | A3A8AF light, 7B818B dark | Picked by eye against the shots' dimmed commands. |
| `Handle` | 0A84FF | The Text tool's resize handle. |

These are Windows additions, each with a contrast test in `Tokens.ContrastPairs`:

| Token | Light | Dark | Checked against |
| --- | --- | --- | --- |
| `KnobRing` | 7C848E | 9AA3AE | Panel, at least 3 to 1 |
| `SliderValue` | 848C95 | B4BBC4 | Panel, at least 3 to 1 |
| `DisabledText` | 6B717A | B0B6BF | Panel, at least 4.5 to 1 |
| `OnAccent` | FFFFFF | 101D2B | Accent, at least 4.5 to 1 |
| `ControlEdge` | B9BEC4 | 737A85 | Panel, at least 1.6 to 1 |
| `FieldBottom` | 7C848E | A3ABB5 | Field, at least 3 to 1 |
| `PanelEdge` | CDD1D6 | 5F6671 | Stage, at least 1.45 to 1 |

## Where the picture still differs from the Mac

| Place | Difference | Why |
| --- | --- | --- |
| Canvas | Colors and shapes differ a little, most in light mode, where the Mac's blues are stronger | The brief stretches the 512 by 384 fixture. The Mac shot shows a 4096 by 3072 image. |
| Canvas in Curves and Hue / Saturation | The tint is close but not equal | The Mac shows the engine's live preview. This tool has no engine, so `Renderer.CanvasBitmap` uses plain math in its place. It is not the effect's real result. |
| Canvas text | Letter shapes differ | Segoe UI bold in place of Avenir Next. I set the size to 36.2 so the width matches the Mac's. |
| All text | Words are a little narrower | Segoe UI is narrower than the Mac font at the same size. Items that flow shift, and the status bar's "Selection" starts about 27 layout pixels further right. |
| Options row | The Mac cuts off "Opac" and "Blend". Windows cuts off "Op" in the Brush row and leaves one letter of "Blend" in the Text row, before its double chevron | The chevron takes 30 pixels from the row. W7 replaces the clipped word when the row is real. |
| Selection scene status hint | The Mac cuts its hint short with an ellipsis, and Windows shows all of it | Segoe UI is narrower. |
| Hex field and the Hue field | The Mac shows the text as selected, and these renders do not | Avalonia draws a selection only in a focused field, and these scenes give nothing focus. W7 gets this from real focus. |
| Every icon | Different drawings | Fluent icons in place of SF Symbols, by rule. The largest visible changes: Text shows two letters A, Save is a floppy disk, Exit is an arrow out of a door, and the History rows' adjustment icon fills its bottom half, not its left half. |
| Dashed outlines | The dash and gap are 4 layout pixels, measured from the Mac shot, but the phase differs | The Mac's overlay starts its dashes from another point. W5's engine overlay draws the real one. |
| Dialogs | Each is 32 pixels taller than on the Mac | The new title row. |
| Shadows | Window and panel shadows are close, not equal | Avalonia's box shadow is simpler than macOS's. |
| Status bar and layer text | The Mac's values look slightly heavier | The same font weight draws differently in each font. |

## Critique

Root wrote this section. The done-test asks for 2 critiques, and only 1 has run.

- **Astra: NOT RUN.** Astra uses the same Codex usage limit as Sol, and the limit runs until 2026-11-04 21:32. Root reruns it after that, against the fixed renders. Until then this done-test item is open.
- **Fresh Opus reviewer: done, 595 s, 27 findings.** It did not see this file. Root checked the main claims against the code and the icon package.

| Finding | Decision |
| --- | --- |
| Lasso and Ellipse Select look alike; Rectangle Select lost its dashes; Paintbrush used the Recolor shape | Fixed with new icons |
| Paint Bucket, Relief, Straighten, Save As, Zoom to Selection, Pixel Grid, Copy Selection, Exit and Duplicate Layer did not carry their meaning | Fixed with new icons |
| Icon tests passed with replaced art, a fake source name, and 2 unlisted symbols | Fixed. Each fault now has a fixture that must fail. Root also replaced the Pencil art by hand, and the tests failed by name |
| One row per Mac symbol locked unrelated uses to one icon | Fixed. Rows are now by use: 240 rows |
| Dialogs had no title or close, and OK looked like Cancel | Fixed in the picture. Dragging is W7 |
| Light slider knob looked disabled; History's disabled text was faint; control edges and panel edges were faint | Fixed, with a contrast test |
| The text size field lost its list; selected-tool dot sat in the icon box; 11 px text; dashes read as dots; weak bold | Fixed |
| The license record was thin | Fixed: commit, hashes, and a W19 duty to ship the notice |
| A render with a missing panel passed; a scene could swap Lasso and Pencil | Fixed, with fixtures that must fail |
| The options row clips controls with no way to reach them | Drawn as a "more" button. W7 makes it work. The clipped "Op" beside it stays until then |
| Caption buttons, Alt and access keys, focus states, 100% scale, panels leaving the window, Ctrl+Y | Moved to W7 and W8, listed below |
| Some effect icons look alike, Merge Down looks like Download, the fixed accent color, Segoe UI Variable, the app name in the title | Not changed. Menu text carries the effect names, and Merge Down has the Mac's shape. W7 decides the title and the font |

Checks at the end of the fix round, run by root: `dotnet test tools/ui-look/tests` gives 42 passed, 0 failed, and all 12 renders were opened at full size. 217 icon rows come from Fluent (156 files) and 23 are own work (15 files).

## For W7 and W8

This picture does not settle these decisions.

1. **Real caption behavior.** Snap Layouts on the maximize button, the red hover on close, and the inactive title tint all need the real system caption or careful copies. A picture cannot show them.
2. **Alt and the menus.** Decide what Alt does after canvas use, because Alt is also a tool modifier here, and how menu access keys work.
3. **Hover, press and focus.** Draw a scene for these before W8. Every scene here shows the resting state only.
4. **Sharp icons at 100% scale.** Put icons on whole pixels. Use 16 px art where an icon draws at 16, not the 20 px art scaled down. Check both at 100% scale in a real window.
5. **Panels outside the window.** The Mac's panels are separate windows, and here they are drawn inside. Decide whether they can leave the window.
6. **Ctrl+Y.** Windows users expect it for Redo. The Mac uses Shift+Command+Z. Decide whether to accept both.
7. **The options row.** W7 must make the double chevron open the rest of the options, and must also let the row scroll sideways.

## Not checked

- A real window. Everything here comes from the headless renderer, so text may look different with Windows' own text drawing.
- Display scales other than 2x, in particular 100%.
- Hover, pressed and focus states.
- High contrast mode.
