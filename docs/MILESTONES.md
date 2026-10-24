# OpenPainter milestones

50 milestones, 10 checkpoints (CP), 6 phases. Status column: `todo`, `doing`, `done (commit)`.

- Lanes: **E** engine core Rust (core, tools, formats), **O** ops and effects Rust plus WGSL, **U** Swift app.
- **★** = deep-dive design review before building.
- Every op milestone ships the CPU reference, the GPU kernel, a GPU-vs-CPU parity test at ±1/255, and a parameter schema. Effect dialogs come from the schema (M20), so op milestones need no UI work.
- Golden tests compare against OpenPDN ports, Pinta's reference images, or paint.net 5.1 captures, at ±1/255 per channel unless `tests/tolerances.toml` says otherwise.
- OpenPDN file names below were written from memory of the 3.36 tree; M02 checks each one against the real tree and fixes this file.
- Per milestone: unit tests and targeted checks only (`--tier fast`, then `--tier milestone`). Checkpoints run `--tier checkpoint`, the Linux run, the speed and size bars, a review, and publish an update page.

## Phase 1: Foundations (M01–M10)
**Bar:** the app opens a PNG and shows it color-managed, with zoom and pan. The engine has layers, blend modes, unlimited undo and GPU parity.

| ID | Milestone and scope | Reference | Exit gate | Diff / h | Deps | Lane | Status |
|---|---|---|---|---|---|---|---|
| M01 | Repo, Cargo workspace skeleton (all crates), xtask tiers, golden-image harness, launch and size scripts, Linux run script. | none | `cargo xtask check --tier fast` passes on Mac and on Linode. The harness fails a 2/255 diff and passes a 1/255 diff. | easy / 4 | none | E | todo |
| M02 | Reference corpus and capture spike. Vendor OpenPDN sources needed for ports (as port notes, not built) and Pinta test assets. Capture kit for paint.net 5.1 on alpha-wolf: plugin spike, then FlaUI via scheduled task. Manifest per capture (effect, params, version, hash). Verify every OpenPDN path in this file. | paint.net 5.1, OpenPDN, Pinta tests | One SSH command captures Gaussian r=4 and Invert Colors on 3 inputs with no hand-holding, or a committed fallback decision. Timebox 8 h. | hard / 8 | M01 | O | todo |
| M03 | Pixel core: RGBA8 straight type, premultiplied f32 helpers, the rounding rule, Surface, unary and binary pixel ops. | `src/Core/ColorBgra.cs`, `Surface.cs`, `UnaryPixelOps.cs`, `BinaryPixelOps.cs` | Ported ops match OpenPDN vectors on 10,000 random pixels. | easy / 4 | M01 | E | todo |
| M04 ★ | Tile store and history: 256² copy-on-write tiles, LZ4, disk spill, named steps, jump to any step. | `src/HistoryMementos/*.cs` | 200 random edits on 24 MP, undo all: hash equals the original. RAM stays under the cap. A 50-step jump < 200 ms. | hard / 8 | M03 | E | todo |
| M05 ★ | Document and CPU compositor: layers (name, visible, opacity, blend), 14 paint.net blend ops plus Overwrite, dirty-tile composite. | `src/Data/UserBlendOps.Generated.cs`, `UserBlendOp.cs`, `Layer.cs`, `BitmapLayer.cs`, Pinta blend tests | 15 modes × 5 opacities within ±1/255 of OpenPDN vectors; Pinta blend goldens pass. | hard / 6 | M04 | E | todo |
| **CP1** | | | | | | | |
| M06 ★ | GPU backend and parity runner: wgpu device, tile upload and readback, compute pipelines, presenter spike on CAMetalLayer, a runner that diffs every registered op on CPU and GPU. | none | GPU compositor matches CPU on all 15 modes within ±1/255 on Metal and Linux lavapipe. Presenter spike decision written. | hard / 7 | M05 | O | todo |
| M07 | C interface and Swift bridge: cbindgen header, ids, status codes, async jobs with progress and cancel, Swift wrapper package. | none | A Swift test opens a document, adds a layer, inverts it, undoes, reads pixels back. Leak check clean. | medium / 5 | M05 | E | todo |
| M08 | Design: 3 rendered mockups of the main window (classic paint.net floating panels, Mac unified toolbar with floating panels, dark pro). Astra critique plus a fresh adversarial review; pick one; design spec and tokens. | paint.net 5.1 screenshots | 3 PNGs, critiques and the chosen spec committed. | medium / 4 | M01 | U | todo |
| M09 | App shell and Metal canvas: document-based app, canvas fed by engine tiles, zoom 1–3200%, checkerboard, pixel grid, Zoom and Pan tools. | none | A 24 MP image pans at 60 fps. Zoom steps match paint.net's list. | hard / 8 | M07, M08 | U | todo |
| M10 ★ | Color management: lcms2, read and write embedded ICC, assign and convert, display path. | ICC test files | sRGB → P3 → sRGB within ±1/255. An Adobe RGB file matches Preview within ΔE < 1 at 5 probe points. Passes on Linux. | hard / 7 | M05, M09 | E | todo |
| **CP2** | Update page 1 | | | | | | |

