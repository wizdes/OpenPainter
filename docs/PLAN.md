# Plan: OpenPainter, a paint.net successor for Mac (engine ready for Windows and Linux)

> Revision 2 (2026-10-24): after the Step 0 reviews, the milestone list is now 60 milestones and about 620 agent hours. `docs/MILESTONES.md` and `docs/reviews/step0-decisions.md` replace the milestone section below.

## Context
The user wants a simple, fast, exact photo editor that does 80 to 90% of what paint.net does, with good UI. Mac first; Windows and Linux later. It will be a public, MIT-licensed, free download. Research (below) found MIT paint.net 3.36 source (OpenPDN) and Pinta's 126 reference images to port and test against. The user answered 24 scope questions (table below). After approval, I build all 50 milestones without asking again. At every 5 milestones I publish an update page.

Tier 3: a new project with storage (a native file format), signing and a deploy. So it gets its own worktree, unit, integration and full end-to-end tests, an independent review at each checkpoint, and a change diagram.

## Architecture (from the planning pass; full detail goes into `docs/ARCHITECTURE.md` in M01)
- **Repo** `/Users/yili/work/Projects/OpenPainter` (new Git repo, public on GitHub at release). `engine/` is a Rust workspace:
  - `op-core`: document, tiles, selection, history
  - `op-ops`: CPU kernels, blend modes, the effect registry and parameter schema
  - `op-gpu`: wgpu, WGSL kernels, the compositor, the canvas presenter
  - `op-color`: lcms2
  - `op-formats`: pure-Rust codecs and the native format
  - `op-tools`: tool state machines
  - `op-engine`: commands, a per-document actor thread, jobs, events
  - `op-ffi`: the C interface, built with cbindgen as a staticlib
  - `op-cli`: run any command headless
  - `op-testkit`: test helpers
  - `fuzz/` and `xtask/`
- **Mac app** `mac/`: an XcodeGen `project.yml` plus a SwiftPM package with these modules:
  - `EngineBridge`: Swift command types, generated from the CLI's catalog
  - `CanvasView`: the CAMetalLayer
  - `Panels`: SwiftUI inside floating NSPanels
  - `EffectDialogs`: dialogs built from the parameter schema
  - `PlatformServices`: ImageIO (HEIC, AVIF, RAW), Vision (remove background) and ColorSync
- **Pixels** are stored as 8-bit straight RGBA in 256×256 copy-on-write tiles. Every kernel works in premultiplied f32, with one shared rounding helper.
- **History** stores a full immutable document state per step. Tiles are shared between steps, so a step costs only the tiles it changed. Jumping to a step swaps the state, which restores the selection and the active layer too. Tiles used only by history are compressed with LZ4 and spill to disk under a RAM budget.
- **Selection** keeps an f64 polygon path as the source of truth, combined with the `i_overlay` crate. A cached antialiased coverage mask is built from it. The wand's bitmap is traced back to a path.
- **Commands:** every action is a command with a stable id, such as `effect.blur.gaussian`. Parameters are JSON checked against a schema. Menus, shortcuts, history, tests and the CLI all use the same ids. Effects register themselves (one file per effect), so builders do not collide.
- **GPU:** wgpu handles compositing, mip levels, canvas presentation and effect kernels. The CPU f32 version of each op is the test oracle and the fallback. GPU must match CPU within ±1/255. Parity runs on Metal and on Linux lavapipe (software Vulkan, no GPU needed). The canvas shares wgpu's Metal device through the CAMetalLayer. M06 starts with a spike; the fallback is an IOSurface handoff.
- **Live previews** (effect dialogs, strokes, shapes, moving pixels) are drawn as a preview stack on top of the document. They do not touch history. Finish commits exactly one history step.
- **Threading:** the UI thread never blocks. The engine publishes snapshots through `arc-swap`. Jobs can be cancelled and render tiles outward from the view center.
- **Color:** each document keeps its embedded profile, and math runs on the encoded values, as in paint.net. Assign and Convert are history steps. On Mac, `CAMetalLayer.colorspace` is set to the document profile. Other platforms use a 33³ lookup-table path.
- **Native format** `.opaint`: a valid OpenRaster zip (layer PNGs, stack.xml, ICC). paint.net-only blend modes go in our own namespace, so Krita and GIMP can still open the file.
- **C interface:** about 25 functions:
  - post a command (`op_dispatch`) or read data (`op_query`) with JSON parameters
  - batched plain-struct pointer events (with pressure)
  - `op_view_attach` for the CAMetalLayer
  - an event callback
  - the platform-service function table
  - `catch_unwind` on every call, so a Rust panic becomes an error code instead of a crash

