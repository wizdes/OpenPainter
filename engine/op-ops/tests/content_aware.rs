use op_core::doc::{DocState, MaskGrid, SelectionSource, SelectionState};
use op_core::tiles::{TILE_BYTES, TILE_SIZE, TileCoord, TileData, TileStore};
use op_ops::effect::{self, Rect};
use op_ops::effects::content_aware::fill_rgba;
use std::sync::{Arc, Mutex};

struct Spill(Mutex<Vec<u8>>);
impl op_core::tiles::SpillIo for Spill {
    fn append(&self, bytes: &[u8]) -> std::io::Result<u64> {
        let mut v = self.0.lock().unwrap();
        let at = v.len() as u64;
        v.extend_from_slice(bytes);
        Ok(at)
    }
    fn read_exact_at(&self, at: u64, bytes: &mut [u8]) -> std::io::Result<()> {
        let v = self.0.lock().unwrap();
        bytes.copy_from_slice(&v[at as usize..at as usize + bytes.len()]);
        Ok(())
    }
}

fn image(w: usize, h: usize, f: impl Fn(usize, usize) -> [u8; 4]) -> Vec<u8> {
    let mut out = Vec::with_capacity(w * h * 4);
    for y in 0..h {
        for x in 0..w {
            out.extend(f(x, y));
        }
    }
    out
}

fn square(w: usize, h: usize, x0: usize, y0: usize, side: usize) -> Vec<u8> {
    let mut mask = vec![0; w * h];
    for y in y0..(y0 + side).min(h) {
        for x in x0..(x0 + side).min(w) {
            mask[y * w + x] = 255;
        }
    }
    mask
}
fn erase(input: &[u8], mask: &[u8]) -> Vec<u8> {
    let mut damaged = input.to_vec();
    for (i, &v) in mask.iter().enumerate() {
        if v > 0 {
            damaged[4 * i..4 * i + 4].copy_from_slice(&[0, 0, 0, 255]);
        }
    }
    damaged
}

#[test]
fn empty_and_full_masks_report_clear_errors() {
    let input = image(16, 16, |x, y| [x as u8, y as u8, 40, 255]);
    for mask in [vec![0; 256], vec![255; 256]] {
        let error = fill_rgba(&input, 16, 16, &mask, || Ok(()), |_, _| {}).unwrap_err();
        assert!(error.to_string().contains("selection") || error.to_string().contains("source"));
    }
}

#[test]
fn texture_replay_and_known_pixels() {
    let w = 48;
    let h = 48;
    let input = image(w, h, |x, y| {
        let v = ((x % 4) * 49 + (y % 4) * 13) as u8;
        [v, v / 2, 255 - v, 255]
    });
    let mask = square(w, h, 17, 17, 12);
    let damaged = erase(&input, &mask);
    let run = || fill_rgba(&damaged, w, h, &mask, || Ok(()), |_, _| {}).unwrap();
    let first = run();
    assert_eq!(first, run());
    for i in 0..w * h {
        if mask[i] == 0 {
            assert_eq!(&first[i * 4..i * 4 + 4], &input[i * 4..i * 4 + 4]);
        }
    }
}

#[test]
fn hidden_hole_bytes_do_not_affect_fill() {
    let w = 64;
    let h = 64;
    let original = image(w, h, |x, y| {
        let v = ((x * 37 + y * 83 + x * y * 3) % 256) as u8;
        [v, 255 - v, v / 2, 255]
    });
    let mask = square(w, h, 22, 22, 20);
    let first_input = erase(&original, &mask);
    let mut second_input = first_input.clone();
    for (i, &m) in mask.iter().enumerate() {
        if m > 0 {
            second_input[4 * i..4 * i + 4].copy_from_slice(&[239, 14, 199, 0]);
        }
    }
    let first = fill_rgba(&first_input, w, h, &mask, || Ok(()), |_, _| {}).unwrap();
    let second = fill_rgba(&second_input, w, h, &mask, || Ok(()), |_, _| {}).unwrap();
    assert_eq!(first, second);
}

#[test]
fn cancellation_is_seen_between_iterations() {
    let input = image(32, 32, |x, y| [x as u8, y as u8, 40, 255]);
    let mask = square(32, 32, 12, 12, 8);
    let count = std::cell::Cell::new(0);
    let result = fill_rgba(
        &input,
        32,
        32,
        &mask,
        || {
            count.set(count.get() + 1);
            if count.get() > 2 {
                Err(op_ops::effect::EffectError::Cancelled)
            } else {
                Ok(())
            }
        },
        |_, _| {},
    );
    assert!(matches!(
        result,
        Err(op_ops::effect::EffectError::Cancelled)
    ));
}