## Phase 2: Usable photo editor (M11–M20)
**Bar:** open a JPEG, HEIC or RAW photo; crop at an aspect ratio and straighten; resize; Levels, Curves and other adjustments; undo from History; save with the profile intact.

| ID | Milestone and scope | Reference | Exit gate | Diff / h | Deps | Lane | Status |
|---|---|---|---|---|---|---|---|
| M11 | Image I/O: pure-Rust PNG, JPEG (quality), BMP, GIF, TIFF, WebP; ImageIO for HEIC, AVIF, RAW; ICC and EXIF orientation. | sample corpus | 30 files per format round-trip. RAW from 3 camera makes opens. Orientation correct. | medium / 6 | M07, M10 | U | todo |
| M12 | Native `.opaint`: OpenRaster zip (stack.xml, layer PNGs, ICC), our namespace for paint.net-only blend modes, versioned. | OpenRaster spec | A 20-layer document round-trips with identical pixels. 10,000 fuzzed mutations, no crash. | medium / 4 | M05 | E | todo |
| M13 | Main window: floating Tools, Colors, Layers, History panels (F5–F8), thumbnail strip, tool-options bar, status bar, menu skeleton. | paint.net 5.1 layout | A UI test toggles every panel with F5–F8. Panel positions persist across relaunch. | medium / 6 | M09 | U | todo |
| M14 | Layers and History panels: add, delete, duplicate, merge down, reorder, Properties with live preview; click a History row to jump. | `src/Data/Layer.cs` | A UI test makes 3 layers, sets a blend mode, jumps to step 1, redoes. Pixels match an engine-only run. | medium / 6 | M13 | U | todo |
| M15 | Resampling and Image menu ops: nearest, bilinear, bicubic, supersampling, best quality; resize by percent or size with aspect lock; canvas size with anchor; flip; rotate 90/180; flatten. | `src/Core/Surface.cs` (ResizeBicubic, SuperSamplingFitSurface, ResizeBilinear) | Each mode within ±1/255 of OpenPDN or captures on 10 images. | medium / 6 | M06 | O | todo |
| **CP3** | | | | | | | |
| M16 ★ | Selection engine: polygon set plus coverage mask; Replace, Add, Subtract, Intersect, Xor; pixelated and antialiased; invert, offset, expand; selection changes are history steps. | `src/Selection.cs`, `src/Core/PdnGraphicsPath.cs`, `PdnRegion.cs` | Mode truth tables on 50 random shape pairs. AA coverage within ±1/255 of a 16× supersampled reference. | hard / 8 | M04 | E | todo |
| M17 | Rectangle Select with marching ants; Crop tool with aspect presets and straighten; crop to selection; Resize and Canvas Size dialogs. | paint.net 5.1 dialogs | A UI test crops at 3:2 with 2.5° straighten and gets the expected size. Aspect lock holds. | medium / 7 | M14, M15, M16 | U | todo |
| M18 | Adjustments 1: Auto-Level, Black and White, Brightness/Contrast, Invert Colors, Invert Alpha, Posterize, Sepia, Hue/Saturation. | `src/Effects/AutoLevelEffect.cs`, `DesaturateEffect.cs`, `BrightnessAndContrastAdjustment.cs`, `InvertColorsEffect.cs`, `PosterizeAdjustment.cs`, `SepiaEffect.cs`, `HueAndSaturationAdjustment.cs`, Pinta tests | All 8 pass goldens and GPU parity. | medium / 6 | M06 | O | todo |
| M19 | Adjustments 2: Curves (luminosity and RGB), Levels (with Auto), Exposure, Highlights/Shadows. | `src/Effects/CurvesEffect.cs`, `LevelsEffect.cs`, `UnaryPixelOps.Level`, 5.1 captures | Curves and Levels match OpenPDN; Exposure and Highlights/Shadows match captures; all within ±1/255. | hard / 7 | M18, M02 | O | todo |
| M20 | Effect dialog framework: SwiftUI controls from ParamSchema, cancellable tiled live preview, Apply is one history step, Cmd+F repeats, custom controls (curves, levels, angle, point). | none | Brightness/Contrast preview < 100 ms on 12 MP. Cancel leaves the hash unchanged. Cmd+F reapplies the same params. | medium / 6 | M14, M18 | U | todo |
| **CP4** | Update page 2 | | | | | | |

