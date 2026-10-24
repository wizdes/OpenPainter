use op_core::doc::{DocState, LayerBlendMode};
use op_core::doc::{MaskGrid, PolygonSet, SelectionSource, SelectionState};
use op_core::history::History;
use op_core::tiles::SpillIo;
use op_core::tiles::{TILE_BYTES, TileCoord, TileData, TileId, TileStore};
use proptest::prelude::*;
use proptest::test_runner::{RngSeed, TestCaseError};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::io;
use std::sync::Arc;
use std::sync::Mutex;

fn pixel(state: &DocState, layer: usize, x: usize, y: usize) -> [u8; 4] {
    let tile = state
        .read_tile(
            layer,
            TileCoord {
                x: (x / 256) as i32,
                y: (y / 256) as i32,
            },
        )
        .unwrap();
    let offset = ((y % 256) * 256 + x % 256) * 4;
    tile[offset..offset + 4].try_into().unwrap()
}

const BUDGET: usize = 8 * 1024 * 1024;
const MAX_LAYERS: usize = 6;
const WIDTH: usize = 1000;
const HEIGHT: usize = 700;
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

fn next_random(n: &mut u64) -> u64 {
    *n ^= *n << 13;
    *n ^= *n >> 7;
    *n ^= *n << 17;
    *n
}

/// What a written tile holds. `Noise` and `ClearNoise` do not compress.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Fill {
    /// Random bytes in every channel.
    Noise,
    /// Random straight colors under alpha 0 across the whole tile.
    ClearNoise,
    /// One straight color under alpha 0 across the whole tile.
    ClearColor,
    /// Gradients and a checkerboard; LZ4 compresses it well.
    Pattern,
    /// Noise in the top half, pattern in the bottom half.
    Split,
    /// All (0,0,0,0); writing it removes the tile.
    Empty,
}
impl Fill {
    fn from_seed(seed: u64) -> Self {
        [
            Fill::Noise,
            Fill::Noise,
            Fill::ClearNoise,
            Fill::ClearColor,
            Fill::Pattern,
            Fill::Split,
            Fill::Empty,
        ][(seed % 7) as usize]
    }
    fn bytes(self, seed: u64) -> Vec<u8> {
        let mut n = seed | 1;
        let mut bytes = vec![0u8; TILE_BYTES];
        let pattern = |offset: usize, n: u64| {
            let (x, y, c) = ((offset / 4) % 256, offset / 1024, offset % 4);
            match c {
                0 => (x as u64 + n) as u8,
                1 => (y as u64 * 3 + (n >> 8)) as u8,
                2 => ((x ^ y) as u64 + (n >> 16)) as u8,
                _ if (x / 16 + y / 16) % 2 == 0 => 255,
                _ => (n >> 24) as u8,
            }
        };
        match self {
            Fill::Noise => {
                for chunk in bytes.chunks_exact_mut(8) {
                    chunk.copy_from_slice(&next_random(&mut n).to_le_bytes());
                }
            }
            Fill::ClearNoise => {
                for chunk in bytes.chunks_exact_mut(4) {
                    let rgb = next_random(&mut n).to_le_bytes();
                    chunk.copy_from_slice(&[rgb[0], rgb[1], rgb[2] | 1, 0]);
                }
            }
            Fill::ClearColor => {
                let rgb = next_random(&mut n).to_le_bytes();
                for chunk in bytes.chunks_exact_mut(4) {
                    chunk.copy_from_slice(&[rgb[0], rgb[1] | 1, rgb[2], 0]);
                }
            }
            Fill::Pattern => {
                let seed = next_random(&mut n);
                for (offset, byte) in bytes.iter_mut().enumerate() {
                    *byte = pattern(offset, seed);
                }
            }
            Fill::Split => {
                let seed = next_random(&mut n);
                let (top, bottom) = bytes.split_at_mut(TILE_BYTES / 2);
                for chunk in top.chunks_exact_mut(8) {
                    chunk.copy_from_slice(&next_random(&mut n).to_le_bytes());
                }
                for (offset, byte) in bottom.iter_mut().enumerate() {
                    *byte = pattern(offset + TILE_BYTES / 2, seed);
                }
            }
            Fill::Empty => (),
        }
        bytes
    }
    /// The fewest bytes a tile with this fill can take in memory, compressed or not, when
    /// `valid` bytes of it lie inside the canvas. Random literals cannot shrink under LZ4.
    fn memory_floor(self, valid: usize) -> usize {
        match self {
            Fill::Noise => valid.saturating_sub(4096),
            Fill::ClearNoise => (valid * 3 / 4).saturating_sub(4096),
            _ => 0,
        }
    }
}

