# EngineBridge

A thin Swift wrapper over the OpenPainter engine's C interface
(`engine/op-ffi/include/openpainter.h`). It needs macOS 15.

- `Engine` owns one engine. Its `events` stream delivers every engine event in order.
  The C callback copies each payload into `Data` while still on the engine thread.
- `Document` is one open document. `dispatch` queues a command and returns a ticket.
  `run` dispatches and waits for that ticket's `JobFinished` event.
  `info()` and `pixels()` read the latest published snapshot.
- `Engine.close()` frees the engine. Queued work and running jobs finish first.
  No event arrives after it returns.

## Build order

The package links the Rust static library `libop_ffi.a`, so build that first.

1. From the repo root, build the engine library in release mode:

   ```sh
   cd engine && cargo build -p op-ffi --release && cd ..
   ```

   This writes `engine/target/release/libop_ffi.a` and regenerates the header.
   To use another folder, set `OP_ENGINE_LIB_DIR` to the folder that holds `libop_ffi.a`.

2. Build and test the Swift package:

   ```sh
   swift test --package-path mac/EngineBridge \
     --scratch-path "$PWD/.swift-cache/scratch" --cache-path "$PWD/.swift-cache/cache"
   ```

Rebuild step 1 after any Rust change. SwiftPM does not know that the library depends on the Rust sources.

## Tests

`Tests/EngineBridgeTests` opens `tests/fixtures/ramp_256x16.png` and checks these cases:

- Add a layer, invert, undo, then read the pixels back and compare them byte by byte.
- 100 quick preview updates and then a commit give exactly one history step.
- Preview cancel and ticket cancel leave the document unchanged.
- A panic command makes the document read-only.
- Freeing the engine with pending jobs returns, and no callback runs after it.