## Test system
- One entry point `cargo xtask check --tier fast|milestone|checkpoint`:
  - **fast** (each task, under 2 min): unit tests, small property tests, CPU goldens for the crates touched, Swift build
  - **milestone** (under 10 min): all goldens, Metal parity, CLI scenarios, a UI smoke test with screenshots
  - **checkpoint**: 10k-case property tests, the paint.net reference suite, 10 min of fuzzing per reader on Linode, a Linode run with lavapipe, benchmarks, the size gate, the full UI suite
- **Which reference wins** when sources disagree: a paint.net 5.1 capture first, then the OpenPDN port, then Pinta's reference images. Brushes, shapes and text use our own goldens, reviewed once by eye and replayed from pointer logs. Healing and content-aware fill are checked by quality metrics, not exact pixels.
- **Tolerance:** within ±1/255 per channel, with color ignored where alpha is 0. Exceptions must be listed in `tests/tolerances.toml` with a reason.
- **Speed and size budgets** (`perf/budgets.toml`): cold launch < 3 s (aim < 1 s), app < 75 MB (expect 22 to 30), open a 24 MP JPEG < 1.5 s, Gaussian blur at 24 MP < 0.5 s on GPU, 60 fps pan, brush latency < 16 ms. A checkpoint fails if a number gets more than 10% worse.
- **paint.net captures:** first a one-day spike on an "OPCapture" paint.net plugin. If that fails, FlaUI UI automation run by a scheduled task in the logged-in session, driven over SSH. Last fallback: OpenPDN plus Pinta only.

## Milestones: 50 milestones, 10 checkpoints, 6 phases
Lanes: E = engine Rust, O = ops and effects Rust plus WGSL, U = Swift app. Up to 3 builders run at once, one per lane. ★ = a deep-dive design review (Astra) before building. "h" = budget in agent hours. Each op milestone includes the CPU reference, the GPU kernel, a parity test, and a parameter schema; its UI comes free from M20.

**Phase 1: Foundations.** Bar: the app opens a PNG and shows it color-managed, with zoom and pan. The engine has layers, blend modes, undo and GPU parity.
- M01 Repo, xtask tiers, golden harness (fails a 2/255 diff, passes 1/255), launch and size scripts, Linux run. E, 4h
- M02 Reference corpus: vendor OpenPDN and the Pinta assets; spike the paint.net 5.1 capture on alpha-wolf. Gate: one SSH command captures Gaussian r=4 and Invert on 3 inputs, or a committed fallback decision. Timebox 8h. O
- M03 Pixel core (ColorBgra, Surface, unary and binary ops, port). Gate: matches OpenPDN on 10k random pixels. E, 4h
- M04 ★ Tile store and history (copy-on-write, LZ4, disk spill, jump). Gate: 200 random edits on 24 MP, undo all gives the same hash; RAM under the cap; a 50-step jump < 200 ms. E, 8h
- M05 ★ Document plus CPU compositor, 14 modes plus Overwrite (port of UserBlendOps.Generated.cs). Gate: 15 modes × 5 opacities within ±1/255. E, 6h
- CP1
- M06 ★ wgpu backend, presenter spike, parity runner. Gate: GPU compositor matches CPU on Metal and lavapipe. O, 7h
- M07 C interface and Swift bridge. Gate: a Swift test opens, adds a layer, inverts, undoes, reads pixels; no leaks. E, 5h
- M08 Design: 3 rendered mockups of the main window, Astra critique plus a fresh adversarial reviewer, I pick one; design tokens. U, 4h
- M09 App shell and Metal canvas, Zoom and Pan tools. Gate: 24 MP pans at 60 fps; paint.net zoom steps. U, 8h
- M10 ★ Color management. Gate: sRGB to P3 to sRGB within ±1/255; Adobe RGB file matches Preview within ΔE < 1. E, 7h
- CP2 (update page 1)