#[derive(Clone)]
struct RefLayer {
    id: u64,
    name: String,
    visible: bool,
    opacity: u8,
    blend: LayerBlendMode,
    /// A full copy of the layer, width * height * 4 bytes. Shared between steps until edited.
    pixels: Arc<Vec<u8>>,
}

#[derive(Clone)]
struct RefDoc {
    width: usize,
    height: usize,
    layers: Vec<RefLayer>,
    active: usize,
    selection: SelectionState,
    profile: Option<Vec<u8>>,
    metadata: BTreeMap<String, String>,
    revision: u64,
}
impl RefDoc {
    fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            layers: vec![RefLayer {
                id: 1,
                name: "Background".into(),
                visible: true,
                opacity: 255,
                blend: LayerBlendMode::Normal,
                pixels: Arc::new(vec![0; width * height * 4]),
            }],
            active: 0,
            selection: SelectionState::None,
            profile: None,
            metadata: BTreeMap::new(),
            revision: 0,
        }
    }
    fn tiles_x(&self) -> usize {
        self.width.div_ceil(256)
    }
    fn tiles_y(&self) -> usize {
        self.height.div_ceil(256)
    }
    /// Copies the part of `tile` that lies inside the canvas; the rest is dropped.
    fn put_tile(&mut self, layer: usize, coord: TileCoord, tile: &[u8]) {
        let (width, height) = (self.width, self.height);
        let x = coord.x as usize * 256;
        let count = (width - x).min(256) * 4;
        let pixels = Arc::make_mut(&mut self.layers[layer].pixels);
        for py in 0..(height - coord.y as usize * 256).min(256) {
            let start = ((coord.y as usize * 256 + py) * width + x) * 4;
            pixels[start..start + count].copy_from_slice(&tile[py * 1024..py * 1024 + count]);
        }
    }
    fn add_layer(&mut self, index: usize, id: u64, name: String) {
        self.layers.insert(
            index,
            RefLayer {
                id,
                name,
                visible: true,
                opacity: 255,
                blend: LayerBlendMode::Normal,
                pixels: Arc::new(vec![0; self.width * self.height * 4]),
            },
        );
        if index <= self.active {
            self.active += 1;
        }
    }
    fn resize(&mut self, width: usize, height: usize, dx: i32, dy: i32) {
        let (sx, sy) = (dx as i64 * 256, dy as i64 * 256);
        let x0 = (-sx).max(0);
        let x1 = (self.width as i64).min(width as i64 - sx);
        for layer in &mut self.layers {
            let mut pixels = vec![0; width * height * 4];
            for y in 0..self.height {
                let ny = y as i64 + sy;
                if ny < 0 || ny >= height as i64 || x0 >= x1 {
                    continue;
                }
                let old = (y * self.width + x0 as usize) * 4;
                let new = (ny as usize * width + (x0 + sx) as usize) * 4;
                let count = (x1 - x0) as usize * 4;
                pixels[new..new + count].copy_from_slice(&layer.pixels[old..old + count]);
            }
            layer.pixels = Arc::new(pixels);
        }
        self.width = width;
        self.height = height;
    }
}

/// Compares every property and every pixel byte, read through the tile store, with the
/// model. `reverse` flips the tile order so a store under pressure is not always read in
/// its worst order. Bytes of edge tiles outside the canvas must be zero.
fn compare_complete(state: &DocState, model: &RefDoc, reverse: bool) {
    assert_eq!(
        (state.width as usize, state.height as usize, state.revision),
        (model.width, model.height, model.revision)
    );
    assert_eq!(state.dpi, 72.0);
    assert_eq!(state.layers.len(), model.layers.len());
    assert_eq!(state.active_layer, model.active);
    assert_eq!(state.selection, model.selection);
    assert_eq!(state.profile.as_deref(), model.profile.as_deref());
    assert_eq!(state.metadata.as_ref(), &model.metadata);
    for (index, layer) in model.layers.iter().enumerate() {
        let actual = &state.layers[index];
        assert_eq!(
            (
                actual.id.0,
                &actual.name,
                actual.visible,
                actual.opacity,
                actual.blend
            ),
            (
                layer.id,
                &layer.name,
                layer.visible,
                layer.opacity,
                layer.blend
            )
        );
    }
    let mut order = Vec::new();
    for index in 0..model.layers.len() {
        for ty in 0..model.tiles_y() {
            for tx in 0..model.tiles_x() {
                order.push((index, tx, ty));
            }
        }
    }
    if reverse {
        order.reverse();
    }
    for (index, tx, ty) in order {
        let layer = &model.layers[index];
        let coord = TileCoord {
            x: tx as i32,
            y: ty as i32,
        };
        let tile = state.read_tile(index, coord).unwrap();
        let count = (model.width - tx * 256).min(256) * 4;
        let rows = (model.height - ty * 256).min(256);
        for py in 0..256 {
            let row = &tile[py * 1024..(py + 1) * 1024];
            let (inside, outside) = row.split_at(if py < rows { count } else { 0 });
            let start = ((ty * 256 + py) * model.width + tx * 256) * 4;
            assert!(
                inside.is_empty() || inside == &layer.pixels[start..start + inside.len()],
                "layer {index} tile {coord:?} row {py} differs from the model"
            );
            assert!(
                outside.iter().all(|&b| b == 0),
                "layer {index} tile {coord:?} row {py} holds bytes outside the canvas"
            );
        }
    }
}

