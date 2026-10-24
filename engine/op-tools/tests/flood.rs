use op_core::doc::{DocState, SelectionSource, SelectionState, ToolCompositeMode};
use op_core::selection::{CombineMode, MaskGrid};
use op_core::tiles::{TILE_SIZE, TileCoord, TileData, TileStore};
use op_tools::flood::{
    AlphaMode, FloodOptions, Sampling, detect_region, magic_wand, paint_bucket, tolerance_distance,
};
use std::path::Path;
use std::sync::Arc;

fn doc(w: u32, h: u32, pixels: &[[u8; 4]]) -> DocState {
    let mut d = DocState::new(
        Arc::new(TileStore::new(std::env::temp_dir(), 64 * 1024 * 1024).unwrap()),
        w,
        h,
        96.0,
    )
    .unwrap();
    for ty in 0..h.div_ceil(TILE_SIZE as u32) {
        for tx in 0..w.div_ceil(TILE_SIZE as u32) {
            let mut tile = vec![0; TILE_SIZE * TILE_SIZE * 4];
            for y in 0..TILE_SIZE {
                for x in 0..TILE_SIZE {
                    let px = tx as usize * TILE_SIZE + x;
                    let py = ty as usize * TILE_SIZE + y;
                    if px < w as usize && py < h as usize {
                        tile[(y * TILE_SIZE + x) * 4..(y * TILE_SIZE + x) * 4 + 4]
                            .copy_from_slice(&pixels[py * w as usize + px]);
                    }
                }
            }
            d = d
                .write_tile(
                    0,
                    TileCoord {
                        x: tx as i32,
                        y: ty as i32,
                    },
                    TileData::try_from(tile).unwrap(),
                )
                .unwrap();
        }
    }
    d
}
fn options(contiguous: bool, tolerance: f64) -> FloodOptions {
    FloodOptions {
        contiguous,
        tolerance,
        alpha: AlphaMode::Straight,
        sampling: Sampling::Layer,
        antialias: false,
    }
}
#[test]
fn diagonal_and_disconnected_islands() {
    let d = doc(
        3,
        3,
        &[
            [1, 0, 0, 255],
            [0, 0, 0, 255],
            [0, 0, 0, 255],
            [0, 0, 0, 255],
            [1, 0, 0, 255],
            [0, 0, 0, 255],
            [0, 0, 0, 255],
            [0, 0, 0, 255],
            [1, 0, 0, 255],
        ],
    );
    assert_eq!(
        detect_region(&d, 0, (0, 0), options(true, 0.0))
            .unwrap()
            .bytes
            .as_ref(),
        &[255, 0, 0, 0, 0, 0, 0, 0, 0]
    );
    assert_eq!(
        detect_region(&d, 0, (0, 0), options(false, 0.0))
            .unwrap()
            .bytes
            .as_ref(),
        &[255, 0, 0, 0, 255, 0, 0, 0, 255]
    );
}
#[test]
fn threshold_integer_steps_and_transparent_rgb() {
    let a = [0, 0, 0, 255];
    assert_eq!(
        tolerance_distance(a, [128, 0, 0, 255], AlphaMode::Straight),
        16320
    );
    assert_eq!(
        tolerance_distance(a, [129, 0, 0, 255], AlphaMode::Straight),
        16576
    );
    let d = doc(3, 1, &[a, [128, 0, 0, 255], [129, 0, 0, 255]]);
    assert_eq!(
        detect_region(&d, 0, (0, 0), options(false, 0.5))
            .unwrap()
            .bytes
            .as_ref(),
        &[255, 255, 0]
    );
    assert_eq!(
        tolerance_distance([200, 0, 0, 0], [0, 200, 0, 0], AlphaMode::Straight),
        0
    );
}
#[test]
fn crosses_tile_seam_and_seed_does_not_drift() {
    let mut p = vec![[0, 0, 0, 255]; 260];
    for (x, c) in p.iter_mut().enumerate() {
        *c = [(x.min(255)) as u8, 0, 0, 255];
    }
    let d = doc(260, 1, &p);
    let m = detect_region(&d, 0, (255, 0), options(true, 0.1)).unwrap();
    assert_eq!(m.bytes[256], 255);
    assert_eq!(m.bytes[230], 0);
}
#[test]
fn legacy_oracle_masks_are_byte_exact() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let fixtures = root.join("tests/fixtures");
    let oracle = root.join("tests/oracle/legacy336/flood");
    for (name, sx, sy) in [
        ("impulse_64", 32, 32),
        ("partial_alpha_64", 32, 32),
        ("plasma_512x384", 256, 192),
        ("plasma_alpha_512x384", 256, 192),
        ("alpha_edge_64", 32, 32),
        ("threshold", 0, 0),
    ] {
        let input = if name == "threshold" {
            oracle.join("threshold_input.png")
        } else {
            fixtures.join(format!("{name}.png"))
        };
        let decoder = png::Decoder::new(std::fs::File::open(input).unwrap());
        let mut reader = decoder.read_info().unwrap();
        let mut pixels = vec![0; reader.output_buffer_size()];
        let info = reader.next_frame(&mut pixels).unwrap();
        assert_eq!(info.color_type, png::ColorType::Rgba);
        let p: Vec<[u8; 4]> = pixels[..info.buffer_size()]
            .chunks_exact(4)
            .map(|x| x.try_into().unwrap())
            .collect();
        let d = doc(info.width, info.height, &p);
        let percents: &[u32] = if name == "threshold" {
            &[50]
        } else {
            &[0, 25, 50, 100]
        };
        for percent in percents {
            for contiguous in [true, false] {
                let mode = if contiguous { "contiguous" } else { "global" };
                let expected =
                    std::fs::read(oracle.join(format!("{name}_{percent}_{mode}.bin"))).unwrap();
                let actual = detect_region(
                    &d,
                    0,
                    (sx, sy),
                    options(contiguous, *percent as f64 / 100.0),
                )
                .unwrap();
                assert_eq!(
                    actual.bytes.as_ref(),
                    expected.as_slice(),
                    "{name} {percent} {mode}"
                );
            }
        }
    }
}
#[test]
fn layer_sampling_and_image_sampling_differ() {
    let d = doc(2, 1, &[[10, 0, 0, 255], [20, 0, 0, 255]])
        .add_layer(1, "top")
        .unwrap();
    let layer = detect_region(&d, 1, (0, 0), options(false, 0.0)).unwrap();
    let mut image_options = options(false, 0.0);
    image_options.sampling = Sampling::Image;
    let image = detect_region(&d, 1, (0, 0), image_options).unwrap();
    assert_eq!(layer.bytes.as_ref(), &[255, 255]);
    assert_eq!(image.bytes.as_ref(), &[255, 0]);
}
#[test]
fn wand_combines_coverage_in_all_modes() {
    let d = doc(3, 1, &[[1, 0, 0, 255], [1, 0, 0, 255], [2, 0, 0, 255]]).set_selection(
        SelectionState::Active(SelectionSource::Coverage(Arc::new(
            MaskGrid::new(3, 1, vec![0, 255, 255]).unwrap(),
        ))),
    );
    for (mode, expected) in [
        (CombineMode::Replace, [255, 255, 0]),
        (CombineMode::Add, [255, 255, 255]),
        (CombineMode::Subtract, [0, 0, 255]),
        (CombineMode::Intersect, [0, 255, 0]),
        (CombineMode::Invert, [255, 0, 255]),
    ] {
        let next = magic_wand(&d, 0, (0, 0), options(false, 0.0), mode).unwrap();
        assert_eq!(
            next.rasterize_selection(op_core::selection::RasterMode::Pixelated)
                .bytes
                .as_ref(),
            &expected,
            "{mode:?}"
        );
    }
}
#[test]
fn bucket_applies_selection_once_after_detection() {
    let d = doc(2, 1, &[[1, 0, 0, 255], [1, 0, 0, 255]]).set_selection(SelectionState::Active(
        SelectionSource::Coverage(Arc::new(MaskGrid::new(2, 1, vec![128, 0]).unwrap())),
    ));
    let out = paint_bucket(
        &d,
        0,
        (0, 0),
        options(true, 0.0),
        [255, 0, 0, 255],
        ToolCompositeMode::Overwrite,
    )
    .unwrap();
    let tile = out.read_tile(0, TileCoord { x: 0, y: 0 }).unwrap();
    assert_eq!(&tile[..8], &[128, 0, 0, 255, 1, 0, 0, 255]);
}
#[test]
fn premultiplied_alpha_changes_color_distance() {
    let d = doc(2, 1, &[[200, 0, 0, 128], [0, 0, 0, 128]]);
    let straight = detect_region(&d, 0, (0, 0), options(false, 0.4)).unwrap();
    let mut premultiplied = options(false, 0.4);
    premultiplied.alpha = AlphaMode::Premultiplied;
    let premultiplied = detect_region(&d, 0, (0, 0), premultiplied).unwrap();
    assert_eq!(straight.bytes.as_ref(), &[255, 0]);
    assert_eq!(premultiplied.bytes.as_ref(), &[255, 255]);
}
#[test]
fn provisional_antialias_averages_four_edge_neighbors() {
    let d = doc(3, 1, &[[1, 0, 0, 255], [1, 0, 0, 255], [2, 0, 0, 255]]);
    let mut aa = options(true, 0.0);
    aa.antialias = true;
    assert_eq!(
        detect_region(&d, 0, (0, 0), aa).unwrap().bytes.as_ref(),
        &[102, 102, 51]
    );
}
#[test]
#[ignore = "large 6000x4000 timing run"]
fn benchmark_photo_like_6000x4000() {
    let (w, h) = (6000_u32, 4000_u32);
    let mut d = DocState::new(
        Arc::new(TileStore::new(std::env::temp_dir(), 512 * 1024 * 1024).unwrap()),
        w,
        h,
        96.0,
    )
    .unwrap();
    for ty in 0..h.div_ceil(TILE_SIZE as u32) {
        for tx in 0..w.div_ceil(TILE_SIZE as u32) {
            let mut tile = vec![0; TILE_SIZE * TILE_SIZE * 4];
            for y in 0..TILE_SIZE {
                for x in 0..TILE_SIZE {
                    let gx = tx as usize * TILE_SIZE + x;
                    let gy = ty as usize * TILE_SIZE + y;
                    let k = (y * TILE_SIZE + x) * 4;
                    let n = (gx * 17 + gy * 31) ^ ((gx / 11) * (gy / 7));
                    tile[k] = ((gx / 31 + n % 11) % 256) as u8;
                    tile[k + 1] = ((gy / 23 + n % 13) % 256) as u8;
                    tile[k + 2] = (((gx + gy) / 41 + n % 17) % 256) as u8;
                    tile[k + 3] = 255;
                }
            }
            d = d
                .write_tile(
                    0,
                    TileCoord {
                        x: tx as i32,
                        y: ty as i32,
                    },
                    TileData::try_from(tile).unwrap(),
                )
                .unwrap();
        }
    }
    let start = std::time::Instant::now();
    let mask = detect_region(&d, 0, (3000, 2000), options(true, 0.3)).unwrap();
    println!(
        "6000x4000 contiguous 30% mask: {} ms, {} selected",
        start.elapsed().as_millis(),
        mask.bytes.iter().filter(|&&v| v != 0).count()
    );
}