**Phase 2: Usable photo editor.** Bar: open JPEG, HEIC or RAW; crop at an aspect ratio and straighten; resize; Levels and Curves; undo from History; save with the profile kept.
- M11 Image I/O (pure-Rust codecs plus ImageIO for HEIC, AVIF, RAW; EXIF orientation). Gate: 30 files per format round-trip; RAW from 3 camera makes. U, 6h
- M12 Native `.opaint` (OpenRaster zip). Gate: 20 layers round-trip; 10k fuzz cases, no crash. E, 4h
- M13 Main window: floating panels with F5 to F8 toggles, thumbnail strip, tool-options bar, status bar, menu skeleton. U, 6h
- M14 Layers and History panels, Layer Properties with live preview. U, 6h
- M15 Resampling and Image menu ops (resize, canvas size, flip, rotate, flatten). Gate: each mode within ±1/255 of OpenPDN or captures. O, 6h
- CP3
- M16 ★ Selection engine (5 modes, antialiased and pixelated, invert, offset). Gate: mode truth tables on 50 random shape pairs; AA within ±1/255 of a 16× supersampled reference. E, 8h
- M17 Rectangle Select, Crop tool with aspect presets and straighten, Resize and Canvas Size dialogs. U, 7h
- M18 Adjustments 1: Auto-Level, Black and White, Brightness/Contrast, Invert Colors, Invert Alpha, Posterize, Sepia, Hue/Saturation. O, 6h
- M19 Adjustments 2: Curves, Levels, Exposure, Highlights/Shadows. O, 7h
- M20 Effect dialog framework built from the schema, cancelable tiled preview, Cmd+F repeat. Gate: preview < 100 ms on 12 MP; cancel leaves the hash unchanged. U, 6h
- CP4 (update page 2)

**Phase 3: Selections, painting, first effects.** Bar: everyday paint.net work: all selection tools, move and rotate, clipboard, pressure brushes, fills, gradients, common effects.
- M21 Lasso, Ellipse, Rectangle select tools with modifier keys; the Edit menu. U, 6h
- M22 ★ Magic Wand and Paint Bucket (paint.net distance formula, contiguous or global, layer or image sampling). Gate: masks pixel-exact against captures on 10 images × 4 tolerances. E, 7h
- M23 ★ Move Selected Pixels and Move Selection (nubs, rotation, 5 resampling modes, Ctrl to copy). Gate: 100 drags make one undo step. E, 8h
- M24 Clipboard (cut, copy, copy merged, paste into new layer or image, keeps ICC). U, 4h
- M25 Blurs 1: Gaussian, Motion, Radial, Zoom, Unfocus, Fragment, Median. Gate: Gaussian r=50 at 24 MP < 1 s on GPU. O, 7h
- CP5
- M26 Colors window (wheel with modifiers, More/Less, hex, 96-color palette, paint.net palette files) and Color Picker. U, 6h
- M27 ★ Brush engine with pressure (Paintbrush, Eraser, Pencil). Gate: replayed pointer logs match references; latency < 16 ms. E, 8h
- M28 Gradient tool with live handles (5 types, color and transparency modes). E, 6h
- M29 Artistic and Stylize effects (7). O, 6h
- M30 Noise and Photo effects (Add Noise, Reduce Noise, Glow, Red Eye, Sharpen, Soften Portrait, Vignette). O, 6h
- CP6 (update page 3)

