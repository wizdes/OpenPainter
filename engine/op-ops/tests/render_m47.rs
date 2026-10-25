use op_core::doc::{DocState, MaskGrid, PolygonSet, SelectionSource, SelectionState};
use op_core::tiles::{TILE_BYTES, TileCoord, TileData, TileStore};
use op_ops::effect::{self, Prepared};
use std::sync::Arc;

struct NoSpill;
impl op_core::tiles::SpillIo for NoSpill {
    fn append(&self, _: &[u8]) -> std::io::Result<u64> {
        Err(std::io::Error::other("spill"))
    }
    fn read_exact_at(&self, _: u64, _: &mut [u8]) -> std::io::Result<()> {
        Err(std::io::Error::other("spill"))
    }
}
fn image() -> DocState {
    let store = Arc::new(TileStore::with_io(8 * TILE_BYTES, Box::new(NoSpill)));
    let mut doc = DocState::new(store, 258, 2, 96.0).unwrap();
    for tile_x in 0..2 {
        let mut bytes = vec![0; TILE_BYTES];
        for y in 0..2 {
            for px in 0..256 {
                let x = tile_x * 256 + px;
                if x >= 258 {
                    break;
                }
                let i = (y * 256 + px) * 4;
                bytes[i..i + 4].copy_from_slice(&[
                    (x % 251) as u8,
                    (x / 2) as u8,
                    (x * 3 + y) as u8,
                    if x == 256 { 0 } else { 117 },
                ]);
            }
        }
        doc = doc
            .write_tile(
                0,
                TileCoord {
                    x: tile_x as i32,
                    y: 0,
                },
                TileData::try_from(bytes).unwrap(),
            )
            .unwrap();
    }
    doc
}
fn tiles(doc: &DocState) -> Vec<Vec<u8>> {
    (0..2)
        .map(|x| doc.read_tile(0, TileCoord { x, y: 0 }).unwrap())
        .collect()
}

const IDS: [&str; 4] = [
    "effect.render.clouds",
    "effect.render.julia_fractal",
    "effect.render.mandelbrot_fractal",
    "effect.render.turbulence",
];
const REQUIRED_CASES: [&str; 8] = [
    "zero",
    "default",
    "max",
    "image_edges",
    "partial_alpha_and_hidden_color",
    "geometry_selection",
    "soft_mask_selection",
    "tile_seams",
];
const CASE_CATALOG: [(&str, &[&str]); 4] = [
    (IDS[0], &REQUIRED_CASES),
    (IDS[1], &REQUIRED_CASES),
    (IDS[2], &REQUIRED_CASES),
    (IDS[3], &REQUIRED_CASES),
];

#[test]
fn render_menu_case_catalog() {
    let actual: Vec<_> = effect::registered()
        .into_iter()
        .filter(|spec| spec.category == "Render")
        .map(|spec| spec.id)
        .collect();
    assert_eq!(actual, IDS);
    assert_eq!(CASE_CATALOG.len(), IDS.len());
    for id in IDS {
        let cases = CASE_CATALOG
            .iter()
            .find(|row| row.0 == id)
            .expect("missing case catalog entry")
            .1;
        for required in REQUIRED_CASES {
            assert!(cases.contains(&required), "{id} {required}");
        }
        let spec = effect::find(id).expect("every Render effect has a case entry");
        let defaults = spec.default_params();
        for field in &spec.schema.fields {
            if let Some(min) = field.min {
                let mut case = defaults.clone();
                case[field.key] = if matches!(field.kind, effect::ParamKind::Int) {
                    serde_json::json!(min as i64)
                } else {
                    serde_json::json!(min)
                };
                let params = spec.schema.validate(case).unwrap();
                let prep = Prepared {
                    width: 64,
                    height: 64,
                    ..Prepared::default()
                };
                let _ = (spec.render_cpu)(&|_, _| [19, 37, 73, 0], 0, 0, &prep, &params);
            }
            if let Some(max) = field.max {
                let mut case = defaults.clone();
                case[field.key] = if matches!(field.kind, effect::ParamKind::Int) {
                    serde_json::json!(max as i64)
                } else {
                    serde_json::json!(max)
                };
                let params = spec.schema.validate(case).unwrap();
                let prep = Prepared {
                    width: 64,
                    height: 64,
                    ..Prepared::default()
                };
                let _ = (spec.render_cpu)(&|_, _| [19, 37, 73, 0], 63, 63, &prep, &params);
            }
        }
        let prep = Prepared {
            width: 64,
            height: 64,
            ..Prepared::default()
        };
        let sample = |_: i32, _: i32| [19, 37, 73, 0];
        let first = (spec.render_cpu)(&sample, 0, 0, &prep, &defaults);
        let again = (spec.render_cpu)(&sample, 0, 0, &prep, &defaults);
        assert_eq!(first, again, "{id} must be deterministic at the image edge");
    }
}