## Phase 3: Selections, painting, first effects (M21–M30)
**Bar:** everyday paint.net work: all selection tools, moving and rotating pixels, clipboard, pressure brushes, fills and gradients, the most-used blur, sharpen, noise and stylize effects.

| ID | Milestone and scope | Reference | Exit gate | Diff / h | Deps | Lane | Status |
|---|---|---|---|---|---|---|---|
| M21 | Lasso, Ellipse, Rectangle select tools with mode bar and modifier keys; Edit menu (select all, deselect, invert, erase, fill, offset). | `src/tools/LassoSelectTool.cs`, `EllipseSelectTool.cs`, `RectangleSelectTool.cs`, `SelectionTool.cs` | A UI test draws each tool in each of 5 modes; masks equal the engine oracle. | medium / 6 | M16, M17 | U | todo |
| M22 ★ | Magic Wand and Paint Bucket: shared flood fill, contiguous or global, paint.net distance formula, layer or image sampling, antialias. | `src/tools/FloodToolBase.cs`, `MagicWandTool.cs`, `PaintBucketTool.cs`, forum topic 117085 | Masks pixel-exact against 5.1 captures on 10 images × 4 tolerances. | hard / 7 | M16, M02 | E | todo |
| M23 ★ | Move Selected Pixels and Move Selection: nubs, rotation, 5 resampling modes, Ctrl copy, GPU live preview, one history step on commit. | `src/tools/MoveSelectedTool.cs`, `MoveSelectionTool.cs`, `MoveToolBase.cs` | Rotate 30° plus scale 1.5× within ±1/255 of the CPU oracle per mode. 100 drags make one undo step that restores the hash. | hard / 8 | M15, M16 | E | todo |
| M24 | Clipboard: cut, copy, copy merged, paste, paste into new layer, paste into new image, ICC kept, paste opens in transform mode. | paint.net behavior | Copy from Preview, paste into a new image: pixels and profile preserved; transform nubs shown. | medium / 4 | M23, M21 | U | todo |
| M25 | Blurs 1: Gaussian, Motion, Radial, Zoom, Unfocus, Fragment, Median. | `src/Effects/GaussianBlurEffect.cs`, `MotionBlurEffect.cs`, `RadialBlurEffect.cs`, `ZoomBlurEffect.cs`, `UnfocusEffect.cs`, `FragmentEffect.cs`, `MedianEffect.cs`, `LocalHistogramEffect.cs`, Pinta tests | All pass goldens and parity. Gaussian r=50 on 24 MP < 1 s on GPU. | medium / 7 | M06 | O | todo |
| **CP5** | | | | | | | |
| M26 | Colors window (primary/secondary, HSV wheel with Ctrl/Alt/Shift, More/Less, RGB/HSV/hex/alpha, 96-color palette, paint.net palette files) and Color Picker tool (sample size, layer/image). | `src/tools/ColorPickerTool.cs`, palette format | A paint.net palette file loads the same 96 colors. Hex round-trips. A 5×5 sample average matches the oracle. | medium / 6 | M13 | U | todo |
| M27 ★ | Brush engine: dab spacing, antialiasing, pressure to size and opacity; Paintbrush, Eraser, Pencil; one stroke is one history step. | `src/tools/PaintBrushTool.cs`, `EraserTool.cs`, `PencilTool.cs`, libmypaint | Replayed pointer logs match the oracle and captures. Latency < 16 ms on 24 MP. | hard / 8 | M23, M26 | E | todo |
| M28 | Gradient tool: live handles, linear, reflected, diamond, radial, conical, color and transparency modes. | `src/tools/GradientTool.cs`, `src/GradientRenderers.cs` | 5 types × 2 modes within ±1/255 of captures. | medium / 6 | M16, M26 | E | todo |
| M29 | Artistic and Stylize: Ink Sketch, Oil Painting, Pencil Sketch, Edge Detect, Emboss, Outline, Relief. | `src/Effects/InkSketchEffect.cs`, `OilPaintingEffect.cs`, `PencilSketchEffect.cs`, `EdgeDetectEffect.cs`, `EmbossEffect.cs`, `OutlineEffect.cs`, `ReliefEffect.cs`, `ColorDifferenceEffect.cs` | All pass goldens and parity. | medium / 6 | M25 | O | todo |
| M30 | Noise and Photo: Add Noise, Reduce Noise, Glow, Red Eye Removal, Sharpen, Soften Portrait, Vignette. | `src/Effects/AddNoiseEffect.cs`, `ReduceNoiseEffect.cs`, `GlowEffect.cs`, `RedEyeRemoveEffect.cs`, `SharpenEffect.cs`, `SoftenPortraitEffect.cs`, captures | All pass goldens and parity. Seeded noise matches the CPU oracle exactly. | medium / 6 | M25 | O | todo |
| **CP6** | Update page 3 | | | | | | |

