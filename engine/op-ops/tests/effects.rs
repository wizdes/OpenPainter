use op_core::doc::{DocState, MaskGrid, PolygonSet, SelectionSource, SelectionState};
use op_core::tiles::{TILE_BYTES, TILE_SIZE, TileCoord, TileData, TileStore};
use op_ops::effect::{self, Rect};
use std::sync::Arc;

struct MemorySpill(std::sync::Mutex<Vec<u8>>);
impl op_core::tiles::SpillIo for MemorySpill {
    fn append(&self, bytes: &[u8]) -> std::io::Result<u64> {
        let mut data = self.0.lock().unwrap();
        let offset = data.len() as u64;
        data.extend_from_slice(bytes);
        Ok(offset)
    }
    fn read_exact_at(&self, offset: u64, bytes: &mut [u8]) -> std::io::Result<()> {
        let data = self.0.lock().unwrap();
        bytes.copy_from_slice(&data[offset as usize..offset as usize + bytes.len()]);
        Ok(())
    }
}
fn store() -> Arc<TileStore> {
    Arc::new(TileStore::with_io(
        64 * TILE_BYTES,
        Box::new(MemorySpill(std::sync::Mutex::new(Vec::new()))),
    ))
}

fn document(width: u32, height: u32) -> DocState {
    let mut doc = DocState::new(store(), width, height, 96.0).unwrap();
    for y in 0..height.div_ceil(TILE_SIZE as u32) {
        for x in 0..width.div_ceil(TILE_SIZE as u32) {
            let mut bytes = vec![0; TILE_BYTES];
            for py in 0..TILE_SIZE {
                for px in 0..TILE_SIZE {
                    let gx = x as usize * TILE_SIZE + px;
                    let gy = y as usize * TILE_SIZE + py;
                    if gx < width as usize && gy < height as usize {
                        let i = (py * TILE_SIZE + px) * 4;
                        bytes[i..i + 4].copy_from_slice(&[
                            gx as u8,
                            gy as u8,
                            91,
                            ((gx + gy) % 256) as u8,
                        ]);
                    }
                }
            }
            doc = doc
                .write_tile(
                    0,
                    TileCoord {
                        x: x as i32,
                        y: y as i32,
                    },
                    TileData::try_from(bytes).unwrap(),
                )
                .unwrap();
        }
    }
    doc
}

#[test]
fn m25_adjustments_are_registered_in_menu_order() {
    let ids: Vec<_> = effect::registered()
        .into_iter()
        .map(|spec| spec.id)
        .collect();
    let expected = [
        "effect.adjust.auto_level",
        "effect.adjust.black_and_white",
        "effect.adjust.brightness_contrast",
        "effect.adjust.hue_saturation",
        "effect.adjust.posterize",
        "effect.adjust.sepia",
        "effect.adjust.temperature_tint",
    ];
    let positions: Vec<_> = expected
        .iter()
        .map(|id| ids.iter().position(|v| v == id).expect(id))
        .collect();
    assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));
}

#[test]
fn m26_adjustments_are_registered_in_menu_order() {
    let ids: Vec<_> = effect::registered()
        .into_iter()
        .map(|spec| spec.id)
        .collect();
    let expected = [
        "effect.adjust.curves",
        "effect.adjust.exposure",
        "effect.adjust.highlights_shadows",
        "effect.adjust.levels",
    ];
    let positions: Vec<_> = expected
        .iter()
        .map(|id| ids.iter().position(|v| v == id).expect(id))
        .collect();
    assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));
}

#[test]
fn m26_levels_auto_matches_auto_level_and_tile_order() {
    let doc = document(257, 3);
    let rect = Rect::new(0, 0, 257, 3);
    let levels = effect::find("effect.adjust.levels").unwrap();
    let params = serde_json::json!({"Auto": true});
    let whole = effect::render(&doc, &levels, rect, Some(params.clone())).unwrap();
    let mut coords = effect::tile_coords(rect);
    coords.reverse();
    let parallel = effect::render_tiles(&doc, &levels, rect, &coords, true, Some(params)).unwrap();
    let auto = effect::render(
        &doc,
        &effect::find("effect.adjust.auto_level").unwrap(),
        rect,
        None,
    )
    .unwrap();
    for coord in effect::tile_coords(rect) {
        assert_eq!(
            whole.read_tile(0, coord).unwrap(),
            parallel.read_tile(0, coord).unwrap()
        );
        assert_eq!(
            whole.read_tile(0, coord).unwrap(),
            auto.read_tile(0, coord).unwrap()
        );
    }
}

#[test]
fn m26_parameterized_curves_tiles_match_parallel_and_reverse_order() {
    let doc = document(257, 3);
    let rect = Rect::new(0, 0, 257, 3);
    let spec = effect::find("effect.adjust.curves").unwrap();
    let params = serde_json::json!({"ColorTransferMode":"Rgb", "ControlPoints":[
        [[0,0],[96,150],[255,255]], [[0,0],[128,90],[255,255]], [[0,0],[192,120],[255,255]]
    ]});
    let whole = effect::render(&doc, &spec, rect, Some(params.clone())).unwrap();
    let mut coords = effect::tile_coords(rect);
    coords.reverse();
    let parallel = effect::render_tiles(&doc, &spec, rect, &coords, true, Some(params)).unwrap();
    for coord in effect::tile_coords(rect) {
        assert_eq!(
            whole.read_tile(0, coord).unwrap(),
            parallel.read_tile(0, coord).unwrap()
        );
    }
}

#[test]
fn m26_curve_levels_and_own_effect_endpoints_keep_alpha_and_hidden_rgb() {
    let doc = document(2, 1);
    let rect = Rect::new(0, 0, 2, 1);
    for (id, params) in [
        (
            "effect.adjust.curves",
            serde_json::json!({"ColorTransferMode":"Rgb", "ControlPoints":[[[0,255],[255,0]],[[0,0],[255,255]],[[0,0],[255,255]]]}),
        ),
        (
            "effect.adjust.levels",
            serde_json::json!({"InputBlack":[0,0,0],"InputWhite":[255,255,255],"Gamma":[1.0,1.0,2.0],"OutputBlack":[0,0,0],"OutputWhite":[255,255,255]}),
        ),
        (
            "effect.adjust.exposure",
            serde_json::json!({"Exposure":5.0}),
        ),
        (
            "effect.adjust.highlights_shadows",
            serde_json::json!({"Highlights":100,"Shadows":-100}),
        ),
    ] {
        let spec = effect::find(id).unwrap();
        let result = effect::render(&doc, &spec, rect, Some(params)).unwrap();
        let tile = result.read_tile(0, TileCoord { x: 0, y: 0 }).unwrap();
        assert_eq!(tile[3], 0, "{id}");
        assert_ne!(&tile[..3], &[0, 0, 91], "{id}");
    }
}

