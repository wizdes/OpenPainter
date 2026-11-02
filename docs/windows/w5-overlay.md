# W5b Windows canvas overlays

The engine now draws canvas overlays over its Windows view in the same GPU frame as the document. The overlay list uses document points. The view shader moves the shapes with pan and zoom, while line widths and handles stay in screen points. The engine keeps the GPU geometry until the item list changes. The ants clock changes one number and does not copy path points.

The C call `op_view_set_overlay` accepts a list of paths, rectangles, ellipses, handles, pivots, crops, carets, edit frames, underlines, flood handles, and zoom rectangles. Paths may be open or closed. The offscreen test draws a 10,000-point path. `op_view_set_overlay_dash` changes the black and white dash phase. Avalonia's `SPIKE_OVERLAY_SET=1` sends a 200-point lasso, ellipse, seven handles, rotated crop, and caret, then animates only the dash phase on its own thread. WinUI has a focusable floating panel button. Both UI Automation menu walks use the File menu and its Open dialog item.

## Checks run by the builder

- The GPU overlay test ran on an offscreen texture and read its pixels back: 8 passed, 0 failed. It checks three zooms, moving dashes, fixed handle and pivot sizes, rotated and crossed crop parity, a 10,000-point path, an empty list, text marks, a flood handle, a static zoom rectangle, and a partly covered edge pixel.
- The C interface windowless test ran: 5 passed, 0 failed. It checks null pointers, bad kinds, counts, flags, coordinates, and dash values. `OP_ERR_BAD_ARGUMENT` names invalid input; `OP_ERR_NO_SUCH_DOC` names an unknown view.
- Three old `windows_views.rs` overlay tests changed only for the new interface: `panel_and_overlay_reject_bad_arguments_without_a_surface` now passes a list count; `overlay_math_uses_document_points_and_dash_offset` became `overlay_items_validate_document_points` because the old rectangle sampler was removed; `windows_overlay_shader_compiles_without_a_surface` now sets an item so the lazy overlay pipeline is compiled. `hwnd_attach_rejects_a_non_window` is unchanged.
- Avalonia's headless test ran: 2 passed, 0 failed. One saved test follows the File menu item to its dialog. The older test still checks its prior input and Tab assertions.
- The no-window script tests ran: `WINDOW_MEASURE_SELF_TEST_PASS 4 cases` and `INPUT_EVENT_SELF_TEST_PASS 3 cases`.
- The WinUI debug build passed with 0 warnings and 0 errors. Both PowerShell scripts parsed, both Python scripts compiled, and the focus walk's SendInput C# type compiled.
- The first .NET test and WinUI build attempts used `--no-restore` and failed with `NETSDK1004` because their assets files did not exist. The commands with restore then passed. The first baseline `cargo test -p op-gpu --test canvas --no-default-features -- --list` failed because the old Windows-only `CanvasParams.overlay` field was missing from an existing test initializer. Moving overlay state onto `ViewRenderer` fixed that baseline failure without changing the test.
- The first full GPU run failed in an existing float-target test because the new blend pipeline was made for an unused overlay. The pipeline is now made only when an overlay is set. The exact failing test passed after that change.
- The final full `op-gpu` and `op-ffi` Rust test command passed: 108 passed, 0 failed, 1 existing manual Metal timing test ignored. The added dash-color assertions then passed in the eight-test overlay run.
- The final release `op-ffi` DLL build passed after the antialias change. `engine/target/release/op_ffi.dll` exists and is 10,882,560 bytes.

## What still differs from the Mac

These are work estimates for overlay drawing and its proof on a real window. They do not include real tools, moving handles, hit testing, the Mac app using this code, or a shared GPU texture.

| Work left | Hours |
| --- | ---: |
| Match Move tool's accent box, copy-mode dash, white pivot fill, and all eight nubs | 3 |
| Round the Flood handle's corners and match its arrow caps and small strokes | 2 |
| Give the text frame its fixed 3-point outset, the text move nub, and both marked-text underline weights | 4 |
| Match line joins, ellipse smoothness at large zoom, and backing scales above 1 | 4 |
| Match the crop's extra handle positions and exact draw order when shapes overlap | 2 |
| Compare real-window pixels with the Mac and fix differences; prove 59 fps with the 200-point lasso | 6 |
| **Total still needed** | **21** |

The estimate is 21 hours, above the 20-hour named-stop line in `decisions.md`. Root should reopen the stack choice after its window measurements. I could not match the Mac details listed above in this round. The visible FPS and exact colors at Windows backing scale 2 are still unknown.

## Root's real-window run, 2026-11-02

Root built this code as native programs and ran it as the signed-in user. The builder's estimate above is 21 hours, so root reopened the choice. `decisions.md` has the result.

| Check | Result |
| --- | --- |
| `run-window-checks.ps1`, both stacks, visible and hidden | All pass, exit 0 |
| The full overlay set in a real window | Seen in a screenshot. Median 60.0 fps over 5 runs, lowest 60.0, worst single gap 32 ms |
| Plain ants, alternating with those runs | Median 60.0 fps, lowest 59.8 |
| `input-check.py`, both stacks | Nothing missing, exit 0 |
| `focus-walk.ps1`, both stacks | Pass |
| `pan-check.py`, both stacks | 57.5 fps each |
| Mac type check of the whole workspace | Passes, after root fixed a field that was missing its Windows-only mark. Types only: nothing ran on a Mac |

