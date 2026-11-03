# W7: app shell and canvas

This file says what each done-test item needs and how to run it. Root finishes it with real results. The numbers from the builder's proof runs are in the last section. They are not done-test results.

## Done-test items

| # | Item | What proves it | How to run it |
| --- | --- | --- | --- |
| 1 | A 24 MP PNG pans for 30 s at 59 fps or more, with no stall over 50 ms, on the RTX | The harness's `pan trace` part. It needs a quiet PC | `--parts pan` |
| 2 | Real pixels on the live canvas, so a black frame fails | The harness's `pixel read-back` part | `--parts pixels` |
| 3 | The color check on the live window | The harness's `color check` part, light and dark | `--parts color` |
| 4 | Menu order and words match `opcli dump-catalog`, with Ctrl for Command | `MenuTests` in the app tests | `dotnet test windows/OpenPainter.Tests/OpenPainter.Tests.csproj` |
| 5 | The DLL's exports match the bridge by name | `BridgeTests` in the app tests | The same command, after a release `op_ffi.dll` build |
| 6 | Layout matches the W6 pictures, light and dark, at 100% and 200% | Root looks at each shot | `--parts shots`. The harness never changes the display's scale, so a scale the display is not at prints its own NOT RUN row |
| 7 | `--tier milestone` passes, and `--tier window` passes when the screen is free | The level's summary | See "The levels" below |
| 8 | The 15 overlay hours in `w5-overlay.md` still fit | Root's written judgement | No command |

## The harness

`windows/harness` is a console program. Root runs it as the signed-in user, and it opens real windows on the visible desktop.

```powershell
$env:DOTNET_ROOT = "$env:USERPROFILE\.dotnet10"; $env:PATH = "$env:DOTNET_ROOT;C:\Program Files (x86)\Microsoft Visual Studio\Installer;$env:PATH"
Push-Location engine; cargo build -p op-ffi --release -j 8; Pop-Location
dotnet publish windows/OpenPainter/OpenPainter.csproj -c Release -r win-x64 -o windows/OpenPainter/bin/publish
dotnet run --project windows/harness/OpenPainter.Harness.csproj -c Release -- --app windows/OpenPainter/bin/publish/OpenPainter.exe --out windows/harness/bin/last-run
```

`cargo xtask check --tier window` runs the same steps.

Each part prints one line: PASS, FAIL or NOT RUN with a reason. A PASS must come from evidence that the check ran. The last line is `HARNESS: PASS` or `HARNESS: FAIL` with the names. Any part that did not pass fails the run.

Before any part, the harness runs `scripts/windows/preflight.ps1 -Level window`, then looks for known dialogs: `consent`, `CredentialUIBroker`, `WerFault` and `WerFaultSecure`, when they have a window. A locked screen, a known dialog, a wrong session or under 40 GB free prints `UI TESTS: NOT RUN (reason)`, and every part prints NOT RUN.

Switches for a proof run. The window level passes none of them:

| Switch | What it does |
| --- | --- |
| `--parts report,pixels,paint-fault,panels,color,shots,pan,exit,fatal,small-screen` | Runs only these parts |
| `--pan-seconds <s>` | A shorter trace. The part then prints NOT RUN with its numbers |
| `--allow-busy` | Runs the trace on a busy PC. The part then prints NOT RUN with its numbers |
| `--first-frame-runs <n>` | How many launches `pixels` measures. The default is 3 |
| `--ignore-disk` | Goes on with under 40 GB free on C: |

### How each part works

**Rules for all of them (PLAN 4.3).** Every run passes a temp `--data-root`. The harness fails if the real settings folders change. Pictures come from `PrintWindow` on one of the app's own windows. A screen pixel is read only after `WindowFromPoint` says the app's own window is there. Input is messages posted to the canvas's own window. `SendInput` and moving the pointer are not used, and a test fails if the harness or the probe calls them. The app's windows rise above other programs with no focus change.

**Pan trace.** The harness writes a 6000 by 4000 PNG to its temp folder. The app opens it through `--test-load-png` at 100%. The Pan tool then drags along a fixed path, posted from its own thread 250 times a second. Each axis runs a triangle wave, so the pointer never rests. A pointer at rest asks for no frame, and that would look like a stall. Frame times come from `op_view_take_frame_times`, which the engine stamps on its render thread. No Avalonia callback is in the count.