#[test]
fn m26_curves_soft_mask_and_geometry_clip_output() {
    let doc = document(3, 1);
    let params = serde_json::json!({"ColorTransferMode":"Rgb", "ControlPoints":[[[0,255],[255,0]],[[0,0],[255,255]],[[0,0],[255,255]]]});
    let spec = effect::find("effect.adjust.curves").unwrap();
    let rect = Rect::new(0, 0, 3, 1);
    let full = effect::render(&doc, &spec, rect, Some(params.clone())).unwrap();
    let full_bytes = full.read_tile(0, TileCoord { x: 0, y: 0 }).unwrap();
    let soft = doc
        .clone()
        .set_selection(SelectionState::Active(SelectionSource::Coverage(Arc::new(
            MaskGrid {
                width: 3,
                height: 1,
                bytes: Arc::from([0u8, 128, 255]),
            },
        ))));
    let result = effect::render(&soft, &spec, rect, Some(params.clone())).unwrap();
    let bytes = result.read_tile(0, TileCoord { x: 0, y: 0 }).unwrap();
    assert_eq!(&bytes[..4], &[0, 0, 91, 0]);
    assert_eq!(&bytes[8..12], &full_bytes[8..12]);
    assert_eq!(bytes[4], effect::coverage_lerp(1, full_bytes[4], 128));
    let geometry = doc.set_selection(SelectionState::Active(SelectionSource::Geometry(Arc::new(
        PolygonSet(vec![vec![(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)]]),
    ))));
    let result = effect::render(&geometry, &spec, rect, Some(params)).unwrap();
    let bytes = result.read_tile(0, TileCoord { x: 0, y: 0 }).unwrap();
    assert_eq!(&bytes[..4], &full_bytes[..4]);
    assert_eq!(&bytes[4..8], &[1, 0, 91, 1]);
}

#[test]
fn m26_identity_gpu_reference_accepts_empty_preparation() {
    for id in ["effect.adjust.curves", "effect.adjust.exposure"] {
        let spec = effect::find(id).unwrap();
        let source = [17, 93, 211, 0];
        let actual = (spec.render_cpu)(
            &|_, _| source,
            0,
            0,
            &effect::Prepared::default(),
            &spec.default_params(),
        );
        assert_eq!(actual, source, "{id}");
    }
}

#[test]
fn m26_own_defaults_keep_all_five_oracle_inputs() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../");
    let manifest: serde_json::Value = serde_json::from_slice(
        &std::fs::read(root.join("tests/oracle/legacy336/effects/manifest.json")).unwrap(),
    )
    .unwrap();
    let cases: Vec<_> = manifest
        .as_array()
        .unwrap()
        .iter()
        .filter(|entry| entry["op"] == "Curves" && entry["parameters"] == serde_json::json!({}))
        .collect();
    assert_eq!(cases.len(), 5);
    let doc = document(1, 1);
    for id in ["effect.adjust.exposure", "effect.adjust.highlights_shadows"] {
        let spec = effect::find(id).unwrap();
        let params = spec.default_params();
        let prepared = (spec.prepare)(&doc, &SelectionState::None, &params).unwrap();
        for case in &cases {
            let input = op_formats::png::decode_png(
                &std::fs::read(root.join(case["input"].as_str().unwrap())).unwrap(),
            )
            .unwrap();
            for pixel in input.pixels.chunks_exact(4) {
                let source = [pixel[0], pixel[1], pixel[2], pixel[3]];
                let actual = (spec.render_cpu)(&|_, _| source, 0, 0, &prepared, &params);
                assert_eq!(actual, source, "{id} {}", case["input"]);
            }
        }
    }
}

#[test]
fn m26_own_endpoint_variants_cover_all_five_oracle_inputs() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../");
    let manifest: serde_json::Value = serde_json::from_slice(
        &std::fs::read(root.join("tests/oracle/legacy336/effects/manifest.json")).unwrap(),
    )
    .unwrap();
    let cases: Vec<_> = manifest
        .as_array()
        .unwrap()
        .iter()
        .filter(|entry| entry["op"] == "Curves" && entry["parameters"] == serde_json::json!({}))
        .collect();
    assert_eq!(cases.len(), 5);
    let doc = document(1, 1);
    for (id, field, endpoints) in [
        ("effect.adjust.exposure", "Exposure", [-5.0, 5.0]),
        (
            "effect.adjust.highlights_shadows",
            "Highlights",
            [-100.0, 100.0],
        ),
        (
            "effect.adjust.highlights_shadows",
            "Shadows",
            [-100.0, 100.0],
        ),
    ] {
        let spec = effect::find(id).unwrap();
        for endpoint in endpoints {
            let mut params = spec.default_params();
            let value = if id == "effect.adjust.exposure" {
                serde_json::json!(endpoint)
            } else {
                serde_json::json!(endpoint as i64)
            };
            params.as_object_mut().unwrap().insert(field.into(), value);
            let prepared = (spec.prepare)(&doc, &SelectionState::None, &params).unwrap();
            let mut changed = false;
            for case in &cases {
                let input = op_formats::png::decode_png(
                    &std::fs::read(root.join(case["input"].as_str().unwrap())).unwrap(),
                )
                .unwrap();
                for pixel in input.pixels.chunks_exact(4) {
                    let source = [pixel[0], pixel[1], pixel[2], pixel[3]];
                    let actual = (spec.render_cpu)(&|_, _| source, 0, 0, &prepared, &params);
                    assert_eq!(actual[3], source[3], "{id} {field} {endpoint}");
                    for channel in 0..3 {
                        if endpoint > 0.0 {
                            assert!(actual[channel] >= source[channel]);
                        } else {
                            assert!(actual[channel] <= source[channel]);
                        }
                        changed |= actual[channel] != source[channel];
                    }
                }
            }
            assert!(changed, "{id} {field} {endpoint}");
        }
    }
}

