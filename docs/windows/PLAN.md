# OpenPainter for Windows: plan

Status: revision 2, waiting for the user's OK. Written 2026-11-01 on the Windows PC. A fresh Opus reviewer read revision 1; `docs/windows/reviews/plan-review.md` records each finding. Astra did not review it, because root cannot reach the `agent` account yet (decision 1).

## 1. The short version

The Rust engine moves to Windows with modest changes. The Mac app (Swift) does not move, so Windows gets a new C# app. This PC has paint.net 5.1.12, so effects and behavior can be compared with the real thing. That comparison was blocked for the whole Mac build (old milestone M02). Fixes go into the shared engine, so the Mac app gets them too.

The work is 21 milestones, W0 to W20, about 350 agent hours. It is Tier 3: a new subsystem, file storage, signing and a release. It uses its own worktree, a full test run and fresh review at each phase end, and a change diagram.

```
Engine (Rust, shared) --> C interface --> Mac app (Swift, exists)
                                     \--> Windows app (C#, new)

paint.net 5.1.12 --> pixel captures, behavior checks --> engine fixes, Windows app fixes
```

## 2. What I measured on this PC

| Fact | Result |
| --- | --- |
| paint.net | 5.1.12 installed (file version 5.112.9563.32325), on .NET 9. The user's copy is running. 5.2 test builds have existed since May 2026, so a pinned copy is needed. |
| Machine | Windows 11 Home, i7-11700F, 64 GB RAM, RTX 3060 Ti (8 GB), 150 GB free. One screen, no pen found. |
| Tools | Rust 1.99.1, Visual Studio 2022 Build Tools, .NET SDK 9.0.319. .NET 9 support ends 2026-11-10, so W0 installs .NET 10. |
| Engine compile | 2 source files stop it: `op-core/src/tiles.rs` and `op-engine/src/recovery.rs`. |
| Test compile | 4 test files, `xtask` and the fuzz example use Unix-only calls. |
| GPU | wgpu 27 can draw into a Windows window or SwapChainPanel. The view host asks for Metal only (`op-ffi/src/view.rs:218`). Effects use `Instance::default()` (`op-gpu/src/lib.rs:707`), so the backend must be pinned. |
| Mac app size | About 20,000 lines of Swift, 16,760 lines of Swift tests, about 395 Swift tests. Engine: about 69,000 lines of Rust, 675 tests. |
| Oracle gap | `tests/oracle/targets.toml` has 55 effect rows (41 from paint.net 3.36, 14 our own), and 0 from 5.1. The menu catalog came from web docs, not the app. |
| Tools never connected on the Mac | Gradient, Line/Curve, Shapes, Clone Stamp, Recolor, Healing Brush, Spot Heal. |

I also ran the engine's tests on Windows in a throwaway copy of commit 5f1b61b, with small patches for the 2 source files. I removed 4 test files that do not compile here (`opaint_limits`, `release_review`, `recovery_fixes`, `huge_document`) and left out `xtask` and `op-present-spike`. Result: **568 passed, 42 failed, 6 ignored, in 344 s.** This is a first look, not a pass of any milestone.

| Failures | Tests | What I saw |
| --- | --- | --- |
| Saving and recovery | 38 | The ones I read stop on "Access is denied" (os error 5). `sync_dir` in `recovery.rs` opens a folder as a file, which Windows refuses by default. Not yet proven for all 38. |
| Mandelbrot Fractal | 2 | Up to 23 of 255 levels off the reference. Cause unknown. First suspect: a different math library for `f64` functions. |
| GPU color table against CPU | 1 | Worst error 0.558. I did not record which GPU backend ran. |
| Damaged ICC profile | 1 | A panic inside the `lcms2` crate (`profile.rs:307`). A bad image could crash the app. |

Whole test files marked Mac-only (`views`, `live_stroke`, `low_disk`) did not run. The reviewer also proved a design problem. Windows refuses to rename a folder while a file inside it is open, and the recovery code does exactly that (`recovery.rs:659`). W2 is there for this.

## 3. Choices in this plan

### 3.1 UI stack: start with Avalonia, decide with W5