These fail the part, on any PC:

- The engine's view did not follow the pointer. The hook reads the view 4 times a second. The view must sweep at least 90% of what the pointer sweeps, stay within 150 pixels of the pointer's path, and get a new frame between any two reads.
- Real pixels do not move. The harness reads one real screen pixel 4 times a second during the trace, and its color must change in at least 25% of the reads, because flat cells pass under it.
- After the trace, the canvas does not show the view. The app stays open, and a capture must match the image at the last view.

These fail the part on a quiet PC and only make it NOT RUN on a busy one:

- The average is under 59 fps, or a gap between frames is over 50 ms.
- The first frame comes over 50 ms after the first move, or the last comes over 50 ms before the end. The two ends together may lose at most 100 ms of the time asked. Before this rule, a freeze of up to 0.6 s at an end went unseen.

**A busy run (PLAN 4.3).** The harness reads CPU and GPU use before the launch and during the trace, and throws out a busy run as NOT RUN with its numbers. The parts before it leave the PC busy for a while. So the harness reads the PC every 3 s, for up to 60 s, until two reads in a row are under the limits. It prints how long it waited and each read's numbers. A PC still busy after the wait prints NOT RUN and names the top programs by CPU. `--allow-busy` takes one read and does not wait. The limits are other programs at 15% CPU before the start (the PC's use less the harness's own), other programs at 15% CPU on average during the trace (the PC's use less the app's own), and the GPU at 15% before or 50% on average during. GPU use that cannot be read counts as busy. Sunshine and LM Studio are named when they run, and they count as busy when a stream or a loaded model is in use or when nobody can tell, as in `scripts/windows/preflight.ps1`.

**Pixel read-back.** The test image is flat colored cells from a formula, and no cell is near black. The harness knows what each document pixel must be. After the first-frame signal it captures the main window 5 times, and each capture's grid of up to 2,560 points must match within 1 of 255. It does this for 3 launches of a small image, a dark launch of it, 24 MP at 100%, 24 MP at Fit, and 24 MP in dark.

**Screen pixels.** The same launches read the real screen at up to 8 points away from color edges, from the middle outward. A point another window covers, or comes within 64 pixels of, does not count. A launch with no point judged prints NOT RUN by the launch's name, so this row cannot pass with nothing judged.

**Launch paint.** The same launches read the real screen in the canvas's area from launch. The window first shows what is behind it for a moment, until Avalonia paints. Windows says the window owns its pixels from the moment it exists, and what is behind can be any color, so a color cannot say that the app has painted. `--test-first-paint` writes a file when Avalonia's renderer has drawn the window, and the harness reads pixels against that file on its own clock. From there until the first frame every pixel must be the stage color, and none may be black. A read before the file that is not the stage color shows the window was still unpainted then, and that may be at most 250 ms after the window appeared. On a PC where other programs take 15% of the CPU or more during that launch, that time prints by name and is not judged (PLAN 4.3). The colors still are.

A read counts only when the app's window holds the point and 64 pixels around it, before and after the read, because another window's shadow falls on the app with no hit test to show it. A read within 500 ms of another program holding that read point does not count either, because a closing window fades out over the app after Windows stops finding it there. The harness reads the first 3 of 15 points it holds in each round, and it keeps the app above other programs until the first frame.

**Launch paint fault.** `--test-paint-fault` plants two faults in the app: a black canvas, which is the real W7 fault, and a wrong stage color. Each launch must fail `launch paint`, or this row fails. It runs in `--parts paint-fault`.

**Panels.** `--parts panels` runs in light and dark. `--test-panels` builds a layer stack and steps through Layer Properties, and the harness looks at each stage from outside: 7 history rows, the layer names and pictures, the dialog owned by the main window and above it and the canvas, a title row that Windows says drags it, and a real canvas pixel that follows the preview and comes back on Cancel. It saves `shots/panel-history-<theme>.png`, `panel-layers-<theme>.png`, `panel-layer-properties-<theme>.png` and `panels-main-<theme>.png` for root to look at.

**First-frame signal.** `--test-first-frame <file>` makes the app write the file once the engine has presented a frame at the asked zoom. The harness waits for that file, then calls `DwmFlush` twice. No part waits a fixed time for the first frame. Each harness run first deletes the numbered folders in `<out>/runs`, and a launch refuses a folder that is not empty. A file left by an earlier run would pass for this one's, with a dead process's window handles. The `pixels` part also fails, by name, if the canvas's window handle changes during a launch.