#[test]
fn m26_all_adjustments_clip_nondefault_output_to_soft_and_geometry_selections() {
    let doc = document(3, 1);
    let rect = Rect::new(0, 0, 3, 1);
    let soft = doc
        .clone()
        .set_selection(SelectionState::Active(SelectionSource::Coverage(Arc::new(
            MaskGrid {
                width: 3,
                height: 1,
                bytes: Arc::from([0u8, 128, 255]),
            },
        ))));
    let geometry = doc
        .clone()
        .set_selection(SelectionState::Active(SelectionSource::Geometry(Arc::new(
            PolygonSet(vec![vec![(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)]]),
        ))));
    for (id, params) in [
        (
            "effect.adjust.curves",
            serde_json::json!({"ColorTransferMode":"Rgb", "ControlPoints":[[[0,255],[255,0]],[[0,0],[255,255]],[[0,0],[255,255]]]}),
        ),
        (
            "effect.adjust.levels",
            serde_json::json!({"Gamma":[1.0,1.0,2.0]}),
        ),
        (
            "effect.adjust.exposure",
            serde_json::json!({"Exposure":2.0}),
        ),
        (
            "effect.adjust.highlights_shadows",
            serde_json::json!({"Shadows":100}),
        ),
    ] {
        let spec = effect::find(id).unwrap();
        let full = effect::render(&doc, &spec, rect, Some(params.clone())).unwrap();
        let whole = full.read_tile(0, TileCoord { x: 0, y: 0 }).unwrap();
        let selected = effect::render(&soft, &spec, rect, Some(params.clone())).unwrap();
        let bytes = selected.read_tile(0, TileCoord { x: 0, y: 0 }).unwrap();
        assert_eq!(&bytes[..4], &[0, 0, 91, 0], "{id}");
        assert_eq!(&bytes[8..12], &whole[8..12], "{id}");
        for channel in 0..4 {
            assert_eq!(
                bytes[4 + channel],
                effect::coverage_lerp([1, 0, 91, 1][channel], whole[4 + channel], 128),
                "{id}"
            );
        }
        let selected = effect::render(&geometry, &spec, rect, Some(params)).unwrap();
        let bytes = selected.read_tile(0, TileCoord { x: 0, y: 0 }).unwrap();
        assert_eq!(&bytes[..4], &whole[..4], "{id}");
        assert_eq!(&bytes[4..8], &[1, 0, 91, 1], "{id}");
    }
}

#[test]
fn m25_profile_and_hidden_colors_do_not_change_the_result() {
    let doc = document(2, 1);
    let with_profile = doc.set_profile(Some(Arc::from([1u8, 2, 3])));
    for spec in effect::registered().into_iter().filter(|spec| {
        spec.id != "effect.adjust.invert_alpha" && spec.id != "effect.fill.content_aware"
    }) {
        let rect = Rect::new(0, 0, 2, 1);
        let original = effect::render(&doc, &spec, rect, None).unwrap();
        let profiled = effect::render(&with_profile, &spec, rect, None).unwrap();
        assert_eq!(
            original.read_tile(0, TileCoord { x: 0, y: 0 }).unwrap(),
            profiled.read_tile(0, TileCoord { x: 0, y: 0 }).unwrap(),
            "{}",
            spec.id
        );
        assert_eq!(
            original.read_tile(0, TileCoord { x: 0, y: 0 }).unwrap()[3],
            0,
            "{}",
            spec.id
        );
    }
}

#[test]
fn temperature_tint_identity_endpoints_and_soft_mask() {
    let spec = effect::find("effect.adjust.temperature_tint").unwrap();
    let doc = document(2, 1);
    let rect = Rect::new(0, 0, 2, 1);
    let identity = effect::render(&doc, &spec, rect, None).unwrap();
    assert_eq!(
        identity.read_tile(0, TileCoord { x: 0, y: 0 }).unwrap(),
        doc.read_tile(0, TileCoord { x: 0, y: 0 }).unwrap()
    );
    let warm = effect::render(
        &doc,
        &spec,
        rect,
        Some(serde_json::json!({"Temperature":100,"Tint":-100})),
    )
    .unwrap();
    assert_eq!(
        &warm.read_tile(0, TileCoord { x: 0, y: 0 }).unwrap()[..4],
        &[0, 150, 0, 0]
    );
    let selected = doc.set_selection(SelectionState::Active(SelectionSource::Coverage(Arc::new(
        MaskGrid {
            width: 2,
            height: 1,
            bytes: Arc::from([0u8, 128]),
        },
    ))));
    let tinted = effect::render(
        &selected,
        &spec,
        rect,
        Some(serde_json::json!({"Tint":100})),
    )
    .unwrap();
    let output = tinted.read_tile(0, TileCoord { x: 0, y: 0 }).unwrap();
    assert_eq!(&output[..4], &[0, 0, 91, 0]);
    assert_eq!(&output[4..8], &[26, 0, 116, 1]);
}

#[test]
fn auto_level_histogram_uses_selected_pixels() {
    let mut doc = document(3, 1);
    let mut tile = doc.read_tile(0, TileCoord { x: 0, y: 0 }).unwrap();
    for (x, value) in [10u8, 20, 200].into_iter().enumerate() {
        tile[x * 4..x * 4 + 4].copy_from_slice(&[value, value, value, 255]);
    }
    doc = doc
        .write_tile(
            0,
            TileCoord { x: 0, y: 0 },
            TileData::try_from(tile).unwrap(),
        )
        .unwrap();
    let selected = doc.set_selection(SelectionState::Active(SelectionSource::Coverage(Arc::new(
        MaskGrid {
            width: 3,
            height: 1,
            bytes: Arc::from([255u8, 128, 0]),
        },
    ))));
    let spec = effect::find("effect.adjust.auto_level").unwrap();
    let output = effect::render(&selected, &spec, Rect::new(0, 0, 3, 1), None).unwrap();
    let result = output.read_tile(0, TileCoord { x: 0, y: 0 }).unwrap();
    assert_eq!(&result[0..4], &[0, 0, 0, 255]);
    assert_eq!(&result[4..8], &[138, 138, 138, 255]);
    assert_eq!(&result[8..12], &[200, 200, 200, 255]);
}

