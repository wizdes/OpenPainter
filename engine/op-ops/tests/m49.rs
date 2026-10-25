use op_ops::effect::{self, Prepared, Rect};

const CASES: [(&str, &str, &str); 6] = [
    ("effect.color.quantize", "Color", "colors"),
    ("effect.distort.crystalize", "Distort", "cell_size"),
    ("effect.distort.dents", "Distort", "scale"),
    ("effect.distort.morphology", "Distort", "width"),
    ("effect.object.drop_shadow", "Object", "shadow_radius"),
    ("effect.layers.rotate_zoom", "Layers", "angle"),
];

#[test]
fn case_catalog_covers_registry_and_parameters() {
    for &(id, category, parameter) in &CASES {
        let spec = effect::find(id).expect(id);
        assert_eq!(spec.category, category);
        let defaults = spec.default_params();
        assert!(defaults.get(parameter).is_some(), "{id}: {parameter}");
        let prep = Prepared {
            width: 300,
            height: 300,
            ..Default::default()
        };
        let needed = (spec.required_input)(Rect::new(256, 0, 44, 100), &prep);
        assert!(needed.x < 256, "{id}: tile seam read");
        for field in &spec.schema.fields {
            let mut variants = vec![field.default.clone().unwrap()];
            if let (Some(min), Some(max)) = (field.min, field.max) {
                if matches!(field.kind, effect::ParamKind::Int) {
                    variants.push(serde_json::json!(min as i64));
                    variants.push(serde_json::json!(max as i64));
                } else if matches!(field.kind, effect::ParamKind::Float) {
                    variants.push(serde_json::json!(min));
                    variants.push(serde_json::json!(max));
                }
            }
            if let effect::ParamKind::Choice(options) = &field.kind {
                variants.push(serde_json::json!(options[0]));
                variants.push(serde_json::json!(options[options.len() - 1]));
            }
            if matches!(field.kind, effect::ParamKind::Bool) {
                variants.push(serde_json::json!(false));
                variants.push(serde_json::json!(true));
            }
            for value in variants {
                let mut params = defaults.clone();
                params[field.key] = value;
                let params = spec.schema.validate(params).expect("endpoint schema");
                let prep = Prepared {
                    width: 3,
                    height: 3,
                    ..Default::default()
                };
                let sample = |sx: i32, sy: i32| [sx as u8 * 70, sy as u8 * 70, 17, 127];
                let first = (spec.render_cpu)(&sample, 0, 0, &prep, &params);
                assert_eq!(
                    first,
                    (spec.render_cpu)(&sample, 0, 0, &prep, &params),
                    "{id}: {} repeat",
                    field.key
                );
            }
        }
    }
    let ids: Vec<_> = effect::registered()
        .into_iter()
        .filter(|s| CASES.iter().any(|c| c.0 == s.id))
        .map(|s| s.id)
        .collect();
    assert_eq!(ids, CASES.map(|c| c.0));
}

#[test]
fn transparent_and_partial_alpha_inputs_are_defined() {
    let prep = Prepared {
        width: 3,
        height: 3,
        ..Default::default()
    };
    for &(id, _, _) in &CASES {
        let spec = effect::find(id).expect(id);
        let params = spec.default_params();
        for alpha in [0, 127, 255] {
            let pixel = (spec.render_cpu)(&|_, _| [23, 91, 177, alpha], 1, 1, &prep, &params);
            assert_eq!(pixel.len(), 4, "{id}");
        }
    }
}

#[test]
fn dents_matches_legacy_fixtures_and_parameter_variants() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../");
    let manifest: serde_json::Value = serde_json::from_slice(
        &std::fs::read(root.join("tests/oracle/legacy336/effects/manifest.json")).unwrap(),
    )
    .unwrap();
    let cases: Vec<_> = manifest
        .as_array()
        .unwrap()
        .iter()
        .filter(|entry| entry["op"] == "Dents" && entry["status"] == "ok")
        .collect();
    assert_eq!(cases.len(), 65);
    for case in cases {
        let input = op_formats::png::decode_png(
            &std::fs::read(root.join(case["input"].as_str().unwrap())).unwrap(),
        )
        .unwrap();
        let expected = op_formats::png::decode_png(
            &std::fs::read(root.join(case["output"].as_str().unwrap())).unwrap(),
        )
        .unwrap();
        let spec = effect::find("effect.distort.dents").unwrap();
        let params = spec.schema.validate(case["parameters"].clone()).unwrap();
        let prep = Prepared {
            width: input.width as i32,
            height: input.height as i32,
            ..Default::default()
        };
        let sample = |x: i32, y: i32| {
            let sx = x.clamp(0, input.width as i32 - 1) as usize;
            let sy = y.clamp(0, input.height as i32 - 1) as usize;
            let i = (sy * input.width as usize + sx) * 4;
            input.pixels[i..i + 4].try_into().unwrap()
        };
        let mut actual = Vec::with_capacity(input.pixels.len());
        for y in 0..input.height as i32 {
            for x in 0..input.width as i32 {
                actual.extend_from_slice(&(spec.render_cpu)(&sample, x, y, &prep, &params));
            }
        }
        let differences = actual
            .iter()
            .zip(expected.pixels.iter())
            .filter(|(a, b)| a.abs_diff(**b) > 1)
            .count();
        let max = actual
            .iter()
            .zip(expected.pixels.iter())
            .map(|(a, b)| a.abs_diff(*b))
            .max()
            .unwrap();
        let first: Vec<_> = actual
            .iter()
            .zip(expected.pixels.iter())
            .enumerate()
            .filter(|(_, (a, b))| a.abs_diff(**b) > 1)
            .take(8)
            .map(|(i, (a, b))| (i, *a, *b))
            .collect();
        assert_eq!(differences, 0, "{} max={max} {first:?}", case["output"]);
    }
}

