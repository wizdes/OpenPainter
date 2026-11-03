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
| 6 | Layout matches the W6 pictures, light and dark, at 100% and 200% | Root looks at each shot | `--parts shots`. 200% is not in this set yet |
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

Each part prints one line: PASS, FAIL or NOT RUN with a reason. The last line is `HARNESS: PASS` or `HARNESS: FAIL` with the names. Any part that did not pass fails the run.

Before any part, the harness runs `scripts/windows/preflight.ps1 -Level window`, then looks for known dialogs: `consent`, `CredentialUIBroker`, `WerFault` and `WerFaultSecure`, when they have a window. A locked screen, a known dialog, a wrong session or under 40 GB free prints `UI TESTS: NOT RUN (reason)`, and every part prints NOT RUN.

Switches for a proof run. The window level passes none of them:

| Switch | What it does |
| --- | --- |
| `--parts report,pixels,color,shots,pan` | Runs only these parts |
| `--pan-seconds <s>` | A shorter trace. The part then prints NOT RUN with its numbers |
| `--allow-busy` | Runs the trace on a busy PC. The part then prints NOT RUN with its numbers |
| `--first-frame-runs <n>` | How many launches `pixels` measures. The default is 3 |
| `--ignore-disk` | Goes on with under 40 GB free on C: |

### How each part works

**Rules for all of them (PLAN 4.3).** Every run passes a temp `--data-root`. The harness fails if the real settings folders change. Pictures come from `PrintWindow` on one of the app's own windows. A screen pixel is read only after `WindowFromPoint` says the app's own window is there. Input is messages posted to the canvas's own window. `SendInput` and moving the pointer are not used, and a test fails if the harness or the probe calls them. The app's windows rise above other programs with no focus change.

**Pan trace.** The harness writes a 6000 by 4000 PNG to its temp folder. The app opens it through `--test-load-png` at 100%. The Pan tool then drags along a fixed path, posted from its own thread 250 times a second. Each axis runs a triangle wave, so the pointer never rests. A pointer at rest asks for no frame, and that would look like a stall. Frame times come from `op_view_take_frame_times`, which the engine stamps on its render thread. No Avalonia callback is in the count. The part fails if the frames cover under 98% of the time asked, the average is under 59 fps, or a gap is over 50 ms. A PC at 15% CPU or more before the start prints NOT RUN, because a busy run does not count.

**Pixel read-back.** The test image is flat colored cells from a formula, and no cell is near black. The harness knows what each document pixel must be. After the first-frame signal it captures the main window 5 times, and each capture's grid of up to 2,560 points must match within 1 of 255. It also reads the real screen at the canvas's middle, and that pixel must match too. It does this for 3 launches of a small image, 24 MP at 100%, and 24 MP at Fit. Before the signal it counts black captures and black screen pixels, so the time a black canvas shows is measured.

**First-frame signal.** `--test-first-frame <file>` makes the app write the file once the engine has presented a frame at the asked zoom. The harness waits for that file, then calls `DwmFlush` twice. No part waits a fixed time for the first frame.

**Color check.** The image has 18 patches: primaries, secondaries, a gray ramp and mixed colors. Each must match within 1 of 255 at 100%. The stage around the image must match the palette, so no seam shows beside Avalonia's stage. Expected values exist only for an sRGB display profile. Another profile with a difference prints NOT RUN and names the profile.

**Shots.** The harness saves 6 pictures in `<out>/shots`: the main window, the File menu open and the "more" popup open, each in light and dark. Each is the app's windows painted over each other, with nothing from any other program. Root must look at them. A headless shot is never the visual check.

**The report.** `--test-report` runs as before, in light and dark, and the harness splits it into `hit test`, `panels`, `canvas input`, `menus and options`, `icons` and `window minimum size`.

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

Root writes the done-test results here.

### Builder's proof runs, not done-test results

The PC was running a 10,000-case test in another copy, so the speed numbers do not count. C: had under 40 GB free, so the runs passed `--ignore-disk`.

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

It is a real fault, and it is small. The canvas's window exists before its first frame, and nothing paints it. Reading the real screen at the canvas's middle before the first frame:

| Image | Screen pixels that were black | Canvas window to first frame |
| --- | --- | --- |
| 24 MP, two launches | 14 of 14 | About 0.7 to 1 s |
| Small image, four launches | 2 of 4. Only one pixel was read per launch | About 0.1 s |

Over 14 launches, a small image's first frame came 624 to 808 ms after launch, and a 24 MP image's 1,224 and 1,261 ms. After the signal, none of the 70 captures was black. Stage 2's one black shot in 12 at a fixed 5 s came from a slow load on a busy PC. The harness now waits on the signal, so this cannot fail a check by chance. The fault is still there for a user: a black box shows in a light window until the image is ready.