#[test]
fn posterize_links_channels_and_preserves_hidden_alpha() {
    let mut doc = document(1, 1);
    let mut tile = doc.read_tile(0, TileCoord { x: 0, y: 0 }).unwrap();
    tile[..4].copy_from_slice(&[30, 100, 220, 0]);
    doc = doc
        .write_tile(
            0,
            TileCoord { x: 0, y: 0 },
            TileData::try_from(tile).unwrap(),
        )
        .unwrap();
    let spec = effect::find("effect.adjust.posterize").unwrap();
    let rect = Rect::new(0, 0, 1, 1);
    let linked = effect::render(
        &doc,
        &spec,
        rect,
        Some(serde_json::json!({"RedLevels": 2, "GreenLevels": 64, "BlueLevels": 64})),
    )
    .unwrap();
    assert_eq!(
        &linked.read_tile(0, TileCoord { x: 0, y: 0 }).unwrap()[..4],
        &[0, 0, 255, 0]
    );
    let separate = effect::render(&doc, &spec, rect, Some(serde_json::json!({"RedLevels": 2, "GreenLevels": 64, "BlueLevels": 64, "LinkLevels": false}))).unwrap();
    assert_eq!(
        &separate.read_tile(0, TileCoord { x: 0, y: 0 }).unwrap()[..4],
        &[0, 101, 222, 0]
    );
}

#[test]
fn black_and_white_obeys_geometry_selection() {
    let doc =
        document(2, 1).set_selection(SelectionState::Active(SelectionSource::Geometry(Arc::new(
            PolygonSet(vec![vec![(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)]]),
        ))));
    let spec = effect::find("effect.adjust.black_and_white").unwrap();
    let output = effect::render(&doc, &spec, Rect::new(0, 0, 2, 1), None).unwrap();
    let result = output.read_tile(0, TileCoord { x: 0, y: 0 }).unwrap();
    assert_eq!(&result[..4], &[10, 10, 10, 0]);
    assert_eq!(&result[4..8], &[1, 0, 91, 1]);
}

#[test]
fn selection_coverage_and_empty() {
    let doc = document(3, 1);
    let selected = doc.set_selection(SelectionState::Active(SelectionSource::Coverage(Arc::new(
        MaskGrid {
            width: 3,
            height: 1,
            bytes: Arc::from([0u8, 128, 255]),
        },
    ))));
    let spec = effect::find("effect.adjust.invert_colors").unwrap();
    let out = effect::render(&selected, &spec, Rect::new(0, 0, 3, 1), None).unwrap();
    let bytes = out.read_tile(0, TileCoord { x: 0, y: 0 }).unwrap();
    assert_eq!(&bytes[0..4], &[0, 0, 91, 0]);
    assert_eq!(&bytes[4..8], &[128, 128, 128, 1]);
    assert_eq!(&bytes[8..12], &[253, 255, 164, 2]);
    let empty = doc.set_selection(SelectionState::Active(SelectionSource::Coverage(Arc::new(
        MaskGrid {
            width: 3,
            height: 1,
            bytes: Arc::from([0u8, 0, 0]),
        },
    ))));
    let unchanged = effect::render(&empty, &spec, Rect::new(0, 0, 3, 1), None).unwrap();
    assert_eq!(
        unchanged.read_tile(0, TileCoord { x: 0, y: 0 }).unwrap(),
        doc.read_tile(0, TileCoord { x: 0, y: 0 }).unwrap()
    );
}

#[test]
fn neighborhood_reads_outside_selection() {
    let mut doc = document(2, 1);
    let mut tile = doc.read_tile(0, TileCoord { x: 0, y: 0 }).unwrap();
    tile[0] = 200;
    tile[4] = 0;
    doc = doc
        .write_tile(
            0,
            TileCoord { x: 0, y: 0 },
            TileData::try_from(tile).unwrap(),
        )
        .unwrap();
    doc = doc.set_selection(SelectionState::Active(SelectionSource::Coverage(Arc::new(
        MaskGrid {
            width: 2,
            height: 1,
            bytes: Arc::from([0u8, 255]),
        },
    ))));
    let mut spec = effect::find("effect.adjust.invert_colors").unwrap();
    spec.required_input =
        |output, _| Rect::new(output.x - 1, output.y, output.width + 1, output.height);
    spec.passes = &[effect::PassKind::Neighborhood];
    spec.render_cpu = |sample, x, y, _, _| sample(x - 1, y);
    let out = effect::render(&doc, &spec, Rect::new(0, 0, 2, 1), None).unwrap();
    let bytes = out.read_tile(0, TileCoord { x: 0, y: 0 }).unwrap();
    assert_eq!(bytes[0], 200);
    assert_eq!(bytes[4], 200);
}

#[test]
fn neighborhood_clamp_border_reads_edge_pixel() {
    let mut doc = document(1, 1);
    let mut tile = doc.read_tile(0, TileCoord { x: 0, y: 0 }).unwrap();
    tile[0] = 200;
    doc = doc
        .write_tile(
            0,
            TileCoord { x: 0, y: 0 },
            TileData::try_from(tile).unwrap(),
        )
        .unwrap();
    let mut spec = effect::find("effect.adjust.invert_colors").unwrap();
    spec.required_input =
        |output, _| Rect::new(output.x - 1, output.y, output.width + 1, output.height);
    spec.passes = &[effect::PassKind::Neighborhood];
    spec.render_cpu = |sample, x, y, _, _| sample(x - 1, y);
    let output = effect::render(&doc, &spec, Rect::new(0, 0, 1, 1), None).unwrap();
    assert_eq!(
        output.read_tile(0, TileCoord { x: 0, y: 0 }).unwrap()[0],
        200
    );
}

#[test]
fn geometry_selection_uses_pixel_centers() {
    let doc =
        document(2, 1).set_selection(SelectionState::Active(SelectionSource::Geometry(Arc::new(
            PolygonSet(vec![vec![(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)]]),
        ))));
    let spec = effect::find("effect.adjust.invert_alpha").unwrap();
    let out = effect::render(&doc, &spec, Rect::new(0, 0, 2, 1), None).unwrap();
    let bytes = out.read_tile(0, TileCoord { x: 0, y: 0 }).unwrap();
    assert_eq!(&bytes[..4], &[0, 0, 91, 255]);
    assert_eq!(&bytes[4..8], &[1, 0, 91, 1]);
}

