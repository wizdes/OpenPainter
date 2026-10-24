# OpenPainter architecture

This is the design every builder follows. If a milestone needs to change it, the change is written here first.

## 1. The big picture

OpenPainter has two parts:

1. **The engine** (`engine/`, Rust). It holds all image data and all image math: layers, tiles, undo history, selection, blend modes, adjustments, effects, tools, file formats and color management. It has no UI. It builds on macOS, Windows and Linux.
2. **The Mac app** (`mac/`, Swift). It draws windows, panels, menus and dialogs, and it turns mouse, pen and keyboard input into engine commands. It also gives the engine a few Mac-only services: ImageIO (HEIC, AVIF, RAW), Vision (remove background) and CoreText (text).

They talk through a small C interface (`op-ffi`). Every user action is a **command** with a stable string id. Menus, shortcuts, the History panel, tests and the `opcli` command-line tool all send the same commands.

## 2. Repo layout

```
engine/                 Rust workspace
  op-core/              Document, Layer, TileGrid, TileStore, Selection, History
  op-ops/               CPU kernels: blend modes, adjustments, effects, resampling,
                        dab raster, flood fill, heal, PatchMatch. Effect registry
                        and ParamSchema. spec/ holds f64 reference functions.
  op-gpu/               wgpu device, WGSL shaders, GPU twins of ops, compositor,
                        canvas presenter
  op-color/             lcms2 wrapper: profiles, transforms, display LUT
  op-formats/           Pure-Rust PNG, JPEG, BMP, GIF, TIFF, WebP codecs and the
                        native .opaint format (OpenRaster zip)
  op-tools/             Tool state machines (brush, shapes, line/curve, gradient,
                        text placement, move with nubs, crop, heal, wand, select),
                        pressure curves, stroke smoothing
  op-engine/            Command registry, per-document actor thread, job scheduler
                        with cancellation, render scheduling, events,
                        PlatformServices trait
  op-ffi/               C ABI (cbindgen header include/openpainter.h), staticlib
  op-cli/               `opcli`: run any command, replay scenario scripts, render,
                        benchmark, dump the command and effect catalogs
  op-testkit/           (dev only) image diff, golden runner, tolerance file,
                        synthetic fixtures
  fuzz/                 cargo-fuzz targets (outside the workspace)
  xtask/                `cargo xtask check --tier fast|milestone|checkpoint`
mac/
  project.yml           XcodeGen project (never hand-edit the .xcodeproj)
  OpenPainterKit/       SwiftPM package
    EngineBridge/       C module map plus Swift wrappers; command structs are
                        generated from `opcli dump-catalog`
    CanvasView/         NSView with CAMetalLayer; pointer, pen and key input
    Panels/             SwiftUI views hosted in floating NSPanels
    EffectDialogs/      Dialogs generated from ParamSchema
    PlatformServices/   ImageIO, Vision, CoreText, ColorSync
  OpenPainter/          App target (thin)
  Tests/                XCTest and XCUITest
tests/
  oracle/openpdn/       Port notes per ported file
  oracle/pinta/         Pinta reference images plus inputs (MIT, credited)
  oracle/pdn51-captures/  PNGs captured from real paint.net 5.1
  fixtures/             Input images
  goldens/              Our own reviewed goldens
  scenarios/            JSON command scripts for opcli
  tolerances.toml       Every exception to the ±1/255 rule, with a reason
perf/budgets.toml       Speed and size budgets
tools/pdn-capture/      Windows capture kit for paint.net reference images
tools/codegen/          Catalog to Swift generator
```

**Parallel work.** Effects (`op-ops/src/effects/*.rs`) and commands (`op-engine/src/commands/*.rs`) register themselves. A `build.rs` lists those folders and generates the module list, so adding an effect touches one new file. The FFI header and the Swift catalog are generated, never hand-edited. Lanes: E (core, engine, tools, formats), O (ops, effects, shaders), U (Swift).

## 3. Data model

### Pixels
- Stored as **8-bit RGBA, straight alpha** (not premultiplied), like paint.net. Reason: ported paint.net code expects straight color, file formats are straight, and premultiplied 8-bit loses color at low alpha.
- Every kernel loads to **premultiplied f32**, computes, and stores back. One shared rounding helper: `u8 = floor(clamp(v, 0, 1) * 255 + 0.5)`. CPU and GPU use the same rule.
- Where a ported paint.net integer algorithm exists, the port matches its integer math first; the f32 path must agree within ±1/255.

