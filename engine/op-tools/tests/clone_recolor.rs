use op_core::doc::{DocState, ToolCompositeMode};
use op_core::tiles::{TileCoord, TileData, TileStore};
use op_tools::flood::AlphaMode;
use op_tools::{
    BrushSettings, CloneSource, PressureMode, RecolorOptions, StrokeSample, StrokeSession, Tool,
};
use std::sync::Arc;

fn doc() -> DocState {
    DocState::new(
        Arc::new(TileStore::new(std::env::temp_dir(), 1 << 20).unwrap()),
        16,
        4,
        96.0,
    )
    .unwrap()
}
fn put(d: DocState, layer: usize, x: usize, color: [u8; 4]) -> DocState {
    let c = TileCoord { x: 0, y: 0 };
    let mut t = d.read_tile(layer, c).unwrap();
    t[x * 4..x * 4 + 4].copy_from_slice(&color);
    d.write_tile(layer, c, TileData::try_from(t).unwrap())
        .unwrap()
}
fn pixel(d: &DocState, layer: usize, x: usize) -> [u8; 4] {
    d.read_tile(layer, TileCoord { x: 0, y: 0 }).unwrap()[x * 4..x * 4 + 4]
        .try_into()
        .unwrap()
}
fn cfg(tool: Tool) -> BrushSettings {
    BrushSettings {
        tool,
        width: 1.0,
        color: [255, 255, 255, 255],
        antialias: false,
        spacing: 1.0,
        pressure: PressureMode::None,
        blend: ToolCompositeMode::Normal,
    }
}
fn at(x: f64) -> StrokeSample {
    StrokeSample::mouse(x + 0.5, 0.5, 0)
}

#[test]
fn clone_offsets_and_frozen_source() {
    let mut d = doc();
    for x in 0..8 {
        d = put(d, 0, x, [10 + x as u8, 0, 0, 255]);
    }
    let mut a = StrokeSession::new_clone(
        d.clone(),
        0,
        cfg(Tool::CloneStamp),
        1,
        CloneSource::Layer(0),
        (0, 0),
        None,
    )
    .unwrap();
    a.append(&[at(2.0), at(3.0), at(4.0)]).unwrap();
    assert_eq!(pixel(a.preview(), 0, 4), [12, 0, 0, 255]);
    let offset = a.clone_offset().unwrap();
    let d = a.finish().unwrap();
    let mut aligned = StrokeSession::new_clone(
        d.clone(),
        0,
        cfg(Tool::CloneStamp),
        1,
        CloneSource::Layer(0),
        (0, 0),
        Some(offset),
    )
    .unwrap();
    aligned.append(&[at(6.0)]).unwrap();
    assert_eq!(pixel(aligned.preview(), 0, 6), [12, 0, 0, 255]);
    let mut unaligned = StrokeSession::new_clone(
        d,
        0,
        cfg(Tool::CloneStamp),
        1,
        CloneSource::Layer(0),
        (0, 0),
        None,
    )
    .unwrap();
    unaligned.append(&[at(7.0)]).unwrap();
    assert_eq!(pixel(unaligned.preview(), 0, 7), [10, 0, 0, 255]);
}

#[test]
fn clone_other_layer_image_and_outside() {
    let d = put(doc(), 0, 0, [90, 10, 0, 255])
        .add_layer(1, "Top")
        .unwrap();
    let d = put(d, 1, 1, [0, 70, 0, 255]);
    for (source, expected) in [
        (CloneSource::Layer(0), [90, 10, 0, 255]),
        (CloneSource::Image, [90, 10, 0, 255]),
    ] {
        let mut s =
            StrokeSession::new_clone(d.clone(), 1, cfg(Tool::CloneStamp), 1, source, (0, 0), None)
                .unwrap();
        s.append(&[at(4.0)]).unwrap();
        assert_eq!(pixel(s.preview(), 1, 4), expected);
    }
    let mut s = StrokeSession::new_clone(
        d,
        1,
        cfg(Tool::CloneStamp),
        1,
        CloneSource::Layer(0),
        (-10, 0),
        None,
    )
    .unwrap();
    s.append(&[at(4.0)]).unwrap();
    assert_eq!(pixel(s.preview(), 1, 4), [0; 4]);
}

