use op_core::doc::{DocState, MaskGrid, PolygonSet, SelectionSource, SelectionState};
use op_core::tiles::{TILE_BYTES, TILE_SIZE, TileCoord, TileData, TileStore};
use op_ops::effect::{self, Rect};
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
fn document(width: u32, height: u32) -> DocState {
    let store = Arc::new(TileStore::with_io(512 * TILE_BYTES, Box::new(NoSpill)));
    let mut doc = DocState::new(store, width, height, 96.0).unwrap();
    for ty in 0..height.div_ceil(TILE_SIZE as u32) {
        for tx in 0..width.div_ceil(TILE_SIZE as u32) {
            let mut bytes = vec![0; TILE_BYTES];
            for y in 0..TILE_SIZE {
                for x in 0..TILE_SIZE {
                    let gx = tx as usize * TILE_SIZE + x;
                    let gy = ty as usize * TILE_SIZE + y;
                    if gx >= width as usize || gy >= height as usize {
                        continue;
                    }
                    let i = (y * TILE_SIZE + x) * 4;
                    bytes[i..i + 4].copy_from_slice(&[
                        (gx * 53) as u8,
                        (gy * 71) as u8,
                        (gx * 19 + gy * 11) as u8,
                        ((gx + gy) % 4 * 85) as u8,
                    ]);
                }
            }
            doc = doc
                .write_tile(
                    0,
                    TileCoord {
                        x: tx as i32,
                        y: ty as i32,
                    },
                    TileData::try_from(bytes).unwrap(),
                )
                .unwrap();
        }
    }
    doc
}

const EFFECTS: [(&str, &str); 7] = [
    ("effect.artistic.ink_sketch", "Artistic"),
    ("effect.artistic.oil_painting", "Artistic"),
    ("effect.artistic.pencil_sketch", "Artistic"),
    ("effect.stylize.edge_detect", "Stylize"),
    ("effect.stylize.emboss", "Stylize"),
    ("effect.stylize.outline", "Stylize"),
    ("effect.stylize.relief", "Stylize"),
];

const REQUIRED_CASES: [&str; 8] = [
    "zero",
    "default",
    "max",
    "edges",
    "alpha",
    "geometry",
    "soft_mask",
    "tile_seams",
];
const CASE_CATALOG: [(&str, &[&str]); 7] = [
    ("effect.artistic.ink_sketch", &REQUIRED_CASES),
    ("effect.artistic.oil_painting", &REQUIRED_CASES),
    ("effect.artistic.pencil_sketch", &REQUIRED_CASES),
    ("effect.stylize.edge_detect", &REQUIRED_CASES),
    ("effect.stylize.emboss", &REQUIRED_CASES),
    ("effect.stylize.outline", &REQUIRED_CASES),
    ("effect.stylize.relief", &REQUIRED_CASES),
];

#[test]
fn m40_case_catalog_covers_every_effect_and_case() {
    assert_eq!(CASE_CATALOG.len(), EFFECTS.len());
    for (id, _) in EFFECTS {
        let cases = CASE_CATALOG.iter().find(|row| row.0 == id).expect(id).1;
        for required in REQUIRED_CASES {
            assert!(cases.contains(&required), "{id} {required}");
        }
    }
}

