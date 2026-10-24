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
        .filter(|spec| spec.category == "Adjustments")
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
                    } else {
                        [original[0], original[1], original[2], 255 - original[3]]
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
        .lines()
        .filter_map(|line| {
            line.strip_prefix("id = \"")
                .and_then(|s| s.strip_suffix('"'))
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
fn blur_effects_match_legacy_oracles() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../");
    let manifest: serde_json::Value = serde_json::from_slice(
        &std::fs::read(root.join("tests/oracle/legacy336/effects/manifest.json")).unwrap(),
    )
    .unwrap();
    for (name, id) in [
        ("Fragment Blur", "effect.blur.fragment"),
        ("Gaussian Blur", "effect.blur.gaussian"),
        ("Median Blur", "effect.blur.median"),
        ("Motion Blur", "effect.blur.motion"),
        ("Radial Blur", "effect.blur.radial"),
        ("Zoom Blur", "effect.blur.zoom"),
    ] {
        let cases: Vec<_> = manifest
            .as_array()
            .unwrap()
            .iter()
            .filter(|entry| {
                entry["op"] == name && entry["output"].as_str().is_some_and(|s| !s.is_empty())
            })
            .collect();
        assert!(cases.len() >= 5, "{name}");
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
            let rect = Rect::new(0, 0, input.width, input.height);
            let mut doc = DocState::new(store(), input.width, input.height, 96.0).unwrap();
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
