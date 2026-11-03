# Critical flows

These are the flows a person cannot lose. `cargo xtask check --tier milestone` reads this table and prints one row per flow. A flow with `yes` runs its own test. A flow with `no` prints NOT RUN by name and fails the level, as PLAN 4.1 says of any part that the level does not reach.

The "Test" name is `FlowTests.Flow<number>` in `windows/OpenPainter.Tests/src/FlowTests.cs`. `FlowListTests` holds this table to those tests. A flow with `yes` and no test fails by name, and so does a test with no row.

Each flow runs headless on the AOT build, against the real engine. When the flow reaches real windows, `--tier window` runs the parts of the harness named under the table.

| # | Flow | Steps | Possible from | How | Runs now |
| --- | --- | --- | --- | --- | --- |
| 1 | Start the app and see the document | Open the window, bind a document, read the title, strip, status bar and four panels | W7 | headless, and `report` and `pixels` in real windows | yes |
| 2 | Build a layer stack from the menus | Add, Duplicate, Go to Layer Below, Move Layer to Top, Toggle Visibility, Merge Down, Delete, from the Layers menu and its keys | W8 | headless, and `panels` in real windows | yes |
| 3 | Walk the history | Make 3 steps, Ctrl+Z, Ctrl+Y, Ctrl+Shift+Z, jump with a History row, see the modified mark and the strip preview follow | W8 | headless | yes |
| 4 | Choose colors | X, D and C on the canvas, type a hex color and press Enter, see keys return to the canvas, pick a palette swatch | W8 | headless, and `color` in real windows | yes |
| 5 | Choose a tool and its options | A tool letter, Shift with the letter, repeat presses, [ and ] for the width, the options row follows | W8 | headless | yes |
| 6 | Move around the image | Ctrl++, Ctrl+-, Ctrl+B, Ctrl+0, change the units | W7 | headless, and `pan` in real windows | yes |
| 7 | Arrange the panels | F5 to F8, Next Panel and Previous Panel, Reset Panel Layout | W8 | headless, and `panels` in real windows | yes |
| 8 | Work with the keyboard alone | Tab into the Layers rows, arrow keys, Enter, Alt then a menu letter, an arrow and Enter, Esc | W8 | headless, and `alt-states` in real windows | yes |
| 9 | Change a layer in Layer Properties | F4, type a name, change opacity and blend, Enter, then Ctrl+Z, and F4 then Esc adds no step | W8 | headless, and `panels` in real windows | yes |
| 10 | Paint and undo | Choose the Paintbrush, draw a stroke, see the pixels change, Ctrl+Z | W9 | headless, and real windows | no |
| 11 | Open, edit and save | File > Open, make a change, Ctrl+S, close, open it again, compare the pixels | W10 | headless, and real windows | no |
| 12 | Run an effect | Adjustments > Sepia or an effect dialog with its preview, OK, Ctrl+Z, Repeat Effect | W11 | headless, and real windows | no |

## Who can run each

- `milestone` runs flows 1 to 9 headless on the AOT build, once each, with `OpenPainter.HeadlessAot --only FlowTests.Flow<number>`.
- `window` runs the harness parts the table names. No flow needs the user's pointer, because the harness posts its messages to the app's own windows.
- A real pointer drag and real hover stay owed from W7.

## What a headless flow cannot do

Each flow gives the same input a person gives, through the app's windows, as far as a headless run can. These parts are NOT RUN by the flow tests. A real-window run must cover them.

- Flows 2, 6 and 7 run a menu item with no key by raising its click. A real pointer on an open menu is NOT RUN.
- Flows 3 and 4 post a press and release to the panel's window for the History row and the palette swatch. A real pointer on them is NOT RUN.
- Flow 3 makes its one step that changes pixels with an engine command, because the Adjustments menu comes in W11. Making that step from the menu is NOT RUN.
- Flows 4, 8 and 9 type by giving text input to the focused field. The system's input methods are NOT RUN.
- Flow 8 sends Alt and F10 to the windows as key events. The native canvas's own path for them, `OnCanvasSystemKey`, needs a real window, so it is NOT RUN headless.
- Flow 8 does not press Alt-Tab. `AltAndF10Tests` stands in by hiding and showing the window. A real Alt-Tab to another program is NOT RUN.
- Flow 9 types the opacity, as its steps say. A real drag of its slider is NOT RUN.

## Changing this list

Set "Runs now" to `yes` in the same change that adds the flow's test. Keep 9 to 12 rows, because the level fails by name outside that range.