/// Records the memory floor of the tile just written at `coord`.
fn note_floor(
    state: &DocState,
    model: &RefDoc,
    layer: usize,
    coord: TileCoord,
    fill: Fill,
    floors: &mut HashMap<TileId, usize>,
) {
    if let Some(id) = state.layers[layer].tiles.get(coord) {
        let valid = (model.width - coord.x as usize * 256).min(256)
            * (model.height - coord.y as usize * 256).min(256)
            * 4;
        floors.insert(id, fill.memory_floor(valid));
    }
}

/// Builds the opened document: 4 or 5 layers, the first 4 filled with noise on every tile,
/// so its content alone needs more than the 8 MiB budget.
fn opened_document(
    store: &Arc<TileStore>,
    layers: usize,
    seed: u64,
    floors: &mut HashMap<TileId, usize>,
) -> (DocState, RefDoc) {
    let mut state = DocState::new(store.clone(), WIDTH as u32, HEIGHT as u32, 72.0).unwrap();
    let mut model = RefDoc::new(WIDTH, HEIGHT);
    for index in 1..layers {
        let name = format!("Layer {}", index + 1);
        state = state.add_layer(index, name.clone()).unwrap();
        model.add_layer(index, index as u64 + 1, name);
    }
    let mut n = seed | 1;
    for index in 0..layers {
        for ty in 0..model.tiles_y() {
            for tx in 0..model.tiles_x() {
                let fill = if index < 4 {
                    Fill::Noise
                } else {
                    Fill::from_seed(next_random(&mut n))
                };
                let coord = TileCoord {
                    x: tx as i32,
                    y: ty as i32,
                };
                let bytes = fill.bytes(next_random(&mut n));
                state = state
                    .write_tile(index, coord, TileData::try_from(bytes.clone()).unwrap())
                    .unwrap();
                model.put_tile(index, coord, &bytes);
                note_floor(&state, &model, index, coord, fill, floors);
            }
        }
    }
    model.revision = state.revision;
    (state, model)
}

/// The least memory that the live tiles of all steps need. Above the budget, some must spill.
fn live_floor(history: &History, floors: &HashMap<TileId, usize>) -> usize {
    let mut live = HashSet::new();
    for index in 0..history.len() {
        for layer in &history.state(index).unwrap().layers {
            live.extend(layer.tiles.iter().map(|(_, id)| id));
        }
    }
    live.iter().filter_map(|id| floors.get(id)).sum()
}

struct Step {
    model: RefDoc,
    identity: u64,
    label: String,
    command_id: String,
}

fn proptest_cases() -> u32 {
    std::env::var("OP_PROPTEST_CASES")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(64)
}

