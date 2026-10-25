use op_core::doc::{DocState, MaskGrid, PolygonSet, SelectionSource, SelectionState};
use op_core::tiles::{TILE_BYTES, TILE_SIZE, TileCoord, TileData, TileStore};
use op_ops::effect::{self, Prepared, Rect};
use std::sync::{Arc, Mutex};

const IDS: [&str; 5] = [
    "effect.blur.bokeh",
    "effect.blur.sketch",
    "effect.blur.square",
    "effect.blur.surface",
    "effect.blur.unfocus",
];

#[test]
fn m48_case_catalog_has_all_new_blurs() {
    let cases = [
        (IDS[0], &["radius", "gamma_boost", "quality"][..]),
        (IDS[1], &["radius", "percentile", "smoothness"][..]),
        (IDS[2], &["radius", "gamma_boost"][..]),
        (IDS[3], &["radius", "threshold"][..]),
        (IDS[4], &["radius"][..]),
    ];
    assert_eq!(cases.len(), IDS.len());
    let blurs: Vec<_> = effect::registered()
        .into_iter()
        .filter(|s| IDS.contains(&s.id))
        .collect();
    assert_eq!(blurs.iter().map(|s| s.id).collect::<Vec<_>>(), IDS);
    for (id, names) in cases {
        let spec = effect::find(id).expect("M48 effect registered");
        assert_eq!(spec.category, "Blurs");
        assert_eq!(
            spec.schema.fields.iter().map(|p| p.key).collect::<Vec<_>>(),
            names
        );
        let defaults = spec.default_params();
        let prep = Prepared {
            width: 512,
            height: 256,
            halo: defaults["radius"].as_i64().unwrap() as i32,
            ..Prepared::default()
        };
        let needed = (spec.required_input)(Rect::new(256, 0, 256, 256), &prep);
        assert!(needed.x < 256, "{id} reads across tile seam");
        for field in &spec.schema.fields {
            for value in [field.min, field.max].into_iter().flatten() {
                let mut params = defaults.clone();
                params[field.key] = serde_json::json!(value as i64);
                assert!(
                    spec.schema.validate(params).is_ok(),
                    "{id} {}={value}",
                    field.key
                );
            }
        }
    }
}

#[test]
fn m48_legacy_hand_cases_include_alpha_and_edges() {
    let pixels = [[255, 0, 0, 255], [0, 0, 255, 0], [0, 255, 0, 128]];
    let sample = |x: i32, _: i32| pixels[x.clamp(0, 2) as usize];
    let prep = Prepared {
        width: 3,
        height: 1,
        ..Prepared::default()
    };
    let unfocus = effect::find("effect.blur.unfocus").unwrap();
    let p = unfocus
        .schema
        .validate(serde_json::json!({"radius": 1}))
        .unwrap();
    assert_eq!(
        (unfocus.render_cpu)(&sample, 1, 0, &prep, &p),
        [85, 42, 0, 127]
    );
    let surface = effect::find("effect.blur.surface").unwrap();
    let p = surface
        .schema
        .validate(serde_json::json!({"radius": 1, "threshold": 100}))
        .unwrap();
    assert_eq!((surface.render_cpu)(&sample, 1, 0, &prep, &p)[3], 0);
}

#[test]
fn m48_legacy_defaults_match_five_oracle_fixtures() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../");
    let manifest: serde_json::Value = serde_json::from_slice(
        &std::fs::read(root.join("tests/oracle/legacy336/effects/manifest.json")).unwrap(),
    )
    .unwrap();
    for (name, id) in [("Unfocus", IDS[4]), ("Surface Blur", IDS[3])] {
        let cases: Vec<_> = manifest
            .as_array()
            .unwrap()
            .iter()
            .filter(|e| e["op"] == name && e["output"].as_str().unwrap_or("").contains("_default_"))
            .collect();
        assert_eq!(cases.len(), 5, "{name}");
        for case in cases {
            let input = op_formats::png::decode_png(
                &std::fs::read(root.join(case["input"].as_str().unwrap())).unwrap(),
            )
            .unwrap();
            let expected = op_formats::png::decode_png(
                &std::fs::read(root.join(case["output"].as_str().unwrap())).unwrap(),
            )
            .unwrap();
            let actual =
                effect::reference_rgba8(id, input.width, input.height, &input.pixels).unwrap();
            assert_eq!(actual, expected.pixels, "{name} {}", case["input"]);
        }
    }
}

struct Spill(Mutex<Vec<u8>>);
impl op_core::tiles::SpillIo for Spill {
    fn append(&self, bytes: &[u8]) -> std::io::Result<u64> {
        let mut data = self.0.lock().unwrap();
        let offset = data.len() as u64;
        data.extend_from_slice(bytes);
        Ok(offset)
    }
    fn read_exact_at(&self, offset: u64, out: &mut [u8]) -> std::io::Result<()> {
        out.copy_from_slice(&self.0.lock().unwrap()[offset as usize..offset as usize + out.len()]);
        Ok(())
    }
}

