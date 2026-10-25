//! M38: pen pressure endpoints, brush hardness and batch independence with
//! pressure and tilt (the live-stroke path sends samples in small batches).
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
fn cfg(tool: Tool, width: f64, pressure: PressureMode) -> BrushSettings {
    BrushSettings {
        tool,
        width,
        color: [10, 20, 30, 255],
        antialias: true,
        spacing: 0.1,
        pressure,
        blend: ToolCompositeMode::Normal,
    }
}
fn sample(x: f64, y: f64, pressure: f64, t: u64) -> StrokeSample {
    StrokeSample {
        x,
        y,
        pressure,
        timestamp_ms: t,
        tilt_x: 0.25,
        tilt_y: -0.5,
    }
}
fn bytes(s: &DocState) -> Vec<u8> {
    let mut out = Vec::new();
    for ty in 0..s.height.div_ceil(TILE_SIZE as u32) {
        for tx in 0..s.width.div_ceil(TILE_SIZE as u32) {
            let c = TileCoord {
                x: tx as i32,
                y: ty as i32,
            };
            out.extend(s.read_tile(0, c).unwrap());
        }
    }
    out
}
fn alpha(s: &DocState, x: usize, y: usize) -> u8 {
    let t = s.read_tile(0, TileCoord { x: 0, y: 0 }).unwrap();
    t[(y * TILE_SIZE + x) * 4 + 3]
}
fn painted(s: &DocState) -> usize {
    let t = s.read_tile(0, TileCoord { x: 0, y: 0 }).unwrap();
    t.chunks_exact(4).filter(|p| p[3] > 0).count()
}
/// One dab at (64, 64) with the given pressure.
fn dab(settings: BrushSettings, pressure: f64, hardness: f64) -> DocState {
    let mut s = StrokeSession::new(doc(128, 128), 0, settings, 1).unwrap();
    s.set_hardness(hardness).unwrap();
    s.append(&[sample(64.0, 64.0, pressure, 0)]).unwrap();
    s.finish().unwrap()
}

#[test]
fn pressure_for_size_endpoints() {
    let c = cfg(Tool::Paintbrush, 40.0, PressureMode::Size);
    let zero = dab(c.clone(), 0.0, 1.0);
    let half = dab(c.clone(), 0.5, 1.0);
    let full = dab(c, 1.0, 1.0);
    assert_eq!(painted(&zero), 0, "pressure 0 paints nothing");
    // Area of a 20 px and a 40 px circle, plus a one-pixel antialiased rim.
    let (h, f) = (painted(&half) as f64, painted(&full) as f64);
    let area = |d: f64| std::f64::consts::PI * d * d / 4.0;
    assert!((h - area(20.0)).abs() < 70.0, "half pressure: {h} px");
    assert!((f - area(40.0)).abs() < 140.0, "full pressure: {f} px");
    // Size pressure keeps the full opacity at the center.
    assert_eq!(alpha(&half, 64, 64), 255);
    assert_eq!(alpha(&full, 64, 64), 255);
}

#[test]
fn pressure_for_opacity_endpoints() {
    let c = cfg(Tool::Paintbrush, 20.0, PressureMode::Opacity);
    assert_eq!(painted(&dab(c.clone(), 0.0, 1.0)), 0);
    assert_eq!(alpha(&dab(c.clone(), 0.5, 1.0), 64, 64), 128);
    assert_eq!(alpha(&dab(c.clone(), 1.0, 1.0), 64, 64), 255);
    // Opacity pressure keeps the size.
    assert_eq!(
        painted(&dab(c.clone(), 0.5, 1.0)),
        painted(&dab(c, 1.0, 1.0))
    );
}

#[test]
fn pressure_for_size_and_opacity_together() {
    let c = cfg(Tool::Paintbrush, 40.0, PressureMode::SizeAndOpacity);
    let half = dab(c.clone(), 0.5, 1.0);
    assert_eq!(alpha(&half, 64, 64), 128);
    assert!(painted(&half) < painted(&dab(c, 1.0, 1.0)) / 2);
}

