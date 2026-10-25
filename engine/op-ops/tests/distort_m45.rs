use op_ops::effect::{self, Prepared, Rect};

const IDS: [&str; 6] = [
    "effect.distort.bulge",
    "effect.distort.frosted_glass",
    "effect.distort.pixelate",
    "effect.distort.polar_inversion",
    "effect.distort.tile_reflection",
    "effect.distort.twist",
];

#[test]
fn m45_registry_and_case_catalog() {
    let registered: Vec<_> = effect::registered()
        .into_iter()
        .filter(|s| IDS.contains(&s.id))
        .collect();
    assert_eq!(registered.iter().map(|s| s.id).collect::<Vec<_>>(), IDS);
    for id in IDS {
        let spec = effect::find(id).expect("catalog effect must be registered");
        let p = spec.default_params();
        let prep = Prepared {
            width: 600,
            height: 300,
            ..Prepared::default()
        };
        let needed = (spec.required_input)(Rect::new(256, 0, 256, 256), &prep);
        assert!(
            needed.x < 256 && needed.width > 256,
            "{id} must read across the left tile seam"
        );
        assert!(!p.as_object().unwrap().is_empty(), "{id} needs parameters");
    }
}

use op_core::doc::{DocState, MaskGrid, PolygonSet, SelectionSource, SelectionState};
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
fn store() -> Arc<TileStore> {
    Arc::new(TileStore::with_io(
        16 * TILE_BYTES,
        Box::new(MemorySpill(Mutex::new(Vec::new()))),
    ))
}
fn image() -> DocState {
    let mut doc = DocState::new(store(), 258, 2, 96.0).unwrap();
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
fn m54_every_registered_effect_has_a_target_and_deterministic_tiles() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../");
    let targets = std::fs::read_to_string(root.join("tests/oracle/targets.toml")).unwrap();
    let doc = image();
    let mut count = 0;
    for spec in effect::registered() {
        assert!(
            targets.contains(&format!("id = \"{}\"", spec.id)),
            "{} lacks an oracle target row",
            spec.id
        );
        let selected;
        let content_doc;
        let input = if spec.id == "effect.fill.content_aware" {
            let mut built = DocState::new(store(), 258, 64, 96.0).unwrap();
            for x in 0..2 {
                let mut bytes = vec![0; TILE_BYTES];
                for pixel in bytes.chunks_exact_mut(4) {
                    pixel.copy_from_slice(&[80, 120, 160, 255]);
                }
                built = built
                    .write_tile(0, TileCoord { x, y: 0 }, TileData::try_from(bytes).unwrap())
                    .unwrap();
            }
            content_doc = built;
            selected = content_doc.clone().set_selection(SelectionState::Active(
                SelectionSource::Coverage(Arc::new(MaskGrid {
                    width: 258,
                    height: 64,
                    bytes: Arc::from(
                        (0..258 * 64)
                            .map(|i| u8::from(i == 32 * 258 + 128) * 255)
                            .collect::<Vec<_>>(),
                    ),
                })),
            ));
            &selected
        } else {
            &doc
        };
        let effect_rect = Rect::new(0, 0, 258, input.height);
        let effect_coords = effect::tile_coords(effect_rect);
        let whole = effect::render(input, &spec, effect_rect, None).unwrap();
        let mut reverse = effect_coords.clone();
        reverse.reverse();
        let reversed = effect::render_tiles_checked(
            input,
            &spec,
            effect_rect,
            &reverse,
            None,
            || Ok(()),
            |_, _| {},
        )
        .unwrap();
        let parallel =
            effect::render_tiles(input, &spec, effect_rect, &effect_coords, true, None).unwrap();
        assert_eq!(tiles(&whole), tiles(&reversed), "{} reversed", spec.id);
        assert_eq!(tiles(&whole), tiles(&parallel), "{} parallel", spec.id);
        count += 1;
    }
    assert_eq!(count, 55, "registry size changed; review matrix coverage");
}

