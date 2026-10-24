# Presenter decision (M09)

**Decision: path A.** wgpu renders straight into the view's `CAMetalLayer`. The engine converts document color to display color with a 33³ lookup table, and the layer is tagged with the display profile.

Both paths passed every test. Path A wins because it is simpler and keeps the whole frame in one place, and its color stays within ΔE00 0.77 of ColorSync. Path B passed with exact bytes, but it needs a second GPU queue, a Swift-owned ring, a reallocation of the ring on every resize, and unsafe wgpu-hal calls. Details are below.

Date: 2026-10-24. Machine: Apple M2, macOS 27.0, one external 1920×1080 display at backing scale 1, 60 Hz. wgpu 27.0.1 (wgpu-hal 27.0.4, Metal).

## 1. The key fact behind the decision

`docs/ARCHITECTURE.md` section 7 says wgpu re-sets `CAMetalLayer.colorspace` on every configure. That is true of wgpu's trunk at the time of the Step 0 review. It is **not** true of the version we pin.

`wgpu-hal-27.0.4/src/metal/surface.rs`, `Surface::configure`, sets only these layer properties:

- `opaque` (from the alpha mode)
- `device`, `pixelFormat`, `framebufferOnly` (false when the usage is more than `COLOR_TARGET`)
- `presentsWithTransaction`, `wantsExtendedDynamicRangeContent` (only for `Rgba16Float`)
- `maximumDrawableCount` (frame latency + 1), `drawableSize` (the configured extent)
- `allowsNextDrawableTimeout = false`, `displaySyncEnabled` (from the present mode)

It never touches `colorspace`. The tests confirm this. The path A scenarios counted 253 configures, and the create/destroy test added 100 more. Every frame checks the counter, and the tag was the same before and after every configure (`tag changed by configure 0`). The presenter still checks the tag after each configure and sets it again if it changed, so a later wgpu version that does overwrite it would be repaired before the frame is shown. The test `tag changed by configure` will fail loudly if that ever happens.

## 2. What was built

- `engine/op-gpu/src/presenter.rs` and `presenter.wgsl`: a platform-neutral view renderer. It draws an `rgba8uint` document into any BGRA8 target at a zoom and pan, with a checkerboard under alpha, nearest sampling at 100% and above, a box filter of up to 4×4 below 100% (no mips yet), and an optional 33³ table applied with hand-written trilinear interpolation (no hardware filtering). Tests: `engine/op-gpu/tests/presenter.rs` (byte-exact at 100% with and without an identity table, 25% box filter, GPU table vs CPU table within 0.5 level, bad input refused).
- `engine/op-present-spike`: a staticlib with the C interface in `include/op_present_spike.h`. It has one shared context (instance, device, queue, uncaptured-error handler), path A presenters (surface from the `CAMetalLayer` pointer) and path B presenters (a ring of 2 or 3 IOSurfaces wrapped as wgpu textures through wgpu-hal). The table is built with lcms2 (float transform, relative colorimetric). Every exported function catches panics and returns `OP_PS_PANIC`. macOS only; on other systems the crate is empty.
- `mac/PresenterSpike`: a SwiftPM package (macOS 15). `PresenterSpikeKit` holds the window, both paths, the charts, the ColorSync reference and the scenarios. `PresenterSpikeHost` runs the suite and the bench. `PresenterSpikeTests` has 16 XCTests.

