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

## Color Profile dialog (M27)

- `Shell/ColorProfile.swift`: Image > Color Profile…, a sheet. It shows the document profile (or "None, treated as sRGB"), then Convert (`image.convert_profile` with an intent and black point compensation) or Assign (`image.assign_profile`, pixel bytes unchanged). Targets: sRGB, Display P3, Adobe RGB (1998), ProPhoto RGB, the window's display profile, or an .icc/.icm file (sent as `base64:` ICC bytes). OK runs one command, so one history step; Cancel runs nothing.
- The engine queries `doc.profile` (embedded, description, color space, ICC size) and `doc.profile_icc` (the raw bytes, empty when there is no profile). `doc.info` also has `profile_embedded`.
- The status bar shows the profile name (cut at 18 characters, full name in the tooltip; "sRGB (none)" with no profile).
- The display path already follows the screen (see the canvas section); `ColorProfileTests` checks that a screen change rebuilds the table once and leaves the document alone.

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

## Rectangle Select, Crop tool, Resize and Canvas Size (M23)

- `Shell/SelectCropTool.swift`: the canvas sends presses, drags and releases of the `.overlay` tool to `SelectCropController`.
  - Rectangle Select: one `selection.rectangle` per finished drag (one history step). Combine mode comes from the keys, else the options row: Command adds, Option subtracts, Command with the right button (or Control-click) inverts, Option with the right button intersects. Shift makes a square. Styles: Normal, Fixed Ratio, Fixed Size (pixels). Eight handles change the last rectangle; the release sends `selection.rectangle` with `amend`, which replaces that same step. A click without a drag deselects.
  - Crop tool (Image > Crop Tool): a frame with aspect presets (Free, Original, 1:1, 4:3, 3:2, 16:9, 3:4, 2:3, 9:16) and a straighten angle (-45 to 45 degrees). Enter or Crop sends one `image.crop_rect` (rotate with the Rotate / Zoom effect, then crop, in one step). Escape, Cancel or another tool leaves the image as it was.
- `Shell/SelectionOverlay.swift`: marching ants from the engine query `doc.selection_path` (rings for shape selections, edge runs for Magic Wand masks), redrawn for each viewport change. The ants move with a Core Animation dash-phase animation.
- `Shell/ImageSizeDialogs.swift`: the Resize and Canvas Size sheets. They show the new size before OK and send `image.resize` or `image.canvas_size` once. The resolution field converts between pixel and print size only; the document's resolution is not changed.
- `Shell/SelectCropGeometry.swift`: the drag, handle, preset and rotation math.
- A size change (crop, resize, canvas size, 90 degree rotation) refits the canvas to the new image size.

## Ellipse Select, Lasso Select and the Edit menu selection items (M29)