Avalonia 12 with C# and NativeAOT (C# compiled to one native program) is my working pick, with medium confidence. W5 tests it against WinUI 3 before any app work.

| What matters | Avalonia 12 | WinUI 3 |
| --- | --- | --- |
| UI tests with no window on screen | Yes: a headless mode with real layout, input and rendered frames | No: tests need a real window |
| Tests as builder account `agent` over SSH | Should work, W0 proves it | Not expected |
| Linux later | Same UI code | A second app |
| **Controls over the canvas** | **Not over a native child window.** Marching ants, handles, crop dim and the text caret must move into the engine's view shader, or use a shared GPU texture. | **Yes**, XAML draws over SwapChainPanel |
| Windows look | Own drawing, Fluent theme | The real Windows look |
| Empty-app launch, one public test, not this PC, no AOT | 757 ms (Avalonia 11.3) | 308 ms (WinUI 1.7) |

The Mac draws those overlays in Swift (`mac/OpenPainterKit/Sources/Shell/SelectionOverlay.swift`), so WinUI has a real advantage here. Avalonia stays first because headless testing removes most of the need to touch the user's screen, and that was QuickEdit's largest source of trouble. WPF lost because its Native AOT is not ready. All-Rust Win32 lost because every control and screen-reader path would be hand-built.

W5 chooses WinUI 3 if Avalonia fails any of: overlays at full frame rate, pen pressure, text input with IME, Narrator, launch budget, or tests against the AOT build.

### 3.2 How paint.net is used

- **Own pinned copy.** The portable 5.1.12 ZIP goes to `C:\agent-work\pdn-5.1.12`, updates off, hash recorded. Only root runs it.
- **The user's copy is never touched.** Stop processes by PID only, never by name. W0 tests safely whether a second copy hands work to the user's window, and whether portable mode writes to `%LOCALAPPDATA%`. If either happens, captures run only while the user's copy is closed.
- **Outputs and behavior only.** Its license forbids derivative works. We compare pixels and behavior. We never decompile it or load private code, and we never copy its icons, help text or resources. Standard menu names stay as a compatibility list. The OpenPDN `.resx`, `.png` and GPC ban still applies.
- **Captures use the public plugin API first.** The API exposes built-in effects to plugins, and CodeLab shows a plugin running an effect through `CreateInstance` and `CreateRenderer`. W4 tries this first, then UI Automation, then the OpenPDN oracle.
- **Tools keep our own goldens.** `docs/ARCHITECTURE.md:205` says "Never capture pixels" for tools. Tools get reviewed shots and behavior checks against paint.net.
- **The product page and Store listing never name paint.net** (user decision, 2026-10-25). W19 scans for it.

### 3.3 What "improve the code" means

`docs/windows/parity.md` starts in W4 and grows at every milestone, as QuickEdit's did. Each row ends as "same", "better (why)", or "left out on purpose (why)". For effects, the bar is the user's ±1/255. An effect that cannot reach it goes on a list for the user. A builder cannot pass it with a written reason, because loosening a tolerance needs the user's OK. Old `legacy-3.36` goldens stay in the repo for the record.

## 4. How testing works (from QuickEdit's questions)

I could not find a saved "QuickEdit prompt" on this PC. The Mac session logs are not here, and `prompt_library` has no QuickEdit file. I used the questions and answers in `Projects/QuickEdit/docs/decisions.md`, `docs/plan.md`, `docs/STATE.md`, `POST_MVP.md` and the memory note.

### 4.1 Test levels

These extend the existing `cargo xtask check --tier`.

| Level | When | Time | What |
| --- | --- | --- | --- |
| fast | each change | under 2 min | Changed crates, C# unit tests, headless UI for the changed area |
| milestone | each milestone | under 10 min | Plus all headless UI, on the AOT build, and 9 to 12 critical flows |
| window | each milestone, when the screen is free | under 15 min | Real windows: critical flows and one shot per changed screen |
| checkpoint | phase end | about 2 h, in parts | All real-window flows, GPU matrix on RTX and on WARP (software GPU), paint.net comparison, fuzz, Mac and Linux runs |
| speed | release | root only | Launch, size, open, blur and brush next to paint.net, n=5 medians |

