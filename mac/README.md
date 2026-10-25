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

- `OpenPainter/`: the thin app target (`NSDocument`, launch arguments, installs the menu).
- `OpenPainterKit/Sources/`: the canvas (`MetalCanvasView`, `CanvasContainerView`), the zoom presets and viewport math, the trace runner and the launch clock.
- `OpenPainterKit/Sources/Shell/`: the main window, panels, menus, tool state and focus rules (M15).
- `OpenPainterKit/Sources/PlatformServices/`: ImageIO import and export, and the file-open routing (M17).
- `EngineBridge/`: the Swift wrapper over the C interface (`View.swift` wraps `op_view_*`).
- `Tests/OpenPainterKitTests/`: XCTests. No XCUITest yet (macOS Automation Mode is not enabled on this Mac).

## Main window (M15)

`OpenPainterKit/Sources/Shell/` holds the Classic v2 main window (`docs/design/SPEC.md`). The app target only makes one `Editor` and one `EditorWindowController` per `NSDocument`.

- `Workspace`: the open images in strip order, the active one, the shared `ToolState`, the four panels and the effect catalog (`effects.list`). It reads engine events and refreshes the editor they name.
- `Editor`: one image. It caches `doc.info`, `doc.selection` and `commands.list`, makes the strip thumbnail from the engine composite, and runs every action as an engine command (`tool.stroke`, `tool.magic_wand`, `tool.paint_bucket`, `selection.select_all`, `selection.deselect`, `layer.set_active`, `history.jump`, the image and effect commands).
- `EditorWindowController`: the unified toolbar (title with the system Edited state; New, Open, Save, Print | Cut, Copy, Paste | Crop to Selection, Deselect | Undo, Redo | Pixel Grid, Rulers; the four panel toggles), the options row, the image strip, the stage and the status bar.
- `PanelManager` and `PanelLayout`: the Tools, History, Layers and Colors `NSPanel`s (utility, non-activating, hidden while the app is inactive, close button on the left). Frames are saved per display arrangement in `UserDefaults` (`OPPanels.<screen frames>`). A panel whose title strip is on no screen moves onto the screen it overlaps most. A moved panel moves clear of the others. Window > Reset Panel Layout puts them back.
- `MenuSkeleton`: the menu bar in paint.net 5.1 order. Its header lists where Mac keys replace paint.net keys. `Tests/OpenPainterKitTests/menu-skeleton.txt` is the checked-in item list; change it together with the menu.
- The fit rectangle is the stage less the shown panels (left and right reserves) less 16 pt. Fit recomputes when the window resizes or a panel moves, shows or hides. A zoom the user chose stays.

## Colors panel and Color Picker (M36)

- `Shell/ColorsPanel.swift`: the Colors panel. Primary and secondary swatches (the notch marks the active slot; click the other swatch to move it, C on the canvas does the same), swap (X), black and white reset, the hex field, the HSV wheel, More » / « Less (RGB, HSV and alpha sliders with fields), and the palette (32 swatches compact, 96 expanded). More grows the panel up to 470 pt and shortens Tools to keep a 12 pt gap (`PanelLayout.colorsFrames`).
- Wheel keys (paint.net Ctrl, Alt, Shift): Command keeps the saturation, Option keeps the hue, Shift snaps the hue to 15 degree spokes, Command-Shift does both. Control-click is the Mac right-click, so it sets the other slot.
- Palette files are paint.net's: one `aarrggbb` per line, `;` comments, at most 96 colors, missing slots white. Malformed lines are skipped and named in the status bar.
- `Shell/ColorModel.swift`: HSV math, the wheel rules, palette parsing and the default palette.
- `Shell/ColorPickerTool.swift`: the Color Picker. Click sets primary, right-click or Control-click secondary, Command-click samples the image. The engine query `doc.sample_color` averages a square window (1 to 51 px) clipped at the canvas edges, weighted by alpha.
- Right-button (or Control) drags and Paint Bucket clicks use the secondary color.

## Layers and History panels (M20)