## Phase 4: Drawing tools, text, more effects (M31–M40)
**Bar:** every paint.net tool works, including editable text, lines and shapes. Saving gives full control of quality and metadata. Most of the Effects menu exists.

| ID | Milestone and scope | Reference | Exit gate | Diff / h | Deps | Lane | Status |
|---|---|---|---|---|---|---|---|
| M31 | Clone Stamp and Recolor on the brush engine. | `src/tools/CloneStampTool.cs`, `RecolorTool.cs` | Replayed strokes match the oracle and captures. | medium / 5 | M27 | E | todo |
| M32 ★ | Line/Curve (spline, bezier, straight) and the edit-until-Finish framework shared with Shapes and Text; dashes, arrowheads, antialiasing. | 5.1 captures | Path raster within ±1/255 of captures. Re-editing nubs after 5 actions, then Finish, leaves one history step. | hard / 7 | M28 | E | todo |
| M33 | Distort 1: Bulge, Frosted Glass, Pixelate, Polar Inversion, Tile Reflection, Twist. | `src/Effects/WarpEffectBase.cs`, `BulgeEffect.cs`, `FrostedGlassEffect.cs`, `PixelateEffect.cs`, `PolarInversionEffect.cs`, `TileReflectionEffect.cs`, `TwistEffect.cs` | All pass goldens and parity. | medium / 6 | M29 | O | todo |
| M34 | Shapes: 29 presets as vector paths; fill, outline or both; editable until Finish; shape picker. | 5.1 captures | 29 shapes × 3 draw modes within ±1/255 of captures. | medium / 7 | M32 | E | todo |
| M35 | Render: Clouds (with blend mode), Julia, Mandelbrot, Turbulence, Voronoi. | `src/Effects/CloudsEffect.cs`, `JuliaFractalEffect.cs`, `MandelbrotFractalEffect.cs`, captures | Same seed gives identical CPU and GPU output; goldens match. | medium / 6 | M30 | O | todo |
| **CP7** | | | | | | | |
| M36 ★ | Text tool: CoreText shapes and rasterizes on the Mac side and hands coverage to the engine; font, size, bold, italic, underline, alignment, antialias, blend; editable until Finish; input methods. | `src/tools/TextTool.cs` | Re-editing before Finish works; one history step on commit; coverage matches CoreText's own output; an IME test passes. | hard / 8 | M32, M26 | U | todo |
| M37 | Blurs 2 (5.x only): Bokeh, Sketch Blur, Square, Surface. | 5.1 captures | Within ±1/255 of captures or the documented fallback tolerance; parity passes. | hard / 7 | M25, M02 | O | todo |
| M38 | Save options and metadata: per-format settings with a size preview (JPEG quality and subsampling, PNG and TIFF compression, WebP and AVIF quality and lossless, HEIC quality, GIF dithering); keep or strip EXIF, XMP, GPS; flatten prompt for flat formats. | paint.net 5.1 save dialogs | Each setting changes the output as expected (size and decoded pixels). EXIF survives JPEG, HEIC, TIFF, AVIF. Strip leaves no GPS tag. | medium / 6 | M11, M12 | E | todo |
| M39 | Distort 2 and Color: Crystalize, Dents, Morphology, Quantize. | 5.1 captures | All pass captures and parity. | hard / 7 | M33 | O | todo |
| M40 | Object effects (Drop Shadow, Outline Object, Feather Object, Align Object), 3D Rotate/Zoom, layer flip, Import from File. | `src/Effects/RotateZoomEffect.cs`, captures | All match captures and parity. Import adds a layer with the profile converted. | medium / 7 | M35, M14 | O | todo |
| **CP8** | Update page 4 | | | | | | |