**Color check.** The image has 18 patches: primaries, secondaries, a gray ramp and mixed colors. Each must match within 1 of 255 at 100%. The stage around the image must match the palette, so no seam shows beside Avalonia's stage. Expected values exist only for an sRGB display profile. Another profile with a difference prints NOT RUN and names the profile.

**Shots.** The harness saves 6 pictures in `<out>/shots`: the main window, the File menu open and the "more" popup open, each in light and dark. Each is the app's windows painted over each other, with nothing from any other program. Root must look at them. A headless shot is never the visual check. `window shots at 100%` and `window shots at 200%` are their own rows, and the one the display is not at prints NOT RUN. So a run on one display scale prints HARNESS: FAIL and the `window` level cannot pass there.

**The report.** `--test-report` runs as before, in light and dark, and the harness splits it into `hit test`, `panels`, `canvas input`, `menus and options`, `icons` and `window minimum size`. Each check that runs adds to `checks_run` in the report, and each part needs at least the count a full run gives, which `ReportJudge.LeastChecks` holds. So a check taken out of the report fails its part. A panel or the "more" popup under another program prints NOT RUN in its own part.

**App exit.** The main window closes in 5 states: all panels shown, one hidden, all four hidden, one hidden and shown again, one hidden and the window minimized. Each must end the process with exit code 0 within 15 s. A hidden panel has no owner, and Avalonia's default shutdown waits for every window, so the app now ends on its main window's close, and the main window ends every panel.

**Fatal path.** `--test-throw <place>` plants a throw in the main window's `Closed`, in each window's `WM_GETMINMAXINFO` hook, and in each window's move. Each must end the process with exit code 1 and the message.

**Small screen.** `--test-work-area 1000x620` makes the app take that as its monitor's work area. The window must fit it at launch and when asked for a tiny size, and the status bar must end inside it. The part saves `shots/small-screen-light.png` for root to look at.

## The levels

| Level | What W7 adds |
| --- | --- |
| fast | The app tests run when `windows/`, the harness, the vectors or three Swift test files change. A row named `headless UI for the changed area, inside Windows app tests` takes those tests' result. They run whole, not by area, and take about 8 s |
| milestone | Publishes `windows/harness/HeadlessAot` with NativeAOT and runs it. It compiles the app's headless test source, and runs every test with a small runner and no test framework |
| milestone | `9 to 12 critical flows` stays NOT RUN, so this level still fails. No list of flows exists, and a flow needs tools, files and undo, which come in W8 to W10. Done-test 7 therefore cannot pass at W7 unless the user waives this row |
| window | Release DLL, AOT app, the harness. The old rows `critical flows in real windows` and `one shot per changed screen` no longer print on Windows. Shots are in the harness. Real-window flows wait for the same reason |

`level_parts` and `all_level_parts` still hold the old `headless UI on the AOT build` row, because 4 tests of earlier stages hold them to it. `run_parts` is what a level really runs, and it replaces that row with the two AOT steps.

## Test values

`windows/tests/vectors` holds values read out of the Swift tests:

| File | Values | Source | Checked on Windows code |
| --- | --- | --- | --- |
| `zoom-steps.json` | 21: the presets, next larger, next smaller, clamp, labels | `ZoomTests.swift` | Yes |
| `fit-math.json` | 4 | `ZoomTests.swift` | Yes |
| `shortcut-words.json` | 65 menu keys | `menu-skeleton.txt` | Yes |
| `color-math.json` | 27: hex fields, channel fields, HSV | `ColorsTests.swift` | W8. No color code exists yet |

`AVectorFileMatchesItsSwiftSource` reads the Swift source again each run. A value that differs, is new or was removed fails by the vector's name. To refresh the files, run `dotnet run --project windows/harness/OpenPainter.Harness.csproj -c Release -- --export-vectors`.

Left out for now: `ZoomTests.swift`'s viewport cases, which `ViewTests.cs` ports by hand; the color wheel's modifier math, the palette files and the brush width keys, which W8 can add when it builds them.

## Results

Root's results at `5eaf688`, 2026-11-03. W7 took about 5 agent hours of the 18 budgeted.