- `Shell/LayersPanel.swift`: one 49 pt row per layer, top layer first: a 40 x 40 pt thumbnail, the name, a detail line (Hidden, blend mode, opacity) when it is not plain, the 24 x 24 pt eye, and the active row's 2 pt edge and diamond. The action row runs Add New, Delete, Duplicate, Merge Down, Move Up, Move Down and Properties. Dragging a row runs one `layer.move`. Clicking a row runs `layer.set_active`, which replaces the current history step's state: no new step and no change to the Edited state.
- Thumbnails come from the engine query `layer.thumbnail` (`{"id","max"}`: width, height, the layer's content key, then RGBA). `LayerThumbnailCache` keys them by document, layer id and that content key (`revision` in `doc.info` layers), so an edit makes a new one and a History jump back reuses the old one.
- `Shell/LayerProperties.swift`: the Layer Properties sheet (name, visible, the 14 blend modes, opacity 0 to 255 as a slider and a field). The first change starts an engine preview session (`preview.begin` with `layer.properties`); later changes send `preview.update`, and the canvas shows the preview. OK sends `preview.commit` (one "Layer Properties" step); Cancel sends `preview.cancel`. Panel and menu layer commands are off while the sheet is up.
- `Shell/HistoryPanel.swift`: one 27 pt row per step with an icon for its command (`history_commands` in `doc.info`). Undone steps use `muted` text and a redo arrow. A click runs `history.jump`.

## Brush input and live strokes (M38)

- `PenInput.swift`: the canvas turns each mouse, tablet or Force Touch trackpad event into a `PenSample` (document pixels, pressure, tilt, event time). A mouse is pressure 1. Tablet events (`NSEvent.subtype == .tabletPoint`) read the 16-bit tablet pressure field and `tilt`. A trackpad uses the last `pressureChange` pressure of the drag (the stroke tool sets `NSPressureConfiguration(.primaryGeneric)`, so there is no force click).
- Mouse coalescing is off during a brush drag, so every pen sample arrives; it is restored on release or Escape.
- `LiveStroke` collects the samples of one main run-loop turn and sends them in one `op_stroke_append` (`op_stroke_begin` at the press, `op_stroke_end` at the release: one history step named after the tool; Escape sends `op_stroke_cancel`, which leaves the bytes and the history as they were). The canvas shows the engine's preview; there is no stand-in line. `Shell/BrushStroke.swift` connects the canvas to the window's tool options.
- The engine view uploads only the tiles a change touched (`region_uploads` in `op_view_info`) and wakes at once after an append.
- Tool options (paint.net 5): Paintbrush has width, hardness, antialiasing, blend mode, opacity, and pen pressure for size and for opacity. Eraser has the same without blend mode. Pencil (always 1 px, hard, aliased) has blend mode, opacity and pen pressure for opacity.
- `TabletLog.swift`: the JSON tablet log (`t`, `x`, `y`, `pressure`, `tilt`, `buttons`), a recorder (`MetalCanvasView.recorder`) and a replay that rebuilds tablet `NSEvent`s. `Tests/OpenPainterKitTests/tablet-logs/` holds the checked-in log.
- Latency: `op_view_take_frame_stamps` gives each frame's GPU-done time (CLOCK_UPTIME_RAW, the clock of `NSEvent.timestamp`) and the preview serial it shows; `PenInputTests.testPenEventToFrameLatencyOn6000x4000` prints a `LATENCY` line with the median and p95.

## Text tool (M46)

The Mac side shapes text; the engine only composites what it is given, so the engine stays portable.

- `Text/TextLayout.swift`: CoreText layout and raster. The result has coverage runs (one byte per pixel, painted with the primary color), color-glyph runs (premultiplied RGBA, such as color emoji), line metrics and baselines, grapheme cluster bounds, caret positions, the fonts used (cascade included), and the UTF-16 to UTF-8 offset mapping. Sizes are points at 96 / 72 pixels per point. A family that is not installed falls back to Helvetica and the status bar names it. Bold or italic that a family lacks is synthesized.
- `Text/TextEditingModel.swift`: the text, caret and marked (composing) range in UTF-16. Moves and deletes go by grapheme cluster.
- `Text/TextInputView.swift`: an `NSTextInputClient` view over the canvas while text is open. It draws the edit frame, the caret, the marked-text underline (thick under the input method's selected segment) and the move nub. `firstRect(forCharacterRange:)` puts the candidate window at the caret at any zoom.
- `Shell/TextToolController.swift`: the session. A click starts text at that point (Left grows right, Center both ways, Right grows left). Each edit sends `tool.text.update` with the runs, origin, primary color, blend mode, text and caret (UTF-8 bytes).
- Keys: Return makes a new line, but while an input method composes it only confirms the composition. Escape drops the composition only; with nothing composing it finishes the text, as paint.net 5 does (https://www.getpaint.net/doc/latest/TextTool.html; OpenPDN 3.36 too). Home and End go to the line ends. Escape, Command-Return, the Finish button, a click outside the text or another tool commits one "Text" history step; empty text commits nothing. Cancel has no key: the engine's `tool.text.cancel` drops the text with no history step and no pixel change. Undo while typing commits first, then undoes, so Redo brings the text back.
- Engine: `tool.text.begin`, `.update`, `.finish`, `.cancel` (`engine/op-engine/src/text_commands.rs` lists the parameters; `engine/op-tools/src/text.rs` composites). The selection clips with its coverage.

## File formats and drag and drop (M17)

The engine decodes PNG itself. ImageIO decodes HEIC, HEIF, AVIF and every Camera RAW type that `CGImageSourceCopyTypeIdentifiers()` lists (30 on macOS 27). Swift hands the engine straight 8-bit RGBA and the ICC profile through `op_import_pixels`.

- `PlatformServices/ImageFormats.swift`: which files open (the route reads the file's first bytes, so a wrong extension still opens the right way) and which types Save As offers. HEIC is always offered. AVIF is offered only when `CGImageDestinationCopyTypeIdentifiers()` lists `public.avif`.
- `PlatformServices/ImageIOImport.swift`: decodes the first image, converts it with vImage to straight RGBA8 in the image's own RGB color space (no color conversion), and applies the EXIF orientation (all 8). The color space's ICC bytes become the document profile. A 16-bit, float or HDR source becomes 8-bit, and the status bar says so. A gray or CMYK source becomes sRGB, with a note.
- Metadata: the engine has no metadata slot, so `PainterDocument.metadata` keeps it for M50. It has the XMP packet (EXIF, TIFF, GPS and IPTC namespaces) and the EXIF, ExifAux, TIFF, GPS and IPTC property dictionaries. Orientation is reset to 1, because the pixels are already turned.
- `PlatformServices/ImageIOExport.swift` and `DocumentLoader.export`: File > Save As writes HEIC (and AVIF where it can) at quality 0.9 with the document profile (the engine's `doc.profile` query) and the kept metadata. M50 adds the options dialog.
- `Shell/FileDrop.swift`: files dropped on the canvas area or the image strip open one at a time, in drop order. A 2 pt accent outline shows while files are over the target. Files that cannot open are named in one status-bar message, and nothing opens for them. Import as Layer comes later.
- Camera RAW tests use three CC0 samples from https://raw.pixls.us. `tools/fetch-raw-samples.sh` puts them in `~/Library/Caches/OpenPainter/raw-samples`, and `tools/raw-samples.txt` lists each URL, license and SHA-256. Without them, the RAW test skips and prints why.

## Focus rules

Cmd+A, Delete, Escape, Enter, Undo (Cmd+Z) and Redo (Shift+Cmd+Z) go to the active document unless a text field or a dialog has focus. `FocusRouter.target(in:)` decides; `EditorWindow` and `ShellPanel` apply it.

| Focus | Cmd+A | Delete | Escape | Enter | Undo, Redo |
| --- | --- | --- | --- | --- | --- |
| Canvas, or any control that is not a text field (strip item, toolbar button) | Select All pixels | Erase Selection (says "comes in a later version" for now) | Cancel a brush drag (no change, no history step), else Deselect | Finish (nothing to finish yet) | Document history |
| A panel with no text field editing | Same as the canvas: the panel forwards to the active document | Same | Same | Same | Same |
| A text field or text view is editing (options row, status zoom, Colors hex) | Selects the field's text | Deletes text | Ends editing | Ends editing | The field's own undo |
| A sheet or modal dialog is up | The dialog's field | The dialog | The dialog | The dialog's default button | The dialog; document commands are disabled |

Letter keys pick tools only while the canvas has focus (paint.net keys: S, M, Z, H, F, G, B, E, P, K, L, R, T, O; S, M and O cycle).

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
