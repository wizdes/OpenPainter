# M54 performance table

Written by `cargo test --release -p op-engine --test m54_parity m54_write_perf_table -- --ignored --test-threads=1` (engine/op-engine/tests/m54_parity.rs). Do not edit by hand.

- Build: release (`--release`, the workspace's release profile).
- Machine: Apple M2, 8 cores, 8 GB RAM, macOS 27.0.1.
- Load average before: { 6.73 6.14 6.92 }; after: { 2.78 6.03 10.29 }. Other builders shared this Mac during the run, so treat times as upper bounds. Total run time 2572 s.
- Path: both columns are the engine (`Engine::dispatch` on the whole layer, no selection), timed from dispatch to finished document, including tile reads, upload, dispatch, readback and tile writes. CPU is an engine with `force_cpu`; GPU uses the default 2048 px chunks. The GPU column warms the kernel once before timing.
- Input: the synthetic gradient-and-noise image with clear, opaque and partial alpha bands, at 1000 × 1000, 4000 × 3000 and 6000 × 4000.
- Each time is the median of 3 runs. A run over the 20 s cap is reported alone. A CPU cell at 12 or 24 MP is not run when the 1 MP time, scaled by pixel count, passes the cap; the cell says so and gives the 1 MP time (no estimate is printed as a result). The last column is the time since the run started, to show when each row ran.

| Effect | Parameter set | Size | CPU median of 3 | Engine GPU median of 3 | Elapsed |
| --- | --- | --- | ---: | ---: | ---: |
| effect.adjust.auto_level | default (defaults) | 1 MP (1000 × 1000) | 25 ms | CPU only | 0 s |
| effect.adjust.black_and_white | default (defaults) | 1 MP (1000 × 1000) | 22 ms | 13 ms | 0 s |
| effect.adjust.brightness_contrast | default (defaults) | 1 MP (1000 × 1000) | 33 ms | 13 ms | 1 s |
| effect.adjust.curves | default (defaults) | 1 MP (1000 × 1000) | 34 ms | 11 ms | 1 s |
| effect.adjust.exposure | default (defaults) | 1 MP (1000 × 1000) | 21 ms | 10 ms | 1 s |
| effect.adjust.highlights_shadows | default (defaults) | 1 MP (1000 × 1000) | 35 ms | 9 ms | 1 s |
| effect.adjust.hue_saturation | default (defaults) | 1 MP (1000 × 1000) | 53 ms | CPU only | 1 s |
| effect.adjust.invert_alpha | default (defaults) | 1 MP (1000 × 1000) | 25 ms | 9 ms | 2 s |
| effect.adjust.invert_colors | default (defaults) | 1 MP (1000 × 1000) | 24 ms | 10 ms | 2 s |
| effect.adjust.levels | default (defaults) | 1 MP (1000 × 1000) | 28 ms | 9 ms | 2 s |
| effect.adjust.posterize | default (defaults) | 1 MP (1000 × 1000) | 52 ms | 10 ms | 2 s |
| effect.adjust.sepia | default (defaults) | 1 MP (1000 × 1000) | 53 ms | 10 ms | 2 s |
| effect.adjust.temperature_tint | default (defaults) | 1 MP (1000 × 1000) | 39 ms | 10 ms | 3 s |
| effect.artistic.ink_sketch | default (defaults) | 1 MP (1000 × 1000) | 3811 ms | 20 ms | 14 s |
| effect.artistic.oil_painting | default (defaults) | 1 MP (1000 × 1000) | 965 ms | 80 ms | 18 s |
| effect.artistic.pencil_sketch | default (defaults) | 1 MP (1000 × 1000) | 1039 ms | 10 ms | 21 s |
| effect.blur.fragment | default (defaults) | 1 MP (1000 × 1000) | 163 ms | 9 ms | 22 s |
| effect.blur.gaussian | default (defaults) | 1 MP (1000 × 1000) | 93 ms | 6 ms | 22 s |
| effect.blur.gaussian | large radius (radius=50) | 1 MP (1000 × 1000) | 658 ms | 16 ms | 24 s |
| effect.blur.motion | default (defaults) | 1 MP (1000 × 1000) | 1152 ms | 10 ms | 28 s |
| effect.blur.radial | default (defaults) | 1 MP (1000 × 1000) | 4620 ms | 20 ms | 42 s |
| effect.blur.zoom | default (defaults) | 1 MP (1000 × 1000) | 1042 ms | 12 ms | 45 s |
| effect.blur.bokeh | default (defaults) | 1 MP (1000 × 1000) | 2233 ms | 17 ms | 52 s |
| effect.blur.sketch | default (defaults) | 1 MP (1000 × 1000) | 524 ms | 27 ms | 54 s |
| effect.blur.square | default (defaults) | 1 MP (1000 × 1000) | 3575 ms | 18 ms | 64 s |
| effect.blur.surface | default (defaults) | 1 MP (1000 × 1000) | 2980 ms | 24 ms | 74 s |
| effect.blur.surface | large radius (radius=30) | 1 MP (1000 × 1000) | 56825 ms (1 run; over 20 s cap) | 369 ms | 132 s |
| effect.blur.unfocus | default (defaults) | 1 MP (1000 × 1000) | 1274 ms | 13 ms | 136 s |
| effect.blur.unfocus | large radius (radius=30) | 1 MP (1000 × 1000) | 50033 ms (1 run; over 20 s cap) | 117 ms | 187 s |
| effect.color.quantize | default (defaults) | 1 MP (1000 × 1000) | 171 ms | CPU only | 187 s |
| effect.distort.bulge | default (defaults) | 1 MP (1000 × 1000) | 164 ms | 7 ms | 188 s |
| effect.distort.crystalize | default (defaults) | 1 MP (1000 × 1000) | 279 ms | CPU only | 189 s |
| effect.distort.dents | default (defaults) | 1 MP (1000 × 1000) | 701 ms | CPU only | 191 s |
| effect.distort.frosted_glass | default (defaults) | 1 MP (1000 × 1000) | 254 ms | CPU only | 192 s |
| effect.distort.morphology | default (defaults) | 1 MP (1000 × 1000) | 235 ms | 12 ms | 192 s |
| effect.distort.pixelate | default (defaults) | 1 MP (1000 × 1000) | 93 ms | 7 ms | 193 s |
| effect.distort.polar_inversion | default (defaults) | 1 MP (1000 × 1000) | 450 ms | CPU only | 194 s |
| effect.distort.tile_reflection | default (defaults) | 1 MP (1000 × 1000) | 1013 ms | CPU only | 197 s |
| effect.distort.twist | default (defaults) | 1 MP (1000 × 1000) | 582 ms | 9 ms | 199 s |
| effect.noise.add | default (defaults) | 1 MP (1000 × 1000) | 29 ms | 22 ms | 199 s |
| effect.blur.median | default (defaults) | 1 MP (1000 × 1000) | 6209 ms | 2794 ms | 229 s |
| effect.noise.reduce | default (defaults) | 1 MP (1000 × 1000) | 5663 ms | 27 ms | 246 s |
| effect.noise.reduce | large radius (radius=30) | 1 MP (1000 × 1000) | 46494 ms (1 run; over 20 s cap) | 95 ms | 293 s |
| effect.object.drop_shadow | default (defaults) | 1 MP (1000 × 1000) | 1319 ms | CPU only | 297 s |
| effect.photo.glow | default (defaults) | 1 MP (1000 × 1000) | 701 ms | 26 ms | 300 s |
| effect.photo.glow | large radius (radius=20) | 1 MP (1000 × 1000) | 941 ms | 29 ms | 303 s |
| effect.photo.red_eye_removal | default (defaults) | 1 MP (1000 × 1000) | 29 ms | 7 ms | 303 s |
| effect.photo.sharpen | default (defaults) | 1 MP (1000 × 1000) | 675 ms | 356 ms | 306 s |
| effect.photo.soften_portrait | default (defaults) | 1 MP (1000 × 1000) | 794 ms | 29 ms | 309 s |
| effect.photo.vignette | default (defaults) | 1 MP (1000 × 1000) | 64 ms | 7 ms | 309 s |
| effect.render.clouds | default (defaults) | 1 MP (1000 × 1000) | 202 ms | 201 ms | 311 s |
| effect.render.julia_fractal | default (defaults) | 1 MP (1000 × 1000) | 664 ms | CPU only | 313 s |
| effect.render.mandelbrot_fractal | default (defaults) | 1 MP (1000 × 1000) | 8233 ms | CPU only | 338 s |
| effect.render.turbulence | default (defaults) | 1 MP (1000 × 1000) | 129 ms | 12 ms | 338 s |
| effect.stylize.edge_detect | default (defaults) | 1 MP (1000 × 1000) | 242 ms | 8 ms | 339 s |
| effect.stylize.emboss | default (defaults) | 1 MP (1000 × 1000) | 163 ms | 8 ms | 340 s |
| effect.stylize.outline | default (defaults) | 1 MP (1000 × 1000) | 1347 ms | 225 ms | 345 s |
| effect.stylize.relief | default (defaults) | 1 MP (1000 × 1000) | 169 ms | 8 ms | 346 s |
| effect.fill.content_aware | default | 1 MP (1000 × 1000) | not run: needs a selected hole | CPU only | n/a |
| effect.layers.rotate_zoom | default (defaults) | 1 MP (1000 × 1000) | 263 ms | CPU only | 346 s |
| effect.adjust.auto_level | default (defaults) | 12 MP (4000 × 3000) | 324 ms | CPU only | 347 s |
| effect.adjust.black_and_white | default (defaults) | 12 MP (4000 × 3000) | 346 ms | 161 ms | 349 s |
| effect.adjust.brightness_contrast | default (defaults) | 12 MP (4000 × 3000) | 429 ms | 102 ms | 351 s |
| effect.adjust.curves | default (defaults) | 12 MP (4000 × 3000) | 373 ms | 93 ms | 352 s |
| effect.adjust.exposure | default (defaults) | 12 MP (4000 × 3000) | 319 ms | 96 ms | 354 s |
| effect.adjust.highlights_shadows | default (defaults) | 12 MP (4000 × 3000) | 422 ms | 101 ms | 356 s |
| effect.adjust.hue_saturation | default (defaults) | 12 MP (4000 × 3000) | 558 ms | CPU only | 357 s |
| effect.adjust.invert_alpha | default (defaults) | 12 MP (4000 × 3000) | 261 ms | 99 ms | 358 s |
| effect.adjust.invert_colors | default (defaults) | 12 MP (4000 × 3000) | 305 ms | 102 ms | 360 s |
| effect.adjust.levels | default (defaults) | 12 MP (4000 × 3000) | 281 ms | 109 ms | 361 s |
| effect.adjust.posterize | default (defaults) | 12 MP (4000 × 3000) | 627 ms | 199 ms | 364 s |
| effect.adjust.sepia | default (defaults) | 12 MP (4000 × 3000) | 653 ms | 177 ms | 367 s |
| effect.adjust.temperature_tint | default (defaults) | 12 MP (4000 × 3000) | 471 ms | 109 ms | 368 s |
| effect.artistic.ink_sketch | default (defaults) | 12 MP (4000 × 3000) | not run: 1 MP took 3811 ms, so this size would pass the 20 s cap | 244 ms | 369 s |
| effect.artistic.oil_painting | default (defaults) | 12 MP (4000 × 3000) | 13155 ms | 998 ms | 414 s |
| effect.artistic.pencil_sketch | default (defaults) | 12 MP (4000 × 3000) | 21529 ms | 204 ms | 477 s |
| effect.blur.fragment | default (defaults) | 12 MP (4000 × 3000) | 4197 ms | 142 ms | 490 s |
| effect.blur.gaussian | default (defaults) | 12 MP (4000 × 3000) | 2397 ms | 165 ms | 498 s |
| effect.blur.gaussian | large radius (radius=50) | 12 MP (4000 × 3000) | 16881 ms | 310 ms | 547 s |
| effect.blur.motion | default (defaults) | 12 MP (4000 × 3000) | 20670 ms | 210 ms | 609 s |
| effect.blur.radial | default (defaults) | 12 MP (4000 × 3000) | not run: 1 MP took 4620 ms, so this size would pass the 20 s cap | 252 ms | 611 s |
| effect.blur.zoom | default (defaults) | 12 MP (4000 × 3000) | 21906 ms (1 run; over 20 s cap) | 159 ms | 633 s |
| effect.blur.bokeh | default (defaults) | 12 MP (4000 × 3000) | not run: 1 MP took 2233 ms, so this size would pass the 20 s cap | 288 ms | 635 s |
| effect.blur.sketch | default (defaults) | 12 MP (4000 × 3000) | 16841 ms | 493 ms | 687 s |
| effect.blur.square | default (defaults) | 12 MP (4000 × 3000) | not run: 1 MP took 3575 ms, so this size would pass the 20 s cap | 267 ms | 688 s |
| effect.blur.surface | default (defaults) | 12 MP (4000 × 3000) | not run: 1 MP took 2980 ms, so this size would pass the 20 s cap | 322 ms | 690 s |
| effect.blur.surface | large radius (radius=30) | 12 MP (4000 × 3000) | not run: 1 MP took 56825 ms, so this size would pass the 20 s cap | 4387 ms | 707 s |
| effect.blur.unfocus | default (defaults) | 12 MP (4000 × 3000) | 18115 ms | 243 ms | 764 s |
| effect.blur.unfocus | large radius (radius=30) | 12 MP (4000 × 3000) | not run: 1 MP took 50033 ms, so this size would pass the 20 s cap | 1589 ms | 770 s |
| effect.color.quantize | default (defaults) | 12 MP (4000 × 3000) | 3046 ms | CPU only | 780 s |
| effect.distort.bulge | default (defaults) | 12 MP (4000 × 3000) | 1957 ms | 525 ms | 787 s |
| effect.distort.crystalize | default (defaults) | 12 MP (4000 × 3000) | 5124 ms | CPU only | 803 s |
| effect.distort.dents | default (defaults) | 12 MP (4000 × 3000) | 11776 ms | CPU only | 837 s |
| effect.distort.frosted_glass | default (defaults) | 12 MP (4000 × 3000) | 4009 ms | CPU only | 851 s |
| effect.distort.morphology | default (defaults) | 12 MP (4000 × 3000) | 2967 ms | 191 ms | 861 s |
| effect.distort.pixelate | default (defaults) | 12 MP (4000 × 3000) | 1768 ms | 133 ms | 867 s |
| effect.distort.polar_inversion | default (defaults) | 12 MP (4000 × 3000) | 7653 ms | CPU only | 889 s |
| effect.distort.tile_reflection | default (defaults) | 12 MP (4000 × 3000) | 31356 ms | CPU only | 969 s |
| effect.distort.twist | default (defaults) | 12 MP (4000 × 3000) | 10332 ms | 150 ms | 1004 s |
| effect.noise.add | default (defaults) | 12 MP (4000 × 3000) | 551 ms | 339 ms | 1007 s |
| effect.blur.median | default (defaults) | 12 MP (4000 × 3000) | not run: 1 MP took 6209 ms, so this size would pass the 20 s cap | 37220 ms (1 run; over 20 s cap) | 1081 s |
| effect.noise.reduce | default (defaults) | 12 MP (4000 × 3000) | not run: 1 MP took 5663 ms, so this size would pass the 20 s cap | 276 ms | 1083 s |
| effect.noise.reduce | large radius (radius=30) | 12 MP (4000 × 3000) | not run: 1 MP took 46494 ms, so this size would pass the 20 s cap | 1233 ms | 1088 s |
| effect.object.drop_shadow | default (defaults) | 12 MP (4000 × 3000) | 22890 ms (1 run; over 20 s cap) | CPU only | 1111 s |
| effect.photo.glow | default (defaults) | 12 MP (4000 × 3000) | 10196 ms | 380 ms | 1146 s |
| effect.photo.glow | large radius (radius=20) | 12 MP (4000 × 3000) | 12319 ms | 379 ms | 1185 s |
| effect.photo.red_eye_removal | default (defaults) | 12 MP (4000 × 3000) | 440 ms | 126 ms | 1187 s |
| effect.photo.sharpen | default (defaults) | 12 MP (4000 × 3000) | 9476 ms | 4688 ms | 1234 s |
| effect.photo.soften_portrait | default (defaults) | 12 MP (4000 × 3000) | 10200 ms | 382 ms | 1266 s |
| effect.photo.vignette | default (defaults) | 12 MP (4000 × 3000) | 824 ms | 133 ms | 1270 s |
| effect.render.clouds | default (defaults) | 12 MP (4000 × 3000) | 2525 ms | 2719 ms | 1289 s |
| effect.render.julia_fractal | default (defaults) | 12 MP (4000 × 3000) | 8759 ms | CPU only | 1315 s |
| effect.render.mandelbrot_fractal | default (defaults) | 12 MP (4000 × 3000) | not run: 1 MP took 8233 ms, so this size would pass the 20 s cap | CPU only | 1315 s |
| effect.render.turbulence | default (defaults) | 12 MP (4000 × 3000) | 1625 ms | 179 ms | 1320 s |
| effect.stylize.edge_detect | default (defaults) | 12 MP (4000 × 3000) | 3907 ms | 248 ms | 1333 s |
| effect.stylize.emboss | default (defaults) | 12 MP (4000 × 3000) | 2618 ms | 212 ms | 1342 s |
| effect.stylize.outline | default (defaults) | 12 MP (4000 × 3000) | 20941 ms (1 run; over 20 s cap) | 2885 ms | 1375 s |
| effect.stylize.relief | default (defaults) | 12 MP (4000 × 3000) | 2898 ms | 248 ms | 1385 s |
| effect.fill.content_aware | default | 12 MP (4000 × 3000) | not run: needs a selected hole | CPU only | n/a |
| effect.layers.rotate_zoom | default (defaults) | 12 MP (4000 × 3000) | 5057 ms | CPU only | 1401 s |
| effect.adjust.auto_level | default (defaults) | 24 MP (6000 × 4000) | 859 ms | CPU only | 1404 s |
| effect.adjust.black_and_white | default (defaults) | 24 MP (6000 × 4000) | 1013 ms | 484 ms | 1410 s |
| effect.adjust.brightness_contrast | default (defaults) | 24 MP (6000 × 4000) | 1359 ms | 299 ms | 1415 s |
| effect.adjust.curves | default (defaults) | 24 MP (6000 × 4000) | 1128 ms | 268 ms | 1420 s |
| effect.adjust.exposure | default (defaults) | 24 MP (6000 × 4000) | 800 ms | 418 ms | 1424 s |
| effect.adjust.highlights_shadows | default (defaults) | 24 MP (6000 × 4000) | 1217 ms | 273 ms | 1430 s |
| effect.adjust.hue_saturation | default (defaults) | 24 MP (6000 × 4000) | 1577 ms | CPU only | 1434 s |
| effect.adjust.invert_alpha | default (defaults) | 24 MP (6000 × 4000) | 789 ms | 321 ms | 1439 s |
| effect.adjust.invert_colors | default (defaults) | 24 MP (6000 × 4000) | 623 ms | 303 ms | 1442 s |
| effect.adjust.levels | default (defaults) | 24 MP (6000 × 4000) | 878 ms | 336 ms | 1447 s |
| effect.adjust.posterize | default (defaults) | 24 MP (6000 × 4000) | 1731 ms | 562 ms | 1455 s |
| effect.adjust.sepia | default (defaults) | 24 MP (6000 × 4000) | 2013 ms | 455 ms | 1463 s |
| effect.adjust.temperature_tint | default (defaults) | 24 MP (6000 × 4000) | 1026 ms | 265 ms | 1467 s |
| effect.artistic.ink_sketch | default (defaults) | 24 MP (6000 × 4000) | not run: 1 MP took 3811 ms, so this size would pass the 20 s cap | 529 ms | 1470 s |
| effect.artistic.oil_painting | default (defaults) | 24 MP (6000 × 4000) | not run: 1 MP took 965 ms, so this size would pass the 20 s cap | 2369 ms | 1479 s |
| effect.artistic.pencil_sketch | default (defaults) | 24 MP (6000 × 4000) | not run: 1 MP took 1039 ms, so this size would pass the 20 s cap | 451 ms | 1481 s |
| effect.blur.fragment | default (defaults) | 24 MP (6000 × 4000) | 6828 ms | 321 ms | 1504 s |
| effect.blur.gaussian | default (defaults) | 24 MP (6000 × 4000) | 3743 ms | 368 ms | 1517 s |
| effect.blur.gaussian | large radius (radius=50) | 24 MP (6000 × 4000) | 29261 ms (1 run; over 20 s cap) | 640 ms | 1549 s |
| effect.blur.motion | default (defaults) | 24 MP (6000 × 4000) | not run: 1 MP took 1152 ms, so this size would pass the 20 s cap | 412 ms | 1551 s |
| effect.blur.radial | default (defaults) | 24 MP (6000 × 4000) | not run: 1 MP took 4620 ms, so this size would pass the 20 s cap | 702 ms | 1554 s |
| effect.blur.zoom | default (defaults) | 24 MP (6000 × 4000) | not run: 1 MP took 1042 ms, so this size would pass the 20 s cap | 306 ms | 1556 s |
| effect.blur.bokeh | default (defaults) | 24 MP (6000 × 4000) | not run: 1 MP took 2233 ms, so this size would pass the 20 s cap | 575 ms | 1558 s |
| effect.blur.sketch | default (defaults) | 24 MP (6000 × 4000) | 18627 ms | 1129 ms | 1616 s |
| effect.blur.square | default (defaults) | 24 MP (6000 × 4000) | not run: 1 MP took 3575 ms, so this size would pass the 20 s cap | 695 ms | 1620 s |
| effect.blur.surface | default (defaults) | 24 MP (6000 × 4000) | not run: 1 MP took 2980 ms, so this size would pass the 20 s cap | 1199 ms | 1624 s |
| effect.blur.surface | large radius (radius=30) | 24 MP (6000 × 4000) | not run: 1 MP took 56825 ms, so this size would pass the 20 s cap | 9121 ms | 1667 s |
| effect.blur.unfocus | default (defaults) | 24 MP (6000 × 4000) | not run: 1 MP took 1274 ms, so this size would pass the 20 s cap | 503 ms | 1670 s |
| effect.blur.unfocus | large radius (radius=30) | 24 MP (6000 × 4000) | not run: 1 MP took 50033 ms, so this size would pass the 20 s cap | 3189 ms | 1683 s |
| effect.color.quantize | default (defaults) | 24 MP (6000 × 4000) | 20061 ms (1 run; over 20 s cap) | CPU only | 1703 s |
| effect.distort.bulge | default (defaults) | 24 MP (6000 × 4000) | 14366 ms | 290 ms | 1749 s |
| effect.distort.crystalize | default (defaults) | 24 MP (6000 × 4000) | 19251 ms | CPU only | 1806 s |
| effect.distort.dents | default (defaults) | 24 MP (6000 × 4000) | 29427 ms (1 run; over 20 s cap) | CPU only | 1836 s |
| effect.distort.frosted_glass | default (defaults) | 24 MP (6000 × 4000) | 20681 ms (1 run; over 20 s cap) | CPU only | 1856 s |
| effect.distort.morphology | default (defaults) | 24 MP (6000 × 4000) | 16802 ms | 489 ms | 1909 s |
| effect.distort.pixelate | default (defaults) | 24 MP (6000 × 4000) | 13150 ms | 269 ms | 1950 s |
| effect.distort.polar_inversion | default (defaults) | 24 MP (6000 × 4000) | 22722 ms (1 run; over 20 s cap) | CPU only | 1973 s |
| effect.distort.tile_reflection | default (defaults) | 24 MP (6000 × 4000) | not run: 1 MP took 1013 ms, so this size would pass the 20 s cap | CPU only | 1973 s |
| effect.distort.twist | default (defaults) | 24 MP (6000 × 4000) | 27809 ms (1 run; over 20 s cap) | 434 ms | 2003 s |
| effect.noise.add | default (defaults) | 24 MP (6000 × 4000) | 852 ms | 770 ms | 2009 s |
| effect.blur.median | default (defaults) | 24 MP (6000 × 4000) | not run: 1 MP took 6209 ms, so this size would pass the 20 s cap | 81321 ms (1 run; over 20 s cap) | 2166 s |
| effect.noise.reduce | default (defaults) | 24 MP (6000 × 4000) | not run: 1 MP took 5663 ms, so this size would pass the 20 s cap | 566 ms | 2169 s |
| effect.noise.reduce | large radius (radius=30) | 24 MP (6000 × 4000) | not run: 1 MP took 46494 ms, so this size would pass the 20 s cap | 2473 ms | 2179 s |
| effect.object.drop_shadow | default (defaults) | 24 MP (6000 × 4000) | not run: 1 MP took 1319 ms, so this size would pass the 20 s cap | CPU only | 2179 s |
| effect.photo.glow | default (defaults) | 24 MP (6000 × 4000) | 24092 ms (1 run; over 20 s cap) | 725 ms | 2206 s |
| effect.photo.glow | large radius (radius=20) | 24 MP (6000 × 4000) | not run: 1 MP took 941 ms, so this size would pass the 20 s cap | 718 ms | 2209 s |
| effect.photo.red_eye_removal | default (defaults) | 24 MP (6000 × 4000) | 968 ms | 293 ms | 2214 s |
| effect.photo.sharpen | default (defaults) | 24 MP (6000 × 4000) | 18853 ms | 9188 ms | 2307 s |
| effect.photo.soften_portrait | default (defaults) | 24 MP (6000 × 4000) | 19567 ms | 631 ms | 2368 s |
| effect.photo.vignette | default (defaults) | 24 MP (6000 × 4000) | 1718 ms | 240 ms | 2375 s |
| effect.render.clouds | default (defaults) | 24 MP (6000 × 4000) | 4800 ms | 4984 ms | 2410 s |
| effect.render.julia_fractal | default (defaults) | 24 MP (6000 × 4000) | 14686 ms | CPU only | 2454 s |
| effect.render.mandelbrot_fractal | default (defaults) | 24 MP (6000 × 4000) | not run: 1 MP took 8233 ms, so this size would pass the 20 s cap | CPU only | 2454 s |
| effect.render.turbulence | default (defaults) | 24 MP (6000 × 4000) | 3018 ms | 291 ms | 2464 s |
| effect.stylize.edge_detect | default (defaults) | 24 MP (6000 × 4000) | 5698 ms | 454 ms | 2483 s |
| effect.stylize.emboss | default (defaults) | 24 MP (6000 × 4000) | 3943 ms | 383 ms | 2497 s |
| effect.stylize.outline | default (defaults) | 24 MP (6000 × 4000) | not run: 1 MP took 1347 ms, so this size would pass the 20 s cap | 5584 ms | 2519 s |
| effect.stylize.relief | default (defaults) | 24 MP (6000 × 4000) | 3941 ms | 430 ms | 2533 s |
| effect.fill.content_aware | default | 24 MP (6000 × 4000) | not run: needs a selected hole | CPU only | n/a |
| effect.layers.rotate_zoom | default (defaults) | 24 MP (6000 × 4000) | 12961 ms | CPU only | 2572 s |