The GPU matrix alone took 1,969 s on the Mac, so it has its own part. A part that is not reached prints NOT RUN and fails the level.

### 4.2 What each QuickEdit answer changes

| QuickEdit answer | What Windows OpenPainter does |
| --- | --- |
| U11: as few test runs as possible until the end | Builders run only tests for what they change. Matches the OpenPainter rule of 2026-10-24. |
| U6: a quick check under 10 minutes, full run only for a release | The milestone level, with a named list of critical flows |
| U5: a listed flaky test may pass alone, if recorded | A retry counts only for a test on `docs/windows/flaky.md`, and each is written down. Any other retry is a failure. (Choice to check.) |
| U8: a flaky contrast audit became a unit test at 4.5:1 | A contrast unit test covers every window in light and dark. Accessibility Insights runs at W18. |
| U9, U12: do not wait; one review was skipped | Keep going. Reviews run at phase ends, and a skipped review is reported as skipped. |
| U13: the speed run was skipped, then stayed unrun for 5 releases | The release report prints the speed result or NOT RUN. |
| U1: no real printing, because a print prompt broke later tests | Print is not in this version. No test opens a system print or network dialog. |
| V1 to V4: Astra and Fable vote on hard questions | Same, in `docs/windows/decisions.md`. Root breaks a tie. |
| QuickEdit's away log and "NOT RUN" rule | A milestone closes on headless checks, marked "window checks NOT RUN". A phase cannot close until they run or the user waives them. |

### 4.3 QuickEdit's traps, with the Windows rule

| What went wrong | Rule here |
| --- | --- |
| A UI test typed into the user's Claude terminal, and it arrived as a user message | Tests use UI Automation actions and messages to the app's own window. A lint bans raw `SendInput`, except in one helper that checks the front window before each key. |
| Full-screen shots showed the user's desktop | Window-only captures. Pixel checks read engine data or saved files. Root looks at each shot first. |
| Offscreen shots drew tab bars as black bars | A headless shot is never the visual check. |
| A locked screen hung UI tests, and one lock check lied | `preflight.ps1` runs first and prints `UI TESTS: NOT RUN (reason)`. |
| A stray system prompt broke later tests | No network calls in tests, update checks off, and the preflight looks for known dialogs. |
| Tests left 3,837 settings files behind | Every test passes `--data-root <temp>`. The run fails if the real settings folder changed. |
| Another job's memory use spoiled speed numbers | Speed runs record load, memory and GPU use (LM Studio, Sunshine). Busy runs are reported and thrown out. |
| The suite outgrew its time limit | The run prints what it did not reach, and fails. |
| A reviewer read a repo that root was editing | Reviewers get their own clone. |
| Tool-only milestones never reached the app (OpenPainter) | W12 and W13 connect every engine tool. |
| The disk filled with 23 GB of build cache (OpenPainter) | `cargo clean` on merged clones. The preflight stops under 40 GB free. |

### 4.4 Keeping windows off the user's screen

Each test runs at the lowest layer that proves it.

1. **Headless.** Avalonia headless, runs as `agent`. Most UI tests live here. W5 checks what these tests can see of the canvas.
2. **Hidden desktop.** A real window on a second Windows desktop (`CreateDesktop`). W0 tests DX12 drawing, focus, UI Automation and capture there. It may not work.
3. **Visible desktop.** Only what the others cannot prove, while the user is away with the screen unlocked (decision 4).

Layers 2 and 3 run only as root, as `yjli`, and only on code root has already read and merged. Builders never run them. Heavy runs use below-normal priority and at most 8 build jobs. GPU runs check first that Sunshine and LM Studio are not busy. Sleep and GPU reset are simulated through a test hook, not done for real. Scaling tests use a test setting, not the system setting.

## 5. The milestones

Each milestone ends with a working build, a short hands-on script, and a row of speed numbers. Budgets are agent hours. When a milestone runs 50% over, root sends a note and cuts back to the done-test.

### Phase A: engine on Windows, and the reference (52 h)