#[test]
fn render_selection_and_tile_seams() {
    let doc = image();
    let rect = effect::Rect::new(0, 0, 258, 2);
    for id in IDS {
        let spec = effect::find(id).unwrap();
        let whole = effect::render(&doc, &spec, rect, None).unwrap();
        let reverse = effect::render_tiles(
            &doc,
            &spec,
            rect,
            &[TileCoord { x: 1, y: 0 }, TileCoord { x: 0, y: 0 }],
            false,
            None,
        )
        .unwrap();
        let parallel = effect::render_tiles(
            &doc,
            &spec,
            rect,
            &[TileCoord { x: 0, y: 0 }, TileCoord { x: 1, y: 0 }],
            true,
            None,
        )
        .unwrap();
        assert_eq!(tiles(&whole), tiles(&reverse), "{id} reverse seam");
        assert_eq!(tiles(&whole), tiles(&parallel), "{id} parallel seam");
        let mask = Arc::new(MaskGrid {
            width: 258,
            height: 2,
            bytes: Arc::from(
                (0..516)
                    .map(|i| if i == 256 { 128 } else { 0 })
                    .collect::<Vec<_>>(),
            ),
        });
        let soft = doc
            .clone()
            .set_selection(SelectionState::Active(SelectionSource::Coverage(mask)));
        let soft_out = effect::render(&soft, &spec, rect, None).unwrap();
        assert_eq!(tiles(&soft_out)[0], tiles(&doc)[0], "{id} soft clip");
        for channel in 0..4 {
            assert_eq!(
                tiles(&soft_out)[1][channel],
                effect::coverage_lerp(tiles(&doc)[1][channel], tiles(&whole)[1][channel], 128),
                "{id} soft pixel"
            );
        }
        let geometry =
            doc.clone()
                .set_selection(SelectionState::Active(SelectionSource::Geometry(Arc::new(
                    PolygonSet(vec![vec![
                        (256.0, 0.0),
                        (258.0, 0.0),
                        (258.0, 2.0),
                        (256.0, 2.0),
                    ]]),
                ))));
        let geometry_out = effect::render(&geometry, &spec, rect, None).unwrap();
        assert_eq!(
            tiles(&geometry_out)[0],
            tiles(&doc)[0],
            "{id} geometry clip"
        );
        assert_eq!(
            &tiles(&geometry_out)[1][..8],
            &tiles(&whole)[1][..8],
            "{id} geometry selected"
        );
    }
}

