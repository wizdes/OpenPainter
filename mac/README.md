# OpenPainter for Mac

The app is an XcodeGen project (`mac/project.yml`). The `.xcodeproj` is generated; never edit it by hand.

## Build order

1. The Rust staticlib `engine/target/release/libop_ffi.a` (and its header `engine/op-ffi/include/openpainter.h`). `mac/scripts/build-engine.sh` runs `cargo build -p op-ffi --release` with a clean environment and `MACOSX_DEPLOYMENT_TARGET=15.0`.
2. `xcodegen generate --spec mac/project.yml`.
3. `xcodebuild`: the `EngineBridge` package, the `OpenPainterKit` framework, then the `OpenPainter` app.

`make -C mac` does all three (Release). The `OpenPainterKit` target also runs step 1 as its first build phase, so a build from Xcode picks up Rust changes.

| Command | What it does |
| --- | --- |
| `make -C mac` | Engine, project, Release app at `mac/build/DerivedData/Build/Products/Release/OpenPainter.app` |
| `make -C mac test` | All XCTests, including the 30 s trace gate |
| `make -C mac launch-time` | `scripts/launch-time.sh` on the Release app (median of 5 cold launches) |
| `make -C mac trace` | Writes a 6000x4000 PNG to `mac/build/`, then runs `OpenPainter --trace` on it |

Bundle id `dev.yili.OpenPainter`, macOS 15, arm64, ad-hoc signed.

## Layout

- `OpenPainter/`: the thin app target (menus, `NSDocument`, one window per document, launch arguments).
- `OpenPainterKit/Sources/`: the canvas (`MetalCanvasView`, `CanvasContainerView`), the zoom presets and viewport math, the trace runner and the launch clock.
- `EngineBridge/`: the Swift wrapper over the C interface (`View.swift` wraps `op_view_*`).
- `Tests/OpenPainterKitTests/`: XCTests. No XCUITest yet (macOS Automation Mode is not enabled on this Mac).

## Canvas

- The canvas is an `NSView` backed by a `CAMetalLayer`. The engine retains the layer (`op_view_attach`) and draws into it on its render thread (path A, `docs/presenter-decision.md`).
- Swift sends the view size, backing scale, zoom, scroll origin and the window screen's ICC profile with `op_view_set`. It sends again on `NSWindow.didChangeScreenNotification`, `didChangeScreenProfileNotification`, `didChangeBackingPropertiesNotification`, `NSScreen.colorSpaceDidChangeNotification` and screen parameter changes. Sleep sets `OP_VIEW_SUSPENDED` (the engine drops the surface); wake clears it.
- Checkerboard: 8 x 8 point squares, white and 80% grey, fixed in screen space.
- At 100% and above the engine samples the nearest document pixel. Below 100% it reads a mip pyramid (premultiplied 2 x 2 box filter, built on the GPU the first time the view zooms out) and box-filters 1 or 2 texels of the chosen level. No trilinear blend between levels.
- The pixel grid (View > Pixel Grid) shows at 400% and above. It is drawn after the display table, in display space.
- 100% means one document pixel per point.

## Zoom and keys

Zoom steps are paint.net's `ScaleFactor` presets (1% to 3200%), ported from OpenPDN `src/Core/ScaleFactor.cs` (MIT, see `THIRD_PARTY.md`).

| Command | paint.net (Windows) | OpenPainter (Mac) |
| --- | --- | --- |
| Zoom In | Ctrl + | Cmd = |
| Zoom Out | Ctrl - | Cmd - |
| Zoom to Window | Ctrl B (3.36) or Ctrl 0 | Cmd 0 |
| Actual Size | Ctrl Shift A (3.36) | Cmd Option 0 |
| Pixel Grid | none | Cmd ' |
| Zoom with the wheel | Ctrl + wheel | Cmd + wheel, or pinch |
| Zoom tool | Z; click in, right-click out, drag a rectangle | Z; click in, Option-click (or right-click) out, drag a rectangle |
| Pan tool | H; hold Space for a temporary pan | H; hold Space for a temporary pan |

Zoom to Window fits the document inside the view less 10 points and never goes above 100% (paint.net `DocumentView.ZoomToWindow`). A Zoom tool click steps one preset and centers the clicked point (paint.net `ZoomTool`).

## Launch arguments

- `OpenPainter <file.png> ...` opens the files.
- `OpenPainter --trace <file.png>` opens the file in a 960 x 600 point window, runs the 30 s trace, prints one `TRACE ...` line and exits with 0 (gate passed) or 1.
- `OpenPainter --make-trace-png <out.png>` writes the 6000 x 4000 trace image and exits.
- The app prints `OP_READY <ms>` (milliseconds since the process started) when the first document frame is presented, or when the empty start window is shown.