#[test]
fn m40_parameter_edges_alpha_and_selections() {
    let doc = document(5, 3);
    let rect = Rect::new(0, 0, 5, 3);
    let mask = Arc::new(MaskGrid {
        width: 5,
        height: 3,
        bytes: Arc::from([0u8, 128, 255, 0, 0, 0, 128, 255, 0, 0, 0, 128, 255, 0, 0]),
    });
    let soft = doc
        .clone()
        .set_selection(SelectionState::Active(SelectionSource::Coverage(mask)));
    let polygon = PolygonSet(vec![vec![(0.0, 0.0), (1.0, 0.0), (1.0, 3.0), (0.0, 3.0)]]);
    let geometry = doc
        .clone()
        .set_selection(SelectionState::Active(SelectionSource::Geometry(Arc::new(
            polygon,
        ))));
    for (id, _) in EFFECTS {
        let spec = effect::find(id).unwrap();
        let params = spec.default_params();
        let whole = effect::render(&doc, &spec, rect, Some(params.clone())).unwrap();
        let original = doc.read_tile(0, TileCoord { x: 0, y: 0 }).unwrap();
        let expected = whole.read_tile(0, TileCoord { x: 0, y: 0 }).unwrap();
        let soft_bytes = effect::render(&soft, &spec, rect, Some(params.clone()))
            .unwrap()
            .read_tile(0, TileCoord { x: 0, y: 0 })
            .unwrap();
        let geometry_bytes = effect::render(&geometry, &spec, rect, Some(params.clone()))
            .unwrap()
            .read_tile(0, TileCoord { x: 0, y: 0 })
            .unwrap();
        assert_eq!(
            &soft_bytes[..4],
            &original[..4],
            "{id} zero mask and hidden color"
        );
        assert_eq!(&soft_bytes[8..12], &expected[8..12], "{id} full mask");
        assert_eq!(
            &geometry_bytes[..4],
            &expected[..4],
            "{id} geometry reads neighbors"
        );
        assert_eq!(
            &geometry_bytes[4..8],
            &original[4..8],
            "{id} geometry clips"
        );
        for c in 0..4 {
            assert_eq!(
                soft_bytes[4 + c],
                effect::coverage_lerp(original[4 + c], expected[4 + c], 128),
                "{id} soft channel {c}"
            );
        }
        for field in &spec.schema.fields {
            match &field.kind {
                effect::ParamKind::Bool => {
                    for value in [false, true] {
                        let mut altered = params.clone();
                        altered[field.key] = serde_json::json!(value);
                        effect::render(&doc, &spec, rect, Some(altered)).unwrap();
                    }
                }
                effect::ParamKind::Choice(choices) => {
                    for value in *choices {
                        let mut altered = params.clone();
                        altered[field.key] = serde_json::json!(value);
                        effect::render(&doc, &spec, rect, Some(altered)).unwrap();
                    }
                }
                _ => {}
            }
            for value in [field.min, field.max].into_iter().flatten() {
                let mut altered = params.clone();
                altered[field.key] = if matches!(&field.kind, effect::ParamKind::Int) {
                    serde_json::json!(value as i64)
                } else {
                    serde_json::json!(value)
                };
                let result = effect::render(&doc, &spec, rect, Some(altered)).unwrap();
                assert_eq!(
                    result.read_tile(0, TileCoord { x: 0, y: 0 }).unwrap().len(),
                    TILE_BYTES,
                    "{id} {} {value}",
                    field.key
                );
            }
            if field.min.is_some_and(|min| min <= 0.0) && field.max.is_some_and(|max| max >= 0.0) {
                let mut zero = params.clone();
                zero[field.key] = if matches!(&field.kind, effect::ParamKind::Int) {
                    serde_json::json!(0)
                } else {
                    serde_json::json!(0.0)
                };
                effect::render(&doc, &spec, rect, Some(zero)).unwrap();
            }
        }
    }
}

#[test]
fn m40_tiles_match_whole_reversed_and_parallel() {
    let doc = document(258, 2);
    let rect = Rect::new(0, 0, 258, 2);
    for (id, _) in EFFECTS {
        let spec = effect::find(id).unwrap();
        let whole = effect::render(&doc, &spec, rect, None).unwrap();
        let mut coords = effect::tile_coords(rect);
        coords.reverse();
        for parallel in [false, true] {
            let tiled = effect::render_tiles(&doc, &spec, rect, &coords, parallel, None).unwrap();
            for coord in &coords {
                assert_eq!(
                    tiled.read_tile(0, *coord).unwrap(),
                    whole.read_tile(0, *coord).unwrap(),
                    "{id} {coord:?} parallel={parallel}"
                );
            }
        }
    }
}