| # | Item | Result |
| --- | --- | --- |
| 1 | 24 MP pan, 59 fps, no stall over 50 ms | NOT RUN on a quiet PC. Another session's jobs kept the PC busy through 9 tries over 70 minutes. The last try had 17% other CPU: 59.9 fps, longest stall 58.0 ms, 1,801 frames. That run counts as neither a pass nor a fail. Other busy runs gave 60.0 fps and 26 to 33 ms |
| 2 | Real pixels on the live canvas | PASS. 7 launches, every capture matched, 56 real screen points matched |
| 3 | Color check on the live window | PASS, light and dark. 18 patches within 1 of 255 |
| 4 | Menu order and words | PASS, in the app tests |
| 5 | DLL exports against the bridge | PASS, in the app tests, with parameter and return types as well as names |
| 6 | Layout against the W6 pictures | 100%: root looked at main, more and File menu shots from real runs, and they match apart from the differences listed under stage 2. 200%: NOT RUN, because this PC is at 100% and the harness does not change the display |
| 7 | `--tier milestone` and `--tier window` | Both fail. Milestone: 3 known test failures (2 Mandelbrot, W14; 1 load-dependent timing test) and `9 to 12 critical flows` NOT RUN. It took 2,012 s against a 10-minute target. Window: the 200% row and the pan NOT RUN, and on the last run `panels (light)` and `menus and options (light)` NOT RUN, because another window covered them |
| 8 | The 15 overlay hours in `w5-overlay.md` | They still fit. W7 used none of them and found nothing that changes the overlay route. Avalonia still cannot draw over the canvas, and owned panels sit over it as planned. Tool overlays are first exercised in W9, so this is a judgement, not a measurement |

W7 closes under PLAN 4.2 on its headless checks, with these window checks NOT RUN. The phase cannot close until they run or the user waives them.

Open at the close:

- `launch paint` did not pass in 11 of 53 `pixels` runs before the last fix. The cause is not found. A window behind the app can read close to the dark stage color, and one run of 6 launches saw no stage color at all. W8 starts with this.
- A real mouse drag, a real monitor going away, and real hover were not tried.
- Test hooks ship in the app with no build gate. W19 must remove them or gate them.
- The whole-workspace 10,000-case run at `6085a58`: 861 passed, 3 failed (the same 3), 7 ignored.

### Builder's proof runs, not done-test results

The PC was running a 10,000-case test in another copy, so the speed numbers do not count. C: had under 40 GB free, so the runs passed `--ignore-disk`.

These are stage 3's runs. The fix round's are in the next section.

| Run | Result |
| --- | --- |
| App tests, `dotnet test windows/OpenPainter.Tests/OpenPainter.Tests.csproj` | 239 passed, 0 failed |
| `cargo xtask check --tier fast` | PASS in 20 s, with 118 `xtask` tests |
| AOT headless run | `Failed: 0, Passed: 187, Skipped: 0, Total: 187, 143 test methods on the AOT build` |
| Pan trace, 30 s, PC 35% busy | Average 60.0 fps, longest stall 26.1 ms, 1,804 frames, canvas 1280 by 672 at 100%, 60 Hz display, RTX 3060 Ti. Printed NOT RUN, because the PC was busy |
| Pan trace, 30 s, PC 28% busy, last build | Average 60.0 fps, longest stall 33.2 ms, 1,803 frames. Printed NOT RUN, because the PC was busy |
| `cargo xtask check --tier window` | Built the DLL and the AOT app, ran the harness, and printed NOT RUN, because C: had under 40 GB free. It proves the path, not the checks |
| Pixel read-back, 12 launches of a small image, 24 MP at 100% and at Fit | PASS. 70 captures after the signal all matched: 2,220 points at 100%, 342 at Fit |
| Screen pixel after the signal, 6 launches | The one judged in each launch matched. The Fit run's point was near a color edge, so it was not judged |
| Color check, light and dark | PASS. 18 patches, `sRGB Color Space Profile.icm`, within 1 of 255 |
| Window minimum size | FAIL first, at 1100 by 823 client pixels. PASS at 1100 by 792 after the fix |

### The black canvas at launch