#[test]
fn geometry_selection_uses_antialiased_area_coverage() {
    // A rectangle covering the left half of pixel 0: the selection engine's
    // analytic coverage is 0.5, so Invert Alpha moves alpha 0 halfway (128).
    let doc =
        document(2, 1).set_selection(SelectionState::Active(SelectionSource::Geometry(Arc::new(
            PolygonSet(vec![vec![(0.0, 0.0), (0.5, 0.0), (0.5, 1.0), (0.0, 1.0)]]),
        ))));
    let spec = effect::find("effect.adjust.invert_alpha").unwrap();
    let out = effect::render(&doc, &spec, Rect::new(0, 0, 2, 1), None).unwrap();
    let bytes = out.read_tile(0, TileCoord { x: 0, y: 0 }).unwrap();
    assert_eq!(bytes[3], 128);
    assert_eq!(&bytes[4..8], &[1, 0, 91, 1]);
}

#[test]
fn tile_order_and_parallel_match_whole_image() {
    let doc = document(1000, 700);
    let rect = Rect::new(0, 0, 1000, 700);
    for spec in effect::registered()
        .into_iter()
<<<<<<< HEAD
        .filter(|spec| spec.id != "effect.fill.content_aware")
=======
        .filter(|spec| spec.category == "Adjustments")
>>>>>>> m35
    {
        let baseline = effect::render(&doc, &spec, rect, None).unwrap();
        // This whole-image reference computes from canvas coordinates, without
        // reading any output tile while making the expected pixels.
        for coord in effect::tile_coords(rect) {
            let tile = baseline.read_tile(0, coord).unwrap();
            for py in 0..TILE_SIZE {
                for px in 0..TILE_SIZE {
                    let x = coord.x as usize * TILE_SIZE + px;
                    let y = coord.y as usize * TILE_SIZE + py;
                    if x >= 1000 || y >= 700 {
                        continue;
                    }
                    let original = [x as u8, y as u8, 91, ((x + y) % 256) as u8];
                    let expected = if spec.id == "effect.adjust.invert_colors" {
                        [255 - original[0], 255 - original[1], 164, original[3]]
                    } else if spec.id == "effect.adjust.invert_alpha" {
                        [original[0], original[1], original[2], 255 - original[3]]
                    } else {
                        continue;
                    };
                    let i = (py * TILE_SIZE + px) * 4;
                    assert_eq!(tile[i..i + 4], expected, "{} at {x},{y}", spec.id);
                }
            }
        }
        let mut coords = effect::tile_coords(rect);
        coords.reverse();
        let reverse = effect::render_tiles(&doc, &spec, rect, &coords, false, None).unwrap();
        let parallel = effect::render_tiles(&doc, &spec, rect, &coords, true, None).unwrap();
        for coord in effect::tile_coords(rect) {
            let expected = baseline.read_tile(0, coord).unwrap();
            assert_eq!(
                reverse.read_tile(0, coord).unwrap(),
                expected,
                "{} {coord:?}",
                spec.id
            );
            assert_eq!(
                parallel.read_tile(0, coord).unwrap(),
                expected,
                "{} {coord:?}",
                spec.id
            );
        }
    }
}

#[test]
fn invert_alpha_keeps_hidden_color_and_is_involution() {
    let doc = document(1, 1);
    let spec = effect::find("effect.adjust.invert_alpha").unwrap();
    let once = effect::render(&doc, &spec, Rect::new(0, 0, 1, 1), None).unwrap();
    assert_eq!(
        &once.read_tile(0, TileCoord { x: 0, y: 0 }).unwrap()[..4],
        &[0, 0, 91, 255]
    );
    let twice = effect::render(&once, &spec, Rect::new(0, 0, 1, 1), None).unwrap();
    assert_eq!(
        twice.read_tile(0, TileCoord { x: 0, y: 0 }).unwrap(),
        doc.read_tile(0, TileCoord { x: 0, y: 0 }).unwrap()
    );
}

#[test]
fn effect_render_cpu_accepts_tile_inputs_and_returns_tile_bytes() {
    let spec = effect::find("effect.adjust.invert_alpha").unwrap();
    let sample = |_: i32, _: i32| [17, 29, 41, 0];
    let input = effect::TileInputs {
        output_rect: Rect::new(0, 0, TILE_SIZE as u32, TILE_SIZE as u32),
        sample: &sample,
    };
    let bytes = effect::EffectSpec::render_cpu(
        &spec,
        &input,
        &effect::Prepared::default(),
        &serde_json::json!({}),
    );
    assert_eq!(bytes.len(), TILE_BYTES);
    assert_eq!(&bytes[..4], &[17, 29, 41, 255]);
}

#[test]
fn registry_matches_target_rows() {
    let rows = include_str!("../../../tests/oracle/targets.toml");
    for spec in effect::registered() {
        assert!(
            rows.contains(&format!("id = \"{}\"", spec.id)),
            "missing {}",
            spec.id
        );
    }
    let ids: Vec<_> = rows
        .split("[[effect]]")
        .skip(1)
        .filter_map(|section| {
            section.lines().find_map(|line| {
                line.strip_prefix("id = \"")
                    .and_then(|s| s.strip_suffix('"'))
            })
        })
        .collect();
    assert_eq!(ids.len(), effect::registered().len());
    for id in ids {
        assert!(effect::find(id).is_some(), "unknown target row {id}");
    }
}

#[test]
fn gaussian_radius_zero_keeps_hidden_color() {
    let doc = document(3, 1);
    let spec = effect::find("effect.blur.gaussian").expect("Gaussian Blur registered");
    let out = effect::render(
        &doc,
        &spec,
        Rect::new(0, 0, 3, 1),
        Some(serde_json::json!({"radius":0})),
    )
    .unwrap();
    assert_eq!(
        out.read_tile(0, TileCoord { x: 0, y: 0 }).unwrap(),
        doc.read_tile(0, TileCoord { x: 0, y: 0 }).unwrap()
    );
}