## Phase 5: Retouch and full parity (M41–M45)
**Bar:** healing, content-aware fill and remove background work. Every adjustment and effect is in paint.net menu order and passes parity.

| ID | Milestone and scope | Reference | Exit gate | Diff / h | Deps | Lane | Status |
|---|---|---|---|---|---|---|---|
| M41 | Healing brush and spot heal: texture from source, Poisson blend for tone; spot heal picks its source. | Pérez et al. 2003 Poisson editing | Poisson residual under threshold on 10 cases; no visible seam on reviewed outputs. | hard / 7 | M31 | E | todo |
| M42 ★ | Content-aware fill: multi-scale PatchMatch on the selection. | Barnes et al. 2009 | PSNR target on 10 synthetic holes. < 5 s at 12 MP with a 5% hole. Fixed seed gives the same result. | hard / 8 | M16, M06 | O | todo |
| M43 | Remove background: Vision foreground mask to a selection or layer alpha; one history step. | `VNGenerateForegroundInstanceMaskRequest` | IoU ≥ 0.9 against hand masks on 10 photos. | easy / 3 | M21 | U | todo |
| M44 ★ | GPU parity sweep and perf: every op on Apple GPU and lavapipe; fix divergences; tune the tile scheduler. | parity runner | Zero ops outside ±1/255. Per-effect perf table published. Nothing over 3 s at 24 MP. | hard / 7 | M40, M39 | O | todo |
| M45 | Menus complete: Adjustments and Effects in 5.1 order, icons, a dialog review per effect, Cmd+F for every effect. | paint.net 5.1 menus | A script compares menu order to the 5.1 list. Cmd+F passes for all effects. | medium / 6 | M44, M20 | U | todo |
| **CP9** | | | | | | | |

## Phase 6: Polish and release (M46–M50)
**Bar:** a stranger downloads a signed DMG from GitHub, installs it, and edits photos day to day. Every bar is met.

| ID | Milestone and scope | Reference | Exit gate | Diff / h | Deps | Lane | Status |
|---|---|---|---|---|---|---|---|
| M46 | View menu (zoom presets, actual size, fit, rulers and units, pixel grid), Settings, light and dark mode. | paint.net 5.1 | A UI test checks each View item; rulers correct in px, in, cm; both appearances look right in screenshots. | medium / 5 | M13 | U | todo |
| M47 | Robustness: autosave and crash recovery, 200 MP document, low-disk spill, fuzz every loader, 1,000-step history stress. | none | Kill the app mid-edit and recover the document. 1 h of fuzzing, no crash. Low disk gives a clear error with no data loss. | medium / 7 | M11, M12, M38, M04 | E | todo |
| M48 | Speed and size bars: strip, LTO, prune shaders, lazy effects, Instruments launch trace. | none | Cold launch < 3 s; app < 75 MB; both checked by script. | medium / 5 | M45 | U | todo |
| M49 | Keyboard and accessibility: shortcut table mapped from paint.net to Cmd, VoiceOver labels, strings ready for translation, help links. | paint.net shortcuts | Every menu item has a label; the shortcut table has no conflicts. | medium / 5 | M45 | U | todo |
| M50 | Release: Developer ID signing, notarization, DMG, GitHub Releases, project page on yili.dev. | none | A clean Mac downloads and opens with no Gatekeeper warning and passes the smoke test. The page is live. | medium / 5 | M48, M49 | U | todo |
| **CP10** | Final checkpoint and v1.0 tag | | | | | | |

## Critical path
M01 → M03 → M04 → M05 → M07 → M09 → M13 → M14 → M17 → M21 → M24 → M26 → M27 → M28 → M32 → M36 → M45 → M48 → M50. The effects lane (O) is off the critical path; it needs M06, plus M02 for 5.x-only effects. About 300 agent hours in total.

## Not in this version
- `.pdn` and PSD import; ORA, PDF or SVG export beyond `.opaint`; animated GIF and WebP frame editing.
- Layer masks, groups, adjustment layers, any non-destructive editing.
- 16-bit, float or HDR storage; CMYK and Lab editing.
- paint.net plugins or a public plugin API; scripting; batch processing.
- Windows, Linux or iPad UI (the engine stays portable).
- Printing; cloud sync; App Store build; generative AI fill beyond PatchMatch and Vision.
- Layer styles; vector layers; brush presets beyond paint.net's brush options.