| # | Milestone | What it adds | Done-test | Budget | Needs |
| --- | --- | --- | --- | --- | --- |
| W0 | Machine and test setup | .NET 10 for both accounts. Pinned paint.net. `preflight.ps1`. Runner refresh. `.gitattributes`. Check tiers in `xtask`. Probes: hidden desktop, GPU over SSH as `agent`, second paint.net copy. | The preflight passes. A headless Avalonia test passes as `agent`. A one-page result says what each probe found. | 6 h | D1 |
| W1 | Engine files and tests | Positional file reads, folder sync, atomic replace with retries when antivirus holds a file, drive letters, long paths, DLL build. Port all Unix-only tests, and the fuzz driver. Guard the `lcms2` panic. Find the Mandelbrot cause. | Per-file test counts are compared with the Mac; every file that does not run is named. All non-GPU tests pass. GPU tests are listed as waiting for W3. | 10 h | W0 |
| W2 | Recovery redesign | Lock file moved outside the record folder, close-before-rename, a new same-file check, `TerminateProcess` kill test. `ARCHITECTURE.md` changes first. Mac behavior is kept. | Kill before, during and after a record write recovers on Windows and on the Mac. Fresh review of this change. | 8 h | W1 |
| W3 | GPU and canvas on DX12 | One pinned backend for both device paths, WARP selector, backend assert. Small Win32 test host. Window handle in `op_view_attach`. Display profile from Windows. Device-lost hook. Shader compiler choice. A guard against long GPU jobs that trigger a Windows GPU reset. | M54 matrix 0 failures on RTX and WARP, and the test fails unless DX12 or WARP ran. Ported resize, switch and create-destroy scenarios pass. Color within ΔE00 1 of direct lcms2 transforms. Mac view tests still pass. | 14 h | W1 |
| W4 | paint.net capture kit (old M02) | Plugin or driver. The real 5.1.12 catalog. `targets.toml` gains `pdn-5.1`. Private oracle repo. `parity.md` started. Fallback list. | One effect per family, twice, with the same hashes. The manifest pins build, parameters, ICC, inputs, GPU and driver. Catalog and fallback list committed. | 14 h | W0 |

### Phase B: the Windows app (122 h)

| # | Milestone | What it adds | Done-test | Budget | Needs |
| --- | --- | --- | --- | --- | --- |
| W5 | UI stack test | The same small shell in both stacks: canvas, marching ants, floating panel, menu, dialog, pen log, IME field. On AOT builds. | A table of launch, size, resize, overlay frame rate, Narrator, and what headless tests see. Astra and Fable vote. Written in `decisions.md` and `ARCHITECTURE.md`. | 12 h | W3 |
| W6 | Windows design and icons | At least 3 rendered mockups next to real paint.net shots, from `docs/design/SPEC.md`. Critique plus adversarial review. New icon set: open license only, no SF Symbols, no paint.net icons. | A side-by-side page with a pick. All icons exist with license records. | 8 h | W4, W5 |
| W7 | App shell and canvas | `windows/` project. C# bridge generated from `opcli dump-catalog`. Window, strip, status bar, command row of 7 buttons, empty panels, menu bar, Zoom, Pan. Overlay path. Test vectors exported from the Swift tests. Harness. | Open a 24 MP PNG; pans at 60 fps on the RTX. Color check on the live window. Menu order matches the W4 catalog. | 14 h | W5, W6 |
| W8 | Panels | Tools, History, Layers, Colors, Layer Properties, palettes, Windows shortcuts. Color math passes the Swift vectors. | Every panel action is tested headless. Shortcuts match the catalog with 0 differences. | 12 h | W7 |
| W9 | Core tools | Selection tools and Wand first, then Brush, Eraser, Pencil, Bucket, Picker, Move, Crop, options row. Windows pointer input for pen. Overlays. | Replayed pointer logs give the engine's goldens. Undo returns every byte. Real pen is NOT RUN (no pen here). | 16 h | W8 |
| W10 | Files | New, Open, Save, Save As, Save a Copy, Revert, Save All, Recent, drag and drop, clipboard, close prompts, save options, GPS and metadata strip, recovery sheet, file association, opening a second file in the running app. Windows Imaging codecs, with a clear message when one is missing. Local app data for recovery. | Save and reopen keep every byte. Kill mid-edit recovers. A codec table passes. Long paths are tested with the system setting's value recorded. | 14 h | W9, W2 |
| W11 | Effects and menus complete | Dialogs from the schema, live preview, repeat-last, all Edit, Image, Layers, Adjustments and Effects items, Color Profile dialogs, Content-Aware Fill. | Every catalog item works or has a parity row. Preview equals the result. Cancel changes nothing. | 10 h | W9, W4 |
| W12 | The 7 tools the Mac never connected | Gradient, Line/Curve, Shapes, Clone Stamp, Recolor, Healing Brush, Spot Heal. New C interface paths and new design. | Each is usable from its panel button, with live preview and Finish, and passes engine goldens or reviewed shots. | 20 h | W9 |
| W13 | Text | DirectWrite layout, IME, right-to-left text, emoji, the text tool's live editing. | Same gates as Mac M46: IME, bidi and emoji reviewed goldens. Caret overlay at full frame rate. | 16 h | W9 |

