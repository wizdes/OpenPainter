use op_core::doc::LayerBlendMode;
use op_core::pixel::Rgba8;
use op_ops::composite::composite_pixel;

#[test]
fn float_normal_regression() {
    let bottom = Rgba8::from([0, 0, 0, 2]);
    let top = Rgba8::from([255, 255, 255, 64]);
    let result = composite_pixel(bottom, top, LayerBlendMode::Normal, 255);
    assert_eq!(result, Rgba8::from([249, 249, 249, 65]));
    assert_eq!(op_core::legacy::normal_apply(bottom, top).r, 251);
}

use op_core::doc::{DocState, ToolCompositeMode};
use op_core::history::History;
use op_core::tiles::{TILE_BYTES, TileCoord, TileData, TileStore};
use op_ops::composite::{CompositeCache, composite_tile, spec, tool_composite};
use op_testkit::{RgbaImage, Tolerance, compare};
use proptest::prelude::*;
use std::sync::Arc;

const MODES: [LayerBlendMode; 14] = [
    LayerBlendMode::Normal,
    LayerBlendMode::Multiply,
    LayerBlendMode::Additive,
    LayerBlendMode::ColorBurn,
    LayerBlendMode::ColorDodge,
    LayerBlendMode::Reflect,
    LayerBlendMode::Glow,
    LayerBlendMode::Overlay,
    LayerBlendMode::Difference,
    LayerBlendMode::Negation,
    LayerBlendMode::Lighten,
    LayerBlendMode::Darken,
    LayerBlendMode::Screen,
    LayerBlendMode::Xor,
];
const OPACITIES: [u8; 8] = [0, 1, 64, 127, 128, 192, 254, 255];
fn near(a: Rgba8, b: Rgba8) {
    let actual: [u8; 4] = a.into();
    let expected: [u8; 4] = b.into();
    if a.a == 0 && b.a == 0 {
        return;
    }
    assert!(
        actual.iter().zip(expected).all(|(x, y)| x.abs_diff(y) <= 1),
        "actual {a:?}, spec {b:?}"
    );
}
#[test]
fn testkit_comparison_contract() {
    let a = RgbaImage {
        width: 1,
        height: 1,
        data: vec![1, 2, 3, 0],
    };
    let b = RgbaImage {
        width: 1,
        height: 1,
        data: vec![200, 210, 220, 0],
    };
    assert!(compare(&a, &b, Tolerance::default()).passed);
}

