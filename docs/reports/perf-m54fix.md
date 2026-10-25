# M54 performance table

Written by `cargo test --release -p op-engine --test m54_parity m54_write_perf_table -- --ignored --test-threads=1` (engine/op-engine/tests/m54_parity.rs). Do not edit by hand.

- Build: release (`--release`, the workspace's release profile).
- Machine: Apple M2, 8 cores, 8 GB RAM, macOS 27.0.1.
- Load average before: { 9.58 13.82 12.63 }; after: { 25.12 48.43 42.45 }. Other builders shared this Mac during the run, so treat times as upper bounds. Total run time 1828 s.
- Path: both columns are the engine (`Engine::dispatch` on the whole layer, no selection), timed from dispatch to finished document, including tile reads, upload, dispatch, readback and tile writes. CPU is an engine with `force_cpu`; GPU uses the default 2048 px chunks. The GPU column warms the kernel once before timing.
- Input: the synthetic gradient-and-noise image with clear, opaque and partial alpha bands, at 1000 × 1000, 4000 × 3000 and 6000 × 4000.
- Each time is the median of 3 runs. A run over the 160 s cap is reported alone. A CPU cell at 12 or 24 MP is not run when the 1 MP time, scaled by pixel count, passes the cap; the cell says so and gives the 1 MP time (no estimate is printed as a result). The last column is the time since the run started, to show when each row ran.

**Partial run:** OP_M54_ONLY limited this run to effect.artistic.pencil_sketch, effect.blur.gaussian, effect.blur.median, effect.distort.bulge, effect.photo.glow, effect.photo.red_eye_removal, effect.photo.soften_portrait.

| Effect | Parameter set | Size | CPU median of 3 | Engine GPU median of 3 | Elapsed |
| --- | --- | --- | ---: | ---: | ---: |
| effect.artistic.pencil_sketch | default (defaults) | 1 MP (1000 × 1000) | 1050 ms | 15 ms | 4 s |
| effect.blur.gaussian | default (defaults) | 1 MP (1000 × 1000) | 94 ms | 8 ms | 4 s |
| effect.blur.gaussian | large radius (radius=50) | 1 MP (1000 × 1000) | 687 ms | 27 ms | 6 s |
| effect.distort.bulge | default (defaults) | 1 MP (1000 × 1000) | 141 ms | 12 ms | 7 s |
| effect.blur.median | default (defaults) | 1 MP (1000 × 1000) | 9100 ms | 89 ms | 35 s |
| effect.photo.glow | default (defaults) | 1 MP (1000 × 1000) | 902 ms | 44 ms | 38 s |
| effect.photo.glow | large radius (radius=20) | 1 MP (1000 × 1000) | 1113 ms | 52 ms | 42 s |
| effect.photo.red_eye_removal | default (defaults) | 1 MP (1000 × 1000) | 36 ms | 20 ms | 42 s |
| effect.photo.soften_portrait | default (defaults) | 1 MP (1000 × 1000) | 1042 ms | 41 ms | 47 s |
| effect.artistic.pencil_sketch | default (defaults) | 12 MP (4000 × 3000) | 19239 ms | 212 ms | 102 s |
| effect.blur.gaussian | default (defaults) | 12 MP (4000 × 3000) | 1278 ms | 165 ms | 106 s |
| effect.blur.gaussian | large radius (radius=50) | 12 MP (4000 × 3000) | 13135 ms | 706 ms | 150 s |
| effect.distort.bulge | default (defaults) | 12 MP (4000 × 3000) | 7002 ms | 224 ms | 170 s |
| effect.blur.median | default (defaults) | 12 MP (4000 × 3000) | 132249 ms | 1209 ms | 573 s |
| effect.photo.glow | default (defaults) | 12 MP (4000 × 3000) | 18812 ms | 446 ms | 630 s |
| effect.photo.glow | large radius (radius=20) | 12 MP (4000 × 3000) | 27262 ms | 564 ms | 725 s |
| effect.photo.red_eye_removal | default (defaults) | 12 MP (4000 × 3000) | 599 ms | 124 ms | 728 s |
| effect.photo.soften_portrait | default (defaults) | 12 MP (4000 × 3000) | 12474 ms | 515 ms | 771 s |
| effect.artistic.pencil_sketch | default (defaults) | 24 MP (6000 × 4000) | 38699 ms | 500 ms | 894 s |
| effect.blur.gaussian | default (defaults) | 24 MP (6000 × 4000) | 3345 ms | 391 ms | 906 s |
| effect.blur.gaussian | large radius (radius=50) | 24 MP (6000 × 4000) | 26615 ms | 1023 ms | 993 s |
| effect.distort.bulge | default (defaults) | 24 MP (6000 × 4000) | 31797 ms | 843 ms | 1097 s |
| effect.blur.median | default (defaults) | 24 MP (6000 × 4000) | not run: 1 MP took 9100 ms, so this size would pass the 160 s cap | 3051 ms | 1109 s |
| effect.photo.glow | default (defaults) | 24 MP (6000 × 4000) | 69908 ms | 3263 ms | 1347 s |
| effect.photo.glow | large radius (radius=20) | 24 MP (6000 × 4000) | 71926 ms | 1256 ms | 1587 s |
| effect.photo.red_eye_removal | default (defaults) | 24 MP (6000 × 4000) | 1745 ms | 850 ms | 1596 s |
| effect.photo.soften_portrait | default (defaults) | 24 MP (6000 × 4000) | 71474 ms | 1978 ms | 1828 s |