/// Random edits, undo, redo, jumps, branches and saves on a 1000x700 document with up to
/// 6 layers of full tiles under an 8 MiB budget. After every transition the complete state
/// must equal a full-copy model, and spilling must have happened.
fn check_history_case(
    layers: usize,
    seed: u64,
    actions: &[(u8, u64)],
) -> Result<(), TestCaseError> {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(TileStore::new(dir.path(), BUDGET).unwrap());
    let mut floors = HashMap::new();
    let (opened, model) = opened_document(&store, layers, seed, &mut floors);
    let mut history = History::new(opened);
    let mut steps = vec![Step {
        model,
        identity: 0,
        label: "Open".into(),
        command_id: "document.open".into(),
    }];
    let (mut current, mut saved, mut next_identity) = (0usize, Some(0u64), 1u64);
    let mut next_layer = layers as u64 + 1;
    let mut saw_spill = false;
    // The first turn (kind 255) does nothing, so the opened document is checked too.
    for (turn, &(kind, value)) in std::iter::once(&(255, 0)).chain(actions).enumerate() {
        let mut model = steps[current].model.clone();
        let old = history.current_state();
        let len = model.layers.len();
        let pick = |n: usize| (value % n as u64) as usize;
        let high = (value >> 32) as usize;
        let coord = TileCoord {
            x: ((value >> 8) % model.tiles_x() as u64) as i32,
            y: ((value >> 16) % model.tiles_y() as u64) as i32,
        };
        let mut next = None;
        match kind {
            0..=6 => {
                let fill = Fill::from_seed(value >> 24);
                let layer = pick(len);
                let bytes = fill.bytes(value);
                let state = old
                    .write_tile(layer, coord, TileData::try_from(bytes.clone()).unwrap())
                    .unwrap();
                model.put_tile(layer, coord, &bytes);
                note_floor(&state, &model, layer, coord, fill, &mut floors);
                next = Some(("paint", "tile.write", state));
            }
            7 => {
                let layer = pick(len);
                next = Some(("clear", "tile.clear", old.clear_tile(layer, coord).unwrap()));
                model.put_tile(layer, coord, &vec![0; TILE_BYTES]);
            }
            8 if len < MAX_LAYERS => {
                let index = pick(len + 1);
                let name = format!("new {}", high % 1000);
                next = Some((
                    "add layer",
                    "layer.add",
                    old.add_layer(index, name.clone()).unwrap(),
                ));
                model.add_layer(index, next_layer, name);
                next_layer += 1;
            }
            9 if len > 1 => {
                let index = pick(len);
                next = Some((
                    "delete layer",
                    "layer.delete",
                    old.delete_layer(index).unwrap(),
                ));
                model.layers.remove(index);
                if model.active > index {
                    model.active -= 1;
                } else if model.active == index {
                    model.active = model.active.min(model.layers.len() - 1);
                }
            }
            10 => {
                let (from, to) = (pick(len), high % len);
                next = Some((
                    "move layer",
                    "layer.move",
                    old.move_layer(from, to).unwrap(),
                ));
                let active_id = model.layers[model.active].id;
                let layer = model.layers.remove(from);
                model.layers.insert(to, layer);
                model.active = model.layers.iter().position(|l| l.id == active_id).unwrap();
            }
            11 if len < MAX_LAYERS => {
                let index = pick(len);
                next = Some((
                    "duplicate layer",
                    "layer.duplicate",
                    old.duplicate_layer(index).unwrap(),
                ));
                let mut layer = model.layers[index].clone();
                layer.id = next_layer;
                next_layer += 1;
                layer.name.push_str(" copy");
                model.layers.insert(index + 1, layer);
            }
            12 => {
                let (index, name) = (pick(len), format!("name {}", high % 1000));
                next = Some((
                    "rename",
                    "layer.name",
                    old.set_layer_name(index, name.clone()).unwrap(),
                ));
                model.layers[index].name = name;
            }
            13 => {
                let (index, visible) = (pick(len), high.is_multiple_of(2));
                next = Some((
                    "visibility",
                    "layer.visible",
                    old.set_layer_visible(index, visible).unwrap(),
                ));
                model.layers[index].visible = visible;
            }
            14 => {
                let (index, opacity) = (pick(len), high as u8);
                next = Some((
                    "opacity",
                    "layer.opacity",
                    old.set_layer_opacity(index, opacity).unwrap(),
                ));
                model.layers[index].opacity = opacity;
            }
            15 => {
                let (index, blend) = (pick(len), MODES[high % 14]);
                next = Some((
                    "blend",
                    "layer.blend",
                    old.set_layer_blend(index, blend).unwrap(),
                ));
                model.layers[index].blend = blend;
            }
            16 => {
                let index = pick(len);
                next = Some((
                    "active",
                    "layer.active",
                    old.set_active_layer(index).unwrap(),
                ));
                model.active = index;
            }
            17 => {
                let selection = match high % 3 {
                    0 => SelectionState::None,
                    1 => SelectionState::Active(SelectionSource::Geometry(Arc::new(PolygonSet(
                        vec![vec![
                            (0.5, 1.0),
                            ((high % 997) as f64, 3.25),
                            (7.0, (high % 89) as f64),
                        ]],
                    )))),
                    _ => SelectionState::Active(SelectionSource::Coverage(Arc::new(MaskGrid {
                        width: 8,
                        height: 4,
                        bytes: (0..32u8).map(|i| (high as u8).wrapping_mul(i)).collect(),
                    }))),
                };
                next = Some((
                    "select",
                    "selection.set",
                    old.set_selection(selection.clone()),
                ));
                model.selection = selection;
            }
            18 => {
                let profile = (high % 2 == 1).then(|| value.to_le_bytes().to_vec());
                next = Some((
                    "profile",
                    "image.profile",
                    old.set_profile(profile.clone().map(Arc::<[u8]>::from)),
                ));
                model.profile = profile;
            }
            19 => {
                let (key, val) = (format!("key {}", pick(3)), format!("value {}", high % 1000));
                next = Some((
                    "metadata",
                    "image.metadata",
                    old.set_metadata(key.clone(), val.clone()),
                ));
                model.metadata.insert(key, val);
            }
            20 => {
                let (width, height) =
                    [(1000, 700), (700, 500), (1024, 512), (513, 257), (900, 760)][pick(5)];
                let (dx, dy) = ((high % 3) as i32 - 1, (high / 3 % 3) as i32 - 1);
                next = Some((
                    "resize",
                    "image.canvas_size",
                    old.resize_canvas(width, height, dx, dy).unwrap(),
                ));
                model.resize(width as usize, height as usize, dx, dy);
            }
            21 | 22 => {
                if history.undo() {
                    current -= 1;
                }
            }
            23 => {
                if history.redo() {
                    current += 1;
                }
            }
            24 => {
                current = pick(steps.len());
                history.jump(current).unwrap();
            }
            25 => {
                // Jump with a viewport pinned, then read everything while it stays pinned.
                let target = pick(steps.len());
                let rect = (
                    (high % 1000) as u32,
                    (high >> 10) as u32 % 700,
                    (high >> 20) as u32 % 1920 + 1,
                    (high >> 8) as u32 % 1080 + 1,
                );
                let view = history.jump_checked(target, rect).unwrap();
                current = target;
                compare_complete(view.state(), &steps[current].model, turn % 2 == 1);
                for id in view.tile_ids() {
                    prop_assert_eq!(
                        store.state(id).unwrap(),
                        op_core::tiles::TileStateKind::Resident
                    );
                }
            }
            26 => {
                // An edit that must fail leaves everything unchanged.
                let failed = match high % 5 {
                    0 => old
                        .write_tile(len, coord, TileData::try_from(vec![1; TILE_BYTES]).unwrap())
                        .is_err(),
                    1 => old.delete_layer(len).is_err(),
                    2 => old.set_active_layer(len).is_err(),
                    3 => old.move_layer(0, len).is_err(),
                    _ => old.resize_canvas(0, 10, 0, 0).is_err(),
                };
                prop_assert!(failed);
            }
            27 => {
                history.mark_saved();
                saved = Some(steps[current].identity);
            }
            _ => (),
        }
        if let Some((label, command_id, state)) = next {
            model.revision += 1;
            history.commit(label, command_id, state);
            steps.truncate(current + 1);
            steps.push(Step {
                model,
                identity: next_identity,
                label: label.into(),
                command_id: command_id.into(),
            });
            current += 1;
            next_identity += 1;
        }
        let spilled = store.stats().spilled_bytes;
        saw_spill |= spilled > 0;
        let floor = live_floor(&history, &floors);
        prop_assert!(
            floor <= BUDGET || spilled > 0,
            "live floor {} is over budget but nothing spilled",
            floor
        );
        compare_complete(
            history.current_state(),
            &steps[current].model,
            turn % 2 == 1,
        );
        prop_assert_eq!(history.len(), steps.len());
        prop_assert_eq!(history.current_index(), current);
        prop_assert_eq!(history.label(current), Some(steps[current].label.as_str()));
        prop_assert_eq!(
            history.command_id(current),
            Some(steps[current].command_id.as_str())
        );
        prop_assert_eq!(history.is_dirty(), saved != Some(steps[current].identity));
        prop_assert!(store.take_deferred_error().is_none());
    }
    prop_assert!(
        saw_spill,
        "the opened document alone exceeds the budget, so tiles must spill"
    );
    drop(history);
    prop_assert_eq!(store.stats().tile_count, 0);
    prop_assert_eq!(op_core::doc::release_failures(), 0);
    Ok(())
}

