// Provisional OpenPainter goldens: paint.net 3.36 has no healing tool.
use op_core::doc::{DocState, ToolCompositeMode};
use op_core::tiles::{TileCoord, TileData, TileStore};
use op_tools::{BrushSettings, CloneSource, PressureMode, StrokeSample, StrokeSession, Tool};
use std::sync::Arc;

fn doc(w: u32, h: u32, f: impl Fn(usize, usize) -> [u8; 4]) -> DocState {
    let mut d = DocState::new(
        Arc::new(TileStore::new(std::env::temp_dir(), 1 << 26).unwrap()),
        w,
        h,
        96.0,
    )
    .unwrap();
    for ty in 0..h.div_ceil(256) {
        for tx in 0..w.div_ceil(256) {
            let coord = TileCoord {
                x: tx as i32,
                y: ty as i32,
            };
            let mut tile = d.read_tile(0, coord).unwrap();
            for y in 0..256usize {
                for x in 0..256usize {
                    let px = tx as usize * 256 + x;
                    let py = ty as usize * 256 + y;
                    if px < w as usize && py < h as usize {
                        tile[(y * 256 + x) * 4..(y * 256 + x) * 4 + 4].copy_from_slice(&f(px, py));
                    }
                }
            }
            d = d
                .write_tile(0, coord, TileData::try_from(tile).unwrap())
                .unwrap();
        }
    }
    d
}
fn pixel(d: &DocState, x: usize, y: usize) -> [u8; 4] {
    let tile = d
        .read_tile(
            0,
            TileCoord {
                x: (x / 256) as i32,
                y: (y / 256) as i32,
            },
        )
        .unwrap();
    tile[((y % 256) * 256 + x % 256) * 4..((y % 256) * 256 + x % 256) * 4 + 4]
        .try_into()
        .unwrap()
}
fn cfg(tool: Tool, width: f64) -> BrushSettings {
    BrushSettings {
        tool,
        width,
        color: [255; 4],
        antialias: false,
        spacing: 1.0,
        pressure: PressureMode::None,
        blend: ToolCompositeMode::Normal,
    }
}
fn at(x: f64, y: f64) -> StrokeSample {
    StrokeSample::mouse(x, y, 0)
}

#[test]
fn flat_source_matches_flat_destination_and_boundary() {
    let d = doc(96, 48, |x, _| {
        if x < 40 {
            [180, 40, 30, 255]
        } else {
            [70, 100, 130, 255]
        }
    });
    let mut s = StrokeSession::new_heal(
        d.clone(),
        0,
        cfg(Tool::HealingBrush, 19.0),
        1,
        CloneSource::Layer(0),
        (15, 23),
        None,
    )
    .unwrap();
    s.append(&[at(63.5, 23.5)]).unwrap();
    let p = s.preview();
    assert_eq!(pixel(p, 63, 23), [70, 100, 130, 255]);
    assert_eq!(pixel(p, 73, 23), pixel(&d, 73, 23));
    for x in 55..73 {
        assert!((pixel(p, x, 23)[0] as i16 - pixel(p, x + 1, 23)[0] as i16).abs() <= 2);
    }
    golden("flat.png", p);
}

#[test]
fn gradient_and_texture_keep_source_detail() {
    let d = doc(128, 64, |x, y| {
        if x < 48 {
            [
                ((x * 2 + y) % 180 + 20) as u8,
                ((x * 37 + y * 17) % 51 + 80) as u8,
                100,
                255,
            ]
        } else {
            [110, 110, 110, 255]
        }
    });
    let mut s = StrokeSession::new_heal(
        d,
        0,
        cfg(Tool::HealingBrush, 25.0),
        1,
        CloneSource::Layer(0),
        (23, 31),
        None,
    )
    .unwrap();
    s.append(&[at(82.5, 31.5)]).unwrap();
    let p = s.preview();
    for x in 77..86 {
        assert!((pixel(p, x + 1, 31)[0] as i16 - pixel(p, x, 31)[0] as i16 - 2).abs() <= 1);
    }
    let src: Vec<f64> = (27..37)
        .flat_map(|y| (77..87).map(move |x| ((x - 59) * 37 + y * 17) as f64 % 51.0))
        .collect();
    let out: Vec<f64> = (27..37)
        .flat_map(|y| (77..87).map(move |x| pixel(p, x, y)[1] as f64))
        .collect();
    fn var(a: &[f64]) -> f64 {
        let m = a.iter().sum::<f64>() / a.len() as f64;
        a.iter().map(|v| (v - m).powi(2)).sum::<f64>() / a.len() as f64
    }
    assert!((var(&out) / var(&src) - 1.0).abs() <= 0.10);
    golden("texture.png", p);
}

