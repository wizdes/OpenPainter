# W5 UI stack test: results

Root ran both shells as native (AOT) builds on real windows, as the signed-in user, on 2026-11-02. The PC is a Windows 11 machine with an RTX 3060 Ti and one 60 Hz screen. Both shells use .NET 10 and the same engine DLL.

| What was tested | Avalonia 12.1.4 | WinUI 3 (Windows App SDK 2.6.4) |
| --- | --- | --- |
| Start to first engine frame, visible desktop | 429 to 765 ms | 405 to 750 ms |
| Same, hidden desktop | 435 to 630 ms | 433 to 497 ms |
| Program size without PDB files | 46 MB, 5 files | 136 MB, 292 files, as a self-contained build. A Store package could be smaller |
| Memory at first frame | 199 to 244 MB | 222 to 232 MB |
| Resize, engine frames in the 3 s script | 182 | 181 |
| Ants overlay rate | 60.0 fps, gap p99 18.8 ms, drawn by the engine with the clock off the UI thread | About 58 callbacks per second, XAML over the panel. This counts callbacks, not frames on screen |
| The Mac's overlay set: 200-point lasso, ellipse, 7 handles, rotated crop, caret | Median 60.0 fps over 5 runs, lowest 60.0, drawn by the engine | Not built |
| Same ants clock on the UI thread | 37 to 42 fps: the UI frame callback holds it back | Not applicable |
| UI frame callback alone, doing almost nothing | 41 to 44 per second on all 5 drawing routes tried | About 58 per second |
| Engine frames with the pointer moving the canvas, blank document, 3 runs | 57.3 to 58.0 fps, gap median 16.7 ms | 58.0 to 58.4 fps, gap median 16.7 ms |
| Engine drawing a canvas moved every UI frame | 38 fps: held back by the same UI callback | 60.0 fps, gap p99 18.7 ms |
| Crop dim and caret over the canvas | Seen in screenshots | Seen in screenshots |
| Pop-up menu and floating panel over the canvas | Seen in a screenshot. Each is its own window | Seen in screenshots. Both are XAML in the same window |
| Pointer and wheel on the canvas | Pass | Pass |
| Typed text | Pass, including `é` sent as a Unicode key | Pass, including `é` sent as a Unicode key |
| A real dead key on a dead-key layout | NOT RUN: it would change the user's keyboard layout | NOT RUN: same reason |
| Pen pressure, replayed through Windows' synthetic pen | 0.50 and 0.68, as sent | 0.50 and 0.68, as sent |
| A real pen | NOT RUN: no pen here | NOT RUN: no pen here |
| Tab order, names read by UI Automation | Test menu, Floating panel button, Canvas, and around again | File menu, Canvas, Floating panel button, and around again |
| UI Automation, visible desktop | Pass: File menu, its item, dialog, number box, panel button, canvas | Pass: the same walk. Reaching the dialog once needed the Enter key, and later walks did not |
| UI Automation, hidden desktop | Pass | Pass, after the walk was corrected. An earlier run failed here, and the fault was in the test |
| Tests with no window, as `agent` | 1 test passes: controls only. It removes the native canvas, so it proves neither canvas input nor pixels. It runs as a normal .NET build, not the native one | None possible: exits with `0xC000027B` with no desktop |
| Narrator | NOT RUN: a tool cannot hear it. Focus names above are what it would be given | NOT RUN: same reason |

## What each stack needs that a plain app does not

Avalonia:

- A custom window class for the canvas. A `STATIC` child passes the mouse through to its parent.
- Keyboard handoff: the canvas window must give focus back to the top window on Tab. Its parent is a hidden host, and giving focus to that strands the keyboard.
- Overlays drawn in the engine (`op_view_set_overlay`), with their clock off the UI thread.
- A program manifest, or the native child window cannot be created.
- Pen input read from `WM_POINTER` messages on the canvas window.

WinUI:

- A bridge for the swap chain (`winui/src/PanelBridge.cs`). wgpu sets it from the render thread, and the panel refuses that with `Invalid surface`. The bridge must not wait for the UI thread, or the two block each other.
- Clearing the swap chain before the engine frees its device, or the app crashes in `D3D12Core.dll` on exit.
- A pointer handler that marks events handled, or focus leaves the canvas when the pointer goes up.
- Tests need a real window. Builders have none, so every UI test would run as root.

## Not known

- Why Avalonia's UI frame callback runs at 41 to 44 per second on a 60 Hz screen. It is not the drawing route. Avalonia's own animations were not measured on screen.
- Pan on a real 24 MP image. W7 owns this.
- How either stack behaves at other scales or refresh rates, or with a real document's tiles.
- Whether Narrator reads the names as UI Automation reports them.

The overlay test and its numbers are in `w5-overlay.md`. The choice is in `decisions.md`.
