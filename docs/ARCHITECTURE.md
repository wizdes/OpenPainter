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

(Revised after the Step 0 review; see `docs/reviews/step0-decisions.md`.)

### Pixels and arithmetic contracts
- Stored as **8-bit RGBA, straight alpha**. Straight color is kept everywhere it has meaning: storage, history, persistence, color adjustments, Invert Alpha, and the wand's Straight tolerance mode. Colors under zero alpha are preserved bytes.
- There is no single blanket kernel rule. Every operation declares an **arithmetic contract** in its spec:
  - input alpha form (straight or premultiplied) and where it converts
  - working space: `Encoded` (the document's encoded values), `Linear` (linear light via the document profile's curve), or an operation-specific space
  - precision of each stage and every quantization point (where 8-bit rounding happens mid-way)
  - rounding rule at each quantization point (default `floor(clamp(v,0,1)*255 + 0.5)`)
  - border rule (clamp, wrap, mirror, transparent)
  - target behavior and reference version (`legacy-3.36` or `pdn-5.1`), from `tests/oracle/targets.toml`
- **Layer compositing** uses float math (the user's decision, matching paint.net 5.2's FP32 compositor): premultiplied f32, one rounding at the end. Its oracle is an f64 spec of the paint.net blend formulas. The paint.net 3.36 integer blend is a cross-check with a documented tolerance, not the gate.
- **Ops whose target is `legacy-3.36`** reproduce the legacy integer stages exactly on the CPU. Their GPU twin uses integer math (`u32`, with 64-bit emulation where sums overflow) on `rgba8uint` data.

### Tiles and memory
- 256×256 tiles. A layer is a persistent map from tile coordinate to `TileId`. A missing entry means exactly transparent black `(0,0,0,0)`. Any other zero-alpha color is an allocated tile.
- `TileId` names immutable pixel content. The **storage manager** owns every tile record: `Resident | Compressed (LZ4) | Spilled (offset in a session file)`, plus pin count, byte count, checksum, codec version and last-use counter. A spill location is published only after its write succeeds.
- Writing creates a new `TileId`. Old ids stay alive while any history state, snapshot or job pins them. Reference ownership is tracked by the manager, never by `Arc::strong_count`.
- **Current-state tiles page too.** Only pinned tiles (visible, in use by a job, under the cursor) must be resident. One global memory budget covers document pixels, history, preview overlays, CPU scratch, GPU caches and upload/readback buffers. On this 8 GB Mac the default budget is 2 GB.

### Document
- `DocState { width, height, dpi, profile, layers: Vec<LayerState>, active_layer, selection: SelectionState, metadata, revision }`. `LayerState { id (stable u64), name, visible, opacity: u8, blend: LayerBlendMode, tiles }`.
- `LayerBlendMode` has the 14 paint.net layer modes. Tool composite modes (Normal, Overwrite, and per-tool modes) are a separate `ToolCompositeMode`; Overwrite is not a layer mode.
- A saved revision is stored separately so undoing back to the saved state clears the "unsaved" mark.

### Selection
- `SelectionState = None | Active(SelectionSource)`. `SelectionSource = Geometry(Arc<PolygonSet>) | Coverage(Arc<MaskGrid>)`.
  - No selection: tools and effects affect the whole layer.
  - Active empty selection: edits affect nothing.
  - Select All: an active rectangle covering the canvas.
- Geometry inputs (rectangle, ellipse, lasso) combine with polygon booleans (`i_overlay`, f64) before rasterization. Coverage inputs (wand with antialiasing, remove background) keep their original bytes. Mixed combines follow documented rules (union = max, intersect = min, subtract = saturating subtract, xor = absolute difference) with behavior tests.
- Contours are derived only for marching ants and cached separately.
- Antialiasing references use exact analytic area coverage in f64. Never a supersampled estimate.
- OpenPDN's GPC-based geometry is not ported (non-commercial license).

### History
- `History = Vec<HistoryStep { label, icon, command_id, state: DocState }>` plus a current index. Each step shares tiles with its neighbors.
- Undo, redo and "click a History row" swap the current state atomically. This restores selection, active layer, profile and metadata too.
- A new action after undo drops the redo branch.
- A failed operation leaves the current state and the history index unchanged.