#[test]
fn spot_removes_blot_and_source_offset_is_frozen() {
    let d = doc(80, 48, |x, y| {
        if (x as i32 - 40).pow(2) + (y as i32 - 24).pow(2) <= 9 {
            [0, 0, 0, 255]
        } else {
            [125, 125, 125, 255]
        }
    });
    let mut s = StrokeSession::new_spot_heal(d.clone(), 0, cfg(Tool::SpotHeal, 13.0), 1).unwrap();
    s.append(&[at(40.5, 24.5)]).unwrap();
    for y in 21..28 {
        for x in 37..44 {
            assert!((pixel(s.preview(), x, y)[0] as i16 - 125).abs() <= 2);
        }
    }
    golden("spot.png", s.preview());
    let mut h = StrokeSession::new_heal(
        d,
        0,
        cfg(Tool::HealingBrush, 7.0),
        1,
        CloneSource::Layer(0),
        (-100, 0),
        None,
    )
    .unwrap();
    h.append(&[at(40.5, 24.5)]).unwrap();
    assert_eq!(pixel(h.preview(), 40, 24), [0, 0, 0, 255]);
}

fn golden(name: &str, d: &DocState) {
    let mut raw = Vec::with_capacity((d.width * d.height * 4) as usize);
    for y in 0..d.height as usize {
        for x in 0..d.width as usize {
            raw.extend_from_slice(&pixel(d, x, y));
        }
    }
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, d.width, d.height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&raw)
            .unwrap();
    }
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/goldens/heal")
        .join(name);
    if std::env::var_os("OP_BLESS").is_some() {
        std::fs::write(path, bytes).unwrap();
    } else {
        assert_eq!(std::fs::read(path).unwrap(), bytes);
    }
}

#[test]
fn aligned_offset_reuses_source_and_image_sampling_reads_other_layer() {
    let d = doc(96, 32, |x, _| {
        if x < 32 {
            [x as u8 * 4, 40, 40, 255]
        } else {
            [100, 100, 100, 255]
        }
    });
    let mut top = d.add_layer(1, "Texture").unwrap();
    let coord = TileCoord { x: 0, y: 0 };
    let mut tile = top.read_tile(1, coord).unwrap();
    for y in 0..32usize {
        for x in 0..32usize {
            tile[(y * 256 + x) * 4..(y * 256 + x) * 4 + 4].copy_from_slice(&[
                (x * 7) as u8,
                120,
                80,
                255,
            ]);
        }
    }
    top = top
        .write_tile(1, coord, TileData::try_from(tile).unwrap())
        .unwrap();
    let mut layer = StrokeSession::new_heal(
        top.clone(),
        0,
        cfg(Tool::HealingBrush, 13.0),
        1,
        CloneSource::Layer(0),
        (12, 16),
        None,
    )
    .unwrap();
    layer.append(&[at(48.5, 16.5)]).unwrap();
    assert_eq!(layer.clone_offset(), Some((36, 0)));
    let mut image = StrokeSession::new_heal(
        top.clone(),
        0,
        cfg(Tool::HealingBrush, 13.0),
        1,
        CloneSource::Image,
        (12, 16),
        Some((36, 0)),
    )
    .unwrap();
    image.append(&[at(48.5, 16.5)]).unwrap();
    assert_ne!(
        pixel(layer.preview(), 51, 16),
        pixel(image.preview(), 51, 16)
    );
    let mut aligned = StrokeSession::new_heal(
        top.clone(),
        0,
        cfg(Tool::HealingBrush, 13.0),
        1,
        CloneSource::Layer(0),
        (12, 16),
        layer.clone_offset(),
    )
    .unwrap();
    aligned.append(&[at(68.5, 16.5)]).unwrap();
    assert_eq!(aligned.clone_offset(), Some((36, 0)));
    let mut unaligned = StrokeSession::new_heal(
        top,
        0,
        cfg(Tool::HealingBrush, 13.0),
        1,
        CloneSource::Layer(0),
        (12, 16),
        None,
    )
    .unwrap();
    unaligned.append(&[at(68.5, 16.5)]).unwrap();
    assert_eq!(unaligned.clone_offset(), Some((56, 0)));
}

#[test]
fn healing_reads_frozen_source_across_append_batches() {
    let d = doc(96, 32, |x, y| {
        if (x as i32 - 40).pow(2) + (y as i32 - 16).pow(2) <= 4 {
            [0, 0, 0, 255]
        } else {
            [125, 125, 125, 255]
        }
    });
    let settings = cfg(Tool::HealingBrush, 9.0);
    let mut batched = StrokeSession::new_heal(
        d.clone(),
        0,
        settings.clone(),
        1,
        CloneSource::Layer(0),
        (20, 16),
        None,
    )
    .unwrap();
    batched.append(&[at(40.5, 16.5)]).unwrap();
    assert_ne!(pixel(batched.preview(), 40, 16), pixel(&d, 40, 16));
    batched.append(&[at(60.5, 16.5)]).unwrap();
    let mut together =
        StrokeSession::new_heal(d, 0, settings, 1, CloneSource::Layer(0), (20, 16), None).unwrap();
    together.append(&[at(40.5, 16.5), at(60.5, 16.5)]).unwrap();
    for y in 0..32 {
        for x in 0..96 {
            assert_eq!(
                pixel(batched.preview(), x, y),
                pixel(together.preview(), x, y)
            );
        }
    }
}
