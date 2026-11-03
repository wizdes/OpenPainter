# Windows shortcuts

The Mac's keys work on Windows with Ctrl for Command and Alt for Option. `ShortcutTableTests` reads this file. A difference that is not in the first table fails by name. A row that no longer differs fails by name too. There are 0 unexplained differences.

The Mac's key lists are `windows/tests/vectors/shortcut-words.json` (menus) and `docs/shortcuts.md` (canvas, panels and dialogs).

## Differences from the Mac

| Where | Mac key | Ctrl for Command gives | Windows key | Reason |
| --- | --- | --- | --- | --- |
| OpenPainter > Hide OpenPainter | ⌘H | Ctrl+H | none | Windows apps have no hide command, so the item is gone. |
| OpenPainter > Hide Others | ⌥⌘H | Ctrl+Alt+H | none | Windows apps have no hide command, so the item is gone. |
| OpenPainter > Quit OpenPainter | ⌘Q | Ctrl+Q | Alt+F4 | File > Exit takes the Windows key. Ctrl+Q does nothing, as in paint.net. |
| Edit > Redo | ⇧⌘Z | Ctrl+Shift+Z | Ctrl+Shift+Z, Ctrl+Y | Windows users expect Ctrl+Y, and the Mac key stays. The menu shows the Mac key. |
| Edit > Erase Selection | ⌫ | ⌫ | Delete | The Mac calls its Backspace key Delete. Windows calls this command's key Delete, as paint.net does. |
| Edit > Fill Selection | ⇧⌫ | Shift+⌫ | Shift+Delete | Same key rule as Erase Selection. paint.net uses Backspace, and the Mac decides the key here. |
| View > Zoom In | ⌘= | Ctrl+= | Ctrl++ | Windows names the = key by its plus sign, as paint.net does. Ctrl+= and Ctrl+Shift+= both work, and so does the number pad plus. |
| Layers > Delete Layer | ⇧⌘⌫ | Ctrl+Shift+⌫ | Ctrl+Shift+Delete | Same key rule as Erase Selection. |
| Window > Minimize | ⌘M | Ctrl+M | none | The title row has a minimize button, and Windows owns Win+Down. |
| Canvas: Finish the crop, a move or a fill; else Deselect | ↩ | ↩ | Enter | Windows names this key Enter. |
| Dialogs: Default button; Cancel | ↩; Esc | ↩; Esc | Enter; Esc | Windows names this key Enter. |
| Panels: Pick the focused row, swatch or button | Space or ↩ | Space or ↩ | Space or Enter | Windows names this key Enter. |
| Canvas: Reset colors to black and white | none | none | D | The W8 brief asks for it. The Mac has only the Reset button. |
| Menu bar: open it | none | none | Alt, F10 | Windows users expect these. The Mac has Ctrl+F2 from the system. |
| Menu bar: open a menu | none | none | Alt+F, E, V, I, L, A, C, W, H | Windows users expect access letters. See the decision below. |
| Open menu: move to an item | none | none | a letter | A Windows menu with no marked letters moves to the next item with that first letter. |

## Keys that later milestones turn on

Each of these commands stays in its menu and stays off. Its key does nothing until the milestone named here. `ShortcutTableTests` compares this list with the keys that work now.

| Menu item | Windows key | Comes in |
| --- | --- | --- |
| File > Settings… | Ctrl+, | W16 |
| File > New… | Ctrl+N | W10 |
| File > Open… | Ctrl+O | W10 |
| File > Save | Ctrl+S | W10 |
| File > Save As… | Ctrl+Shift+S | W10 |
| File > Save All | Ctrl+Alt+S | W10 |
| File > Close | Ctrl+W | W10 |
| Edit > Cut | Ctrl+X | W10 |
| Edit > Copy | Ctrl+C | W10 |
| Edit > Copy Merged | Ctrl+Shift+C | W10 |
| Edit > Paste | Ctrl+V | W10 |
| Edit > Paste into New Layer | Ctrl+Shift+V | W10 |
| Edit > Paste into New Image | Ctrl+Alt+V | W10 |
| Edit > Copy Selection | Ctrl+Shift+Alt+C | W11 |
| Edit > Paste Selection > Replace | Ctrl+Shift+Alt+V | W11 |
| Edit > Erase Selection | Delete | W9 |
| Edit > Fill Selection | Shift+Delete | W9 |
| Edit > Invert Selection | Ctrl+I | W9 |
| Edit > Select All | Ctrl+A | W9 |
| Edit > Deselect | Ctrl+D | W9 |
| View > Zoom to Selection | Ctrl+Shift+B | W9 |
| View > Pixel Grid | Ctrl+' | W16 |
| View > Rulers | Ctrl+Alt+R | W16 |
| Image > Crop to Selection | Ctrl+Shift+X | W9 |
| Image > Resize… | Ctrl+R | W11 |
| Image > Canvas Size… | Ctrl+Shift+R | W11 |
| Image > Rotate 90° Counter-Clockwise | Ctrl+G | W11 |
| Image > Rotate 180° | Ctrl+J | W11 |
| Image > Flatten | Ctrl+Shift+F | W11 |
| Adjustments > Auto-Level | Ctrl+Shift+L | W11 |
| Adjustments > Black and White | Ctrl+Shift+G | W11 |
| Adjustments > Brightness / Contrast | Ctrl+Shift+T | W11 |
| Adjustments > Curves | Ctrl+Shift+M | W11 |
| Adjustments > Hue / Saturation | Ctrl+Shift+U | W11 |
| Adjustments > Invert Alpha | Ctrl+Alt+I | W11 |
| Adjustments > Invert Colors | Ctrl+Shift+I | W11 |
| Adjustments > Levels | Ctrl+L | W11 |
| Adjustments > Posterize | Ctrl+Shift+P | W11 |
| Adjustments > Sepia | Ctrl+Shift+E | W11 |
| Effects > Repeat Effect | Ctrl+F | W11 |
| Help > OpenPainter Help | Ctrl+? | W19 |