### Recovery records (M57)
Autosave keeps a crash copy of every open document; it never replaces the user's own file.
- **Where:** `~/Library/Application Support/OpenPainter/Recovery/<doc-uuid>/`, one folder per open document. The app picks the UUID when the document opens. Tests pass their own root. Making a folder also syncs its parent.
- **Files:** `owner.lock` (held with `flock` while a live process owns the folder; the kernel lets go when the process dies, even on SIGKILL), `manifest.json`, and one `snapshot-<rev>-<pid>-<n>.opaint`.
- **Manifest:** `format` (1), `revision`, `original_url` (or null), `time_unix`, `snapshot` (the file name), `snapshot_bytes`, `snapshot_crc32`, `width`, `height`, `layers`, `pid`, and `base_file`: the original file's size and modification time when the document was last opened or saved.
- **Write order:** (1) the snapshot goes to a temp file, is synced, renamed into place, and the folder is synced; (2) the manifest goes the same way (temp, fsync, rename, fsync the folder); (3) only then is the old snapshot deleted and the revision acknowledged (a `Recovery` event and `doc.recovery`). Until step 2's rename, the old manifest still names the old, complete snapshot, so a torn write never wins. Once the rename is done, the snapshot it names is never deleted, even when the folder sync after it fails (that error is reported). A reader accepts a record only when the snapshot's size and CRC-32 match the manifest.
- **Clean saves:** a clean save (or a mark-saved after an ImageIO save) moves a save counter on at once and returns; the writer thread then takes the file's stamp again and deletes the record (manifest first, then snapshots), so the save never waits for a record write. A write reads the counter before it loads the document state, and abandons its commit when the counter moved, so no record older than the save is written. A crash between the save and that delete leaves a record whose stamp no longer matches the file (see below). The app marks a step saved only when AppKit's whole save succeeded, so a save that fails in its last step keeps both the unsaved mark and the record.
- **Older than the file:** if the original file's size or modification time now differs from the manifest's `base_file`, the file was saved after the record (by this app just before a crash, or by another app). Scan marks such a record `stale`: it is never recovered over the file (`recovery_open` refuses it), but the sheet offers "Recover as Untitled", which opens it as a new image with no file (`recovery_open_untitled`), marked unsaved.
- **Locks and deletes:** an attach waits up to 2 s for a scan holding the folder's lock. A folder is deleted only while its lock is held: it is renamed to a hidden tombstone (`.<uuid>.deleted-…`), then removed, so another attach to the same path makes a new folder instead of losing one. After taking a lock, the holder checks that the lock file at the path is the one it locked. A writer that finds its folder gone reports it and never makes it again.
- **When:** one writer thread per document reads the published snapshot (an `arc-swap` load), so it never waits for the actor or the UI. It writes 30 s after the last change, or at the latest 2 minutes after the first change that has no record, while edits go on. A clean document has no record: the writer deletes it, and a failed delete is reported and tried again after the next settle period. Close returns at once, but first renames the record folder to a tombstone (one rename and a parent sync), so a Quit right after can never leave a record to offer. The writer is told to stop: it checks the request between tile rows while building layers and the composite, and between 64-row bands of each PNG encode (the encode streams into the ZIP), abandons the write and deletes the tombstone in the background. On Quit the app waits up to 2 s for such writers (`op_recovery_wait_stopped`); a tombstone left by a Quit cut short is deleted by the next scan.
- **Launch:** the app lists the folders whose lock it can take (`op_recovery_scan`) off the main thread; a folder with a manifest is a document that did not close cleanly. One sheet lists them with Recover and Discard. Recover reads and decodes off the main thread (the row shows a spinner), then opens the snapshot as a new document marked unsaved, with the original file as its file, and keeps writing into the same folder, so the old record stays until a newer one replaces it. Discard deletes the folder. A folder with no manifest (a crash before the first record) is deleted by the scan.
- **Proof and its limits:** `op-cli/tests/recovery_kill.rs` kills a helper process with SIGKILL before, during and after a record write; `op-engine/tests/recovery.rs` and `recovery_fixes.rs` cover timing, save, close, recover, the save race, the stale record, concurrent attaches and scans, an injected disk-full write and an injected folder-sync failure; `op-engine/tests/low_disk.rs` fills a real 20 MB disk image. A SIGKILL leaves the kernel's page cache intact, so the kill tests prove the order of renames, not that the fsyncs reach the disk: proving durability needs a power cut or a VM snapshot without the page cache, which no test here does.