### Tiles
- 256×256 tiles (256 KB at RGBA8). `TileGrid` is a dense `Vec<Option<Arc<Tile>>>`; `None` is fully transparent.
- Copy-on-write through `Arc::make_mut`.
- `TileStore` keeps each tile Resident, Compressed (LZ4) or Spilled (offset in a per-session temp file). Tiles reachable only from history move Resident → Compressed → Spilled by LRU. Budget: 25% of RAM for history-only tiles by default. Tiles of the current state always stay resident.

### Document
- `Document { id, width, height, dpi, profile: Arc<Profile>, layers: Vec<Layer>, active_layer: LayerId, selection: Selection, metadata }`.
- `Layer { id: u64 (stable), name, visible, opacity: u8, blend: BlendMode, tiles: TileGrid }`.
- `DocState` is immutable and cheap to clone because tiles are shared.

### Selection
- The source of truth is an f64 polygon set (like paint.net's geometry). Combine modes Replace, Add (union), Subtract (exclude), Intersect, Invert (xor) use polygon booleans (`i_overlay`).
- A cached antialiased 8-bit coverage mask is built from the path in the same tiled form. Pixelated mode snaps to pixel centers.
- Magic Wand and Remove Background make a bitmap first, which is traced to a path (marching squares plus simplification).
- Marching ants draw the path.

### History
- `History = Vec<HistoryStep { label, icon, command_id, state: DocState }>` plus a current index.
- Each step is a full persistent snapshot with structural sharing. It costs only the tiles it changed.
- Undo, redo and "click a History row" all swap the current state. Selection, active layer, profile and metadata come back for free.
- A new action after undo drops the redo branch, as in paint.net.
- Memory accounting counts tiles owned only by history. Huge ops (resize, rotate) create new grids; the old ones become history-only and spill first.

### Commands
- `CommandSpec { id: "image.flip_horizontal", label_key, params: ParamSchema, enabled(&DocState) -> bool, run(ctx, params) -> Result<Outcome> }`.
- Ids are stable strings: `layer.add`, `edit.undo`, `effect.blur.gaussian`, `tool.select`, `history.jump`.
- Params are JSON validated against the schema.

### Effects
- `Effect { id, category, schema(), halo(params) -> u32, render_cpu(params, src, dst_rect), gpu: Option<GpuKernel> }`. `halo` is how many pixels around a tile the effect reads.
- `ParamSchema` is a list of `Param { key, label, kind, enabled_when }`. Kinds: Int, Float (range, step, default), Bool, Choice, Angle, Point, Color, Seed (with Reseed), Text, Curve, Levels.
- Swift builds dialogs from the schema, like paint.net's IndirectUI.

## 4. Rendering

- **Compositing:** bottom to top. Each visible layer is converted to premultiplied f32, the blend op is applied with layer opacity, the result is stored. The checkerboard is drawn under the image at display time only.
- **Dirty flow:** a command or tool reports dirty rects in document space; the scheduler marks composite tiles dirty at level 0 and their mip parents; visible tiles re-composite first; the presenter redraws. Canvas dirtiness never crosses the FFI.
- **Mips:** the composite cache is a GPU tile pyramid. Levels below 100% are built lazily with a 2×2 box filter. The presenter samples the level just above the target scale with trilinear filtering; above 100% zoom it uses nearest sampling.
- **Canvas on Mac:** Swift creates a CAMetalLayer and passes the pointer. The engine creates the wgpu surface from it and draws image, checkerboard, pixel grid, marching ants and tool overlays in one pass. Fallback if the M06 spike fails: the engine renders into an IOSurface texture and Swift presents it on the same MTLDevice.
- **Live previews** never touch history. Each frame the compositor takes a `PreviewStack`:
  - Override: sparse temporary tiles that replace the active layer's tiles in a region (effect dialogs, strokes in progress, gradients, shapes, text).
  - Floating layer: pixels plus an affine transform (Move Selected Pixels while dragging), baked on Finish.
  - Overlays: vector primitives for nubs, handles, crop guides and ants.
- **Edit until Finish:** shapes, line/curve, gradient, text and move keep their parameters in tool state and re-rasterize the override on each change. Finish (Enter, a click outside, or a tool switch) commits exactly one history step.
- **Threading:** the UI thread only posts work and reads published snapshots; it never blocks. Each document has one actor thread that runs commands in order and publishes an immutable `DocSnapshot` through `arc-swap`. A rayon pool does CPU tile work. One render thread owns the GPU queue. Effects run as cancellable jobs that render tiles in a spiral from the view center and stream progress. Changing a dialog parameter cancels and restarts the job; OK reuses finished tiles.

## 5. GPU and CPU

- GPU: compositing, mips, presentation; effect previews and final renders for effects with a WGSL kernel (blurs, adjustments, distortions, stylize, noise with a seeded hash); move and rotate preview; resampling.
- CPU only: brush dabs, flood fill and wand, selection raster, text raster, heal and PatchMatch (v1), file I/O, history.
- **CPU reference contract:** every op has a CPU f32 implementation. It is the oracle, the fallback, and the CLI default. Blend modes and color math also get an f64 spec function in `op-ops/src/spec/`. CPU kernels are deterministic: no fast-math, fixed reduction order, seeded PCG random numbers hashed from document coordinates.
- **GPU rules:** WGSL kernels use `textureLoad` only (no hardware filtering), the same rounding helper and the same PRNG.
- **Parity tests:** for each GPU op, proptest builds tiles weighted to 0, 1, 127, 128, 254, 255 plus fixture images. Max per-channel difference ≤ 1, color ignored where alpha = 0. They run on Metal and on Linux with Mesa lavapipe (software Vulkan).
- **No usable GPU or device lost:** switch to the CPU backend for everything and log it. Display falls back to a CPU compositor into an IOSurface shown as CALayer contents.

## 6. The C interface

cbindgen, hand-designed, about 25 functions:

```c
OpEngine* op_engine_new(const OpConfig*, OpEventFn cb, void* user, const OpPlatformVTable*);
void      op_engine_free(OpEngine*);
OpStatus  op_dispatch(OpEngine*, uint64_t doc, const char* cmd_id,
                      const uint8_t* json, size_t len, uint64_t* out_ticket);
OpStatus  op_query(OpEngine*, uint64_t doc, const char* query_id,
                   const uint8_t* json, size_t len, OpBuffer* out);
OpStatus  op_cancel(OpEngine*, uint64_t ticket);
OpStatus  op_import_pixels(OpEngine*, const OpPixelImport*, uint64_t* out_doc);
OpStatus  op_export_flat(OpEngine*, uint64_t doc, OpPixelBuffer* out);
OpStatus  op_view_attach(OpEngine*, uint64_t doc, void* ca_metal_layer, uint64_t* out_view);
OpStatus  op_view_set(OpEngine*, uint64_t view, const OpViewState*);
OpStatus  op_pointer(OpEngine*, uint64_t view, const OpPointerEvent*, size_t n);
void      op_buffer_free(OpBuffer*);
void      op_pixels_free(OpPixelBuffer*);
```

- Documents and views are `u64` ids. A stale id returns `OP_ERR_NO_SUCH_DOC`. `OpEngine*` is the only pointer handle.
- Pointer events are plain structs (`x, y, pressure, tilt_x, tilt_y, timestamp_ns, buttons, modifiers, phase`), batched, never JSON.
- Events arrive on an engine thread as `OpEvent { kind, doc, ticket, json_ptr, json_len }`; the payload is valid only during the call. Kinds: DocChanged, HistoryChanged, LayersChanged, SelectionChanged, ToolStateChanged, Progress, JobFinished, ThumbnailReady, GpuFallback.
- Errors: sync calls return `OpStatus`; async failures arrive in JobFinished. Every exported function is wrapped in `catch_unwind`; a panic returns `OP_ERR_PANIC` and marks the document read-only with a prompt to save a copy.
- Memory: inputs are borrowed for the call; outputs are owned by Rust and freed with `op_*_free`. The engine retains the CAMetalLayer on attach.
- Platform services: Swift registers `decode_image`, `encode_image`, `subject_mask`, `shape_text`, `display_profile_changed`. A command that needs a missing service is disabled.
- Decoding split: the engine decodes PNG, JPEG, BMP, GIF, TIFF, WebP and .opaint itself (fuzzable, identical on Linux). Swift decodes HEIC, AVIF and RAW through ImageIO and hands pixels plus ICC over `op_import_pixels`. HEIC export goes through ImageIO; AVIF export only where the OS can encode it.

## 7. Color management

- Each document carries one ICC profile: the embedded one, or sRGB. Pixels stay encoded in it. Blending and effects run on the encoded values, as paint.net does.
- Open: keep the embedded profile by default (a setting can convert to sRGB). Gray expands to RGB. CMYK converts to sRGB (perceptual, black-point compensation) with a notice.
- `image.assign_profile` changes metadata only; `image.convert_profile` transforms pixels with lcms2 (alpha copied, intent selectable). Both are history steps. Pasting or importing from another profile converts into the document profile.
- Display on Mac: `CAMetalLayer.colorspace = CGColorSpace(iccData:)`, so ColorSync follows the window across screens. Portable path: a 33³ 3D LUT (document → display) built by lcms2, applied in the present shader; tested on Linux.
- Export embeds the profile; "convert to sRGB on export" is an option.

## 8. Tests

- One entry point: `cargo xtask check --tier fast|milestone|checkpoint`.
  - fast (< 2 min): unit tests, proptest at 64 cases, CPU goldens for touched crates, Swift build and unit tests.
  - milestone (< 10 min): all CPU goldens, Metal parity, opcli scenarios, XCUITest smoke with screenshots.
  - checkpoint: proptest at 10k cases, the paint.net capture suite, 10 min fuzzing per reader on Linux, the Linux run with lavapipe, benchmarks, size gate, full XCUITest.
- Oracle order when sources disagree: paint.net 5.1 capture, then the OpenPDN port, then Pinta. Logged in `tests/tolerances.toml`.
- Brushes, shapes, text: our own goldens, reviewed once by eye; brushes replay recorded pointer logs.
- Heal and PatchMatch: metrics (no hole pixels left, boundary gradient error, SSIM against a reviewed result), fixed seed.
- History: proptest state machine; undo all equals the start, redo all equals the end, jumps equal a replay.
- Formats: round-trip property tests plus a real-file corpus (camera and phone JPEGs, CMYK and gray JPEGs, 16-bit and palette PNGs, multi-page and LZW TIFFs, animated GIF and WebP first frame).
- Fuzz targets: PNG, JPEG, BMP, GIF, TIFF, WebP, .opaint, ICC parse, command JSON.
- Tolerance: ±1/255 per channel, color ignored where alpha = 0. Exceptions only in `tests/tolerances.toml` with a reason and a bound.
- Goldens are 256² and 512² PNGs; budget 60 MB in the repo. Failures write expected / actual / diff×16 triptychs. Captured references are immutable; our own goldens change only with `cargo xtask goldens --update <id>` and a reviewed diff.

## 9. Speed and size

- Rust builds as a staticlib, arm64 only, thin LTO, stripped, `panic=unwind` (needed for `catch_unwind`). wgpu uses the Metal backend only on Mac.
- Expected: engine about 12 MB, Swift app about 6 MB, assets about 3 MB. Bundle about 22 to 30 MB against a 75 MB limit.
- Lazy work: the window shows first; wgpu device and compositor pipelines start on a background thread; effect pipelines compile on first use; lcms2 transforms are cached on demand; fonts are listed the first time the Text tool is chosen; Vision loads only for Remove Background.
- Budgets in `perf/budgets.toml`: cold launch < 3 s (aim < 1 s), open a 24 MP JPEG < 1.5 s, Gaussian r=10 at 24 MP < 0.5 s on GPU, pan and zoom ≥ 60 fps, brush latency < 16 ms, bundle < 75 MB. A checkpoint fails when a number is more than 10% worse than the stored baseline from the same machine.

## 10. Top risks

1. wgpu surface on CAMetalLayer (resize jitter, colorspace overwritten, frame pacing). Spike in M06; `Presenter` trait; IOSurface fallback.
2. ±1/255 GPU parity. Shared rounding helper, no hardware filtering, edge-value proptests, two backends.
3. paint.net references are hard to capture (GUI-only). Plugin spike, then UI automation from a scheduled task, then OpenPDN plus Pinta.
4. Platform decoders differ from engine decoders (HEIC, AVIF, RAW only on Mac; RAW changes between macOS versions). One import call, Mac-side goldens with wider RAW tolerance, PNG twins for Linux.
5. Memory on this 8 GB Mac with large documents. Sparse tiles, LZ4, disk spill, a budget shown in the UI, a 100 MP × 20 layer stress test.
6. Pen pressure on Mac (Wacom drivers, Sidecar, Force Touch). Timestamped event abstraction, per-device pressure curve, recorded logs replayed in tests.
7. Text parity with paint.net (DirectWrite). Do not chase pixel parity; CoreText on Mac; our own goldens.
8. PatchMatch quality and speed at 24 MP. Multi-scale on a bounded region, time budget with progressive preview, metric tests.
9. SwiftUI panel performance. NSPanel shells, small `@Observable` models keyed by id, low-resolution thumbnails pushed by the engine, NSTableView if profiling shows hitches.
10. Odd ICC profiles. Tests with Display P3, Adobe RGB, gray, CMYK and v4 profiles; fall back to convert-to-sRGB with a notice.

## Later, not v1

- `.pdn` import (PDN3 header, XML block, MS-NRBF reader, gzip chunks), PSD import.
- Layer masks, groups, adjustment layers; 16-bit and HDR storage.
- A portable text shaper (swash) for Windows and Linux.