## Canvas, panel and dialog keys

These are the rows of `docs/shortcuts.md`, in its order, with the state on Windows. Space and the arrow keys are not menu words. Each row that works names its test in `RowTests` in `ShortcutTableTests`, which fails by name on a row with no real test. A row that says "off until" comes in that milestone, and `TheRowsThatAreOffStillDoNothing` presses its keys.

| What it does | Mac key | Windows key | State |
| --- | --- | --- | --- |
| Rectangle Select | S or ⇧S ×4 | S or Shift+S x4 | works |
| Lasso Select | S ×2 or ⇧S ×3 | S x2 or Shift+S x3 | works |
| Ellipse Select | S ×3 or ⇧S ×2 | S x3 or Shift+S x2 | works |
| Magic Wand | S ×4 or ⇧S | S x4 or Shift+S | works |
| Move Selected Pixels | M or ⇧M ×2 | M or Shift+M x2 | works |
| Move Selection | M ×2 or ⇧M | M x2 or Shift+M | works |
| Zoom | Z | Z | works |
| Pan | H | H | works |
| Paint Bucket | F | F | works |
| Gradient | G | G | works |
| Paintbrush | B | B | works |
| Eraser | E | E | works |
| Pencil | P | P | works |
| Color Picker | K | K | works |
| Clone Stamp | L | L | works |
| Recolor | R | R | works |
| Text | T | T | works |
| Line / Curve | O or ⇧O ×2 | O or Shift+O x2 | works |
| Shapes | O ×2 or ⇧O | O x2 or Shift+O | works |
| Brush width −1 | [ | [ | works |
| Brush width +1 | ] | ] | works |
| Brush width −5 | ⌘[ | Ctrl+[ | works |
| Brush width +5 | ⌘] | Ctrl+] | works |
| Swap primary and secondary colors | X | X | works |
| Switch the active color (the notch) | C | C | works |
| Pan while held | Space | Space | works |
| Nudge the Move tools 1 px | Arrows | Arrows | off until W9 |
| Nudge the Move tools 10 px | ⌘Arrows | Ctrl+Arrows | off until W9 |
| Cancel a drag, the crop, a move or a fill; finish text; else Deselect | Esc | Esc | off until W9 |
| Finish the crop, a move or a fill; else Deselect | ↩ | Enter | works: clears the status message, and the rest comes in W9 |
| Next image | ⌃Tab | Ctrl+Tab | one image until W10: the key stays on the canvas and does nothing |
| Previous image | ⌃⇧Tab | Ctrl+Shift+Tab | one image until W10: the key stays on the canvas and does nothing |
| Next or previous control | Tab, ⇧Tab | Tab, Shift+Tab | works |
| Pick the focused row, swatch or button | Space or ↩ | Space or Enter | works |
| Row above or below | ↑ ↓ | ↑ ↓ | works |
| Palette swatch to the left, right, above or below | ← → ↑ ↓ | ← → ↑ ↓ | works |
| Hue −1° / +1° (⇧: 15°); saturation −1 / +1 (⇧: 10) | ← → ↓ ↑ | ← → ↓ ↑ | works |
| Default button; Cancel | ↩; Esc | Enter; Esc | works |

## Alt and access keys: the decision

Alt does what a Windows user expects, and it never gets in the way of Alt as a modifier.

1. **Alt tap.** Press and release Alt alone, on the canvas, in the main window or in a panel. The main window's menu bar takes the keyboard, on File. F10 does the same from each of those places, and with the bar open F10 closes it. Esc or another Alt tap closes it, and the keyboard goes back to where it was.
2. **Access letters.** Alt with F, E, V, I, L, A, C, W or H opens that menu. The letter shows underlined while Alt is down. Effects takes C because Edit has E.
3. **Inside a menu.** The arrow keys and Enter work. A letter moves to the next item that starts with it, because the items have no marked letters. Their words come from the Mac, and marking them would change the words a test holds.
4. **Alt as a modifier.** A press of a key or a mouse button while Alt is down ends the tap, so the release does nothing. This covers Alt-click on the Colors wheel, which keeps the spoke, and Alt-click with the Zoom tool, which zooms out. Avalonia's own menu handler opens the menu on that release, so `TitleMenu.Open` refuses it, for that release only. F10 and Alt with a letter do not go through that refusal.
5. **Left with Windows.** Alt+F4 closes the window. Alt+Space opens its system menu. Alt+Tab switches apps. A window that loses the keyboard with Alt down forgets that press and stops underlining the letters. If Alt is still down when the window comes back, its release is no tap.
6. **AltGr.** Ctrl with Alt is AltGr on many keyboards, so it starts no tap. In a text field a Ctrl+Alt key that the menus use still runs the menu item, as on the Mac. A field with text from such a keyboard is the case to check at W17.