#[test]
fn render_new_controls_change_pixels_and_keep_hidden_input() {
    let prep = Prepared {
        width: 64,
        height: 64,
        ..Prepared::default()
    };
    let sample = |_: i32, _: i32| [83, 171, 219, 117];
    for id in [IDS[1], IDS[2]] {
        let spec = effect::find(id).unwrap();
        let defaults = spec.default_params();
        let plain = (spec.render_cpu)(&sample, 0, 0, &prep, &defaults);
        let blended = spec
            .schema
            .validate(serde_json::json!({"blend_mode":"multiply"}))
            .unwrap();
        let result = (spec.render_cpu)(&sample, 0, 0, &prep, &blended);
        assert_ne!(plain, result, "{id} blend mode");
    }
    let clouds = effect::find(IDS[0]).unwrap();
    let colored = clouds
        .schema
        .validate(serde_json::json!({"color_1":[255,0,0,0],"color_2":[0,0,255,255]}))
        .unwrap();
    let pixel = (clouds.render_cpu)(&sample, 32, 32, &prep, &colored);
    assert!(pixel[3] < 255, "Clouds transparent color controls alpha");
    let turbulence = effect::find(IDS[3]).unwrap();
    let normal = (turbulence.render_cpu)(&sample, 10, 10, &prep, &turbulence.default_params());
    let sum = turbulence
        .schema
        .validate(serde_json::json!({"noise":"fractal_sum"}))
        .unwrap();
    assert_ne!(
        normal,
        (turbulence.render_cpu)(&sample, 10, 10, &prep, &sum)
    );
    let blended = turbulence
        .schema
        .validate(serde_json::json!({"blend":true}))
        .unwrap();
    assert_eq!(
        (turbulence.render_cpu)(&sample, 10, 10, &prep, &blended)[3],
        117
    );
    let hidden = |_: i32, _: i32| [83, 171, 219, 0];
    assert_eq!(
        (turbulence.render_cpu)(&hidden, 10, 10, &prep, &blended)[3],
        0
    );
}

#[test]
fn clouds_rgb_and_alpha_controls_work_without_new_engine_param_kind() {
    let spec = effect::find(IDS[0]).unwrap();
    let params = spec
        .schema
        .validate(serde_json::json!({
            "color_1":[255,0,0], "color_1_alpha":0,
            "color_2":[0,0,255], "color_2_alpha":255,
        }))
        .unwrap();
    let prep = Prepared {
        width: 64,
        height: 64,
        ..Prepared::default()
    };
    let pixel = (spec.render_cpu)(&|_, _| [19, 37, 73, 0], 32, 32, &prep, &params);
    assert!(pixel[3] < 255);
}

#[test]
fn legacy_render_oracle_every_fixture_and_variant() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../");
    let manifest: serde_json::Value = serde_json::from_slice(
        &std::fs::read(root.join("tests/oracle/legacy336/effects/manifest.json")).unwrap(),
    )
    .unwrap();
    for (name, id) in [
        ("Clouds", IDS[0]),
        ("Julia Fractal", IDS[1]),
        ("Mandelbrot Fractal", IDS[2]),
    ] {
        let cases: Vec<_> = manifest
            .as_array()
            .unwrap()
            .iter()
            .filter(|entry| {
                entry["op"] == name && entry["output"].as_str().is_some_and(|s| !s.is_empty())
            })
            .collect();
        assert!(cases.len() >= 5, "{name} needs five fixtures");
        let spec = effect::find(id).unwrap();
        for case in cases {
            let input = op_formats::png::decode_png(
                &std::fs::read(root.join(case["input"].as_str().unwrap())).unwrap(),
            )
            .unwrap();
            let oracle = op_formats::png::decode_png(
                &std::fs::read(root.join(case["output"].as_str().unwrap())).unwrap(),
            )
            .unwrap();
            let mut params = case["parameters"].clone();
            if name == "Clouds"
                && let Some(power) = params.as_object_mut().unwrap().remove("power")
            {
                params["roughness"] = power;
            }
            let params = spec.schema.validate(params).unwrap();
            let prep = Prepared {
                width: input.width as i32,
                height: input.height as i32,
                ..Prepared::default()
            };
            let sample = |x: i32, y: i32| -> [u8; 4] {
                input.pixels[((y as usize * input.width as usize + x as usize) * 4)..][..4]
                    .try_into()
                    .unwrap()
            };
            let mut worst = 0u8;
            let mut first = None;
            for y in 0..input.height as i32 {
                for x in 0..input.width as i32 {
                    let got = (spec.render_cpu)(&sample, x, y, &prep, &params);
                    let pos = (y as usize * input.width as usize + x as usize) * 4;
                    let expected: &[u8] = &oracle.pixels[pos..pos + 4];
                    if first.is_none() && got != expected {
                        first = Some((x, y, got, expected.to_vec()));
                    }
                    for channel in 0..4 {
                        worst = worst.max(got[channel].abs_diff(expected[channel]));
                    }
                }
            }
            assert!(
                worst <= 1,
                "{name} {} worst {worst} first {first:?}",
                case["output"]
            );
        }
    }
}