/// The cases are split over this many tests so the test harness runs them in parallel.
const SHARDS: u32 = 8;

fn shard_config(shard: u64) -> ProptestConfig {
    ProptestConfig {
        cases: proptest_cases().div_ceil(SHARDS),
        rng_seed: RngSeed::Fixed(0x4d06_0000 + shard),
        failure_persistence: None,
        ..ProptestConfig::default()
    }
}

macro_rules! history_shard {
    ($name:ident, $shard:expr) => {
        proptest! {
            #![proptest_config(shard_config($shard))]
            #[test]
            fn $name(
                layers in 4usize..=5,
                seed in any::<u64>(),
                actions in prop::collection::vec((0u8..28, any::<u64>()), 1..=80),
            ) {
                check_history_case(layers, seed, &actions)?;
            }
        }
    };
}
history_shard!(random_history_matches_full_copy_model_0, 0);
history_shard!(random_history_matches_full_copy_model_1, 1);
history_shard!(random_history_matches_full_copy_model_2, 2);
history_shard!(random_history_matches_full_copy_model_3, 3);
history_shard!(random_history_matches_full_copy_model_4, 4);
history_shard!(random_history_matches_full_copy_model_5, 5);
history_shard!(random_history_matches_full_copy_model_6, 6);
history_shard!(random_history_matches_full_copy_model_7, 7);