### Phase C: compare with paint.net, and improve (136 h)

| # | Milestone | What it adds | Done-test | Budget | Needs |
| --- | --- | --- | --- | --- | --- |
| W14 | Effect comparison and engine fixes | Full parameter matrix from 5.1.12. Report per effect, then the fixes, CPU and GPU, by family, side by side. Root captures, and builders get the images in their clone. | Every effect has a `pdn-5.1` row and a measured error at ±1/255, or sits on the user's list. Mac tests rerun. The budget is an estimate; the first 10 h produce the report, and root resets the budget from it. | 120 h | W3, W4, D3 |
| W15 | Behavior sweep | The same scripted steps in both apps for menus, options, dialogs and non-pointer results. Fixes. | Every row in `parity.md` has one of the 3 endings. | 16 h | W10 to W14 |

### Phase D: finish and release (42 h)

| # | Milestone | What it adds | Done-test | Budget | Needs |
| --- | --- | --- | --- | --- | --- |
| W16 | View, Settings, background removal | Rulers, grid, zoom presets, light and dark, scaling, Settings. Remove Background with a model downloaded on request; MIT or Apache license only. | Rulers correct at 100%, 150%, 200% through a test setting. IoU 0.9 on the 10 photos, or the user decides. Mixed screens NOT RUN (one screen here). | 10 h | W9 |
| W17 | Translation-ready text | All strings in resource files, as Mac M59 did. | A scan finds no hard-coded UI text. | 2 h | W11 |
| W18 | Keyboard and accessibility | Full keyboard use, focus across panels, Narrator names. | A keyboard-only test passes. Accessibility Insights has no errors. The Narrator walk (it speaks aloud) runs in decision 4's time. | 6 h | W10, W11 |
| W19 | Speed, sturdiness, release | Launch work, Windows fuzz, 200 MP, low disk, simulated sleep and GPU reset, 1,000-step history. Icon, MSIX, portable ZIP, update check, page. | Launch under 3 s, app under 75 MB. DLL dependency scan. No banned name on the page or listing. A second account runs it (decision 5). The page deploys from a Mac session only. | 18 h | W15, W18 |
| W20 | Mac and Linux catch-up | Mac test run and a Mac release with the engine fixes. Linux engine checks. | Mac and Linux suites pass. Mac release notes list the effect changes. Runs from a Mac session. | 6 h | W19 |

### Order

```
W0 +-- W1 +-- W2 ---------------------------+
   |      +-- W3 +-- W5 +-- W6 -- W7 -- W8 -- W9 +-- W10 ---+
   |             |      |                        +-- W11 ---+-- W15 -- W19 -- W20
   +-- W4 -------+------+                        +-- W12 ---+
          |      |                               +-- W13 ---+
          +------+-- W14 ------------------------------------+
```

W14 runs beside the app work and needs no UI. W16, W17 and W18 run after W9, W11 and W10 respectively, and feed W19. Phase ends: W4, W13, W15, W20.

## 6. How each milestone runs