#[test]
fn invert_colors_matches_all_legacy_pngs() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../");
    let manifest: serde_json::Value = serde_json::from_slice(
        &std::fs::read(root.join("tests/oracle/legacy336/effects/manifest.json")).unwrap(),
    )
    .unwrap();
    let cases: Vec<_> = manifest
        .as_array()
        .unwrap()
        .iter()
        .filter(|entry| entry["op"] == "Invert Colors")
        .collect();
    assert_eq!(cases.len(), 5);
    let spec = effect::find("effect.adjust.invert_colors").unwrap();
    for case in cases {
        let input = op_formats::png::decode_png(
            &std::fs::read(root.join(case["input"].as_str().unwrap())).unwrap(),
        )
        .unwrap();
        let expected = op_formats::png::decode_png(
            &std::fs::read(root.join(case["output"].as_str().unwrap())).unwrap(),
        )
        .unwrap();
        let mut doc = DocState::new(store(), input.width, input.height, 96.0).unwrap();
        for coord in effect::tile_coords(Rect::new(0, 0, input.width, input.height)) {
            let mut tile = vec![0; TILE_BYTES];
            for py in 0..TILE_SIZE {
                let y = coord.y as usize * TILE_SIZE + py;
                if y >= input.height as usize {
                    break;
                }
                for px in 0..TILE_SIZE {
                    let x = coord.x as usize * TILE_SIZE + px;
                    if x >= input.width as usize {
                        break;
                    }
                    let src = (y * input.width as usize + x) * 4;
                    let dst = (py * TILE_SIZE + px) * 4;
                    tile[dst..dst + 4].copy_from_slice(&input.pixels[src..src + 4]);
                }
            }
            doc = doc
                .write_tile(0, coord, TileData::try_from(tile).unwrap())
                .unwrap();
        }
        let output = effect::render(
            &doc,
            &spec,
            Rect::new(0, 0, input.width, input.height),
            None,
        )
        .unwrap();
        let tiles: std::collections::HashMap<_, _> =
            effect::tile_coords(Rect::new(0, 0, input.width, input.height))
                .into_iter()
                .map(|coord| (coord, output.read_tile(0, coord).unwrap()))
                .collect();
        let mut actual = Vec::with_capacity(input.pixels.len());
        for y in 0..input.height as usize {
            for x in 0..input.width as usize {
                let coord = TileCoord {
                    x: (x / TILE_SIZE) as i32,
                    y: (y / TILE_SIZE) as i32,
                };
                let tile = &tiles[&coord];
                let i = ((y % TILE_SIZE) * TILE_SIZE + x % TILE_SIZE) * 4;
                actual.extend_from_slice(&tile[i..i + 4]);
            }
        }
        assert_eq!(actual, expected.pixels, "{}", case["output"]);
    }
}