use op_core::doc::DocState;
use op_core::tiles::{TILE_BYTES, TILE_SIZE, TileCoord, TileData, TileStore};
use std::sync::{Arc, Mutex};

struct MemorySpill(Mutex<Vec<u8>>);
impl op_core::tiles::SpillIo for MemorySpill {
    fn append(&self, bytes: &[u8]) -> std::io::Result<u64> {
        let mut data = self.0.lock().unwrap();
        let start = data.len() as u64;
        data.extend_from_slice(bytes);
        Ok(start)
    }
    fn read_exact_at(&self, offset: u64, out: &mut [u8]) -> std::io::Result<()> {
        out.copy_from_slice(&self.0.lock().unwrap()[offset as usize..offset as usize + out.len()]);
        Ok(())
    }
}
fn image() -> DocState {
    let store = Arc::new(TileStore::with_io(
        16 * TILE_BYTES,
        Box::new(MemorySpill(Mutex::new(Vec::new()))),
    ));
    let mut doc = DocState::new(store, 258, 2, 96.0).unwrap();
    for tile_x in 0..2 {
        let mut bytes = vec![0; TILE_BYTES];
        for y in 0..2 {
            for px in 0..TILE_SIZE {
                let x = tile_x * TILE_SIZE + px;
                if x >= 258 {
                    break;
                }
                let i = (y * TILE_SIZE + px) * 4;
                bytes[i..i + 4].copy_from_slice(&[
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

#[test]
fn selections_and_tile_orders_match() {
    use op_core::doc::{MaskGrid, PolygonSet, SelectionSource, SelectionState};
    let doc = image();
    let rect = Rect::new(0, 0, 258, 2);
    for &(id, _, _) in &CASES {
        let spec = effect::find(id).unwrap();
        let full = effect::render(&doc, &spec, rect, None).unwrap();
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
        assert_eq!(tiles(&full), tiles(&reverse), "{id}: reverse");
        assert_eq!(tiles(&full), tiles(&parallel), "{id}: parallel");
        let soft = doc
            .clone()
            .set_selection(SelectionState::Active(SelectionSource::Coverage(Arc::new(
                MaskGrid {
                    width: 258,
                    height: 2,
                    bytes: Arc::from(
                        (0..516)
                            .map(|i| if i == 256 { 128 } else { 0 })
                            .collect::<Vec<_>>(),
                    ),
                },
            ))));
        let soft_result = effect::render(&soft, &spec, rect, None).unwrap();
        assert_eq!(
            &tiles(&soft_result)[0][..4],
            &tiles(&doc)[0][..4],
            "{id}: soft clip"
        );
        for c in 0..4 {
            assert_eq!(
                tiles(&soft_result)[1][c],
                effect::coverage_lerp(tiles(&doc)[1][c], tiles(&full)[1][c], 128),
                "{id}: soft coverage"
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
        let geometry_result = effect::render(&geometry, &spec, rect, None).unwrap();
        assert_eq!(
            &tiles(&geometry_result)[0][..4],
            &tiles(&doc)[0][..4],
            "{id}: geometry clip"
        );
        assert_eq!(
            &tiles(&geometry_result)[1][..4],
            &tiles(&full)[1][..4],
            "{id}: geometry coverage"
        );
    }
}

#[test]
fn m49_identity_and_alpha_examples() {
    let prep = Prepared {
        width: 3,
        height: 3,
        ..Default::default()
    };
    let sample = |x: i32, y: i32| {
        if x == 1 && y == 1 {
            [40, 80, 120, 127]
        } else {
            [9, 17, 33, 0]
        }
    };
    let crystal = effect::find("effect.distort.crystalize").unwrap();
    let params = crystal
        .schema
        .validate(serde_json::json!({"cell_size":1}))
        .unwrap();
    assert_eq!(
        (crystal.render_cpu)(&sample, 1, 1, &prep, &params),
        sample(1, 1)
    );
    let dents = effect::find("effect.distort.dents").unwrap();
    let params = dents
        .schema
        .validate(serde_json::json!({"refraction":0,"quality":1}))
        .unwrap();
    assert_eq!(
        (dents.render_cpu)(&sample, 1, 1, &prep, &params),
        sample(1, 1)
    );
    let morph = effect::find("effect.distort.morphology").unwrap();
    let params = morph
        .schema
        .validate(serde_json::json!({"width":1,"height":1}))
        .unwrap();
    assert_eq!(
        (morph.render_cpu)(&sample, 1, 1, &prep, &params),
        sample(1, 1)
    );
    let params = morph
        .schema
        .validate(serde_json::json!({"mode":"dilate","width":3,"height":3}))
        .unwrap();
    assert_eq!(
        (morph.render_cpu)(&sample, 0, 0, &prep, &params),
        [40, 80, 120, 127]
    );
    let shadow = effect::find("effect.object.drop_shadow").unwrap();
    let params = shadow.schema.validate(serde_json::json!({"shadow_radius":0,"distance":1.0,"angle":0.0,"opacity":100,"color":[7,8,9],"only_draw_shadow":true})).unwrap();
    assert_eq!(
        (shadow.render_cpu)(&sample, 2, 1, &prep, &params),
        [7, 8, 9, 127]
    );
    let rotate = effect::find("effect.layers.rotate_zoom").unwrap();
    let params = rotate
        .schema
        .validate(serde_json::json!({"sampling":"nearest"}))
        .unwrap();
    assert_eq!(
        (rotate.render_cpu)(&sample, 1, 1, &prep, &params),
        sample(1, 1)
    );
}
