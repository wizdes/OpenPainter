use op_core::doc::DocState;
use op_core::selection::{MaskGrid, SelectionSource, SelectionState};
use op_core::tiles::{TILE_SIZE, TileCoord, TileData, TileStore};
use op_ops::image::Resampling;
use op_tools::r#move::{Affine, MoveKind, MoveOptions, MoveSession};
use std::sync::Arc;

fn doc() -> (DocState, Vec<u8>) {
    let store = Arc::new(TileStore::new(std::env::temp_dir(), 4 * 1024 * 1024).unwrap());
    let mut state = DocState::new(store, 8, 8, 96.0).unwrap();
    let mut tile = vec![0; TILE_SIZE * TILE_SIZE * 4];
    let mut pixels = vec![0; 8 * 8 * 4];
    for y in 0..8 {
        for x in 0..8 {
            let p = [
                ((x * 27 + y * 11) % 256) as u8,
                ((x * 13 + y * 31) % 256) as u8,
                (x * 17) as u8,
                255,
            ];
            pixels[(y * 8 + x) * 4..(y * 8 + x + 1) * 4].copy_from_slice(&p);
            tile[(y * TILE_SIZE + x) * 4..(y * TILE_SIZE + x + 1) * 4].copy_from_slice(&p);
        }
    }
    state = state
        .write_tile(
            0,
            TileCoord { x: 0, y: 0 },
            TileData::try_from(tile).unwrap(),
        )
        .unwrap();
    (state, pixels)
}
fn opts(transform: Affine, resampling: Resampling, copy: bool) -> MoveOptions {
    MoveOptions {
        transform,
        resampling,
        copy,
    }
}
fn pixel(s: &DocState, x: usize, y: usize) -> [u8; 4] {
    let tile = s.read_tile(0, TileCoord { x: 0, y: 0 }).unwrap();
    let i = (y * TILE_SIZE + x) * 4;
    tile[i..i + 4].try_into().unwrap()
}
#[test]
fn soft_edge_identity_is_byte_exact() {
    let (state, pixels) = doc();
    let mask = MaskGrid::new(8, 8, (0..64).map(|i| (i * 3 + 17) as u8).collect()).unwrap();
    let state = state.set_selection(SelectionState::Active(SelectionSource::Coverage(Arc::new(
        mask,
    ))));
    let session = MoveSession::new(
        state.clone(),
        MoveKind::Pixels,
        opts(Affine::identity(), Resampling::Bicubic, false),
    )
    .unwrap();
    let out = session.finish().unwrap();
    let tile = out.read_tile(0, TileCoord { x: 0, y: 0 }).unwrap();
    for y in 0..8 {
        assert_eq!(
            &tile[y * TILE_SIZE * 4..y * TILE_SIZE * 4 + 8 * 4],
            &pixels[y * 8 * 4..(y + 1) * 8 * 4]
        );
    }
}
#[test]
fn exact_grid_moves_copy_flip_overlap_and_recover() {
    let (state, pixels) = doc();
    let translated = MoveSession::new(
        state.clone(),
        MoveKind::Pixels,
        opts(Affine::translation(1.0, 0.0), Resampling::Bicubic, false),
    )
    .unwrap()
    .finish()
    .unwrap();
    assert_eq!(pixel(&translated, 1, 1), pixels[8 * 4..8 * 4 + 4]);
    assert_eq!(pixel(&translated, 0, 1), [0; 4]);
    let copied = MoveSession::new(
        state.clone(),
        MoveKind::Pixels,
        opts(Affine::translation(1.0, 0.0), Resampling::Nearest, true),
    )
    .unwrap()
    .finish()
    .unwrap();
    assert_eq!(pixel(&copied, 0, 1), pixel(&state, 0, 1));
    assert_eq!(pixel(&copied, 1, 1), pixel(&state, 0, 1));
    let rotated = MoveSession::new(
        state.clone(),
        MoveKind::Pixels,
        opts(
            Affine::components(
                0.0,
                0.0,
                std::f64::consts::FRAC_PI_2,
                1.0,
                1.0,
                (4.0, 4.0),
                false,
            ),
            Resampling::Bilinear,
            false,
        ),
    )
    .unwrap()
    .finish()
    .unwrap();
    assert_eq!(pixel(&rotated, 7, 0), pixel(&state, 0, 0));
    let flipped = MoveSession::new(
        state.clone(),
        MoveKind::Pixels,
        opts(
            Affine::components(0.0, 0.0, 0.0, -1.0, 1.0, (4.0, 4.0), false),
            Resampling::Nearest,
            false,
        ),
    )
    .unwrap()
    .finish()
    .unwrap();
    assert_eq!(pixel(&flipped, 7, 0), pixel(&state, 0, 0));
    let mut pending = MoveSession::new(
        state.clone(),
        MoveKind::Pixels,
        opts(Affine::translation(20.0, 0.0), Resampling::Nearest, false),
    )
    .unwrap();
    assert_eq!(pixel(&pending.preview().unwrap(), 0, 0), [0; 4]);
    pending
        .update(opts(Affine::identity(), Resampling::Nearest, false))
        .unwrap();
    assert_eq!(pixel(&pending.finish().unwrap(), 0, 0), pixel(&state, 0, 0));
}
#[test]
fn selection_move_changes_no_layer_bytes_and_zero_scale_rejected() {
    let (state, _) = doc();
    let mask = MaskGrid::new(8, 8, vec![127; 64]).unwrap();
    let state = state.set_selection(SelectionState::Active(SelectionSource::Coverage(Arc::new(
        mask,
    ))));
    let moved = MoveSession::new(
        state.clone(),
        MoveKind::Selection,
        opts(Affine::translation(1.0, 0.0), Resampling::Bilinear, false),
    )
    .unwrap()
    .finish()
    .unwrap();
    assert_eq!(
        state.read_tile(0, TileCoord { x: 0, y: 0 }).unwrap(),
        moved.read_tile(0, TileCoord { x: 0, y: 0 }).unwrap()
    );
    assert_ne!(state.selection, moved.selection);
    assert!(
        MoveSession::new(
            state,
            MoveKind::Pixels,
            opts(
                Affine::components(0.0, 0.0, 0.0, 0.0, 1.0, (0.0, 0.0), false),
                Resampling::Nearest,
                false
            )
        )
        .is_err()
    );
}
#[test]
fn coverage_outline_is_exact_under_grid_moves() {
    let (state, _) = doc();
    let mask = MaskGrid::new(8, 8, (0..64).map(|i| (i * 3 + 17) as u8).collect()).unwrap();
    let original = SelectionState::Active(SelectionSource::Coverage(Arc::new(mask)));
    let state = state.set_selection(original.clone());
    for mode in [
        Resampling::Nearest,
        Resampling::Bilinear,
        Resampling::Bicubic,
        Resampling::BicubicSmooth,
        Resampling::SuperSampling,
        Resampling::BestQuality,
    ] {
        let identity = MoveSession::new(
            state.clone(),
            MoveKind::Selection,
            opts(Affine::identity(), mode, false),
        )
        .unwrap()
        .finish()
        .unwrap();
        assert_eq!(identity.selection, original, "identity {mode:?}");
        let moved = MoveSession::new(
            state.clone(),
            MoveKind::Selection,
            opts(Affine::translation(1.0, 0.0), mode, false),
        )
        .unwrap()
        .finish()
        .unwrap();
        let SelectionState::Active(SelectionSource::Coverage(grid)) = moved.selection else {
            panic!("expected coverage");
        };
        assert_eq!(grid.bytes[1], 17, "translated {mode:?}");
        assert_eq!(grid.bytes[0], 0, "translated {mode:?}");
    }
}
fn sinc(x: f64) -> f64 {
    if x.abs() < 1e-9 {
        1.0
    } else {
        let p = std::f64::consts::PI * x;
        p.sin() / p
    }
}
fn weight(mode: Resampling, delta: f64) -> f64 {
    let x = delta.abs();
    match mode {
        Resampling::Bicubic => {
            if x < 1.0 {
                1.5 * x.powi(3) - 2.5 * x * x + 1.0
            } else if x < 2.0 {
                -0.5 * x.powi(3) + 2.5 * x * x - 4.0 * x + 2.0
            } else {
                0.0
            }
        }
        Resampling::Lanczos | Resampling::BestQuality => {
            if x < 3.0 {
                sinc(x) * sinc(x / 3.0)
            } else {
                0.0
            }
        }
        _ => (1.0 - x).max(0.0),
    }
}
fn reference(src: &[u8], transform: Affine, mode: Resampling, x: usize, y: usize) -> [u8; 4] {
    let (sx, sy) = transform
        .inverse()
        .unwrap()
        .map(x as f64 + 0.5, y as f64 + 0.5);
    if mode == Resampling::Nearest {
        let ix = (sx - 0.5).round() as isize;
        let iy = (sy - 0.5).round() as isize;
        return if (0..8).contains(&ix) && (0..8).contains(&iy) {
            src[((iy * 8 + ix) * 4) as usize..((iy * 8 + ix) * 4 + 4) as usize]
                .try_into()
                .unwrap()
        } else {
            [0; 4]
        };
    }
    let fx = sx - 0.5;
    let fy = sy - 0.5;
    let radius = if matches!(mode, Resampling::Lanczos | Resampling::BestQuality) {
        3.0
    } else if mode == Resampling::Bicubic {
        2.0
    } else {
        1.0
    };
    let mut sum = [0.0; 4];
    let mut norm = 0.0;
    for iy in (fy - radius).floor() as isize..=(fy + radius).ceil() as isize {
        for ix in (fx - radius).floor() as isize..=(fx + radius).ceil() as isize {
            let k = weight(mode, ix as f64 - fx) * weight(mode, iy as f64 - fy);
            norm += k;
            if (0..8).contains(&ix) && (0..8).contains(&iy) {
                let i = ((iy * 8 + ix) * 4) as usize;
                for c in 0..3 {
                    sum[c] += k * src[i + c] as f64 / 255.0;
                }
                sum[3] += k;
            }
        }
    }
    if norm == 0.0 || sum[3] <= 0.0 {
        return [0; 4];
    }
    let a = (sum[3] / norm).clamp(0.0, 1.0);
    [
        (sum[0] / sum[3] * 255.0).round().clamp(0.0, 255.0) as u8,
        (sum[1] / sum[3] * 255.0).round().clamp(0.0, 255.0) as u8,
        (sum[2] / sum[3] * 255.0).round().clamp(0.0, 255.0) as u8,
        (a * 255.0).round() as u8,
    ]
}
#[test]
fn rotated_scaled_matches_f64_reference_per_mode() {
    let (state, src) = doc();
    let t = Affine::components(
        0.0,
        0.0,
        std::f64::consts::PI / 6.0,
        1.5,
        1.5,
        (4.0, 4.0),
        true,
    );
    for mode in [
        Resampling::Nearest,
        Resampling::Bilinear,
        Resampling::Bicubic,
        Resampling::SuperSampling,
        Resampling::BestQuality,
    ] {
        let out = MoveSession::new(state.clone(), MoveKind::Pixels, opts(t, mode, false))
            .unwrap()
            .finish()
            .unwrap();
        for y in 0..8 {
            for x in 0..8 {
                let got = pixel(&out, x, y);
                let expected = reference(&src, t, mode, x, y);
                for c in 0..4 {
                    assert!(
                        got[c].abs_diff(expected[c]) <= 1,
                        "{mode:?} ({x},{y}) channel {c}: {} != {}",
                        got[c],
                        expected[c]
                    );
                }
            }
        }
    }
}
#[test]
fn supersampling_downscale_uses_source_pixel_area() {
    let (state, mut source) = doc();
    let mut tile = state.read_tile(0, TileCoord { x: 0, y: 0 }).unwrap();
    for y in 0..8 {
        for x in 0..8 {
            let i = (y * 8 + x) * 4;
            source[i..i + 4].copy_from_slice(&[0, 0, 0, 255]);
            let j = (y * TILE_SIZE + x) * 4;
            tile[j..j + 4].copy_from_slice(&[0, 0, 0, 255]);
        }
    }
    source[..4].copy_from_slice(&[255, 0, 0, 255]);
    tile[..4].copy_from_slice(&[255, 0, 0, 255]);
    let state = state
        .write_tile(
            0,
            TileCoord { x: 0, y: 0 },
            TileData::try_from(tile).unwrap(),
        )
        .unwrap();
    let scaled = MoveSession::new(
        state,
        MoveKind::Pixels,
        opts(
            Affine::components(0.0, 0.0, 0.0, 0.25, 0.25, (0.0, 0.0), false),
            Resampling::SuperSampling,
            false,
        ),
    )
    .unwrap()
    .finish()
    .unwrap();
    let expected: [u8; 3] = std::array::from_fn(|channel| {
        let mut sum = 0;
        for y in 0..4 {
            for x in 0..4 {
                sum += source[(y * 8 + x) * 4 + channel] as usize;
            }
        }
        (sum as f64 / 16.0).round() as u8
    });
    let got = pixel(&scaled, 0, 0);
    assert_eq!(&got[..3], &expected);
    assert_eq!(got[3], 255);
}
