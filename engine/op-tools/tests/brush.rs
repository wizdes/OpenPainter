use op_core::doc::{DocState, ToolCompositeMode};
use op_core::tiles::{TILE_SIZE, TileCoord, TileStore};
use op_tools::{BrushSettings, PressureMode, StrokeSample, StrokeSession, Tool};
use std::sync::Arc;
fn doc(w: u32, h: u32) -> DocState {
    DocState::new(
        Arc::new(TileStore::new(std::env::temp_dir(), 64 * 1024 * 1024).unwrap()),
        w,
        h,
        96.0,
    )
    .unwrap()
}
fn cfg(tool: Tool, width: f64) -> BrushSettings {
    BrushSettings {
        tool,
        width,
        color: [220, 30, 60, 128],
        antialias: true,
        spacing: 0.1,
        pressure: PressureMode::None,
        blend: ToolCompositeMode::Normal,
    }
}
fn p(x: f64, y: f64, pressure: f64, timestamp_ms: u64) -> StrokeSample {
    StrokeSample {
        x,
        y,
        pressure,
        timestamp_ms,
        tilt_x: 0.0,
        tilt_y: 0.0,
    }
}
fn pix(s: &DocState, x: u32, y: u32) -> [u8; 4] {
    let t = s
        .read_tile(
            0,
            TileCoord {
                x: (x as usize / TILE_SIZE) as i32,
                y: (y as usize / TILE_SIZE) as i32,
            },
        )
        .unwrap();
    let i = ((y as usize % TILE_SIZE) * TILE_SIZE + x as usize % TILE_SIZE) * 4;
    t[i..i + 4].try_into().unwrap()
}
fn run(base: &DocState, c: BrushSettings, path: &[StrokeSample], batches: &[usize]) -> DocState {
    let mut s = StrokeSession::new(base.clone(), 0, c, 7).unwrap();
    let mut pos = 0;
    for n in batches {
        s.append(&path[pos..pos + n]).unwrap();
        pos += n;
    }
    assert_eq!(pos, path.len());
    s.finish().unwrap()
}
#[test]
fn batching_timing_click() {
    let d = doc(128, 128);
    let a = [
        p(5.0, 5.0, 1.0, 0),
        p(31.0, 29.0, 1.0, 2),
        p(70.0, 70.0, 1.0, 3),
    ];
    let x = run(&d, cfg(Tool::Paintbrush, 11.0), &a, &[3]);
    let b = run(&d, cfg(Tool::Paintbrush, 11.0), &a, &[1, 1, 1]);
    assert_eq!(
        x.read_tile(0, TileCoord { x: 0, y: 0 }).unwrap(),
        b.read_tile(0, TileCoord { x: 0, y: 0 }).unwrap()
    );
    let slow = [
        p(5.0, 5.0, 1.0, 0),
        p(31.0, 29.0, 1.0, 9000),
        p(70.0, 70.0, 1.0, 20000),
    ];
    let y = run(&d, cfg(Tool::Paintbrush, 11.0), &slow, &[2, 1]);
    assert_eq!(
        x.read_tile(0, TileCoord { x: 0, y: 0 }).unwrap(),
        y.read_tile(0, TileCoord { x: 0, y: 0 }).unwrap()
    );
    let one = run(&d, cfg(Tool::Paintbrush, 11.0), &a[..1], &[1]);
    let repeated = run(
        &d,
        cfg(Tool::Paintbrush, 11.0),
        &[a[0], a[0], a[0]],
        &[1, 2],
    );
    assert_eq!(
        one.read_tile(0, TileCoord { x: 0, y: 0 }).unwrap(),
        repeated.read_tile(0, TileCoord { x: 0, y: 0 }).unwrap()
    );
}
#[test]
fn diagonal_no_gaps_and_seam() {
    let d = doc(520, 520);
    let path = [
        p(4.0, 4.0, 1.0, 0),
        p(204.0, 204.0, 1.0, 1),
        p(404.0, 404.0, 1.0, 2),
    ];
    let o = run(&d, cfg(Tool::Paintbrush, 6.0), &path, &[3]);
    for i in 4..405 {
        assert!(pix(&o, i, i)[3] > 0, "gap {i}");
    }
    assert!(pix(&o, 255, 255)[3] > 0 && pix(&o, 256, 256)[3] > 0);
}
#[test]
fn pressure_and_eraser() {
    let d = doc(128, 32);
    let path = [p(8.0, 16.0, 0.0, 0), p(112.0, 16.0, 1.0, 1)];
    let mut c = cfg(Tool::Paintbrush, 16.0);
    c.pressure = PressureMode::Size;
    let o = run(&d, c, &path, &[2]);
    assert!(pix(&o, 100, 21)[3] > pix(&o, 20, 21)[3]);
    let mut c = cfg(Tool::Paintbrush, 8.0);
    c.pressure = PressureMode::Opacity;
    let o = run(&d, c, &path, &[2]);
    assert!(pix(&o, 100, 16)[3] > pix(&o, 20, 16)[3]);
    let mut t = vec![0; TILE_SIZE * TILE_SIZE * 4];
    for px in t.chunks_exact_mut(4) {
        px.copy_from_slice(&[10, 20, 30, 100]);
    }
    let d = d
        .write_tile(
            0,
            TileCoord { x: 0, y: 0 },
            op_core::tiles::TileData::try_from(t).unwrap(),
        )
        .unwrap();
    let e = run(&d, cfg(Tool::Eraser, 8.0), &[p(64.5, 16.5, 1.0, 0)], &[1]);
    assert_eq!(pix(&e, 64, 16), [10, 20, 30, 50]);
}
#[test]
fn pencil_octants() {
    let d = doc(64, 64);
    let mut c = cfg(Tool::Pencil, 1.0);
    c.antialias = false;
    for (dx, dy) in [
        (20i32, 7i32),
        (7, 20),
        (-7, 20),
        (-20, 7),
        (-20, -7),
        (-7, -20),
        (7, -20),
        (20, -7),
    ] {
        let end = p((32 + dx) as f64 + 0.5, (32 + dy) as f64 + 0.5, 1.0, 1);
        let a = run(&d, c.clone(), &[p(32.5, 32.5, 1.0, 0), end], &[2]);
        let b = run(&d, c.clone(), &[p(32.5, 32.5, 1.0, 0), end, end], &[1, 2]);
        assert_eq!(
            a.read_tile(0, TileCoord { x: 0, y: 0 }).unwrap(),
            b.read_tile(0, TileCoord { x: 0, y: 0 }).unwrap()
        );
        assert!(pix(&a, (32 + dx) as u32, (32 + dy) as u32)[3] > 0);
        let count = (0..64)
            .flat_map(|y| (0..64).map(move |x| (x, y)))
            .filter(|&(x, y)| pix(&a, x, y)[3] > 0)
            .count();
        assert_eq!(count, dx.abs().max(dy.abs()) as usize + 1);
        for step in 0..=dx.abs().max(dy.abs()) {
            let major = if dx.abs() >= dy.abs() {
                (32 + step * dx.signum()) as u32
            } else {
                (32 + step * dy.signum()) as u32
            };
            assert!((0..64).any(|other| if dx.abs() >= dy.abs() {
                pix(&a, major, other)[3] > 0
            } else {
                pix(&a, other, major)[3] > 0
            }));
        }
    }
}
#[test]
fn soft_selection_clips_mask() {
    use op_core::doc::{MaskGrid, SelectionSource, SelectionState};
    let base = doc(32, 32);
    let mut mask = vec![0; 32 * 32];
    mask[16 * 32 + 16] = 128;
    let selected = base.set_selection(SelectionState::Active(SelectionSource::Coverage(Arc::new(
        MaskGrid::new(32, 32, mask).unwrap(),
    ))));
    let out = run(
        &selected,
        cfg(Tool::Paintbrush, 8.0),
        &[p(16.5, 16.5, 1.0, 0)],
        &[1],
    );
    assert_eq!(pix(&out, 15, 16)[3], 0);
    assert_eq!(pix(&out, 16, 16)[3], 64);
}
#[test]
fn golden_brush_preview() {
    // brush_path.png shows a red translucent V-shaped stroke with smooth round edges.
    // pencil_octants.png shows eight one-pixel spokes radiating from the center.
    let base = doc(96, 96);
    let a = run(
        &base,
        cfg(Tool::Paintbrush, 13.0),
        &[
            p(9.0, 15.0, 1.0, 0),
            p(44.0, 54.0, 1.0, 1),
            p(85.0, 27.0, 1.0, 2),
        ],
        &[1, 2],
    );
    golden("brush_path.png", &a);
    let mut c = cfg(Tool::Pencil, 1.0);
    c.color = [20, 70, 230, 255];
    c.antialias = false;
    let mut s = base;
    for (dx, dy) in [
        (30i32, 10i32),
        (10, 30),
        (-10, 30),
        (-30, 10),
        (-30, -10),
        (-10, -30),
        (10, -30),
        (30, -10),
    ] {
        s = run(
            &s,
            c.clone(),
            &[
                p(48.5, 48.5, 1.0, 0),
                p((48 + dx) as f64 + 0.5, (48 + dy) as f64 + 0.5, 1.0, 1),
            ],
            &[2],
        );
    }
    golden("pencil_octants.png", &s);
}
fn golden(name: &str, s: &DocState) {
    let mut raw = vec![0; s.width as usize * s.height as usize * 4];
    for y in 0..s.height {
        for x in 0..s.width {
            let i = (y * s.width + x) as usize * 4;
            raw[i..i + 4].copy_from_slice(&pix(s, x, y));
        }
    }
    let mut bytes = Vec::new();
    {
        let mut e = png::Encoder::new(&mut bytes, s.width, s.height);
        e.set_color(png::ColorType::Rgba);
        e.set_depth(png::BitDepth::Eight);
        let mut w = e.write_header().unwrap();
        w.write_image_data(&raw).unwrap();
    }
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/goldens/brush")
        .join(name);
    if std::env::var_os("OP_BLESS").is_some() {
        std::fs::write(&path, &bytes).unwrap();
    } else {
        assert_eq!(std::fs::read(path).unwrap(), bytes);
    }
}
#[test]
fn invalid_batch_keeps_preview_unchanged() {
    let base = doc(32, 32);
    let mut stroke = StrokeSession::new(base.clone(), 0, cfg(Tool::Paintbrush, 8.0), 1).unwrap();
    let invalid = [p(10.5, 10.5, 1.0, 0), p(f64::NAN, 10.5, 1.0, 1)];
    assert!(stroke.append(&invalid).is_err());
    assert_eq!(
        stroke
            .preview()
            .read_tile(0, TileCoord { x: 0, y: 0 })
            .unwrap(),
        base.read_tile(0, TileCoord { x: 0, y: 0 }).unwrap()
    );
    stroke.append(&[p(20.5, 20.5, 1.0, 0)]).unwrap();
    assert_eq!(pix(stroke.preview(), 10, 10)[3], 0);
}
#[test]
fn empty_selection_and_full_alpha_erase() {
    use op_core::doc::{MaskGrid, SelectionSource, SelectionState};
    let base = doc(32, 32);
    let selected = base.set_selection(SelectionState::Active(SelectionSource::Coverage(Arc::new(
        MaskGrid::new(32, 32, vec![0; 32 * 32]).unwrap(),
    ))));
    let out = run(
        &selected,
        cfg(Tool::Paintbrush, 8.0),
        &[p(16.5, 16.5, 1.0, 0)],
        &[1],
    );
    assert_eq!(pix(&out, 16, 16), [0, 0, 0, 0]);
    let mut tile = vec![0; TILE_SIZE * TILE_SIZE * 4];
    for px in tile.chunks_exact_mut(4) {
        px.copy_from_slice(&[10, 20, 30, 255]);
    }
    let base = base
        .write_tile(
            0,
            TileCoord { x: 0, y: 0 },
            op_core::tiles::TileData::try_from(tile).unwrap(),
        )
        .unwrap();
    let out = run(
        &base,
        cfg(Tool::Eraser, 8.0),
        &[p(16.5, 16.5, 1.0, 0)],
        &[1],
    );
    assert_eq!(pix(&out, 16, 16), [10, 20, 30, 127]);
}
#[test]
fn random_batch_sizes_match_one_batch() {
    let base = doc(256, 128);
    let samples: Vec<_> = (0..75)
        .map(|i| {
            p(
                5.0 + i as f64 * 3.0,
                40.0 + (i as f64 * 0.2).sin() * 16.0,
                1.0,
                i,
            )
        })
        .collect();
    let whole = run(
        &base,
        cfg(Tool::Paintbrush, 9.0),
        &samples,
        &[samples.len()],
    );
    let mut batches = Vec::new();
    let mut remaining = samples.len();
    let mut seed = 12345u32;
    while remaining > 0 {
        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        let n = (1 + (seed as usize % 9)).min(remaining);
        batches.push(n);
        remaining -= n;
    }
    let split = run(&base, cfg(Tool::Paintbrush, 9.0), &samples, &batches);
    assert_eq!(
        whole.read_tile(0, TileCoord { x: 0, y: 0 }).unwrap(),
        split.read_tile(0, TileCoord { x: 0, y: 0 }).unwrap()
    );
}
#[test]
fn four_tile_corner_matches_shifted_single_tile() {
    let base = doc(520, 520);
    let mut c = cfg(Tool::Paintbrush, 20.0);
    c.color = [80, 140, 230, 180];
    let seam = run(
        &base,
        c.clone(),
        &[p(246.5, 246.5, 1.0, 0), p(266.5, 266.5, 1.0, 1)],
        &[2],
    );
    let single = run(
        &base,
        c,
        &[p(46.5, 46.5, 1.0, 0), p(66.5, 66.5, 1.0, 1)],
        &[2],
    );
    for y in 230..283 {
        for x in 230..283 {
            assert_eq!(
                pix(&seam, x, y),
                pix(&single, x - 200, y - 200),
                "at {x},{y}"
            );
        }
    }
}
#[test]
fn pressure_ramp_is_monotonic() {
    let base = doc(160, 64);
    let path: Vec<_> = (0..6)
        .map(|i| p(12.5 + i as f64 * 25.0, 32.5, i as f64 / 5.0, i))
        .collect();
    let mut c = cfg(Tool::Paintbrush, 18.0);
    c.pressure = PressureMode::Size;
    let sizes = run(&base, c.clone(), &path, &[6]);
    let mut prior = 0;
    for i in 0..6 {
        let x = 12 + i * 25;
        let width = (18..47).filter(|&y| pix(&sizes, x, y)[3] > 0).count();
        assert!(width >= prior);
        prior = width;
    }
    c.pressure = PressureMode::Opacity;
    let opacities = run(&base, c, &path, &[6]);
    let mut prior = 0;
    for i in 0..6 {
        let alpha = pix(&opacities, 12 + i * 25, 32)[3];
        assert!(alpha >= prior);
        prior = alpha;
    }
}
#[test]
fn pencil_revisited_pixel_is_not_redrawn() {
    let base = doc(32, 32);
    let mut c = cfg(Tool::Pencil, 1.0);
    c.pressure = PressureMode::Opacity;
    let one = run(&base, c.clone(), &[p(10.5, 10.5, 0.25, 0)], &[1]);
    let repeated = run(
        &base,
        c,
        &[
            p(10.5, 10.5, 0.25, 0),
            p(11.5, 10.5, 1.0, 1),
            p(10.5, 10.5, 1.0, 2),
        ],
        &[3],
    );
    assert_eq!(pix(&one, 10, 10), pix(&repeated, 10, 10));
}