**Phase 4: Drawing tools, text, the rest of the effects.** Bar: every paint.net tool works; most of the Effects menu exists.
- M31 Clone Stamp and Recolor. E, 5h
- M32 ★ Line/Curve and the edit-until-Finish framework that Shapes and Text share. E, 7h
- M33 Distort 1: Bulge, Frosted Glass, Pixelate, Polar Inversion, Tile Reflection, Twist. O, 6h
- M34 Shapes: 29 presets × fill, outline, or both. E, 7h
- M35 Render: Clouds, Julia, Mandelbrot, Turbulence, Voronoi. O, 6h
- CP7
- M36 ★ Text tool (CoreText on Mac hands glyph coverage to the engine; editable until Finish; input methods). U, 8h
- M37 Blurs 2 (5.x only: Bokeh, Sketch, Square, Surface) against captures. O, 7h
- M38 Save options and metadata: per-format quality settings with a file-size preview, keep or strip EXIF, XMP and GPS, a flatten prompt for flat formats. Gate: EXIF survives JPEG, HEIC, TIFF and AVIF; strip leaves no GPS. E, 6h
- M39 Distort 2 and Color: Crystalize, Dents, Morphology, Quantize. O, 7h
- M40 Object effects (Drop Shadow, Outline, Feather, Align), 3D Rotate/Zoom, Layer flip, Import from file. O, 7h
- CP8 (update page 4)

**Phase 5: Retouch and full parity.** Bar: healing, content-aware fill and remove background work; every effect is in menu order and passes parity.
- M41 Healing brush and spot heal (Poisson blend). E, 7h
- M42 ★ Content-aware fill (multi-scale PatchMatch). Gate: < 5 s at 12 MP with a 5% hole; quality metric target on 10 cases. O, 8h
- M43 Remove background (Vision mask to a selection or to layer alpha). Gate: IoU ≥ 0.9 on 10 hand-masked photos. U, 3h
- M44 ★ GPU parity sweep and perf table for every op. O, 7h
- M45 Menus complete in paint.net 5.1 order, icons, dialog review. U, 6h
- CP9

**Phase 6: Polish and release.** Bar: a stranger downloads a signed DMG, installs it, and edits photos. Every speed and size bar is met.
- M46 View menu (rulers, units, grid), Settings, light and dark mode. U, 5h
- M47 Robustness: autosave and crash recovery, a 200 MP document, a low-disk spill test, fuzzing every loader, a 1,000-step history stress test. Gate: kill the app mid-edit and recover the document; no crash in 1 h of fuzzing. E, 7h
- M48 Speed and size bars enforced by script. U, 5h
- M49 Shortcuts mapped from paint.net to Cmd, VoiceOver labels, strings ready for translation. U, 5h
- M50 Release: Developer ID signing, notarization, DMG on GitHub Releases, page on yili.dev (an established target). U, 5h
- CP10: tag v1.0

The critical path runs through M01, M03 to M05, M07, M09, M13, M14, M17, M21, M24, M26 to M28, M32, M36, M45, M48 and M50. The total is about 300 agent hours, or about 130 to 150 hours of wall time across 3 lanes.

## Build process after approval
1. **Step 0, before M01** (about 3 h):
   1. Create the repo and its worktree.
   2. Astra deep-dives the 13 ★ milestones and reviews this plan. A fresh Opus reviewer runs at the same time.
   3. I fold the accepted findings into `docs/MILESTONES.md`.
   4. I publish 2 private artifact pages:
      - the overall design and components, with a diagram
      - all milestones by phase and checkpoint, with each bar
2. **Each milestone:**
   1. A builder brief through `prepare_task.py` and `runner.py`, starting at Sol (medium). If no slot is free, it runs as a native Opus builder.
   2. Fast-tier tests, then the milestone tier.
   3. I check the result against the exit gate, then commit and push.
3. **Each checkpoint:**
   1. The checkpoint tier, including the Linode run.
   2. A review: Astra plus Opus for ★ work, one reviewer otherwise.
   3. A diagram of what changed (diagram-design skill).
   4. The update page, with screenshots, numbers and "choices to check".
   5. A push notification.
4. **On a critical question:** pick the safe default, log it, and continue. I still stop for anything on the ask-first list: new spending, credential changes, deleting user data.
5. **Time:** a milestone 50% over its budget gets a note and is cut back to its exit gate.

## Choices to check (my defaults)
- **GPU path.** "GPU effects from the start" became wgpu, with a CPU twin for every op as the test oracle.
- **Math space.** Math runs on profile-encoded values, as in paint.net, not in linear light.
- **Native format.** `.opaint` is an OpenRaster zip.
- **Text.** Text is shaped by CoreText on Mac. A portable shaper comes later.
- **Captures.** paint.net captures run on alpha-wolf through a plugin or a scheduled-task automation. If both fail, the fallback is OpenPDN plus Pinta.
- **AVIF export.** AVIF export works only where macOS can encode it.