It was a real fault, and it is fixed. The canvas's window exists before its first frame. Nothing could paint it: a plain Win32 fill still read black on the screen, because Avalonia's main window has `WS_EX_NOREDIRECTIONBITMAP`, and the window Avalonia holds the canvas in is layered. So that holder now clips away all of the canvas until the first frame, and Avalonia's stage shows there in the right color. The engine draws as usual meanwhile. Once a frame is presented, the clip goes and the view is sent once more.

Real screen pixels before the first frame, at 100% display scale:

| Build | Pixels before the first frame | Black | The stage color |
| --- | --- | --- | --- |
| Stage 3's, the 3 launches of 24 MP, light and dark | 111 | 106 | 0 |
| This fix, the same 3 launches | 111 | 0 | All after the window's first paint |

The window itself shows what is behind it for at most 84 ms before Avalonia's first paint. That is not the canvas, and the harness allows up to 250 ms of it. Small images reach their first frame too fast for a read before it, about 100 ms after the canvas window, so their launches count only when a read lands there.

### What Avalonia does with a throw

I planted a throw at each place on a build with no guard, and ran it on a real window.

| Place | What happened with no guard | Now |
| --- | --- | --- |
| The main window's `Closed` | Avalonia runs it through `Dispatcher.Send`, so it reached the app's `UnhandledException` handler. The message and exit code 1 came through | The same, through `Fatal.Guard` |
| `WindowLimits.OnMessage`, main and panels | It crossed the native boundary. The process died with exit code -1073740791 (`0xC0000409`). A WinExe has no console, so a user sees nothing | Message, exit code 1 |
| `PositionChanged`, main and panels | The same death | Message, exit code 1 |

So the `Closed` part of the finding was not a fault, and the message-hook part was. `Fatal.Raise` sets the exit code at once and shows the message, because on the way out the UI thread may not run another job.

### Panels and screens

`PanelLayout.Clamp`, `IsReachable` and `OnScreen` port the Mac's. A screen change brings every panel no screen can reach fully back onto the best screen, shown or hidden. A move from outside keeps a panel's title strip reachable. The Mac's rule that keeps panels apart is left for W8 or later.

A move uses a smaller step than the Mac's `userMoved`: the shortest move that keeps the title strip reachable, and not a move fully onto the screen. Windows places a dragged window by the pointer on every move, so a move fully onto the screen would jump back and forth under a pointer held past the edge.

### Fix round results, not done-test results

The PC was 20% busy with other work, so the speed numbers do not count, and the pan trace printed NOT RUN. C: had over 40 GB free, and the runs passed `--ignore-disk` and `--allow-busy` all the same. The harness ran as two commands, so the summary lines differ from one full run.

| Run | Result |
| --- | --- |
| App tests | 352 passed, 0 failed |
| `cargo xtask check --tier fast` | PASS in 23 s |
| AOT headless run, by itself, not the milestone level | `Failed: 0, Passed: 253, Skipped: 0, Total: 253, 177 test methods on the AOT build` |
| Pan trace, 30 s, PC 20% busy | Average 60.0 fps, longest stall 33.4 ms, 1,803 frames, first frame 5 ms after the start, last frame after the end, GPU 26% during, 2,280 of 2,280 real pixels right after the trace. Printed NOT RUN, because the PC was busy |
| Report, light and dark | PASS. Checks: hit test 15, panels 46, canvas input 36, menus and options 9, icons 16, window minimum size 1 |
| Pixel read-back, 7 launches | PASS |
| Screen pixels, 7 launches | PASS. 56 points judged |
| Launch paint, 7 launches | PASS. 98 pixels before a first frame, over 3 launches |
| Color check, light and dark | PASS. 18 patches, sRGB |
| App exit, 5 states | PASS. Each exit code 0, in 181 to 229 ms |
| Fatal path, 5 places | PASS. Each exit code 1 with the message |
| Small screen, 1000 by 620 | PASS. The status bar ends at 612 of 612 |
| Window shots at 100% | PASS |
| Window shots at 200% | NOT RUN. The display is at 100% |

Not proven by a real run: a drag with the real mouse, because tests do not move the pointer, so a panel's move came from `SetWindowPos`; a real monitor going away, so the screen list was replaced and `WM_DISPLAYCHANGE` was sent to the main window; and a real display at 150% or 200%, so the size math is proven by tests and by the `--test-work-area` hook.

A pan trace on a quiet PC still needs root's run.