#[test]
fn all_alpha_values_and_asymmetric_colors() {
    let pairs = [
        ([0, 17, 231], [255, 199, 3]),
        ([255, 0, 37], [0, 255, 218]),
        ([1, 254, 89], [237, 1, 166]),
        ([127, 128, 129], [130, 126, 4]),
        ([19, 45, 71], [91, 123, 157]),
        ([240, 200, 160], [120, 80, 40]),
        ([31, 63, 95], [127, 159, 191]),
        ([2, 4, 8], [16, 32, 64]),
        ([3, 105, 207], [251, 149, 47]),
        ([222, 11, 88], [77, 233, 144]),
        ([54, 98, 201], [188, 42, 73]),
        ([255, 255, 0], [0, 1, 255]),
        ([0, 0, 255], [255, 255, 0]),
        ([5, 155, 250], [245, 100, 10]),
        ([100, 200, 50], [200, 50, 100]),
        ([254, 127, 1], [1, 128, 254]),
    ];
    // Exercise the complete source-alpha by backdrop-alpha grid.
    for &(bc, sc) in &pairs {
        for ba in 0..=255u8 {
            for sa in 0..=255u8 {
                let b = [bc[0], bc[1], bc[2], ba].into();
                let s = [sc[0], sc[1], sc[2], sa].into();
                for mode in MODES {
                    for opacity in OPACITIES {
                        near(
                            composite_pixel(b, s, mode, opacity),
                            spec::composite_pixel(b, s, mode, opacity),
                        );
                    }
                }
            }
        }
    }
}
proptest! {
    #[test]
    fn random_pixels(bottom: [u8;4], top: [u8;4], mode in 0usize..14, opacity: u8) {
        near(composite_pixel(bottom.into(), top.into(), MODES[mode], opacity),
             spec::composite_pixel(bottom.into(), top.into(), MODES[mode], opacity));
    }
}
#[test]
fn singular_endpoints_and_tool_coverage() {
    use LayerBlendMode::*;
    // Fully opaque input exposes B exactly. The singular branch wins even
    // when the other channel would ordinarily make the quotient zero.
    for &(mode, b, s, expected) in &[
        (ColorBurn, 255, 0, 0),
        (ColorBurn, 0, 255, 0),
        (ColorBurn, 255, 255, 255),
        (ColorDodge, 0, 255, 255),
        (ColorDodge, 0, 0, 0),
        (ColorDodge, 255, 0, 255),
        (Reflect, 0, 255, 255),
        (Reflect, 0, 0, 0),
        (Reflect, 255, 0, 255),
        (Glow, 255, 0, 255),
        (Glow, 0, 0, 0),
        (Glow, 0, 255, 255),
    ] {
        let got = composite_pixel([b, b, b, 255].into(), [s, s, s, 255].into(), mode, 255);
        assert_eq!(got.r, expected, "{mode:?} b={b} s={s}");
    }
    let dst = [10, 20, 30, 40].into();
    let src = [200, 180, 160, 140].into();
    assert_eq!(
        tool_composite(dst, src, ToolCompositeMode::Overwrite, 0),
        dst
    );
    assert_eq!(
        tool_composite(dst, src, ToolCompositeMode::Overwrite, 255),
        src
    );
    assert_eq!(
        tool_composite(dst, src, ToolCompositeMode::Normal, 255),
        composite_pixel(dst, src, Normal, 255)
    );
}
fn store() -> Arc<TileStore> {
    Arc::new(TileStore::new(tempfile::tempdir().unwrap().path(), 64 * 1024 * 1024).unwrap())
}
fn fill(c: [u8; 4]) -> TileData {
    TileData::try_from(c.repeat(TILE_BYTES / 4)).unwrap()
}
#[test]
fn hundred_layers_and_hidden_layers() {
    let mut doc = DocState::new(store(), 256, 256, 96.0).unwrap();
    let coord = TileCoord { x: 0, y: 0 };
    let mut oracle = spec::Acc::default();
    let mut seed = 0x1729_5eedu32;
    for i in 0..100 {
        if i > 0 {
            doc = doc.add_layer(i, format!("layer {i}")).unwrap();
        }
        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        let rgba = [
            seed as u8,
            (seed >> 8) as u8,
            (seed >> 16) as u8,
            (seed >> 24) as u8,
        ];
        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        let opacity = seed as u8;
        let mode = MODES[((seed >> 8) as usize) % MODES.len()];
        doc = doc.write_tile(i, coord, fill(rgba)).unwrap();
        doc = doc.set_layer_opacity(i, opacity).unwrap();
        doc = doc.set_layer_blend(i, mode).unwrap();
        if i % 7 == 0 {
            doc = doc.set_layer_visible(i, false).unwrap();
        } else {
            oracle.over(rgba.into(), mode, opacity);
        }
    }
    let bytes: [u8; 4] = composite_tile(&doc, coord).unwrap().as_bytes()[..4]
        .try_into()
        .unwrap();
    let got = Rgba8::from(bytes);
    near(got, oracle.pixel());
}
fn check_cache(cache: &mut CompositeCache, doc: &DocState, coords: &[TileCoord]) {
    for &c in coords {
        assert_eq!(
            cache.get(doc, c).unwrap().as_bytes(),
            composite_tile(doc, c).unwrap().as_bytes()
        );
    }
}

#[test]
fn cache_follows_history_edits_and_jumps() {
    let coord = TileCoord { x: 0, y: 0 };
    let mut history = History::new(DocState::new(store(), 512, 256, 96.0).unwrap());
    let mut cache = CompositeCache::new();
    let other = TileCoord { x: 1, y: 0 };
    let mut seed = 15u32;
    let mut actions = [0usize, 1, 2, 3, 4, 5, 6, 7];
    for i in 0..48 {
        if i % 8 == 0 {
            for j in (1..8).rev() {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                actions.swap(j, (seed as usize) % (j + 1));
            }
        }
        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        let state = history.current_state();
        let next = match actions[i % 8] {
            0 => state
                .write_tile(0, coord, fill([seed as u8, 20, 90, 200]))
                .unwrap(),
            1 => state.add_layer(1, "upper").unwrap(),
            2 => state
                .write_tile(
                    state.layers.len() - 1,
                    other,
                    fill([10, seed as u8, 70, 255]),
                )
                .unwrap(),
            3 => state.move_layer(0, state.layers.len() - 1).unwrap(),
            4 => state.set_layer_visible(0, i % 3 != 0).unwrap(),
            5 => state.set_layer_opacity(0, seed as u8).unwrap(),
            6 => state
                .set_layer_blend(0, MODES[(seed as usize) % 14])
                .unwrap(),
            _ => state
                .resize_canvas(if i % 16 == 7 { 768 } else { 512 }, 256, 0, 0)
                .unwrap(),
        };
        history.commit("edit", "test.edit", next);
        check_cache(&mut cache, history.current_state(), &[coord, other]);
        if i % 5 == 0 {
            history.undo();
            check_cache(&mut cache, history.current_state(), &[coord, other]);
            history.redo();
            check_cache(&mut cache, history.current_state(), &[coord, other]);
        }
        if i % 9 == 0 {
            history.jump(0).unwrap();
            check_cache(&mut cache, history.current_state(), &[coord, other]);
            history.jump(history.len() - 1).unwrap();
            check_cache(&mut cache, history.current_state(), &[coord, other]);
        }
    }
}