fn scores(original: &[u8], output: &[u8], mask: &[u8], w: usize, h: usize) -> (f64, f64) {
    let mut mse = 0.0;
    let mut count = 0;
    for i in 0..w * h {
        if mask[i] > 0 {
            for c in 0..3 {
                let d = f64::from(original[4 * i + c]) - f64::from(output[4 * i + c]);
                mse += d * d;
                count += 1;
            }
        }
    }
    let psnr = if mse == 0.0 {
        f64::INFINITY
    } else {
        10.0 * (255.0 * 255.0 / (mse / count as f64)).log10()
    };
    // Standard SSIM constants C1=(0.01 L)^2, C2=(0.03 L)^2 and 7x7 windows.
    let lum = |p: &[u8], i: usize| {
        0.2126 * f64::from(p[4 * i])
            + 0.7152 * f64::from(p[4 * i + 1])
            + 0.0722 * f64::from(p[4 * i + 2])
    };
    let mut total = 0.0;
    let mut windows = 0;
    for y in 3..h - 3 {
        for x in 3..w - 3 {
            let near = (y.saturating_sub(4)..=(y + 4).min(h - 1)).any(|yy| {
                (x.saturating_sub(4)..=(x + 4).min(w - 1)).any(|xx| mask[yy * w + xx] > 0)
            });
            if !near {
                continue;
            }
            let mut a = [0.0; 49];
            let mut b = [0.0; 49];
            let mut k = 0;
            for yy in y - 3..=y + 3 {
                for xx in x - 3..=x + 3 {
                    let i = yy * w + xx;
                    a[k] = lum(original, i);
                    b[k] = lum(output, i);
                    k += 1;
                }
            }
            let ma = a.iter().sum::<f64>() / 49.0;
            let mb = b.iter().sum::<f64>() / 49.0;
            let mut va = 0.0;
            let mut vb = 0.0;
            let mut cov = 0.0;
            for k in 0..49 {
                let da = a[k] - ma;
                let db = b[k] - mb;
                va += da * da;
                vb += db * db;
                cov += da * db;
            }
            va /= 48.0;
            vb /= 48.0;
            cov /= 48.0;
            total += ((2.0 * ma * mb + 2.55_f64.powi(2)) * (2.0 * cov + 7.65_f64.powi(2)))
                / ((ma * ma + mb * mb + 2.55_f64.powi(2)) * (va + vb + 7.65_f64.powi(2)));
            windows += 1;
        }
    }
    (psnr, total / windows as f64)
}

#[test]
fn texture_and_gradient_quality() {
    let w = 64;
    let h = 64;
    let mask = square(w, h, 24, 24, 12);
    let fixtures = [
        (
            "texture",
            image(w, h, |x, y| {
                let v = ((x % 4) * 40 + (y % 4) * 10) as u8;
                [v, v, 255 - v, 255]
            }),
        ),
        (
            "gradient",
            image(w, h, |x, y| {
                let v = (x * 2 + y) as u8;
                [v, v, v, 255]
            }),
        ),
    ];
    for (name, input) in fixtures {
        let damaged = erase(&input, &mask);
        let output = fill_rgba(&damaged, w, h, &mask, || Ok(()), |_, _| {}).unwrap();
        let (psnr, ssim) = scores(&input, &output, &mask, w, h);
        println!("{name}: hole PSNR {psnr:.2} dB, 7x7 rim SSIM {ssim:.4}");
        assert!(psnr >= 30.0, "{name} PSNR {psnr:.2}");
        assert!(ssim >= 0.95, "{name} SSIM {ssim:.4}");
    }
}

#[test]
fn edge_disconnected_and_alpha_sources() {
    let w = 32;
    let h = 32;
    let input = image(w, h, |x, y| {
        [x as u8, y as u8, 80, if x < 8 { 0 } else { 255 }]
    });
    let mut mask = square(w, h, 0, 0, 4);
    let second = square(w, h, 20, 20, 4);
    for i in 0..mask.len() {
        mask[i] = mask[i].max(second[i]);
    }
    let result = fill_rgba(&input, w, h, &mask, || Ok(()), |_, _| {}).unwrap();
    let (psnr, ssim) = scores(&input, &result, &mask, w, h);
    println!("edge/disconnected/alpha: hole PSNR {psnr:.2} dB, 7x7 rim SSIM {ssim:.4}");
    for i in 0..w * h {
        if mask[i] > 0 {
            assert_eq!(result[4 * i + 3], 255);
        } else {
            assert_eq!(&result[4 * i..4 * i + 4], &input[4 * i..4 * i + 4]);
        }
    }
}