#[test]
fn pressure_off_ignores_pressure() {
    let c = cfg(Tool::Paintbrush, 20.0, PressureMode::None);
    assert_eq!(bytes(&dab(c.clone(), 0.0, 1.0)), bytes(&dab(c, 1.0, 1.0)));
}

#[test]
fn pencil_pressure_for_opacity_endpoints() {
    let c = cfg(Tool::Pencil, 1.0, PressureMode::Opacity);
    assert_eq!(painted(&dab(c.clone(), 0.0, 1.0)), 0);
    assert_eq!(alpha(&dab(c.clone(), 0.5, 1.0), 64, 64), 128);
    assert_eq!(alpha(&dab(c, 1.0, 1.0), 64, 64), 255);
}

#[test]
fn hardness_one_is_the_old_hard_brush_and_lower_is_softer() {
    let c = cfg(Tool::Paintbrush, 30.0, PressureMode::None);
    let mut plain = StrokeSession::new(doc(128, 128), 0, c.clone(), 1).unwrap();
    plain.append(&[sample(64.0, 64.0, 1.0, 0)]).unwrap();
    let plain = plain.finish().unwrap();
    assert_eq!(bytes(&dab(c.clone(), 1.0, 1.0)), bytes(&plain));
    let soft = dab(c.clone(), 1.0, 0.5);
    assert_eq!(alpha(&soft, 64, 64), 255, "the core stays solid");
    // 12 px from the center (0.8 of the radius) the soft dab is lighter.
    assert_eq!(alpha(&plain, 76, 64), 255);
    let a = alpha(&soft, 76, 64);
    assert!(a > 0 && a < 255, "soft edge alpha {a}");
    let zero = dab(c, 1.0, 0.0);
    assert!(alpha(&zero, 76, 64) < a, "hardness 0 is softer still");
}

#[test]
fn hardness_is_checked_and_fixed_for_the_stroke() {
    let c = cfg(Tool::Paintbrush, 10.0, PressureMode::None);
    let mut s = StrokeSession::new(doc(64, 64), 0, c, 1).unwrap();
    assert!(s.set_hardness(1.5).is_err());
    assert!(s.set_hardness(f64::NAN).is_err());
    s.set_hardness(0.25).unwrap();
    assert_eq!(s.hardness(), 0.25);
    s.append(&[sample(10.0, 10.0, 1.0, 0)]).unwrap();
    assert!(
        s.set_hardness(1.0).is_err(),
        "no change after the first sample"
    );
}

#[test]
fn pen_batches_match_one_batch() {
    let base = doc(300, 300);
    let path: Vec<StrokeSample> = (0..120)
        .map(|i| {
            let t = i as f64 / 119.0;
            let x = 20.0 + 260.0 * t;
            let y = 150.0 + 90.0 * (t * 9.0).sin();
            sample(x, y, (0.15 + 0.85 * (t * 5.0).sin().abs()).min(1.0), i * 4)
        })
        .collect();
    for (tool, hardness) in [
        (Tool::Paintbrush, 0.4),
        (Tool::Eraser, 1.0),
        (Tool::Pencil, 1.0),
    ] {
        let c = cfg(tool, 18.0, PressureMode::SizeAndOpacity);
        let run = |batches: &[usize]| {
            let mut s = StrokeSession::new(base.clone(), 0, c.clone(), 3).unwrap();
            s.set_hardness(hardness).unwrap();
            let mut at = 0;
            for &n in batches.iter().cycle() {
                if at >= path.len() {
                    break;
                }
                let end = (at + n).min(path.len());
                s.append(&path[at..end]).unwrap();
                at = end;
            }
            bytes(&s.finish().unwrap())
        };
        assert_eq!(run(&[120]), run(&[1]), "{tool:?}");
        assert_eq!(run(&[120]), run(&[3, 1, 7, 2]), "{tool:?}");
    }
}