- Ellipse Select reuses Rectangle Select in `Shell/SelectCropTool.swift`: the same modes and keys, the same styles, Shift for a circle, and the same eight handles. A release sends one `selection.ellipse`; a handle drag sends it with `amend`, which replaces that same step.
- Lasso Select: the drag's points (at least a quarter pixel apart) go in one `selection.lasso`; the engine closes the shape back to the first point and uses the even-odd rule. A click, or a drag that encloses no area, sends nothing (a click in Replace mode deselects). paint.net's pages name only a freeform lasso, so there is no Polygon mode.
- `Shell/SelectionEditActions.swift`: Erase Selection (Delete; transparent pixels, then deselect), Fill Selection (Shift-Delete; the primary color, the selection stays), Expand Selection… (a sheet; a negative number shrinks), Copy Selection (Option-Shift-Command-C; paint.net's `{"polygonList": [...]}` JSON text from the `doc.selection_polygons` query) and the Paste Selection submenu (one item per combine mode; Replace is Option-Shift-Command-V). Invert Selection (Command-I) is the engine command `selection.invert`. Each changing item is one history step, and each item is off when it would do nothing. Enter deselects, as in paint.net.
- Every selection tool and Paste Selection combine with an empty selection when nothing is selected, and new shapes are clipped to the canvas.

## Remove Background (M53)

paint.net 5.1 has no Remove Background command (its Image and Layers menu pages list none), so Layers > Remove Background is OpenPainter's. It sits in Layers because it works on the active layer.

- `Shell/RemoveBackground.swift`: two items, Select Foreground and Remove Background. Each reads the active layer (`layer.pixels`, not the flattened image) and shows a sheet with a progress bar and Cancel while Vision runs off the main thread. Cancel stops Vision and sends nothing. Then one engine command, so one history step: `selection.from_mask` (a Coverage selection holding the soft mask) or `layer.remove_background` (the layer's alpha times the mask). No foreground, or an image that changed meanwhile, sends nothing and says so in the status bar.
- `PlatformServices/ForegroundMask.swift` and the `VisionMask` package (`mac/VisionMask`, Objective-C): `VNGenerateForegroundInstanceMaskRequest` (macOS 14+) with `generateScaledMaskForImage(forInstances:from:)` over all instances, at the full layer size, rounded to one byte per pixel so soft edges stay. Swift code must not `import Vision`: its Swift overlay then loads into every module that imports OpenPainterKit and changes type inference elsewhere (on the macOS 27 SDK, `stride(...).first { ... } ?? 0` becomes `Int?` and `DocumentLifecycleTests` stops compiling).
- Engine: `engine/op-engine/src/mask_commands.rs`. The mask is base64 of width × height bytes.
- Test photos: `tests/fixtures/remove-bg` (10 composites with exact masks; its README lists sources and licenses). `RemoveBackgroundTests` checks IoU ≥ 0.9 at threshold 128 for each.

## Move Selected Pixels and Move Selection (M33)

Rules follow paint.net's Move Tools page (https://www.getpaint.net/doc/latest/MoveTools.html), with Ctrl as Command and Alt as Option.

- `Shell/MoveTool.swift`: while a Move tool is current, the canvas sends the pointer to `MoveToolController` (it is the canvas `pointerSink`; Rectangle Select gets it back on a tool change). The box is the selection bounds, or the whole layer for Move Selected Pixels with nothing selected. Move Selection with nothing selected does nothing.
- Drag inside the box to move (whole pixels). Drag one of the 8 nubs to scale from the opposite nub; past it flips; Shift keeps the aspect ratio; Option scales about the center. Drag in the 24 pt corridor just outside the box, or with the right button anywhere, to rotate about the pivot; Shift snaps the total angle to 15 degrees and the status bar shows the angle. Drag the pivot (circle with a cross, at the box center at first) to move it. Command during a drag works on a copy, so the original pixels stay. Arrow keys nudge 1 px, Command-arrow 10 px (paint.net's Ctrl).
- One session per tool use. The first press on the box (or the first arrow key) sends `op_move_begin`; each drag sends the whole transform (`op_move_update`, at most once per run-loop turn) and the canvas shows the engine's preview, with the ants following the box. Enter, Finish, another tool, a click outside the box and its corridor, Undo, or any engine command finishes it (`op_move_finish`, one "Move Selected Pixels" or "Move Selection" step). Escape or Cancel sends `op_move_cancel`: pixels, selection and history stay as they were.
- The options row has Resampling (the engine's five move modes: Nearest Neighbor, Bilinear, Bicubic (default), Supersampling, Best Quality), Cancel and Finish.
- `Shell/MoveGeometry.swift`: the hit test (nub, pivot, inside, corridor, outside) and the drag math. The session keeps the original box and one cumulative transform, so a second drag continues from the first.
- `EngineBridge/Move.swift` wraps `op_move_begin`, `op_move_update`, `op_move_finish` and `op_move_cancel` (`engine/op-ffi/src/move_tool.rs`).

## Clipboard (M34)

Rules follow paint.net's Edit menu page (https://www.getpaint.net/doc/latest/EditMenu.html).

- `Shell/ClipboardActions.swift`: Cut (Command-X), Copy (Command-C), Copy Merged (Shift-Command-C), Paste (Command-V), Paste into New Layer (Shift-Command-V) and Paste into New Image (Option-Command-V). Cut, Copy and Paste are the standard `cut:`, `copy:` and `paste:` actions, so a focused text field gets them first.
- Copy takes the active layer's pixels under the selection (the whole layer with no selection), with partial coverage as alpha, cropped to the selection bounds (the engine's `clipboard.copy` query). Copy Merged does the same on the flattened image. Both write PNG and TIFF with the document's ICC profile (`clipboard.profile_icc`: the engine's sRGB bytes for a document with no profile). Cut copies, then sends Erase Selection: one history step, and the selection goes away. An open Move or Magic Wand session finishes first.
- Paste reads PNG exactly (`op_png_decode`), else TIFF or any image NSImage reads (ImageIO). Pixels in another profile are converted to the document's profile by the engine (op-color, perceptual), and the status bar says "converted from … to …".
- Paste turns on Move Selected Pixels and opens one engine move session (`op_paste_begin`, `engine/op-ffi/src/clipboard.rs`) with the pixels floating at the top left of the visible part of the canvas, moved in so an image that fits stays inside; the selection is the pasted bounds. Drags, nudges, Finish and Escape work as for any move: Finish commits one "Paste" step, Escape leaves the document exactly as it was. Paste into New Layer adds the layer inside the same session, so it is one "Paste into New Layer" step and Escape leaves no layer behind.
- An image wider or taller than the canvas asks Expand Canvas, Keep Canvas Size or Cancel (`ImageClipboard.oversizePrompt`). Expand sends Canvas Size (top left anchor, transparent) as its own step, then pastes at (0, 0); Keep pastes and the part outside the canvas comes in when moved.
- Paste into New Image opens an untitled image the size of the clipboard image, holding its pixels and profile.
- Items are on only when they can act: Copy and Copy Merged need a selection that covers a pixel (or none); Cut needs a selection; the Paste items need an image on the clipboard. Tests use a private named pasteboard (`ImageClipboard.pasteboard`).

## Magic Wand and Paint Bucket (M31)

- `Shell/FloodTool.swift`: as in paint.net 5, a click with the Magic Wand or the Paint Bucket stays live. The window's `click` starts an engine preview session (`Document.floodBegin`, `EngineBridge/FloodPreview.swift`) at the clicked pixel; nothing enters History yet.
- While it is open, any change to the tolerance (slider or field), Tolerance alpha (Premultiplied or Straight; paint.net's default is Premultiplied), selection or blend mode, sampling, flood mode, antialiasing or the fill color sends the full parameters again (`floodUpdate`). The engine re-runs the flood on the unchanged document, so the result equals one plain `tool.magic_wand` or `tool.paint_bucket` command with the last parameters.
- The click point shows as a handle (a white square with four arrows). Dragging it moves the click point (`MetalCanvasView.onClickPress`, `onHandleDrag`, `onHandleRelease` in the `.click` mode).
- Finish (Enter, the Finish button at the end of the options row, another tool, or a click elsewhere, which then starts a new one) commits one step named "Magic Wand" or "Paint Bucket". Escape and Undo drop it and leave the image as it was. A command from elsewhere (a menu, a panel) commits it in the engine first, and the session ends here too.
- The live wand selection shows its marching ants: `doc.selection_path` with `{"preview": true}`.

## File formats and drag and drop (M17)

The engine decodes PNG itself. ImageIO decodes HEIC, HEIF, AVIF and every Camera RAW type that `CGImageSourceCopyTypeIdentifiers()` lists (30 on macOS 27). Swift hands the engine straight 8-bit RGBA and the ICC profile through `op_import_pixels`.

- `PlatformServices/ImageFormats.swift`: which files open (the route reads the file's first bytes, so a wrong extension still opens the right way) and which types Save As offers. HEIC is always offered. AVIF is offered only when `CGImageDestinationCopyTypeIdentifiers()` lists `public.avif`.
- `PlatformServices/ImageIOImport.swift`: decodes the first image, converts it with vImage to straight RGBA8 in the image's own RGB color space (no color conversion), and applies the EXIF orientation (all 8). The color space's ICC bytes become the document profile. A 16-bit, float or HDR source becomes 8-bit, and the status bar says so. A gray or CMYK source becomes sRGB, with a note.
- Metadata: the engine has no metadata slot, so `PainterDocument.metadata` keeps it for M50. It has the XMP packet (EXIF, TIFF, GPS and IPTC namespaces) and the EXIF, ExifAux, TIFF, GPS and IPTC property dictionaries. Orientation is reset to 1, because the pixels are already turned.
- `PlatformServices/ImageIOExport.swift` and `DocumentLoader.export`: File > Save As writes HEIC (and AVIF where it can) at quality 0.9 with the document profile (the engine's `doc.profile_icc` query) and the kept metadata. M50 adds the options dialog.
- `Shell/FileDrop.swift`: files dropped on the canvas area or the image strip open one at a time, in drop order. A 2 pt accent outline shows while files are over the target. Files that cannot open are named in one status-bar message, and nothing opens for them. Import as Layer comes later.
- Camera RAW tests use three CC0 samples from https://raw.pixls.us. `tools/fetch-raw-samples.sh` puts them in `~/Library/Caches/OpenPainter/raw-samples`, and `tools/raw-samples.txt` lists each URL, license and SHA-256. Without them, the RAW test skips and prints why.

## Document lifecycle (M19)

File > New, Open, Save, Save As, Save a Copy and Revert to Saved, following paint.net's File menu (https://www.getpaint.net/doc/latest/FileMenu.html).

- `PlatformServices/ImageDocument.swift`: the NSDocument for every image. The app's `PainterDocument` is a subclass with no code of its own, so the XCTests drive the real class. `ImageDocumentController` (the app's document controller) shows the New Image dialog and reviews unsaved images on Quit.
- New (`Shell/NewImageDialog.swift`): paint.net's 800 × 600 at 96 pixels/inch, the print size, and the background (White, Secondary color or Transparent). The engine makes one "Background" layer (`op_doc_new`). A new image is untitled, not modified, and saves as `.opaint`.
- Open: `.opaint` and PNG through the engine (`op_doc_open_opaint`, `op_doc_open_png`); JPEG, BMP, GIF, TIFF, WebP, HEIC, HEIF, AVIF and Camera RAW through ImageIO (M17's path). The route reads the file's first bytes.
- Save writes: `.opaint` (all layers, names, opacity, blend modes, visibility, resolution and profile) and flattened PNG through the engine (`op_doc_save`; since M50 PNG goes through `op_encode_image` with the save options), then JPEG (M50 adds the options), BMP, TIFF, GIF, HEIC, and WebP and AVIF where `CGImageDestinationCopyTypeIdentifiers()` lists them, through ImageIO with the kept metadata. JPEG puts transparent pixels on white.
- Flatten prompt: a layered image going to any format but `.opaint` asks Flatten or Cancel. The file is flattened; the open image keeps its layers.
- Unsaved mark: the engine's saved step (`op_core::history::History`). `doc.info`'s `dirty` is true when the current history step is not the saved one, so undo back to the saved step clears the window's edited dot and the strip's `*`, and redo sets them again. Save and Save As mark the step (`op_doc_save`, or `op_doc_mark_saved` after an ImageIO write, which checks the revision the pixels were read at). Save a Copy does not mark it, and the document keeps its file and name.
- Revert to Saved reads the file again and gives the open engine document its content (`op_doc_replace`): one history step, not modified, same window.
- Close and Quit with unsaved work ask Save, Don't Save or Cancel for each image, one at a time. Cancel keeps that window and the app open. Tests answer the prompts through `ImageDocument.unsavedPrompt` and `ImageDocument.flattenPrompt`.

## Save options and metadata (M50)

paint.net's Save Configuration dialog (https://www.getpaint.net/doc/latest/FileMenu.html, Save) as a sheet: `Shell/SaveOptionsDialog.swift`.

- When: Save As and Save a Copy, and Save for a lossy format (JPEG, HEIC, AVIF, WebP, GIF) whose options were never chosen for this image (Save to PNG or TIFF keeps every pixel at any option, so it uses the last options without asking), when the save comes from the menu or the save panel (`save(to:ofType:for:delegate:didSave:contextInfo:)`). The sheet comes before the flatten prompt, as in paint.net. Direct `save(to:ofType:for:completionHandler:)` calls (tests, Close's Save) use this image's choice, else the format's last options, else the defaults. Tests answer through `ImageDocument.saveOptionsPrompt`.
- Options (`PlatformServices/SaveOptions.swift`): JPEG quality (default 95) and chroma subsampling 4:4:4, 4:2:2 (default) or 4:2:0, paint.net's defaults; PNG deflate level 0 to 9 (every level keeps every pixel byte); TIFF compression None, LZW or Deflate; HEIC and AVIF quality (no lossless mode: HEIC at 1.0 still changes pixels and AVIF at 1.0 fails to encode on macOS 27, so AVIF tops out at 99); WebP quality only where ImageIO can encode WebP; GIF dithering (Floyd-Steinberg). The last options per format are in `UserDefaults` under `OpenPainter.SaveOptions.<type>`.
- Encoders (`PlatformServices/SaveEncoder.swift`): PNG, JPEG and GIF through the engine (`op_encode_image`, `op_engine::save_export`), because ImageIO has no chroma subsampling key (its JPEG is 4:2:0 below quality 1.0) and no GIF dithering. TIFF, HEIC, AVIF, WebP and BMP through ImageIO. The app writes the bytes and marks the saved step with `op_doc_mark_saved`, as M19's ImageIO saves did.
- File size: `PlatformServices/SaveSizePreview.swift` encodes at the chosen options on a background queue after each change. A waiting request is replaced by the newer one, and a running encode's result is dropped when a newer request exists, so only the last result shows. Lossy formats also show the decoded result.
- Metadata (`PlatformServices/MetadataFilter.swift`): Keep all, Remove location (GPS) or Remove all (EXIF, XMP, GPS, IPTC). Remove location drops the `{GPS}` dictionary and every XMP `GPS*` tag. No save writes an embedded thumbnail. PNG keeps EXIF (`eXIf`, a raw block ImageIO builds from the kept dictionaries) and XMP (`iTXt`) through the engine's PNG writer, so the pixels stay exact. Opening a PNG reads its EXIF and XMP with ImageIO.
- Resolution: every save writes the document's pixels per inch (PNG `pHYs`, JPEG JFIF density, TIFF, HEIC and AVIF DPI through ImageIO). Opening reads it back: the engine reads `pHYs`, and ImageIO files open at their `DPIWidth` through `op_import_pixels_at` (96 when the file states none).

## View menu, Settings and appearance (M56)

- View menu (paint.net 5.1 order, https://www.getpaint.net/doc/latest/ViewMenu.html): Zoom In, Zoom Out, Zoom to Window, Zoom to Selection, Actual Size, Zoom Presets (paint.net's 25 preset zooms, 3200% to 1%, the current one checked), Pixel Grid, Rulers (Option-Command-R, paint.net Ctrl+Alt+R), and Pixels, Inches, Centimeters. The zoom commands reuse `ZoomPresets` and `Viewport` (M09).
- `RulerScale.swift`: `MeasurementUnit` (pixels per unit from the document's `doc.resolution` DPI: 1, DPI, DPI / 2.54) and the tick math. A labelled tick lands on 1, 2 or 5 times a power of ten units, the smallest step at least 48 pt apart; minor ticks split it into 10, 5, 4 or 2 parts at least 4 pt apart. Pixel steps are whole pixels.
- `Shell/RulerView.swift`: the 20 pt rulers along the top and left of the canvas, with the unit in the corner. They follow zoom and scroll, highlight the selection's bounds with `wash` and mark the pointer with `accent`. `CanvasContainerView` moves the canvas in by 20 pt while they show.
- The status bar shows the selection, pointer and image size in the current units; its units popup and the View menu change the same setting.
- `Shell/AppSettings.swift`: appearance (System, Light, Dark), units, rulers and pixel grid, in `UserDefaults` under `OPSettings.*` (the workspace's panel-store defaults). A change applies to every open window at once and to windows opened later. The pixel grid still shows only at 400% and above (the engine's limit; the paint.net page says 200%).
- `Shell/SettingsWindow.swift`: OpenPainter > Settings… (Cmd+,), one page with those four settings. It works with no document open.
- Appearance: the setting sets `NSApp.appearance`; panels, dialogs and the shell follow through the dynamic `Theme` tokens. The canvas surround is the `stage` token, sent to the engine with `op_view_set_background` whenever the canvas's appearance changes (`op_view_info` reports it back).

## Menus complete (M55)

- `tests/catalog/paintnet-5.1-menus.toml` lists every paint.net 5.1 menu item in order, from paint.net's public docs (not the M02 5.1.12 capture), with its Windows key, our Mac key, our command and its status (v1, mac, not in v1, pending).
- `tools/catalog/compare-menus.py` compares the live menu bar (`mac/build/menu-live.tsv`, written by `MenuCatalogTests` on every test run) with the catalog and prints every difference; `--strict` also fails on pending items. `MenuCatalogTests` runs the same check in process.
- Effects follow paint.net's submenu order; Median Blur sits in Blurs (its engine category stays Noise). `MenuIcons.swift` gives an SF Symbol to every item paint.net shows an icon for; `MenuStates.swift` holds the enabled rules and the Layers items Import From File, Move Layer to Top and Move Layer to Bottom; `PlatformServices/SaveAll.swift` is File > Save All.
- Shortcut changes: Zoom to Window Cmd B, Zoom to Selection Shift-Cmd-B, Actual Size Cmd 0, Go to Top and Bottom Layer Option-Cmd-PageUp and PageDown, Brightness / Contrast Shift-Cmd-T, Invert Alpha Option-Cmd-I. File > Print and Help > Plugins are gone (not in v1).
- `M55_PNG_DIR=<dir>` (as `TEST_RUNNER_M55_PNG_DIR`) makes `testRenderMenuSheets` draw every menu into `menus-light.png` and `menus-dark.png`.

## Focus rules

Cmd+A, Delete, Escape, Enter, Undo (Cmd+Z) and Redo (Shift+Cmd+Z) go to the active document unless a text field or a dialog has focus. `FocusRouter.target(in:)` decides; `EditorWindow` and `ShellPanel` apply it.

| Focus | Cmd+A | Delete | Escape | Enter | Undo, Redo |
| --- | --- | --- | --- | --- | --- |
| Canvas, or any control that is not a text field (strip item, toolbar button) | Select All pixels | Erase Selection; Shift-Delete is Fill Selection (M29) | Cancel a brush drag or a Rectangle Select drag, or turn off the Crop tool (no change, no history step), else Deselect | Finish the Crop tool (crop), else Deselect (M29) | Document history |
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
- The fill around the document is the `stage` token for the canvas's appearance (`op_view_set_background`, M56); before the first call the engine uses a mid grey.
- 100% means one document pixel per point.

## Zoom and keys

Zoom steps are paint.net's `ScaleFactor` presets (1% to 3200%), ported from OpenPDN `src/Core/ScaleFactor.cs` (MIT, see `THIRD_PARTY.md`).

| Command | paint.net (Windows) | OpenPainter (Mac) |
| --- | --- | --- |
| Zoom In | Ctrl + | Cmd = |
| Zoom Out | Ctrl - | Cmd - |
| Zoom to Window | Ctrl B | Cmd B (M55; was Cmd 0) |
| Zoom to Selection | Ctrl Shift B | Cmd Shift B |
| Actual Size | Ctrl 0 | Cmd 0 (M55; was Cmd Option 0) |
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