#[test]
fn boundary_partial_mask_and_no_legal_source() {
    let w = 40;
    let h = 40;
    let input = image(w, h, |x, y| {
        if x < 20 {
            let v = (y % 4 * 45) as u8;
            [v, 30, 160, 255]
        } else {
            let v = (y % 4 * 45) as u8;
            [180, v, 20, 255]
        }
    });
    let mut mask = square(w, h, 16, 16, 8);
    mask[16 * w + 16] = 128;
    let damaged = erase(&input, &mask);
    let output = fill_rgba(&damaged, w, h, &mask, || Ok(()), |_, _| {}).unwrap();
    let (psnr, ssim) = scores(&input, &output, &mask, w, h);
    println!("boundary: hole PSNR {psnr:.2} dB, 7x7 rim SSIM {ssim:.4}");
    assert_eq!(&output[0..4], &input[0..4]);
    let i = 16 * w + 16;
    assert_ne!(&output[4 * i..4 * i + 4], &damaged[4 * i..4 * i + 4]);
    let transparent = image(w, h, |_, _| [20, 30, 40, 0]);
    let error = fill_rgba(&transparent, w, h, &mask, || Ok(()), |_, _| {}).unwrap_err();
    assert!(error.to_string().contains("source"));
}

#[test]
fn registered_effect_renders_selection_and_reports_progress() {
    let w = 32;
    let h = 32;
    let store = Arc::new(TileStore::with_io(
        4 * TILE_BYTES,
        Box::new(Spill(Mutex::new(Vec::new()))),
    ));
    let mut doc = DocState::new(store, w as u32, h as u32, 96.0).unwrap();
    let mut tile = vec![0; TILE_BYTES];
    for y in 0..h {
        for x in 0..w {
            let i = (y * TILE_SIZE + x) * 4;
            tile[i..i + 4].copy_from_slice(&[50, 80, 120, 255]);
        }
    }
    doc = doc
        .write_tile(
            0,
            TileCoord { x: 0, y: 0 },
            TileData::try_from(tile).unwrap(),
        )
        .unwrap();
    let mask = square(w, h, 12, 12, 8);
    doc = doc.set_selection(SelectionState::Active(SelectionSource::Coverage(Arc::new(
        MaskGrid::new(w as u32, h as u32, mask).unwrap(),
    ))));
    let spec = effect::find("effect.fill.content_aware").unwrap();
    assert_eq!(spec.category, "Edit");
    assert_eq!(spec.contract.target, "own");
    let calls = std::cell::Cell::new(0);
    let output = effect::render_tiles_checked(
        &doc,
        &spec,
        Rect::new(0, 0, w as u32, h as u32),
        &[TileCoord { x: 0, y: 0 }],
        None,
        || Ok(()),
        |done, total| {
            assert!(done <= total);
            calls.set(calls.get() + 1);
        },
    )
    .unwrap();
    assert!(calls.get() > 0);
    let tile = output.read_tile(0, TileCoord { x: 0, y: 0 }).unwrap();
    assert_eq!(
        &tile[(14 * TILE_SIZE + 14) * 4..(14 * TILE_SIZE + 14) * 4 + 4],
        &[50, 80, 120, 255]
    );
    let coords = [TileCoord { x: 0, y: 0 }];
    let serial = effect::render_tiles(
        &doc,
        &spec,
        Rect::new(0, 0, w as u32, h as u32),
        &coords,
        false,
        None,
    )
    .unwrap();
    let parallel = effect::render_tiles(
        &doc,
        &spec,
        Rect::new(0, 0, w as u32, h as u32),
        &coords,
        true,
        None,
    )
    .unwrap();
    assert_eq!(
        serial.read_tile(0, coords[0]).unwrap(),
        parallel.read_tile(0, coords[0]).unwrap()
    );
    let missing = DocState::new(
        Arc::new(TileStore::with_io(
            4 * TILE_BYTES,
            Box::new(Spill(Mutex::new(Vec::new()))),
        )),
        w as u32,
        h as u32,
        96.0,
    )
    .unwrap();
    assert!(
        effect::render(&missing, &spec, Rect::new(0, 0, w as u32, h as u32), None)
            .err()
            .unwrap()
            .to_string()
            .contains("selection")
    );
}

#[test]
#[ignore = "release performance gate"]
fn twelve_megapixel_performance() {
    let w = 4000;
    let h = 3000;
    let input = image(w, h, |x, y| {
        let n = ((x as u32 * 1664525).wrapping_add(y as u32 * 1013904223) >> 12) as u8;
        [
            n,
            n.wrapping_add((x / 11) as u8),
            n.wrapping_add((y / 9) as u8),
            255,
        ]
    });
    let mut mask = vec![0; w * h];
    for y in 1125..1875 {
        for x in 1600..2400 {
            mask[y * w + x] = 255;
        }
    }
    assert_eq!(mask.iter().filter(|&&v| v > 0).count(), w * h / 20);
    let start = std::time::Instant::now();
    let level = std::cell::Cell::new(start);
    let _ = fill_rgba(
        &input,
        w,
        h,
        &mask,
        || Ok(()),
        |done, _| {
            if done % 5 == 0 {
                println!(
                    "level {}: {:.3} s",
                    done / 5,
                    level.get().elapsed().as_secs_f64()
                );
                level.set(std::time::Instant::now());
            }
        },
    )
    .unwrap();
    println!("12 MP total: {:.3} s", start.elapsed().as_secs_f64());
    assert!(start.elapsed().as_secs_f64() < 5.0);
}