## Not in this version
- Layer masks, groups, adjustment layers
- 16-bit, float or HDR storage; CMYK editing
- .pdn and PSD import; animated GIF and WebP frames
- A public plugin API, scripting, batch processing
- Windows, Linux and iPad UIs (the engine stays portable)
- Printing; App Store build; generative AI fill

## Verification (end to end)
- **Every checkpoint:** `cargo xtask check --tier checkpoint` passes on the Mac, and the engine part passes on Linode. Benchmarks are within budget. An XCUITest run opens a photo, crops, runs Curves and Gaussian, paints, undoes from History, and saves. A screenshot of that run is reviewed by eye.
- **Final:** on a clean Mac, download the DMG from GitHub, open it with no Gatekeeper warning, and repeat the smoke flow.

## Research findings (2026-10-24, 22 Exa searches plus source reads)

### What paint.net is (the bar to match)
- Tools (getpaint.net docs): Rectangle, Lasso, Ellipse select, Magic Wand; Move Selected Pixels, Move Selection; Zoom, Pan; Paint Bucket, Gradient; Paintbrush, Eraser, Pencil; Color Picker, Clone Stamp, Recolor; Text, Line/Curve, Shapes (29 preset shapes).
- Selection modes: Replace, Add, Subtract, Intersect, Invert (xor). Clipping: pixelated or antialiased.
- Magic Wand and Bucket: Contiguous or Global flood, Tolerance %, sampling Layer or Image, tolerance alpha Premultiplied or Straight. Distance = Euclidean ARGB, alpha-weighted (disassembly on forums.paint.net topic 117085).
- 14 layer blend modes: Normal, Multiply, Additive, Color Burn, Color Dodge, Reflect, Glow, Overlay, Difference, Negation, Lighten, Darken, Screen, Xor. Tools add Overwrite.
- Adjustments: Auto-Level, Black and White, Brightness/Contrast, Curves, Exposure, Highlights/Shadows, Hue/Saturation, Invert Colors, Invert Alpha, Levels, Posterize, Sepia.
- Effects: 9 submenus (Artistic, Blurs, Color, Distort, Noise, Object, Photo, Render, Stylize), about 50 effects in 5.1. Ctrl+F repeats the last effect.
- Live, editable-until-Finish tools: shapes, line/curve, gradient, text, move. Users name this as a main reason they like it ("what you see is what you get").
- Move tools: nubs for resize, rotate in a corridor outside the box, movable rotation center, Shift snaps 15 degrees, Ctrl drags a copy, 5 resampling modes.
- Colors window: primary and secondary slots, HSV wheel with Ctrl/Alt/Shift modifiers, "More" expands to RGB, HSV, hex and alpha; 96-color palette, text palette files `aarrggbb`.
- History window: click any step to jump; unlimited undo limited by memory and disk.
- Speed: Rick Brewster says opening fast is a core value ("not punish people for closing the program"). 5.2 moved compositing to float32 to stop 8-bit rounding errors across layers.

### Reference code and licenses
- OpenPDN (paint.net 3.36.7) is MIT. `src/Effects` holds about 60 effect files (GaussianBlur, Curves, Levels, HueAndSaturation, RedEyeRemove, SurfaceBlur, Median, Dents, Clouds...). `src/Data/UserBlendOps.Generated.cs` holds the blend math. Port these algorithms directly, with credit.
- Pinta (MIT) is a GTK clone of paint.net 3.0, with 3.36 code. `tests/Pinta.Effects.Tests` has 126 reference images: a ready oracle for effect output.
- libmypaint (ISC): dab-based brush engine (spacing, pressure, smoothing filter). Use as a guide for brush stroke spacing and smoothing.
- LazPaint and KolourPaint are GPL: read for behavior only, never copy code.
- `.pdn` format: PDN3 magic, XML header, then .NET binary serialization (MS-NRBF) with gzip chunks. An independent Rust reader exists as prior art (photocraft docs/pdn.md). Paint.NET 6.0 (2027) plans a new documented format.