#[test]
fn legacy336_crosscheck_record() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let blend_dir = root.join("tests/oracle/legacy336/blend");
    let fixtures = root.join("tests/fixtures");
    let mut table = String::from(
        "# Float compositor versus paint.net 3.36\n\nOpenPDN 3.36 uses integer rounding between stages. This table compares our one-rounding f32 result with the saved 3.36 PNGs. A pixel counts when any compared channel differs by more than one byte. Colors are ignored when both alphas are zero.\n\n| Mode | Opacity | Max channel difference | Pixels over 1/255 |\n| --- | ---: | ---: | ---: |\n",
    );
    let cases = [
        ("plasma_512x384", "plasma_alpha_512x384"),
        ("checker_600x300", "alpha_edge_64"),
    ];
    let mut rows = 0;
    for mode in MODES {
        let name = format!("{mode:?}");
        for opacity in [0u8, 64, 128, 192, 255] {
            let mut max_delta = 0u8;
            let mut over = 0usize;
            for &(lower_name, upper_name) in &cases {
                let lower =
                    RgbaImage::load_png(fixtures.join(format!("{lower_name}.png"))).unwrap();
                let upper =
                    RgbaImage::load_png(fixtures.join(format!("{upper_name}.png"))).unwrap();
                let oracle = RgbaImage::load_png(
                    blend_dir.join(format!("{lower_name}_{upper_name}_{name}_{opacity}.png")),
                )
                .unwrap();
                assert_eq!(
                    (oracle.width, oracle.height),
                    (lower.width.min(upper.width), lower.height.min(upper.height))
                );
                for y in 0..oracle.height as usize {
                    for x in 0..oracle.width as usize {
                        let bi = (y * lower.width as usize + x) * 4;
                        let si = (y * upper.width as usize + x) * 4;
                        let oi = (y * oracle.width as usize + x) * 4;
                        let b = &lower.data[bi..bi + 4];
                        let s = &upper.data[si..si + 4];
                        let expected = &oracle.data[oi..oi + 4];
                        let actual = composite_pixel(
                            [b[0], b[1], b[2], b[3]].into(),
                            [s[0], s[1], s[2], s[3]].into(),
                            mode,
                            opacity,
                        );
                        let bytes: [u8; 4] = actual.into();
                        if mode == LayerBlendMode::Normal
                            && opacity == 255
                            && b[3] == 255
                            && s[3] == 255
                        {
                            assert!((0..4).all(|c| bytes[c].abs_diff(expected[c]) <= 1));
                        }
                        if bytes[3] == 0 && expected[3] == 0 {
                            continue;
                        }
                        let delta = (0..4)
                            .map(|c| bytes[c].abs_diff(expected[c]))
                            .max()
                            .unwrap();
                        max_delta = max_delta.max(delta);
                        over += usize::from(delta > 1);
                    }
                }
            }
            table.push_str(&format!("| {name} | {opacity} | {max_delta} | {over} |\n"));
            rows += 1;
        }
    }
    assert_eq!(rows, 70);
    // The table is a committed record. Rewrite it only when asked (OP_BLESS=1),
    // the same way golden images are blessed; otherwise it must match.
    let path = root.join("tests/oracle/legacy336/blend_crosscheck.md");
    if std::env::var("OP_BLESS").as_deref() == Ok("1") {
        std::fs::write(&path, table).unwrap();
    } else {
        let committed = std::fs::read_to_string(&path).unwrap();
        assert_eq!(
            committed, table,
            "blend cross-check changed; rerun with OP_BLESS=1 and review the diff"
        );
    }
}