Path B's ring protocol: each slot moves FREE → WRITING (engine encodes) → READY (only wgpu's `on_submitted_work_done` moves it here) → PRESENTING (Swift takes the newest ready frame) → FREE (Swift releases it after its own blit's command buffer completes). Stale ready frames go back to FREE. The engine never picks a PRESENTING slot. When every slot is busy, it returns `OP_PS_BUSY` instead of overwriting. Swift tags the layer with the profile of the document generation that the frame shows, in the same transaction as that frame.

## 3. How color was checked (no screenshots)

- **Test charts:** three documents of 8×4 patches (32 known values: primaries, secondaries, halves, natural colors, near-black, near-white, near-gamut-edge). Their profiles are sRGB, Display P3 and Adobe RGB (1998), with ICC bytes from `CGColorSpace.copyICCData()`. Each chart has a different odd size, so a switch also changes the texture size.
- **Path A:** the drawable is copied to memory before present (`framebufferOnly = false` in the spike). Each patch center is compared with a **ColorSync reference**: Core Graphics draws the chart into a 32-bit float bitmap in the display space (relative colorimetric, no interpolation). Pass rule: **CIEDE2000 ≤ 1.0** per patch, with Lab from ColorSync (`CGColor.converted` to generic Lab). The level difference is recorded too, and the error is split into three parts with a test hook (`op_ps_debug_convert`).
- **Path B:** after the GPU signals completion, the IOSurface is locked and every document pixel in view is compared byte for byte with the document (BGRA vs RGBA). After every frame, the layer's colorspace ICC bytes must equal the document's ICC bytes.
- **Both:** the layer's `drawableSize` must equal round(points × scale). There must be no new uncaptured wgpu error. Path A also checks the layer tag equals the display ICC (read in Swift and, independently, in Rust after configure) and that `pixelFormat` is `bgra8Unorm`.

## 4. Results

Release host run (`swift run -c release PresenterSpikeHost suite bench`). The XCTest run (`swift test`, debug) gave the same pass or fail results and the same color numbers.

| Test | Path A | Path B |
| --- | --- | --- |
| 200 resizes, 1×1 to 2000×1500 pt, scale 1 and 2 (71 odd pixel sizes, sizes up to 4000×3000 px) | PASS. 200 frames, 200 configures, tag kept 200 of 200, 0 validation errors, drawable size right every time. 6,152 patches checked. Worst ΔE00 0.755, worst 3.0 levels. | PASS. 200 frames, a new ring each resize, 0 ring violations, 0 validation errors. 1,276,053 document pixels byte-exact. Tag = document profile every frame. |
| 100 document switches (sRGB → P3 → Adobe RGB) | PASS. 3,200 patches. Worst ΔE00 0.755, worst 3.0 levels. | PASS. 565,667 pixels exact, tag followed every switch. |
| 50 hide/show (`orderOut` / `orderFront`) | PASS. Worst ΔE00 0.445. Worst frame 8.8 ms. | PASS. Exact. Worst frame 10.1 ms. |
| Render while hidden (10 frames) | 10 of 10 rendered, none blocked. | 10 of 10 rendered, none blocked. |
| 30 screen moves + 30 backing changes (**simulated**) | PASS. 1,920 patches. Display cycled sRGB → P3 → sRGB. Tag followed every change (31 tag sets, 0 changed by configure). Worst ΔE00 0.765. | PASS. 339,980 pixels exact. Tag stayed the document profile. |
| 20 sleep/wake (**simulated**) | PASS. 20 of 20 frames refused while asleep (`OP_PS_SUSPENDED`), surface rebuilt on wake, color checked after. | PASS. 20 of 20 refused while asleep, ring rebuilt on wake, bytes exact after. |
| 100 create/destroy | PASS. Live presenters, surfaces, wgpu textures, views, buffers, bind groups and pipelines back to baseline. 0 of 100 `CAMetalLayer`s alive. | PASS. Same counters back to baseline. 0 of 100 layers and 0 of 300 IOSurfaces alive. |
| Ring protocol (test `testB_RingProtocol`) | n/a | PASS. Engine never wrote the shown slot. Newest frame wins. Wrong release refused and counted. Busy instead of overwrite with 2 slots shown. |
| Panic at the boundary, bad inputs | PASS (shared code). | PASS (shared code). |

The leak counters are wgpu's internal hal counters (`counters` feature) plus the spike's own counters. To prove they were not empty, the test samples them while one presenter is alive: path A showed 1 surface and 1 wgpu texture, path B 3 ring textures and 4 wgpu textures.

### Path A color error, split by source (worst over all patches, in 1/255 levels)

| Source | Real screen profile | Simulated sRGB and P3 displays |
| --- | --- | --- |
| lcms2 float transform vs ColorSync | 3.44 | 0.28 |
| 33³ table (trilinear) vs lcms2 | 2.63 | 1.85 |
| GPU result vs the same table on the CPU | 0.50 | 0.49 |
| **Total, GPU vs ColorSync** | **3.0 levels, ΔE00 0.755** | **1.8 levels, ΔE00 0.765** |

So path A does **not** meet a 1/255 bound. It meets ΔE00 ≤ 1.0 everywhere, with a worst case of 0.77. The two largest parts are the two color engines disagreeing on this machine's display profile, and the table's interpolation near black and near the gamut edge. The GPU adds at most half a level.

## 5. Frame times

A 6000×4000 sRGB document in a 1400×900 point view, 120 frames after 10 warm-up frames. "100%" means one document pixel per point. Both paths wait for the 60 Hz display, so the total is about 16.6 ms either way. The engine's own GPU time is the cost to compare.

| Path, zoom, scale | Total median / worst (ms) | Engine GPU median / worst (ms) | Other |
| --- | --- | --- | --- |
| A, 100%, scale 1 (1400×900 px) | 16.60 / 19.47 | 1.42 / 4.32 | drawable wait 15.16 / 15.54 |
| A, 25%, scale 1 | 16.59 / 17.73 | 3.93 / 5.42 | drawable wait 12.65 / 13.19 |
| A, 100%, scale 2 (2800×1800 px) | 16.63 / 16.99 | 2.61 / 2.71 | drawable wait 14.03 / 14.28 |
| A, 25%, scale 2 | 16.64 / 20.02 | 3.90 / 6.66 | drawable wait 12.74 / 13.67 |
| B, 100%, scale 1 | 16.65 / 16.93 | 1.33 / 1.40 | Swift blit GPU 0.13 / 0.16 |
| B, 25%, scale 1 | 16.65 / 27.22 | 3.83 / 4.14 | Swift blit GPU 0.11 / 0.15 |
| B, 100%, scale 2 | 16.64 / 17.52 | 1.33 / 1.98 | Swift blit GPU 0.48 / 1.06 |
| B, 25%, scale 2 | 16.59 / 17.98 | 3.13 / 4.77 | Swift blit GPU 0.93 / 1.09 |

An earlier run of the same bench had worst totals of 30.2 ms (A) and 27.8 ms (B). Both paths show similar single-frame spikes, and no path dropped below 60 fps at the median. Path A's table costs about 0.1 to 1.3 ms of GPU time per frame (A minus B, same case). Path B's extra blit costs 0.1 to 0.9 ms. The 25% numbers use a 4×4 box filter on the full-size document, because mips do not exist yet (M12).

## 6. Why A and not B

Path B failed no test. It loses on cost and risk:

1. **Two GPU queues and a hand-off.** wgpu's queue writes the IOSurface and a second Metal queue in Swift reads it. The ring state machine works, but every future feature (async frames, cancel, device loss) must keep both sides in step.
2. **Unsafe, version-coupled interop.** Wrapping an IOSurface needs `Device::as_hal`, `wgpu_hal::metal::Device::texture_from_raw` and `create_texture_from_hal`. Path A uses only the public, documented `SurfaceTargetUnsafe::CoreAnimationLayer`.
3. **Ring churn on resize.** Every size change allocates 2 or 3 new IOSurfaces (up to 3 × 48 MB at 4000×3000 px) and waits for the GPU. Path A's configure only resizes the drawables.
4. **Overlays would be in document color.** Everything the engine draws into the surface, including selection outlines, handles, the pixel grid and the checkerboard, is read in the document's profile. A red selection line would look different in an Adobe RGB document. Path A draws overlays in display space after the table.
5. **More Swift code on the hot path.** Swift owns the layer, the ring, the blit and the tag timing. Path A keeps the frame inside the engine, which matches the threading plan in section 4 (one render thread owns the GPU queue).

What path A gives up: its color matches ColorSync to ΔE00 0.77, not exactly. The engine must also follow the display profile when the window changes screens or the user changes the display profile. Both are acceptable, and section 9 lists the follow-ups.

## 7. Simulated, not tested

- **Screen moves:** this Mac has one display. The test posts `NSWindow.didChangeScreenNotification` and changes the target display profile (sRGB → Display P3 → sRGB), as the app's handler would. It also posts `NSWindow.didChangeBackingPropertiesNotification` and flips the scale between 1 and 2. No real second screen, no real scale change, no real display-profile change from System Settings.
- **Sleep and wake:** the Mac was not put to sleep. The test posts `NSWorkspace.willSleepNotification` and `didWakeNotification`. The handlers drop the drawable pipeline (path A: the wgpu surface; path B: the ring) and rebuild it. Real GPU state after a real sleep was not exercised.
- **Backing scale and large views:** the window stays at 400×300 points. The view inside it is resized up to 2000×1500 points (clipped by the window), and `contentsScale` is set to 1 or 2 by hand. The real `backingScaleFactor` stayed at 1.
- **On-screen color:** no screenshots were taken. Path A is checked on the drawable bytes, and Core Animation then shows them through an identity conversion (tag = display). Path B is checked on the IOSurface bytes and the tag, and Core Animation does the conversion. Neither path's final on-screen pixels were measured.
- **Minimize:** not tested (optional in the brief).
- **Device loss:** not tested.

## 8. Text for `docs/ARCHITECTURE.md`

Replace the **Presenter** bullet in section 4 with:

> - **Presenter (decided in M09, `docs/presenter-decision.md`):** wgpu renders straight into the view's `CAMetalLayer` (`SurfaceTargetUnsafe::CoreAnimationLayer`, `Bgra8Unorm`, FIFO, frame latency 2). The engine converts document → display color in the view shader with a 33³ table built by lcms2 (trilinear, hand-interpolated, no hardware filtering) and tags the layer with the display profile, so Core Animation does not convert a second time. The display profile comes from the window's screen; on `NSWindow.didChangeScreenNotification` or a display-profile change, Swift sends the new ICC and the engine rebuilds the table. After every surface configure the presenter re-reads `CAMetalLayer.colorspace` and sets it again if it changed (wgpu 27.0.4 leaves it alone; a test fails if a wgpu upgrade starts changing it). Overlays (selection, handles, pixel grid) are drawn after the table, in display space. On sleep the surface is dropped and rebuilt on wake. Measured in M09: 200 resizes, 100 document switches, hide/show, simulated screen moves and sleep/wake, all with zero validation errors and patch colors within ΔE00 0.77 of ColorSync. The rejected option (IOSurface ring with a Swift-owned layer tagged with the document profile) also passed but needs two GPU queues, a ring reallocation on every resize and wgpu-hal interop.

And replace the **Display** bullet in section 7 with:

> - Display: the document → display table described in section 4 (M09). wgpu 27.0.4's surface configure does not touch `CAMetalLayer.colorspace`; the presenter still re-checks it after every configure. The display profile comes from the window's screen and is updated when the window moves. Table accuracy is measured in ΔE00 against ColorSync and against direct lcms2 transforms (M09: worst ΔE00 0.77, worst 3 levels; 2.6 levels of that from the 33³ table), not assumed.

## 9. Follow-ups

- M13: shrink the table error, for example with per-channel shaper curves before the 33³ table, or a 65³ table. Measure against lcms2 in ΔE00 as before.
- M13: find out why lcms2 and ColorSync differ by up to 3.4 levels on the real display profile (0.28 on sRGB and P3). Check the profile's tag types, the black point handling and the rendering intent.
- M12: hook display-profile changes (`NSScreen` color space changes, `didChangeScreen`) to the engine, and run the hide/show and screen tests on a real second display and a Retina display.
- M12: run a real sleep/wake and a real screen move by hand once, and record the result.
- M12: mips for zoom below 100% (the spike's 25% numbers use a 4×4 box filter).
- Move the lcms2 table builder from `op-present-spike` into `op-color` when M13 starts; the spike crate can then be deleted or kept as a test.
- Build lcms2's C files with `MACOSX_DEPLOYMENT_TARGET` matching the app (the linker warns they are built for macOS 27 while the package targets 15).
- The Linux build of the workspace with the new crate was not checked here (no Linux standard library installed). The crate is empty off macOS, so it should build; check it in the Linux VM.
