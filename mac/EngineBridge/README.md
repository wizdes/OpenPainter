# EngineBridge

A thin Swift wrapper over the OpenPainter engine's C interface
(`engine/op-ffi/include/openpainter.h`). It needs macOS 15.

- `Engine` owns one engine. Its `events` stream delivers every engine event in order.
  The C callback copies each payload into `Data` while still on the engine thread.
- `Document` is one open document. `dispatch` queues a command and returns a ticket.
  `run` dispatches and waits for that ticket's `JobFinished` event.
  `info()` and `pixels()` read the latest published snapshot.
- `EngineView` (in `View.swift`) is one canvas view: the engine draws a document into a
  `CAMetalLayer` on its render thread. `set` sends the view state and the display ICC,
  `info` reads it back, `detach` waits for the in-flight frame and releases the layer.
  `Engine.viewCounters()` returns the live view, layer, surface and wgpu counters.
- `FloodPreview.swift` (M31): `floodBegin`, `floodUpdate`, `floodFinish` and `floodCancel` run a live Magic Wand or
  Paint Bucket session on the engine's preview stack; `selectionPath(preview:)` and `selectionMask(preview:)` read its
  live selection.
- `Engine.close()` frees the engine. Queued work and running jobs finish first.
  No event arrives after it returns.

## Build order

The package links the Rust static library `libop_ffi.a`, so build that first.

1. From the repo root, build the engine library with the app's build script:

   ```sh
   mac/scripts/build-engine.sh
   ```

   It runs `cargo rustc -p op-ffi --profile app-release --crate-type staticlib` (release with fat LTO), writes `engine/target/app-release/libop_ffi.a` and regenerates the header.
   To use another folder, set `OP_ENGINE_LIB_DIR` to the folder that holds `libop_ffi.a`.

2. Build and test the Swift package:

   ```sh
   swift test --package-path mac/EngineBridge \
     --scratch-path "$PWD/.swift-cache/scratch" --cache-path "$PWD/.swift-cache/cache"
   ```

The app (`mac/project.yml`) runs step 1 itself through `mac/scripts/build-engine.sh`.
Rebuild step 1 after any Rust change. SwiftPM does not know that the library depends on the Rust sources.

## Tests

`Tests/EngineBridgeTests` opens `tests/fixtures/ramp_256x16.png` and checks these cases:

- Add a layer, invert, undo, then read the pixels back and compare them byte by byte.
- 100 quick preview updates and then a commit give exactly one history step.
- Preview cancel and ticket cancel leave the document unchanged.
- A panic command makes the document read-only.
- Freeing the engine with pending jobs returns, and no callback runs after it.
- A live Magic Wand session: two tolerance updates show in the live mask, Finish gives one "Magic Wand" step,
  and Cancel keeps the document (`FloodPreviewTests`).