#[test]
<<<<<<< HEAD
fn m25_pointwise_effects_match_legacy_pngs() {
=======
fn blur_effects_match_legacy_oracles() {
>>>>>>> m35
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../");
    let manifest: serde_json::Value = serde_json::from_slice(
        &std::fs::read(root.join("tests/oracle/legacy336/effects/manifest.json")).unwrap(),
    )
    .unwrap();
<<<<<<< HEAD
    for (name, id, count, tolerance) in [
        ("Desaturate", "effect.adjust.black_and_white", 5, 0i16),
        (
            "Brightness and Contrast",
            "effect.adjust.brightness_contrast",
            35,
            0,
        ),
        ("Posterize", "effect.adjust.posterize", 50, 0),
        ("Sepia", "effect.adjust.sepia", 5, 1),
        ("Hue and Saturation", "effect.adjust.hue_saturation", 50, 0),
        ("Auto-Level", "effect.adjust.auto_level", 5, 1),
        ("Levels", "effect.adjust.levels", 20, 1),
        ("Curves", "effect.adjust.curves", 20, 0),
=======
    for (name, id) in [
        ("Fragment Blur", "effect.blur.fragment"),
        ("Gaussian Blur", "effect.blur.gaussian"),
        ("Median Blur", "effect.blur.median"),
        ("Motion Blur", "effect.blur.motion"),
        ("Radial Blur", "effect.blur.radial"),
        ("Zoom Blur", "effect.blur.zoom"),
>>>>>>> m35
    ] {
        let cases: Vec<_> = manifest
            .as_array()
            .unwrap()
            .iter()
<<<<<<< HEAD
            .filter(|entry| entry["op"] == name)
            .collect();
        assert_eq!(cases.len(), count, "{name}");
=======
            .filter(|entry| {
                entry["op"] == name && entry["output"].as_str().is_some_and(|s| !s.is_empty())
            })
            .collect();
        assert!(cases.len() >= 5, "{name}");
>>>>>>> m35
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
<<<<<<< HEAD
            let mut doc = DocState::new(store(), input.width, input.height, 96.0).unwrap();
            let rect = Rect::new(0, 0, input.width, input.height);
=======
            let rect = Rect::new(0, 0, input.width, input.height);
            let mut doc = DocState::new(store(), input.width, input.height, 96.0).unwrap();
>>>>>>> m35
            for coord in effect::tile_coords(rect) {
                let mut tile = vec![0; TILE_BYTES];
                for py in 0..TILE_SIZE {
                    let y = coord.y as usize * TILE_SIZE + py;
                    if y >= input.height as usize {
                        break;
                    }
                    for px in 0..TILE_SIZE {
                        let x = coord.x as usize * TILE_SIZE + px;
                        if x >= input.width as usize {
                            break;
                        }
                        let src = (y * input.width as usize + x) * 4;
                        let dst = (py * TILE_SIZE + px) * 4;
                        tile[dst..dst + 4].copy_from_slice(&input.pixels[src..src + 4]);
                    }
                }
                doc = doc
                    .write_tile(0, coord, TileData::try_from(tile).unwrap())
                    .unwrap();
            }
<<<<<<< HEAD
            let mut params = serde_json::Map::new();
            for (source, target) in [
                ("brightness", "Brightness"),
                ("contrast", "Contrast"),
                ("red", "RedLevels"),
                ("green", "GreenLevels"),
                ("blue", "BlueLevels"),
                ("hue", "Hue"),
                ("saturation", "Saturation"),
                ("lightness", "Lightness"),
            ] {
                if let Some(value) = case["parameters"].get(source) {
                    params.insert(target.into(), value.clone());
                }
            }
            if name == "Posterize" {
                params.insert("LinkLevels".into(), serde_json::json!(false));
            }
            if name == "Levels"
                && let Some(gamma) = case["parameters"].get("gamma")
            {
                params.insert("Gamma".into(), serde_json::json!([gamma, gamma, gamma]));
            }
            if name == "Curves" {
                if let Some(mode) = case["parameters"].get("mode") {
                    params.insert("ColorTransferMode".into(), mode.clone());
                }
                if let Some(points) = case["parameters"].get("points") {
                    params.insert("ControlPoints".into(), points.clone());
                }
            }
            let output =
                effect::render(&doc, &spec, rect, Some(serde_json::Value::Object(params))).unwrap();
            let tiles: std::collections::HashMap<_, _> = effect::tile_coords(rect)
                .into_iter()
                .map(|coord| (coord, output.read_tile(0, coord).unwrap()))
                .collect();
            for y in 0..input.height as usize {
                for x in 0..input.width as usize {
                    let tile = &tiles[&TileCoord {
                        x: (x / TILE_SIZE) as i32,
                        y: (y / TILE_SIZE) as i32,
                    }];
                    let i = ((y % TILE_SIZE) * TILE_SIZE + x % TILE_SIZE) * 4;
                    let j = (y * input.width as usize + x) * 4;
                    for channel in 0..4 {
                        let diff =
                            i16::from(tile[i + channel]) - i16::from(expected.pixels[j + channel]);
                        assert!(
                            diff.abs() <= tolerance,
                            "{} at {x},{y} channel {channel}: {} vs {}",
=======
            let output =
                effect::render(&doc, &spec, rect, Some(case["parameters"].clone())).unwrap();
            for y in 0..input.height as usize {
                for x in 0..input.width as usize {
                    let coord = TileCoord {
                        x: (x / TILE_SIZE) as i32,
                        y: (y / TILE_SIZE) as i32,
                    };
                    let tile = output.read_tile(0, coord).unwrap();
                    let i = ((y % TILE_SIZE) * TILE_SIZE + x % TILE_SIZE) * 4;
                    let j = (y * input.width as usize + x) * 4;
                    for channel in 0..4 {
                        let tolerance = if name == "Motion Blur" { 1 } else { 0 };
                        assert!(
                            tile[i + channel].abs_diff(expected.pixels[j + channel]) <= tolerance,
                            "{name} {} {x},{y} channel {channel}: {} vs {}",
>>>>>>> m35
                            case["output"],
                            tile[i + channel],
                            expected.pixels[j + channel]
                        );
                    }
                }
            }
        }
    }
}
<<<<<<< HEAD
=======

#[test]
fn blur_tiles_and_soft_selection_are_stable() {
    let doc = document(257, 3);
    let rect = Rect::new(0, 0, 257, 3);
    let mut coverage = vec![0u8; 257 * 3];
    coverage[255] = 128;
    coverage[256] = 255;
    let selected = doc.set_selection(SelectionState::Active(SelectionSource::Coverage(Arc::new(
        MaskGrid {
            width: 257,
            height: 3,
            bytes: Arc::from(coverage),
        },
    ))));
    let geometry = doc.set_selection(SelectionState::Active(SelectionSource::Geometry(Arc::new(
        PolygonSet(vec![vec![
            (255.5, 0.0),
            (257.0, 0.0),
            (257.0, 3.0),
            (255.5, 3.0),
        ]]),
    ))));
    for id in [
        "effect.blur.fragment",
        "effect.blur.gaussian",
        "effect.blur.median",
        "effect.blur.motion",
        "effect.blur.radial",
        "effect.blur.zoom",
    ] {
        let spec = effect::find(id).unwrap();
        let full = effect::render(&doc, &spec, rect, None).unwrap();
        let masked = effect::render(&selected, &spec, rect, None).unwrap();
        let geometric = effect::render(&geometry, &spec, rect, None).unwrap();
        let original = doc.read_tile(0, TileCoord { x: 0, y: 0 }).unwrap();
        let masked_left = masked.read_tile(0, TileCoord { x: 0, y: 0 }).unwrap();
        let full_left = full.read_tile(0, TileCoord { x: 0, y: 0 }).unwrap();
        assert_eq!(&masked_left[..4], &original[..4], "{id} outside mask");
        let i = 255 * 4;
        for channel in 0..4 {
            assert_eq!(
                masked_left[i + channel],
                effect::coverage_lerp(original[i + channel], full_left[i + channel], 128),
                "{id} soft mask"
            );
        }
        let full_right = full.read_tile(0, TileCoord { x: 1, y: 0 }).unwrap();
        let masked_right = masked.read_tile(0, TileCoord { x: 1, y: 0 }).unwrap();
        assert_eq!(&masked_right[..4], &full_right[..4], "{id} seam");
        let geometric_left = geometric.read_tile(0, TileCoord { x: 0, y: 0 }).unwrap();
        let geometric_right = geometric.read_tile(0, TileCoord { x: 1, y: 0 }).unwrap();
        assert_eq!(
            &geometric_left[..4],
            &original[..4],
            "{id} outside geometry"
        );
        assert_eq!(
            &geometric_right[..4],
            &full_right[..4],
            "{id} geometry seam"
        );
        let mut coords = effect::tile_coords(rect);
        coords.reverse();
        let reversed = effect::render_tiles(&doc, &spec, rect, &coords, false, None).unwrap();
        let parallel = effect::render_tiles(&doc, &spec, rect, &coords, true, None).unwrap();
        for coord in coords {
            let expected = full.read_tile(0, coord).unwrap();
            assert_eq!(
                reversed.read_tile(0, coord).unwrap(),
                expected,
                "{id} reverse"
            );
            assert_eq!(
                parallel.read_tile(0, coord).unwrap(),
                expected,
                "{id} parallel"
            );
        }
    }
}

#[test]
#[ignore = "manual 24 MP CPU timing"]
fn gaussian_24mp_cpu_timing() {
    let doc = document(6000, 4000);
    let spec = effect::find("effect.blur.gaussian").unwrap();
    let start = std::time::Instant::now();
    let output = effect::render(
        &doc,
        &spec,
        Rect::new(0, 0, 6000, 4000),
        Some(serde_json::json!({"radius":50})),
    )
    .unwrap();
    assert_ne!(output.read_tile(0, TileCoord { x: 0, y: 0 }).unwrap()[0], 0);
    println!(
        "Gaussian CPU 6000x4000 radius=50 full operation {:.3} s",
        start.elapsed().as_secs_f64()
    );
}

#[test]
fn gaussian_provisional_gamma_and_quality_change_pixels() {
    let mut doc = document(33, 1);
    let mut tile = doc.read_tile(0, TileCoord { x: 0, y: 0 }).unwrap();
    for pixel in tile.chunks_exact_mut(4) {
        pixel.copy_from_slice(&[0, 0, 0, 255]);
    }
    tile[16 * 4..16 * 4 + 4].copy_from_slice(&[255, 255, 255, 255]);
    doc = doc
        .write_tile(
            0,
            TileCoord { x: 0, y: 0 },
            TileData::try_from(tile).unwrap(),
        )
        .unwrap();
    let spec = effect::find("effect.blur.gaussian").unwrap();
    let rect = Rect::new(0, 0, 33, 1);
    let linear = effect::render(&doc, &spec, rect, Some(serde_json::json!({"radius":8}))).unwrap();
    let srgb = effect::render(
        &doc,
        &spec,
        rect,
        Some(serde_json::json!({"radius":8,"gamma":"srgb"})),
    )
    .unwrap();
    let low_quality = effect::render(
        &doc,
        &spec,
        rect,
        Some(serde_json::json!({"radius":8,"quality":1})),
    )
    .unwrap();
    let boost = effect::render(
        &doc,
        &spec,
        rect,
        Some(serde_json::json!({"radius":8,"gamma_boost":100.0})),
    )
    .unwrap();
    let encoded = effect::render(
        &doc,
        &spec,
        rect,
        Some(serde_json::json!({"radius":8,"linear_light":false})),
    )
    .unwrap();
    let center = 16 * 4;
    let linear_tile = linear.read_tile(0, TileCoord { x: 0, y: 0 }).unwrap();
    let srgb_tile = srgb.read_tile(0, TileCoord { x: 0, y: 0 }).unwrap();
    let quality_tile = low_quality.read_tile(0, TileCoord { x: 0, y: 0 }).unwrap();
    let boost_tile = boost.read_tile(0, TileCoord { x: 0, y: 0 }).unwrap();
    let encoded_tile = encoded.read_tile(0, TileCoord { x: 0, y: 0 }).unwrap();
    assert!(srgb_tile[center] > linear_tile[center]);
    assert_ne!(quality_tile[center], linear_tile[center]);
    assert!(boost_tile[center] > linear_tile[center]);
    assert_eq!(encoded_tile[center], srgb_tile[center]);
}

#[test]
fn blur_parameter_endpoints_render_deterministically() {
    let doc = document(1, 1);
    let rect = Rect::new(0, 0, 1, 1);
    let cases = [
        (
            "effect.blur.gaussian",
            serde_json::json!({"radius":0,"quality":1,"gamma":"linear"}),
        ),
        (
            "effect.blur.gaussian",
            serde_json::json!({"radius":200,"quality":4,"gamma":"custom","gamma_exponent":5.0}),
        ),
        (
            "effect.blur.fragment",
            serde_json::json!({"fragments":2,"distance":0,"rotation":0.0}),
        ),
        (
            "effect.blur.fragment",
            serde_json::json!({"fragments":50,"distance":100,"rotation":360.0}),
        ),
        (
            "effect.blur.median",
            serde_json::json!({"radius":1,"percentile":0}),
        ),
        (
            "effect.blur.median",
            serde_json::json!({"radius":200,"percentile":100}),
        ),
        (
            "effect.blur.motion",
            serde_json::json!({"angle":-180.0,"distance":1,"centered":false}),
        ),
        (
            "effect.blur.motion",
            serde_json::json!({"angle":180.0,"distance":200,"centered":true}),
        ),
        (
            "effect.blur.radial",
            serde_json::json!({"angle":0.0,"quality":1,"offset_x":-2.0,"offset_y":-2.0}),
        ),
        (
            "effect.blur.radial",
            serde_json::json!({"angle":360.0,"quality":5,"offset_x":2.0,"offset_y":2.0}),
        ),
        (
            "effect.blur.zoom",
            serde_json::json!({"amount":0,"offset_x":-2.0,"offset_y":-2.0}),
        ),
        (
            "effect.blur.zoom",
            serde_json::json!({"amount":100,"offset_x":2.0,"offset_y":2.0}),
        ),
    ];
    for (id, params) in cases {
        let spec = effect::find(id).unwrap();
        let a = effect::render(&doc, &spec, rect, Some(params.clone())).unwrap();
        let b = effect::render(&doc, &spec, rect, Some(params)).unwrap();
        assert_eq!(
            a.read_tile(0, TileCoord { x: 0, y: 0 }).unwrap(),
            b.read_tile(0, TileCoord { x: 0, y: 0 }).unwrap(),
            "{id}"
        );
    }
}

#[test]
fn blur_identity_parameters_keep_opaque_pixels() {
    let mut doc = document(2, 1);
    let mut tile = doc.read_tile(0, TileCoord { x: 0, y: 0 }).unwrap();
    tile[..8].copy_from_slice(&[20, 40, 60, 255, 180, 150, 120, 255]);
    doc = doc
        .write_tile(
            0,
            TileCoord { x: 0, y: 0 },
            TileData::try_from(tile).unwrap(),
        )
        .unwrap();
    for (id, params) in [
        ("effect.blur.gaussian", serde_json::json!({"radius":0})),
        ("effect.blur.fragment", serde_json::json!({"distance":0})),
        ("effect.blur.radial", serde_json::json!({"angle":0.0})),
        ("effect.blur.zoom", serde_json::json!({"amount":0})),
    ] {
        let spec = effect::find(id).unwrap();
        let output = effect::render(&doc, &spec, Rect::new(0, 0, 2, 1), Some(params)).unwrap();
        assert_eq!(
            &output.read_tile(0, TileCoord { x: 0, y: 0 }).unwrap()[..8],
            &doc.read_tile(0, TileCoord { x: 0, y: 0 }).unwrap()[..8],
            "{id}"
        );
    }
}
>>>>>>> m35