fn image(width: u32, height: u32) -> DocState {
    let store = Arc::new(TileStore::with_io(
        16 * TILE_BYTES,
        Box::new(Spill(Mutex::new(Vec::new()))),
    ));
    let mut doc = DocState::new(store, width, height, 96.0).unwrap();
    for tile_x in 0..width.div_ceil(TILE_SIZE as u32) {
        let mut tile = vec![0; TILE_BYTES];
        for y in 0..height as usize {
            for px in 0..TILE_SIZE {
                let x = tile_x as usize * TILE_SIZE + px;
                if x >= width as usize {
                    break;
                }
                let i = (y * TILE_SIZE + px) * 4;
                tile[i..i + 4].copy_from_slice(&[
                    (x % 251) as u8,
                    (x / 2) as u8,
                    (y * 90) as u8,
                    if x == 256 {
                        0
                    } else if x == 257 {
                        113
                    } else {
                        255
                    },
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
                TileData::try_from(tile).unwrap(),
            )
            .unwrap();
    }
    doc
}

fn tiles(doc: &DocState, count: i32) -> Vec<Vec<u8>> {
    (0..count)
        .map(|x| doc.read_tile(0, TileCoord { x, y: 0 }).unwrap())
        .collect()
}

#[test]
fn m48_endpoints_selection_and_tile_seams() {
    let doc = image(258, 2);
    let rect = Rect::new(0, 0, 258, 2);
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
        assert_eq!(tiles(&whole, 2), tiles(&reverse, 2), "{id} reverse");
        assert_eq!(tiles(&whole, 2), tiles(&parallel, 2), "{id} parallel");
        let mut mask = vec![0u8; 516];
        mask[256] = 128;
        let soft = doc
            .clone()
            .set_selection(SelectionState::Active(SelectionSource::Coverage(Arc::new(
                MaskGrid {
                    width: 258,
                    height: 2,
                    bytes: Arc::from(mask),
                },
            ))));
        let selected = effect::render(&soft, &spec, rect, None).unwrap();
        let source_tiles = tiles(&doc, 2);
        let all_tiles = tiles(&whole, 2);
        let soft_tiles = tiles(&selected, 2);
        assert_eq!(
            &soft_tiles[0][..4],
            &source_tiles[0][..4],
            "{id} outside soft mask"
        );
        for channel in 0..4 {
            assert_eq!(
                soft_tiles[1][channel],
                effect::coverage_lerp(source_tiles[1][channel], all_tiles[1][channel], 128),
                "{id} mask"
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
        let selected = effect::render(&geometry, &spec, rect, None).unwrap();
        let geo_tiles = tiles(&selected, 2);
        assert_eq!(
            &geo_tiles[0][..4],
            &source_tiles[0][..4],
            "{id} outside geometry"
        );
        assert_eq!(&geo_tiles[1][..4], &all_tiles[1][..4], "{id} geometry");
    }
    let tiny = image(1, 1);
    for id in IDS {
        let spec = effect::find(id).unwrap();
        let default = spec.default_params();
        for field in &spec.schema.fields {
            let mut values = vec![field.min.unwrap(), field.max.unwrap()];
            if field.min.unwrap() <= 0.0 && field.max.unwrap() >= 0.0 {
                values.push(0.0);
            }
            values.push(default[field.key].as_f64().unwrap());
            for value in values {
                let mut params = default.clone();
                params[field.key] = serde_json::json!(value as i64);
                let out =
                    effect::render(&tiny, &spec, Rect::new(0, 0, 1, 1), Some(params)).unwrap();
                assert_eq!(
                    tiles(&out, 1)[0][..4],
                    tiles(&tiny, 1)[0][..4],
                    "{id} {}={value}",
                    field.key
                );
            }
        }
    }
}

#[test]
fn m48_legacy_parameter_variants_match_oracle_samples_on_every_fixture() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../");
    let manifest: serde_json::Value = serde_json::from_slice(
        &std::fs::read(root.join("tests/oracle/legacy336/effects/manifest.json")).unwrap(),
    )
    .unwrap();
    for (name, id, count) in [("Unfocus", IDS[4], 15), ("Surface Blur", IDS[3], 25)] {
        let cases: Vec<_> = manifest
            .as_array()
            .unwrap()
            .iter()
            .filter(|e| e["op"] == name && e["status"] == "ok")
            .collect();
        assert_eq!(cases.len(), count, "{name} cases");
        let spec = effect::find(id).unwrap();
        for case in cases {
            let input = op_formats::png::decode_png(
                &std::fs::read(root.join(case["input"].as_str().unwrap())).unwrap(),
            )
            .unwrap();
            let expected = op_formats::png::decode_png(
                &std::fs::read(root.join(case["output"].as_str().unwrap())).unwrap(),
            )
            .unwrap();
            let params = spec.schema.validate(case["parameters"].clone()).unwrap();
            let prep = Prepared {
                width: input.width as i32,
                height: input.height as i32,
                ..Prepared::default()
            };
            let sample = |x: i32, y: i32| {
                let i = (y as usize * input.width as usize + x as usize) * 4;
                input.pixels[i..i + 4].try_into().unwrap()
            };
            let points = [
                (0, 0),
                (input.width as i32 - 1, 0),
                (input.width as i32 / 2, input.height as i32 / 2),
                (0, input.height as i32 - 1),
                (input.width as i32 - 1, input.height as i32 - 1),
            ];
            for (x, y) in points {
                let actual = (spec.render_cpu)(&sample, x, y, &prep, &params);
                let i = (y as usize * input.width as usize + x as usize) * 4;
                assert_eq!(
                    actual,
                    expected.pixels[i..i + 4],
                    "{name} {} at {x},{y}",
                    case["output"]
                );
            }
        }
    }
}