Three earlier full-set runs gave 56.9, 57.1 and 60.0 fps with 40 ms gaps, right after other work on the PC. The 5 runs above came later, with the PC otherwise quiet.

Root also fixed two script faults. The foreground guard could not take the front until a tap of Alt was restored, and the pen check did not read WinUI's event format.

## NOT RUN here

This builder has no desktop. I did not launch either shell, inspect a window, send real window input, measure visible FPS, or listen to Narrator. I did not publish the AOT shells or run Mac tests. An independent agent review was not run because this builder was told not to call other agents or models. The WinUI hidden-desktop dialog check failed in W5 and may still fail; the hardened script will now report that failure with a nonzero exit.

Root should build `op_ffi` in release mode again after applying these files, with `CARGO_INCREMENTAL=0` and `CARGO_PROFILE_DEV_DEBUG=0`. Root must publish both shells with the .NET 10 SDK, then run these checks as the signed-in Windows user on a desktop:

From the workspace root, use these exact build commands. The output folder is only for generated test programs and results:

```powershell
$env:CARGO_INCREMENTAL='0'; $env:CARGO_PROFILE_DEV_DEBUG='0'
Push-Location engine
cargo build -p op-ffi --release
Pop-Location
$dotnet = Join-Path $env:USERPROFILE '.dotnet10\dotnet.exe'
$results = Join-Path $env:TEMP 'op-w5b-acceptance'
& $dotnet publish tools/ui-spike/avalonia/SpikeAvalonia.csproj -c Release -r win-x64 -o (Join-Path $results 'avalonia')
& $dotnet publish tools/ui-spike/winui/SpikeWinUI.csproj -c Release -r win-x64 -o (Join-Path $results 'winui')
```

The Rust build should print `Finished release profile`. Each publish should exit 0 and leave `SpikeAvalonia.exe` or `SpikeWinUI.exe` in its named folder. WinUI AOT publish may need Visual Studio's Installer folder, which holds `vswhere.exe`, on `PATH` as described in the spike README.

Then run the visible and hidden checks:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File tools/ui-spike/run-window-checks.ps1 -AvaloniaBuild (Join-Path $results 'avalonia') -WinUIBuild (Join-Path $results 'winui') -OutDir (Join-Path $results 'checks')
python tools/ui-spike/input-check.py (Join-Path $results 'avalonia/SpikeAvalonia.exe') (Join-Path $results 'input') avalonia
python tools/ui-spike/input-check.py (Join-Path $results 'winui/SpikeWinUI.exe') (Join-Path $results 'input') winui
powershell -NoProfile -ExecutionPolicy Bypass -File tools/ui-spike/focus-walk.ps1 -Exe (Join-Path $results 'avalonia/SpikeAvalonia.exe') -Shot (Join-Path $results 'avalonia-focus.png')
powershell -NoProfile -ExecutionPolicy Bypass -File tools/ui-spike/focus-walk.ps1 -Exe (Join-Path $results 'winui/SpikeWinUI.exe') -Shot (Join-Path $results 'winui-focus.png')
python tools/ui-spike/pan-check.py (Join-Path $results 'avalonia/SpikeAvalonia.exe') (Join-Path $results 'avalonia-pan.json')
python tools/ui-spike/pan-check.py (Join-Path $results 'winui/SpikeWinUI.exe') (Join-Path $results 'winui-pan.json')
```

1. Run `tools/ui-spike/run-window-checks.ps1 -AvaloniaBuild <Avalonia publish folder> -WinUIBuild <WinUI publish folder> -OutDir <result folder>`. A full pass prints `PASS` for each visible and hidden UI Automation and measure check and exits 0. Avalonia's measure JSON must show `firstFrameMs` below 3000 and `antsFrames` at least 384 from 3 to 9.5 seconds. Any `FAIL` must produce a nonzero exit.
2. Run `tools/ui-spike/input-check.py <Avalonia exe> <result folder> avalonia` and again for WinUI. Each run should print JSON with `"missing": []`, `"exit": 0`, and pen pressure events `0.50` and `0.68`; a missing event prints `INPUT_CHECK_FAILED` and exits nonzero.
3. Run `tools/ui-spike/focus-walk.ps1 -Exe <Avalonia exe> -Shot <png>` and again for WinUI. Each run should print `FOCUS_WALK_PASS`, a Tab walk, and a Test dialog with Number box. Inspect both screenshots.
4. Run `tools/ui-spike/pan-check.py <Avalonia exe> <result json>` and again for WinUI. Each run should print JSON with a nonzero `pointer_moves`, a measured `engine_fps`, and `"exit": 0`. A lost foreground window prints its named error and exits nonzero.
5. Listen to Narrator on both shells and check the File menu, floating panel button, Canvas, dialog, and number box names. A tool cannot hear Narrator.

Follow-ups: connect engine selection rings and real tool state to the item list; add move-handle gestures; add hit testing; test a shared GPU texture; let the Mac app use the same overlay code.