#[test]
fn m45_cases_cover_each_parameter_and_tile_edge() {
    let doc = image();
    let rect = Rect::new(0, 0, 258, 2);
    for id in IDS {
        let spec = effect::find(id).unwrap();
        let default = spec.default_params();
        let whole = effect::render(&doc, &spec, rect, None).unwrap();
        let reversed = effect::render_tiles(
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
        assert_eq!(tiles(&whole), tiles(&reversed), "{id} reverse seam");
        assert_eq!(tiles(&whole), tiles(&parallel), "{id} parallel seam");
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
        let selected = effect::render(&soft, &spec, rect, None).unwrap();
        assert_eq!(
            &tiles(&selected)[0][..4],
            &tiles(&doc)[0][..4],
            "{id} soft clip"
        );
        for channel in 0..4 {
            assert_eq!(
                tiles(&selected)[1][channel],
                effect::coverage_lerp(tiles(&doc)[1][channel], tiles(&whole)[1][channel], 128),
                "{id} soft coverage"
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
        assert_eq!(
            &tiles(&selected)[0][..4],
            &tiles(&doc)[0][..4],
            "{id} geometry clip"
        );
        assert_eq!(
            &tiles(&selected)[1][..4],
            &tiles(&whole)[1][..4],
            "{id} geometry coverage"
        );
        for field in &spec.schema.fields {
            if let (Some(min), Some(max)) = (field.min, field.max) {
                let values = if min <= 0.0 && max >= 0.0 {
                    vec![min, 0.0, max]
                } else {
                    vec![min, max]
                };
                for value in values {
                    let mut params = default.clone();
                    params[field.key] = match field.kind {
                        effect::ParamKind::Int => serde_json::json!(value as i64),
                        _ => serde_json::json!(value),
                    };
                    let rendered = effect::render(&doc, &spec, rect, Some(params)).unwrap();
                    assert_eq!(tiles(&rendered).len(), 2, "{id} {} {value}", field.key);
                }
            }
            if let effect::ParamKind::Choice(options) = &field.kind {
                for &option in [options[0], options[options.len() - 1]].iter() {
                    let mut params = default.clone();
                    params[field.key] = option.into();
                    let rendered = effect::render(&doc, &spec, rect, Some(params)).unwrap();
                    assert_eq!(tiles(&rendered).len(), 2, "{id} {} {option}", field.key);
                }
            }
        }
    }
}

#[test]
fn m45_legacy_oracle_defaults() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../");
    let manifest: serde_json::Value = serde_json::from_slice(
        &std::fs::read(root.join("tests/oracle/legacy336/effects/manifest.json")).unwrap(),
    )
    .unwrap();
    let mut failures = Vec::new();
    for (id, name) in IDS.into_iter().zip([
        "Bulge",
        "Frosted Glass",
        "Pixelate",
        "Polar Inversion",
        "Tile Reflection",
        "Twist",
    ]) {
        let cases: Vec<_> = manifest
            .as_array()
            .unwrap()
            .iter()
            .filter(|e| e["op"] == name && e["output"].as_str().unwrap_or("").contains("_default_"))
            .collect();
        assert_eq!(cases.len(), 5, "{id}");
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
            let differences = actual
                .iter()
                .zip(expected.pixels.iter())
                .filter(|(a, b)| a.abs_diff(**b) > 1)
                .count();
            if differences > 0 {
                failures.push(format!("{id} {}: {differences} channels", case["input"]));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn m45_warps_sample_across_tile_seam_and_image_edge() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let prep = Prepared {
        width: 514,
        height: 300,
        ..Prepared::default()
    };
    for id in IDS {
        let spec = effect::find(id).unwrap();
        let mut params = spec.default_params();
        if id == "effect.distort.pixelate" {
            params["cell_size"] = serde_json::json!(3);
        }
        let across = AtomicBool::new(false);
        let edge = AtomicBool::new(false);
        let sample = |sx: i32, sy: i32| {
            if sx >= 256 {
                across.store(true, Ordering::Relaxed);
            }
            if sx == 0 || sy == 0 || sx == prep.width - 1 || sy == prep.height - 1 {
                edge.store(true, Ordering::Relaxed);
            }
            [sx as u8, sy as u8, 60, 255]
        };
        for y in (1..299).step_by(3) {
            let _ = (spec.render_cpu)(&sample, 255, y, &prep, &params);
        }
        assert!(across.load(Ordering::Relaxed), "{id} missed seam");
        for x in [0, prep.width - 1] {
            let _ = (spec.render_cpu)(&sample, x, 0, &prep, &params);
        }
        assert!(edge.load(Ordering::Relaxed), "{id} missed image edge");
    }
}