1. Root writes `docs/windows/plans/W<n>.md`: owned files, checks, and "not in this version".
2. One builder makes the milestone in its own clone, split only above about 3,000 changed lines. Builders run Sol through the runner as `agent` on this PC. Opus is not an option: the runner refuses Claude on Windows, and a native builder would run beside `~/keys`. If no slot is free, root waits for one. If Sol fails, root builds it (workspace rule when every route is blocked). Rust-only work may also go to Linux builders once decision 2 allows it.
3. Root runs the milestone level, then the window level when possible, and looks at the shots.
4. Root merges to branch `windows`, updates `docs/windows/STATE.md` and pushes. Mac changes wait for phase ends.
5. At a phase end: the checkpoint level, Astra plus a fresh Opus review, a change diagram, an update page, and a merge to main. The review deadline is set at launch: 900 s for Phase A, and 30 minutes for Phase B and C, since these reviews cover more code.
6. A builder never commits, pushes or uses SSH. Root never deploys from this PC.

## 7. Not in this version

- Linux UI, Windows on ARM, Windows 10 (no machine to test it).
- `.pdn` import, PSD, plugins, scripting, batch work, layer masks, adjustment layers.
- Printing, shell thumbnails, custom shortcut editing.
- New Mac features. The Mac gets engine fixes only.
- paint.net 5.2's new effects.

## 8. Risks

| Risk | What I do about it |
| --- | --- |
| The overlay path costs more than planned | W5 measures it before the app is built. |
| The hidden desktop fails | Headless first; window checks run in the user's away time, and phases stay open until then. |
| W14 is the largest unknown: paint.net 5 rewrote most effects, and we work from outputs alone | Report first, then the budget. A family that stays far off goes to the user. |
| paint.net effects use Direct2D, and a driver update could change the hashes | The manifest pins GPU and driver. Root does not update the driver during W14. |
| Engine changes break the Mac app, and root here cannot reach a Mac | Decision 2, or a Mac session at each phase end. Reported as NOT RUN until then. |
| The app looks too close to paint.net for the Store or its owner | Our own icons and design tokens. This is not legal advice; the user decides at W19. |
| The Store takes days or rejects the app | The portable ZIP ships first. Its page states the SmartScreen warning. |
| A pending Windows reboot, from the Build Tools install on 2026-10-30 | Not needed for this plan so far. Root asks before any reboot. |

## 9. Decisions for the user

1. **SSH key for `agent` on this PC (needs your OK).** Root makes one key for `yjli` and adds it to `agent` here. Without it, Sol and Astra cannot run, and root builds everything itself. Recommended: yes.
2. **The same key on the Macs and the Linux box (needs your OK).** This lets root run Mac and Linux checks from here. Without it, a Mac session runs them at phase ends. Recommended: yes.
3. **The engine follows paint.net 5.1.12 when they differ.** Mac results change too, and 41 effect targets move from `legacy-3.36` to `pdn-5.1`. Recommended: yes, per "paint.net is the gold standard".
4. **A time window for real windows on your screen.** The PC stays unlocked, and I run only what headless and the hidden desktop cannot prove. Narrator will speak aloud. Recommended: yes, tell me the hours.
5. **How to ship.** Free Microsoft Store MSIX plus a portable ZIP, with no new spending. You log in to the Store account, and root makes one extra Windows account for the install test. Azure signing would cost about $9.99 a month, and SmartScreen would still warn at first. Recommended: Store plus ZIP.
6. **Captured images stay in a private repo.** People without access see those comparison tests report NOT RUN. It needs `gh auth login` on this PC. Recommended: yes.
7. **Design pick at W6.** The 2026-10-25 no-prompts rule says I take my recommendation and continue. A new platform's look may deserve your eye. Recommended: I publish the page, continue with my pick, and you can change it later.

## 10. Choices to check

- U5's flaky-test retry rule carries over only for tests on the list.
- Update pages come at phase ends and design picks, following `agent-config/shared/PAGES.md` and the old every-5-milestones rule, not at every milestone.
- Windows 11 x64 is the only tested system.
- Standard menu names stay in the public catalog as a compatibility list.
- On Windows, when Sol has no slot, root waits instead of starting a native Opus builder as the workspace rule says, because of the `~/keys` wall.