#[test]
fn selection_profile_resize_and_alpha_zero_survive_jumps() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(TileStore::new(dir.path(), 8 * 1024 * 1024).unwrap());
    let mut h = History::new(DocState::new(store.clone(), 700, 500, 72.0).unwrap());
    let mut data = vec![0; TILE_BYTES];
    data[..4].copy_from_slice(&[31, 45, 91, 0]);
    h.commit(
        "paint",
        "paint",
        h.current_state()
            .write_tile(
                0,
                TileCoord { x: 2, y: 1 },
                TileData::try_from(data).unwrap(),
            )
            .unwrap(),
    );
    let sel = SelectionState::Active(SelectionSource::Geometry(Arc::new(PolygonSet(vec![vec![
        (1.0, 2.0),
        (3.0, 4.0),
    ]]))));
    h.commit(
        "select",
        "select",
        h.current_state().set_selection(sel.clone()),
    );
    h.commit(
        "profile",
        "profile",
        h.current_state().set_profile(Some(Arc::from([1_u8, 2, 3]))),
    );
    h.commit(
        "resize",
        "resize",
        h.current_state().resize_canvas(600, 400, 0, 0).unwrap(),
    );
    assert_eq!(pixel(h.current_state(), 0, 512, 256), [31, 45, 91, 0]);
    assert_eq!(
        h.current_state()
            .read_tile(0, TileCoord { x: 2, y: 1 })
            .unwrap()[(144 * 256 + 88) * 4..(144 * 256 + 88) * 4 + 4],
        [0; 4]
    );
    h.jump(2).unwrap();
    assert_eq!(h.current_state().selection, sel);
    assert_eq!(h.current_state().profile, None);
    h.jump(3).unwrap();
    assert_eq!(h.current_state().profile.as_deref(), Some(&[1, 2, 3][..]));
    h.jump(1).unwrap();
    assert_eq!(pixel(h.current_state(), 0, 512, 256), [31, 45, 91, 0]);
    drop(h);
    assert_eq!(store.stats().tile_count, 0);
}

#[test]
fn page_sharing_and_layer_ids() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(TileStore::new(dir.path(), 8 * 1024 * 1024).unwrap());
    let mut state = DocState::new(store.clone(), 6000, 4000, 72.0).unwrap();
    let bytes = TileData::try_from(vec![1; TILE_BYTES]).unwrap();
    state = state
        .write_tile(0, TileCoord { x: 0, y: 0 }, bytes.clone())
        .unwrap();
    state = state
        .write_tile(0, TileCoord { x: 20, y: 0 }, bytes.clone())
        .unwrap();
    let next = state
        .write_tile(0, TileCoord { x: 1, y: 0 }, bytes)
        .unwrap();
    assert_eq!(state.layers[0].tiles.page_count(), 2);
    assert_eq!(
        state.layers[0]
            .tiles
            .shared_page_count(&next.layers[0].tiles),
        1
    );
    let added = next.add_layer(1, "new").unwrap();
    let id = added.layers[1].id;
    let deleted = added.delete_layer(1).unwrap();
    let added_again = deleted.add_layer(1, "again").unwrap();
    assert_ne!(id, added_again.layers[1].id);
    drop((state, next, added, deleted, added_again));
    assert_eq!(store.stats().tile_count, 0);
}

#[test]
fn layer_id_is_never_reused_after_history_branch() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(TileStore::new(dir.path(), 8 * 1024 * 1024).unwrap());
    let mut history = History::new(DocState::new(store, 700, 500, 72.0).unwrap());
    let first = history.current_state().add_layer(1, "first").unwrap();
    let removed_id = first.layers[1].id;
    history.commit("add", "layer.add", first);
    history.undo();
    let second = history.current_state().add_layer(1, "second").unwrap();
    assert_ne!(removed_id, second.layers[1].id);
}

#[test]
fn failed_edits_keep_state_and_history_index() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(TileStore::new(dir.path(), 8 * 1024 * 1024).unwrap());
    let mut h = History::new(DocState::new(store, 700, 500, 72.0).unwrap());
    assert!(h.current_state().delete_layer(0).is_err());
    assert!(
        h.current_state()
            .write_tile(
                2,
                TileCoord { x: 0, y: 0 },
                TileData::try_from(vec![1; TILE_BYTES]).unwrap()
            )
            .is_err()
    );
    assert!(h.jump(4).is_err());
    assert_eq!(h.current_index(), 0);
    assert_eq!(h.current_state().revision, 0);
}

#[test]
fn partial_edge_does_not_reveal_pixels_when_canvas_grows() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(TileStore::new(dir.path(), 8 * 1024 * 1024).unwrap());
    let state = DocState::new(store, 257, 257, 72.0).unwrap();
    let data = TileData::try_from(vec![91; TILE_BYTES]).unwrap();
    let state = state.write_tile(0, TileCoord { x: 1, y: 1 }, data).unwrap();
    let grown = state.resize_canvas(500, 500, 0, 0).unwrap();
    assert_eq!(pixel(&grown, 0, 256, 256), [91; 4]);
    assert_eq!(pixel(&grown, 0, 257, 256), [0; 4]);
    assert_eq!(pixel(&grown, 0, 256, 257), [0; 4]);
}