### Commands
- `CommandSpec { id: "image.flip_horizontal", label_key, params: ParamSchema, enabled(&DocState) -> bool, run(ctx, params) -> Result<Outcome> }`.
- Ids are stable strings: `layer.add`, `edit.undo`, `effect.blur.gaussian`, `tool.select`, `history.jump`. Params are JSON validated against the schema.

### Effects
- `EffectSpec { id, category, schema, contract, working_space, prepare(snapshot, selection, params) -> Prepared, required_input(output_rect, &Prepared) -> Rect, passes: [Pointwise | Neighborhood | Reduction | Warp | Iterative], render_cpu(...), gpu: Option<GpuKernel> }`.
- `prepare` runs once per job for whole-image analysis (histograms for Auto-Level and Levels Auto, palettes for Quantize). Effects may read pixels outside the selection; the selection limits only where output is written. Selection coverage is applied once, at the end.
- Tile results must equal whole-image results for every effect (tested).
- `ParamSchema` is a list of `Param { key, label, kind, enabled_when }`. Kinds: Int, Float (range, step, default), Bool, Choice, Angle, Point, Color, Seed (with Reseed), Text, Curve, Levels. Swift builds dialogs from the schema.

## 4. Rendering, jobs and previews

- **Compositing:** bottom to top, per the compositing contract. The checkerboard is drawn under the image at display time only.
- **Dirty flow:** commands and tools report dirty rects in document space. Composite tiles at level 0 and their mip parents are marked dirty. Visible tiles re-composite first. Canvas dirtiness never crosses the FFI.
- **Mips:** a GPU tile pyramid of the composite, built lazily with a 2×2 box filter below 100% zoom. Trilinear below 100%, nearest above.
- **Presenter (decided in M09, `docs/presenter-decision.md`):** wgpu renders straight into the view's `CAMetalLayer` (`SurfaceTargetUnsafe::CoreAnimationLayer`, `Bgra8Unorm`, FIFO, frame latency 2). The engine converts document → display color in the view shader with a 33³ table built by lcms2 (trilinear, hand-interpolated, no hardware filtering) and tags the layer with the display profile, so Core Animation does not convert a second time. The display profile comes from the window's screen; on `NSWindow.didChangeScreenNotification` or a display-profile change, Swift sends the new ICC and the engine rebuilds the table. After every surface configure the presenter re-reads `CAMetalLayer.colorspace` and sets it again if it changed (wgpu 27.0.4 leaves it alone; a test fails if a wgpu upgrade starts changing it). Overlays (selection, handles, pixel grid) are drawn after the table, in display space. On sleep the surface is dropped and rebuilt on wake. Measured in M09: 200 resizes, 100 document switches, hide/show, simulated screen moves and sleep/wake, all with zero validation errors and patch colors within ΔE00 0.77 of ColorSync. The rejected option (IOSurface ring with a Swift-owned layer tagged with the document profile) also passed but needs two GPU queues, a ring reallocation on every resize and wgpu-hal interop.
- **Preview sessions** are one generic mechanism used by Layer Properties, effect dialogs, the wand, the bucket, move, gradient, line/curve, shapes and text. A session holds its base revision, the original pixels it needs, its parameters and a generation counter. It renders into a `PreviewStack` (override tiles, a floating layer with an affine transform, vector overlays) that never touches history. Accept commits exactly one history step through the document actor. Cancel restores everything.
- **Job identity:** every job carries document id, base revision, target layer id, selection revision, preview session id and parameter generation. Results whose identity no longer matches are dropped and never committed. Never mix generations.
- **Threading:** the UI thread only posts work and reads published snapshots. Each document has one actor thread that runs commands in order and publishes an immutable snapshot through `arc-swap`. A rayon pool does CPU tile work. One render thread owns the GPU queue. Jobs render tiles in a spiral from the view center and stream progress. Panics are caught at the FFI entry, at each job boundary and at each frame; a panic marks the document read-only and keeps its last committed snapshot.