#[test]
fn image_source_includes_top_layer_and_each_append_reads_stroke_start() {
    let d = put(doc(), 0, 0, [90, 10, 0, 255]);
    let d = put(d.add_layer(1, "Top").unwrap(), 1, 0, [0, 70, 0, 255]);
    let mut image = StrokeSession::new_clone(
        d.clone(),
        0,
        cfg(Tool::CloneStamp),
        1,
        CloneSource::Image,
        (0, 0),
        None,
    )
    .unwrap();
    image.append(&[at(2.0)]).unwrap();
    assert_eq!(pixel(image.preview(), 0, 2), [0, 70, 0, 255]);

    let mut own = StrokeSession::new_clone(
        d,
        0,
        cfg(Tool::CloneStamp),
        1,
        CloneSource::Layer(0),
        (0, 0),
        None,
    )
    .unwrap();
    own.append(&[at(2.0)]).unwrap();
    own.append(&[at(4.0)]).unwrap();
    assert_eq!(pixel(own.preview(), 0, 4), [0, 0, 0, 0]);
}

#[test]
fn recolor_legacy_shift_and_tolerance_modes() {
    let secondary = [100, 100, 100, 255];
    let primary = [120, 80, 200, 255];
    let d = put(put(doc(), 0, 0, secondary), 0, 1, [110, 90, 120, 255]);
    for (tolerance, changed) in [(0.0, false), (0.5, true), (1.0, true)] {
        for alpha in [AlphaMode::Straight, AlphaMode::Premultiplied] {
            let mut s = StrokeSession::new_recolor(
                d.clone(),
                0,
                cfg(Tool::Recolor),
                1,
                RecolorOptions {
                    primary,
                    secondary,
                    tolerance,
                    alpha,
                },
            )
            .unwrap();
            s.append(&[at(1.0)]).unwrap();
            assert_eq!(
                pixel(s.preview(), 0, 1),
                if changed {
                    [130, 70, 220, 255]
                } else {
                    [110, 90, 120, 255]
                }
            );
        }
    }
}

#[test]
fn recolor_transparent_alpha_modes_differ() {
    let d = put(doc(), 0, 1, [10, 20, 30, 1]);
    for (alpha, expected) in [
        (AlphaMode::Straight, [10, 20, 30, 1]),
        (AlphaMode::Premultiplied, [20, 30, 40, 1]),
    ] {
        let mut s = StrokeSession::new_recolor(
            d.clone(),
            0,
            cfg(Tool::Recolor),
            1,
            RecolorOptions {
                primary: [110, 110, 110, 1],
                secondary: [100, 100, 100, 1],
                tolerance: 0.0,
                alpha,
            },
        )
        .unwrap();
        s.append(&[at(1.0)]).unwrap();
        assert_eq!(pixel(s.preview(), 0, 1), expected);
    }
}

#[test]
fn golden_clone_and_recolor() {
    let mut d = doc();
    for x in 0..16 {
        d = put(d, 0, x, [x as u8 * 14, 40, 180, 255]);
    }
    let mut clone = StrokeSession::new_clone(
        d.clone(),
        0,
        cfg(Tool::CloneStamp),
        1,
        CloneSource::Layer(0),
        (1, 0),
        None,
    )
    .unwrap();
    clone.append(&[at(8.0), at(12.0)]).unwrap();
    golden("clone", "offset.png", clone.preview());
    let mut recolor = StrokeSession::new_recolor(
        d,
        0,
        cfg(Tool::Recolor),
        1,
        RecolorOptions {
            primary: [200, 50, 30, 255],
            secondary: [112, 40, 180, 255],
            tolerance: 0.5,
            alpha: AlphaMode::Straight,
        },
    )
    .unwrap();
    recolor.append(&[at(8.0), at(12.0)]).unwrap();
    golden("recolor", "shift.png", recolor.preview());
}

fn golden(folder: &str, name: &str, d: &DocState) {
    let mut raw = vec![0; (d.width * d.height * 4) as usize];
    for y in 0..d.height as usize {
        let tile = d.read_tile(0, TileCoord { x: 0, y: 0 }).unwrap();
        for x in 0..d.width as usize {
            let from = (y * 256 + x) * 4;
            let to = (y * d.width as usize + x) * 4;
            raw[to..to + 4].copy_from_slice(&tile[from..from + 4]);
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
        .join("../../tests/goldens")
        .join(folder)
        .join(name);
    if std::env::var_os("OP_BLESS").is_some() {
        std::fs::write(path, bytes).unwrap();
    } else {
        assert_eq!(std::fs::read(path).unwrap(), bytes);
    }
}