#[derive(Clone, Copy)]
enum Fault {
    None,
    Read,
    ShortWrite,
    DiskFull,
    Truncate,
}
struct FaultIo {
    bytes: Vec<u8>,
    fault: Fault,
}
struct SharedIo(Arc<Mutex<FaultIo>>);
impl SpillIo for SharedIo {
    fn append(&self, bytes: &[u8]) -> io::Result<u64> {
        let mut io = self.0.lock().unwrap();
        match io.fault {
            Fault::ShortWrite => return Err(io::ErrorKind::WriteZero.into()),
            Fault::DiskFull => return Err(io::Error::from_raw_os_error(28)),
            _ => (),
        }
        let offset = io.bytes.len() as u64;
        io.bytes.extend_from_slice(bytes);
        Ok(offset)
    }
    fn read_exact_at(&self, offset: u64, bytes: &mut [u8]) -> io::Result<()> {
        let io = self.0.lock().unwrap();
        if matches!(io.fault, Fault::Read) {
            return Err(io::Error::other("injected read failure"));
        }
        if matches!(io.fault, Fault::Truncate) {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        bytes.copy_from_slice(
            io.bytes
                .get(offset as usize..offset as usize + bytes.len())
                .ok_or(io::ErrorKind::UnexpectedEof)?,
        );
        Ok(())
    }
}

#[test]
fn storage_faults_leave_history_unchanged_and_recover() {
    for fault in [Fault::ShortWrite, Fault::DiskFull] {
        let io = Arc::new(Mutex::new(FaultIo {
            bytes: vec![],
            fault: Fault::None,
        }));
        let store = Arc::new(TileStore::with_io(0, Box::new(SharedIo(io.clone()))));
        let mut h = History::new(DocState::new(store.clone(), 700, 500, 72.0).unwrap());
        let mut bytes = vec![0; TILE_BYTES];
        bytes[..4].copy_from_slice(&[1, 2, 3, 0]);
        h.commit(
            "paint",
            "paint",
            h.current_state()
                .write_tile(
                    0,
                    TileCoord { x: 0, y: 0 },
                    TileData::try_from(bytes.clone()).unwrap(),
                )
                .unwrap(),
        );
        io.lock().unwrap().fault = fault;
        // Eviction is best effort (CP1 review): a failed spill write does not
        // fail the edit. The tile stays in memory and the store reports it.
        let mut other = bytes.clone();
        other[..4].copy_from_slice(&[7, 8, 9, 0]);
        let next = h
            .current_state()
            .write_tile(
                0,
                TileCoord { x: 1, y: 0 },
                TileData::try_from(other).unwrap(),
            )
            .unwrap();
        let stats = store.stats();
        assert!(stats.evict_failures > 0);
        assert!(stats.over_budget);
        h.commit("paint", "paint", next);
        assert_eq!(h.current_index(), 2);
        assert_eq!(pixel(h.current_state(), 0, 256, 0), [7, 8, 9, 0]);
        assert_eq!(pixel(h.state(1).unwrap(), 0, 0, 0), [1, 2, 3, 0]);
        assert_eq!(pixel(h.state(1).unwrap(), 0, 256, 0), [0, 0, 0, 0]);
        io.lock().unwrap().fault = Fault::None;
        h.undo();
        assert_eq!(pixel(h.current_state(), 0, 0, 0), [1, 2, 3, 0]);
        assert!(!store.stats().over_budget);
        drop(h);
        assert_eq!(store.stats().tile_count, 0);
    }
    for fault in [Fault::Read, Fault::Truncate] {
        let io = Arc::new(Mutex::new(FaultIo {
            bytes: vec![],
            fault: Fault::None,
        }));
        let store = Arc::new(TileStore::with_io(0, Box::new(SharedIo(io.clone()))));
        let mut h = History::new(DocState::new(store.clone(), 700, 500, 72.0).unwrap());
        let mut bytes = vec![0; TILE_BYTES];
        bytes[..4].copy_from_slice(&[4, 5, 6, 0]);
        h.commit(
            "paint",
            "paint",
            h.current_state()
                .write_tile(
                    0,
                    TileCoord { x: 0, y: 0 },
                    TileData::try_from(bytes).unwrap(),
                )
                .unwrap(),
        );
        h.undo();
        io.lock().unwrap().fault = fault;
        assert!(h.jump_checked(1, (0, 0, 256, 256)).is_err());
        assert_eq!(h.current_index(), 0);
        io.lock().unwrap().fault = Fault::None;
        let view = h.jump_checked(1, (0, 0, 256, 256)).unwrap();
        assert_eq!(view.len(), 1);
        drop(view);
        assert_eq!(pixel(h.current_state(), 0, 0, 0), [4, 5, 6, 0]);
        drop(h);
        assert_eq!(store.stats().tile_count, 0);
    }
    assert_eq!(op_core::doc::release_failures(), 0);
}

#[test]
fn branch_restores_state_and_frees_tiles() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(TileStore::new(dir.path(), 8 * 1024 * 1024).unwrap());
    let first = DocState::new(store.clone(), 700, 500, 72.0).unwrap();
    let mut history = History::new(first);
    let coord = TileCoord { x: 0, y: 0 };
    let mut pixels = vec![0; TILE_BYTES];
    pixels[..4].copy_from_slice(&[20, 40, 60, 0]);
    let next = history
        .current_state()
        .write_tile(0, coord, TileData::try_from(pixels).unwrap())
        .unwrap();
    history.commit("paint", "paint", next);
    history.mark_saved();
    assert!(!history.is_dirty());
    history.undo();
    assert!(history.is_dirty());
    let next = history
        .current_state()
        .set_layer_blend(0, LayerBlendMode::Multiply)
        .unwrap();
    history.commit("blend", "layer.blend", next);
    assert!(history.is_dirty());
    assert_eq!(store.stats().tile_count, 0);
    drop(history);
    assert_eq!(store.stats().tile_count, 0);
    assert_eq!(op_core::doc::release_failures(), 0);
}