## 5. GPU and CPU

- GPU: compositing, mips, presentation; effect previews and final renders for effects with a WGSL kernel; move and rotate preview; resampling.
- CPU only: brush dabs, flood fill and wand, selection raster, text raster, heal and PatchMatch in v1, file I/O, history.
- **CPU reference:** every op has a CPU implementation that follows its contract exactly. It is the oracle for the GPU, the fallback, and the CLI default. CPU code is deterministic: no fast-math, fixed reduction order, seeded PCG random numbers hashed from document coordinates.
- **GPU rules:** request the adapter's real limits (not wgpu defaults). `textureLoad` only, no hardware filtering. Thresholds, ranks and comparisons in integer math. Weights, curves and trig precomputed on the CPU as lookup tables. Store through `rgba8uint` with the shared rounding helper. Images wider than 16384 use tiled sources.
- **Parity:** separate tests for (a) reference compatibility, (b) CPU vs GPU, (c) tiles vs whole image, (d) preview vs commit, (e) determinism under reordered tile scheduling. CPU vs GPU is ±1/255 per channel for continuous ops; discrete ops use the "at most N differing pixels" rule from the target table. Runs on Metal and on Linux lavapipe. A parity test fails if the GPU backend did not actually run (no silent CPU fallback); it logs adapter, backend, driver and dispatch counts.
- **No usable GPU or device lost:** switch to the CPU backend for everything, log it, and show it in the status bar.

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
OpStatus  op_view_detach(OpEngine*, uint64_t view);
OpStatus  op_doc_close(OpEngine*, uint64_t doc);
OpStatus  op_pointer(OpEngine*, uint64_t view, const OpPointerEvent*, size_t n);
void      op_buffer_free(OpBuffer*);
void      op_pixels_free(OpPixelBuffer*);
```

- **Views (M12).** `OpViewState` carries the view size in points, the backing scale, the zoom (points per document pixel), the document origin in points, the display ICC (borrowed; length 0 keeps the last one) and flags (`OP_VIEW_PIXEL_GRID`, `OP_VIEW_SUSPENDED`). `op_view_set` only stores the newest state and wakes the engine's render thread; states that arrive between frames are merged. Read-only helpers for tests and the trace: `op_view_info`, `op_view_counters`, `op_view_take_frame_times`, `op_view_wait_frames`. `op_doc_close` detaches the document's views first; `op_engine_free` detaches all views first.
- Documents and views are `u64` ids. A stale id returns `OP_ERR_NO_SUCH_DOC`. `OpEngine*` is the only pointer handle.
- Pointer events are plain structs (`x, y, pressure, tilt_x, tilt_y, timestamp_ns, buttons, modifiers, phase`), batched, never JSON.
- Events arrive on an engine thread as `OpEvent { kind, doc, ticket, json_ptr, json_len }`; the payload is valid only during the call. Kinds: DocChanged, HistoryChanged, LayersChanged, SelectionChanged, ToolStateChanged, Progress, JobFinished, ThumbnailReady, GpuFallback.
- Errors: sync calls return `OpStatus`; async failures arrive in JobFinished. Every exported function, every job and every render frame is wrapped in `catch_unwind`; a panic returns or reports `OP_ERR_PANIC` and marks the document read-only with a prompt to save a copy. A wgpu uncaptured-error handler turns GPU validation errors into events instead of panics.
- Callbacks must not call back into the engine (no `op_*` call from inside `OpEventFn`); Swift copies the payload and hops to its own queue. `op_view_detach` and `op_doc_close` return only after their in-flight frames and jobs stop. `op_engine_free` stops all threads first; after it returns, no callback ever runs again.
- `op_query` never blocks on work: it reads the published snapshot only. Expensive answers (histograms, thumbnails) are jobs that report through events.
- Memory: inputs are borrowed for the call; outputs are owned by Rust and freed with `op_*_free`. The engine retains the CAMetalLayer on attach.
- Platform services: Swift registers `decode_image`, `encode_image`, `subject_mask`, `shape_text`, `display_profile_changed`. A command that needs a missing service is disabled.
- Decoding split: the engine decodes PNG, JPEG, BMP, GIF, TIFF, WebP and .opaint itself (fuzzable, identical on Linux). Swift decodes HEIC, AVIF and RAW through ImageIO and hands pixels plus ICC over `op_import_pixels`. HEIC export goes through ImageIO; AVIF export only where the OS can encode it.

## 7. Color management

- Each document carries one ICC profile: the embedded one, or sRGB. Pixels stay encoded in it. Each operation's contract says whether it works on encoded values or in linear light (paint.net 5.1 effects default to a linear working space for many effects, and Resize has a gamma option). The M02 catalog records this per effect.
- Open: keep the embedded profile by default (a setting can convert to sRGB). Gray expands to RGB. CMYK converts to sRGB (perceptual, black-point compensation) with a notice.
- `image.assign_profile` changes metadata only; `image.convert_profile` transforms pixels with lcms2 (alpha copied, intent selectable). Both are history steps. Pasting or importing from another profile converts into the document profile.
- Display: the document → display table described in section 4 (M09). wgpu 27.0.4's surface configure does not touch `CAMetalLayer.colorspace`; the presenter still re-checks it after every configure. The display profile comes from the window's screen and is updated when the window moves. Table accuracy is measured in ΔE00 against ColorSync and against direct lcms2 transforms (M09: worst ΔE00 0.77, worst 3 levels; 2.6 levels of that from the 33³ table), not assumed.
- lcms2 links statically; the release check runs `otool -L` and fails on any `/opt/homebrew` or `/usr/local` dependency.
- Export embeds the profile; "convert to sRGB on export" is an option.

## 8. Tests

- One entry point: `cargo xtask check --tier fast|milestone|checkpoint`.
  - fast (< 2 min): unit tests, proptest at 64 cases, CPU goldens for touched crates, Swift build and unit tests.
  - milestone (< 10 min): all CPU goldens, Metal parity, opcli scenarios, XCUITest smoke with screenshots.
  - checkpoint: proptest at 10k cases, the paint.net capture suite, 10 min fuzzing per reader on Linux, the Linux run with lavapipe, benchmarks, size gate, full XCUITest.
- **Oracles:** `tests/oracle/targets.toml` (made in M02) lists every operation with its target behavior (`legacy-3.36` or `pdn-5.1`), its oracle, its tolerance rule and its working space.
  - `legacy-3.36` rows: the OpenPDN oracle harness (M03), which runs the real 3.36 pixel code under .NET 8 in the Linux VM. Exact or ±1/255.
  - `pdn-5.1` rows: captures from the pinned paint.net 5.1.12 on the Windows PC, with a tolerance and metric written per row.
  - Layer compositing: the f64 spec (float math, user decision), with the 3.36 integer blend as a documented cross-check.
  - Pinta's images: a weak cross-check (±2) only for effects Pinta did not change.
  - Tools (brush, gradient, line, shapes, text): our own reviewed goldens plus geometry tests. Never capture pixels.
  - Clock-seeded legacy effects: seeded own goldens plus mean and variance checks.
- Brushes replay recorded pointer logs with different event batching; results must not depend on batching.
- Every required test reports a nonzero case count. A checkpoint commits its capability list first: required capabilities must run and pass; later ones are listed as absent, never silently skipped.
- Heal and PatchMatch: metrics (no hole pixels left, boundary gradient error, SSIM against a reviewed result), fixed seed.
- History: proptest state machine; undo all equals the start, redo all equals the end, jumps equal a replay.
- Formats: round-trip property tests plus a real-file corpus (camera and phone JPEGs, CMYK and gray JPEGs, 16-bit and palette PNGs, multi-page and LZW TIFFs, animated GIF and WebP first frame).
- Fuzz targets: PNG, JPEG, BMP, GIF, TIFF, WebP, .opaint, ICC parse, command JSON.
- Tolerance: appearance comparisons are ±1/255 per channel with color ignored where alpha = 0, or compared premultiplied where the target row says so. Persistence, undo and history tests compare every byte, including colors under zero alpha. Exceptions only in `tests/tolerances.toml` with a reason and a bound.
- Goldens are 256² and 512² PNGs; budget 60 MB in the repo. Failures write expected / actual / diff×16 triptychs. Captured references are immutable; our own goldens change only with `cargo xtask goldens --update <id>` and a reviewed diff.

## 9. Speed and size

- Rust builds as a staticlib, arm64 only, fat LTO for the app (the `app-release` cargo profile; M58 measured it 1.2 MB smaller than thin at the same launch time; `cargo rustc --crate-type staticlib`, because with the rlib crate type also listed cargo runs no LTO; other release builds keep thin LTO), stripped, `panic=unwind` (needed for `catch_unwind`). wgpu uses the Metal backend only on Mac.
- Expected: engine about 12 MB, Swift app about 6 MB, assets about 3 MB. Bundle about 22 to 30 MB against a 75 MB limit.
- Lazy work: the window shows first; wgpu device and compositor pipelines start on a background thread; effect pipelines compile on first use and the mip builder on the first draw below 100% (M58 counts them: `op_gpu_pipeline_counts`, printed on the `OP_READY` line); lcms2 transforms are cached on demand; fonts are listed the first time the Text tool is chosen; Vision loads only for Remove Background.
- Budgets in `perf/budgets.toml`: cold launch < 3 s median and < 3 s p95 (aim < 1 s), open a 24 MP JPEG < 1.5 s, Gaussian r=10 at 24 MP < 0.5 s on GPU, pan and zoom ≥ 60 fps, brush latency < 16 ms, bundle < 75 MB. A checkpoint (`scripts/perf-check.sh`) fails when launch median, launch p95, first-effect median or bundle size is more than 10% worse than the stored baseline from the same machine, measured the same way (cache purged or not).

### Releases and updates (M58 notes for M60)

- Updates use Sparkle 2.10.0 (`mac/OpenPainterKit/Sources/Shell/Updates.swift`). The app's `Info.plist` requires a signed feed (`SURequireSignedFeed`), checks each archive before unpacking it (`SUVerifyUpdateBeforeExtraction`), and sets `SUSignedFeedFailureExpirationInterval` to 0 (without it, Sparkle accepts a badly signed feed again 20 days after the first failure). M60 set `SUPublicEDKey` to the release key (made 2026-10-25 with the user's OK; private half `OPENPAINTER_SPARKLE_PRIVATE_KEY` in `~/keys`) and turned on `SUEnableAutomaticChecks`; with an empty key the updater never starts.
- Developer ID fallback: with `SUVerifyUpdateBeforeExtraction` on, Sparkle accepts an archive whose EdDSA check fails if the archive is Developer ID signed by the same team as the installed app (`SUUpdateValidator.m`, the "key rotation" fallback). So the Developer ID certificate is as powerful as the EdDSA key: anyone who can sign with the team's Developer ID can ship an update. Keep both secret, and do not rely on EdDSA alone.
- Version order: Sparkle refuses an archive whose `CFBundleVersion` is lower than the installed one (`SUPlainInstaller.m`, "Refusing to downgrade"; `UpdateFeedTests.testDowngradeIsRefused`). `scripts/release.sh` sets `CFBundleVersion` to the number of commits on `HEAD` and requires the last `v*` tag to be an ancestor, so it always goes up; `sparkle:version` in the appcast matches it.
- The appcast and every archive are signed with Sparkle's `sign_update` (in `mac/build/DerivedData/SourcePackages/artifacts/sparkle/Sparkle/bin/`). Each update must keep `SUPublicEDKey` in its own `Info.plist`: Sparkle rejects an update that drops the key.
- Package pins: `project.yml` pins Sparkle with `exactVersion: 2.10.0`, and the resolved revision is tracked in `mac/Package.resolved`. XcodeGen generates the `.xcodeproj` (not tracked), so `make -C mac project` copies `mac/Package.resolved` into it and every `xcodebuild` in `mac/Makefile` runs with `-onlyUsePackageVersionsFromResolvedFile`. After changing a package, `make -C mac resolve` writes a new file to commit.

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