### Architecture findings
- Copy-on-write tiles (Krita, photocraft, Arte Ogre): 256x256 tiles in reference-counted pointers; undo stores tile swaps, so a small stroke costs only its tiles; history spills to disk past a memory cap; undo must also restore selection and active layer.
- Compositing math: W3C Compositing and Blending Level 1 (premultiplied alpha formulas); blend in float; decide linear vs gamma-encoded per operation.
- Rust core with Swift UI over a C interface is proven (z-image-app, Archon CAD, ImageOptim2). Keep the boundary small: few entry points, each side owns its own memory.
- Rust crates: `zeno` (path rasterization, Skia-quality AA), `tiny-skia` (about 200 KiB), `swash` (text shaping and glyphs), `png`, `zune-jpeg`, `image`, `fast_image_resize`, `lcms2`. On Mac, ImageIO (system) can decode HEIC, RAW, WebP, AVIF for free.
- Canvas display: Metal view outside the scroll view, draw only visible tiles, mipmaps for zoom-out, nearest sampling above 100% zoom.
- Launch speed: one statically linked binary, few dylibs, no work in static initializers, measure cold launch with Instruments.
- Testing: golden images with per-channel tolerance (ImageSharp, zensim-regress), a separate f64 reference for each kernel, property tests, fuzz the file readers.

### Market gap
- HN 2024 and 2026 threads: Mac users still ask for "paint.net on Mac"; Pinta "appears buggy" and feels slower; Pixelmator Pro is the usual answer but is broader and Apple-owned. A paid look-alike (Pixelcasso) exists.

## User decisions (2026-10-24, 24 answers)
| Area | Decision |
| --- | --- |
| Name | OpenPainter |
| Audience | Public free download: signed, notarized DMG on GitHub Releases plus a yili.dev project page; same Developer ID as Simple Menubar Hider and Kelpi |
| License | MIT, open source. Port OpenPDN and Pinta (MIT) with credit; GPL code is behavior-only |
| Min OS | macOS 15 Sequoia |
| Engine | Rust core, Swift UI, C interface |
| GPU | GPU effects from the start. Because the engine is cross-platform, this means wgpu (WGSL) in the core, with a CPU reference for every op as the test oracle |
| Precision | 8-bit RGBA storage, float32 math |
| Color | Full color management (embedded ICC, assign/convert, display profile) |
| Formats | PNG, JPEG, BMP, GIF, TIFF + HEIC, WebP, AVIF + Camera RAW (Mac ImageIO). No .pdn or PSD import in v1 (not chosen) |
| Retouch extras | Crop tool with aspect ratios and straighten, healing and spot-heal brush, content-aware fill, remove background (Mac Vision) |
| Editing model | Destructive like paint.net; every step fully undoable from History (user: "undo should just work... we should copy that") |
| Text | paint.net style: editable until Finish, then pixels |
| Plugins | Internal effect registry with parameter schema; no public API in v1 |
| Pressure | Yes: size and opacity |
| Platforms | Engine tests on Linux (Linode) at each checkpoint; Windows and Linux UIs after Mac v1 |
| Effects order | Nothing deferred; paint.net menu order |
| Layout | paint.net floating panels (F5 to F8) |
| Multi-image | paint.net thumbnail strip |
| Look | paint.net layout, Mac polish, original icons |
| Mockups | 3 mockups, critique, I pick and continue |
| Speed bar | Cold launch < 3 s, app < 75 MB (user's numbers) |
| Accuracy | Match reference outputs within ±1/255 |
| Windows reference | User's PC `alpha-wolf.taila9cdd0.ts.net`, SSH over Tailscale (setup in progress, steps at claude.ai/artifact/JqxYkSwtr4jwdN9YTgt94r). I may install paint.net, Python and a capture helper. paint.net is GUI-only, so captures run through a scheduled task in the logged-in desktop session or a paint.net plugin; fallback OpenPDN + Pinta |
| Checkpoints | Every 5 milestones: publish an update page; on a critical question pick the safe default, list it, push-notify, keep going |