#[test]
fn ensure_resident_keeps_viewport_pinned_under_pressure() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(TileStore::new(dir.path(), 8 * 1024 * 1024).unwrap());
    let mut state = DocState::new(store.clone(), 1000, 700, 72.0).unwrap();
    for (index, coord) in [(0, 0), (1, 0), (0, 1), (3, 2)].into_iter().enumerate() {
        let mut n = index as u64 * 7919 + 1;
        let bytes = (0..TILE_BYTES)
            .map(|_| {
                n ^= n << 13;
                n ^= n >> 7;
                n ^= n << 17;
                n as u8
            })
            .collect::<Vec<_>>();
        state = state
            .write_tile(
                0,
                TileCoord {
                    x: coord.0,
                    y: coord.1,
                },
                TileData::try_from(bytes).unwrap(),
            )
            .unwrap();
    }
    let h = History::new(state);
    let ids = [(0, 0), (1, 0), (0, 1)].map(|(x, y)| {
        h.current_state().layers[0]
            .tiles
            .get(TileCoord { x, y })
            .unwrap()
    });
    let view = h.ensure_resident((0, 0, 300, 300)).unwrap();
    assert_eq!(view.len(), 3);
    assert_eq!(view.tile_ids().collect::<Vec<_>>(), ids);
    let pressure = |seed: u64| {
        let mut n = seed;
        let mut held = Vec::new();
        for _ in 0..48 {
            let bytes = (0..TILE_BYTES)
                .map(|_| {
                    n ^= n << 13;
                    n ^= n >> 7;
                    n ^= n << 17;
                    n as u8
                })
                .collect::<Vec<_>>();
            held.push(store.insert(TileData::try_from(bytes).unwrap()).unwrap());
        }
        held
    };
    let held = pressure(99);
    assert!(store.stats().spilled_bytes > 0);
    for id in ids {
        assert_eq!(
            store.state(id).unwrap(),
            op_core::tiles::TileStateKind::Resident
        );
    }
    assert_eq!(
        view.tile(0, TileCoord { x: 1, y: 0 }).unwrap(),
        &*store.get(ids[1]).unwrap()
    );
    drop(view);
    let more = pressure(7);
    for id in ids {
        assert_eq!(
            store.state(id).unwrap(),
            op_core::tiles::TileStateKind::Spilled
        );
    }
    for id in held.into_iter().chain(more) {
        store.release(id).unwrap();
    }
    assert!(store.take_deferred_error().is_none());
    drop(h);
    assert_eq!(store.stats().tile_count, 0);
    assert_eq!(op_core::doc::release_failures(), 0);
}

/// A page that cannot release a tile it owns is a counting bug: debug builds panic.
/// Release builds count it in `release_failures` instead, which the other tests check stays 0.
#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "releasing owned tile")]
fn page_drop_with_broken_ownership_panics_in_debug() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(TileStore::new(dir.path(), 8 * 1024 * 1024).unwrap());
    let coord = TileCoord { x: 0, y: 0 };
    let state = DocState::new(store.clone(), 300, 300, 72.0)
        .unwrap()
        .write_tile(0, coord, TileData::try_from(vec![5; TILE_BYTES]).unwrap())
        .unwrap();
    // Take away the page's ownership behind its back, so its own release must fail.
    store
        .release(state.layers[0].tiles.get(coord).unwrap())
        .unwrap();
    drop(state);
}