#[test]
fn m40_provisional_5_1_controls_have_observable_effects() {
    let doc = document(5, 3);
    let rect = Rect::new(0, 0, 5, 3);
    let edge = effect::find("effect.stylize.edge_detect").unwrap();
    let default = effect::render(&doc, &edge, rect, None)
        .unwrap()
        .read_tile(0, TileCoord { x: 0, y: 0 })
        .unwrap();
    let zero = effect::render(&doc, &edge, rect, Some(serde_json::json!({"strength":0})))
        .unwrap()
        .read_tile(0, TileCoord { x: 0, y: 0 })
        .unwrap();
    for y in 0..3 {
        for x in 0..5 {
            let i = (y * TILE_SIZE + x) * 4;
            assert_eq!(&zero[i..i + 4], &[0, 0, 0, 255]);
        }
    }
    let sobel = effect::render(
        &doc,
        &edge,
        rect,
        Some(serde_json::json!({"algorithm":"sobel"})),
    )
    .unwrap()
    .read_tile(0, TileCoord { x: 0, y: 0 })
    .unwrap();
    assert_ne!(sobel, default);
    let blurred = effect::render(&doc, &edge, rect, Some(serde_json::json!({"blurring":2})))
        .unwrap()
        .read_tile(0, TileCoord { x: 0, y: 0 })
        .unwrap();
    assert_ne!(blurred, default);
    let overlaid = effect::render(
        &doc,
        &edge,
        rect,
        Some(serde_json::json!({"overlay_edges":true})),
    )
    .unwrap()
    .read_tile(0, TileCoord { x: 0, y: 0 })
    .unwrap();
    let original = doc.read_tile(0, TileCoord { x: 0, y: 0 }).unwrap();
    for y in 0..3 {
        for x in 0..5 {
            let i = (y * TILE_SIZE + x) * 4;
            for c in 0..3 {
                assert!(overlaid[i + c] >= original[i + c]);
            }
        }
    }
    let outline = effect::find("effect.stylize.outline").unwrap();
    let standard = effect::render(&doc, &outline, rect, None)
        .unwrap()
        .read_tile(0, TileCoord { x: 0, y: 0 })
        .unwrap();
    let fast = effect::render(
        &doc,
        &outline,
        rect,
        Some(serde_json::json!({"quality":"fast"})),
    )
    .unwrap()
    .read_tile(0, TileCoord { x: 0, y: 0 })
    .unwrap();
    assert_ne!(fast, standard);
}

#[test]
fn m40_registry_has_seven_effects_in_menu_order() {
    let specs = effect::registered();
    let positions: Vec<_> = EFFECTS
        .iter()
        .map(|(id, category)| {
            specs
                .iter()
                .position(|spec| spec.id == *id && spec.category == *category)
                .expect(id)
        })
        .collect();
    assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));
    assert_eq!(specs.len(), 27);
}

#[test]
fn m40_defaults_match_five_legacy_fixtures() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../");
    let mut failures = Vec::new();
    for (id, name) in [
        ("effect.artistic.ink_sketch", "Ink_Sketch"),
        ("effect.artistic.oil_painting", "Oil_Painting"),
        ("effect.artistic.pencil_sketch", "Pencil_Sketch"),
        ("effect.stylize.edge_detect", "Edge_Detect"),
        ("effect.stylize.emboss", "Emboss"),
        ("effect.stylize.outline", "Outline"),
        ("effect.stylize.relief", "Relief"),
    ] {
        let spec = effect::find(id).unwrap();
        for fixture in [
            "impulse_64",
            "partial_alpha_64",
            "plasma_512x384",
            "plasma_alpha_512x384",
            "alpha_edge_64",
        ] {
            let input = op_formats::png::decode_png(
                &std::fs::read(root.join(format!("tests/fixtures/{fixture}.png"))).unwrap(),
            )
            .unwrap();
            let expected = op_formats::png::decode_png(
                &std::fs::read(root.join(format!(
                    "tests/oracle/legacy336/effects/{name}_default_{fixture}.png"
                )))
                .unwrap(),
            )
            .unwrap();
            let width = input.width as i32;
            let height = input.height as i32;
            let sample = |x: i32, y: i32| -> [u8; 4] {
                if x < 0 || y < 0 || x >= width || y >= height {
                    return [0; 4];
                }
                let i = (y as usize * width as usize + x as usize) * 4;
                input.pixels[i..i + 4].try_into().unwrap()
            };
            let params = spec.default_params();
            let prep = effect::Prepared {
                width,
                height,
                ..effect::Prepared::default()
            };
            let mut different = 0;
            let mut first = None;
            for y in 0..height {
                for x in 0..width {
                    let actual = (spec.render_cpu)(&sample, x, y, &prep, &params);
                    let i = (y as usize * width as usize + x as usize) * 4;
                    for (c, byte) in actual.iter().enumerate() {
                        if *byte != expected.pixels[i + c] {
                            different += 1;
                            first.get_or_insert((x, y, c, *byte, expected.pixels[i + c]));
                        }
                    }
                }
            }
            if different != 0 {
                failures.push(format!("{id} {fixture}: {different} first {first:?}"));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
