//! M57 history stress: 1,000 random steps (brush strokes, effects, layer adds
//! and deletes, selections, undo, redo and History jumps) on a document whose
//! tile memory budget is small, so history pages out to LZ4 and the spill file.
//! Every step checks that tile memory stays under the budget, every undo, redo
//! and jump must show exactly the content that step had when it was made, and
//! after undoing everything the document equals the original byte for byte.
//! When the document closes, its spill file is deleted.
use op_core::doc::{DocState, ToolCompositeMode};
use op_core::selection::RasterMode;
use op_core::tiles::{TILE_SIZE, TileCoord};
use op_engine::{Engine, EngineOptions};
use op_tools::{BrushSettings, PressureMode, StrokeSample, Tool};
use serde_json::json;

const BUDGET: usize = 6 << 20;
const STEPS: usize = 1000;

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
    fn f(&mut self, max: f64) -> f64 {
        (self.next() % 1_000_000) as f64 / 1_000_000.0 * max
    }
}

/// FNV-1a over every byte that defines the document: size, layers (pixels of
/// every tile, including colors under zero alpha, and properties), the active
/// layer, the selection mask and the profile.
fn doc_hash(s: &DocState) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325_u64;
    let mut eat = |bytes: &[u8]| {
        for b in bytes {
            h ^= u64::from(*b);
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
    };
    eat(&s.width.to_le_bytes());
    eat(&s.height.to_le_bytes());
    eat(&s.active_layer.to_le_bytes());
    for (i, layer) in s.layers.iter().enumerate() {
        eat(layer.name.as_bytes());
        eat(&[layer.visible as u8, layer.opacity, layer.blend as u8]);
        for ty in 0..s.height.div_ceil(TILE_SIZE as u32) {
            for tx in 0..s.width.div_ceil(TILE_SIZE as u32) {
                let coord = TileCoord {
                    x: tx as i32,
                    y: ty as i32,
                };
                eat(&s.read_tile(i, coord).unwrap());
            }
        }
    }
    eat(&s.rasterize_selection(RasterMode::Pixelated).bytes);
    eat(s.profile.as_deref().unwrap_or(&[]));
    h
}

fn spill_files() -> Vec<String> {
    let prefix = format!("op-tiles-{}-", std::process::id());
    std::fs::read_dir(std::env::temp_dir())
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with(&prefix) && n.ends_with(".spill"))
        .collect()
}

fn brush(rng: &mut Rng) -> BrushSettings {
    BrushSettings {
        tool: Tool::Paintbrush,
        width: 2.0 + rng.f(30.0),
        color: [
            rng.next() as u8,
            rng.next() as u8,
            rng.next() as u8,
            128 + (rng.next() % 128) as u8,
        ],
        antialias: rng.below(2) == 0,
        spacing: 0.25,
        pressure: PressureMode::None,
        blend: ToolCompositeMode::Normal,
    }
}

#[test]
fn a_thousand_random_steps_undo_back_to_the_original_inside_the_budget() {
    let (w, h) = (512_u32, 384_u32);
    let mut rng = Rng(0x5eed_0057);
    let pixels: Vec<u8> = (0..w * h)
        .flat_map(|i| {
            let (x, y) = (i % w, i / w);
            [(x / 2) as u8, (y / 2) as u8, (x ^ y) as u8, 255]
        })
        .collect();
    let before = spill_files();
    let engine = Engine::with_options(
        None,
        EngineOptions {
            force_cpu: true,
            ..Default::default()
        },
    )
    .unwrap();
    let doc = engine.import_pixels(w, h, &pixels, None).unwrap();
    let store = engine.tile_store(doc).unwrap();
    store.set_budget(BUDGET);
    let original = doc_hash(&engine.snapshot(doc).unwrap().state);
    // The hash of each history step, as made.
    let mut made = vec![original];
    let mut counts = [0_usize; 9];
    let mut peak_memory = 0;
    let mut peak_spilled = 0;
    let mut longest = 0;
    for step in 0..STEPS {
        let snap = engine.snapshot(doc).unwrap();
        let (index, len) = (snap.history.index, snap.history.labels.len());
        let layers = snap.state.layers.len();
        let selected = !matches!(snap.state.selection, op_core::doc::SelectionState::None);
        drop(snap);
        // Mostly edits, so the history grows long; undo, redo and jumps mixed in.
        let kind = match rng.below(20) {
            0..=6 => 0,
            7..=8 => 2,
            9 => 3,
            10 => 4,
            11 => 5,
            12..=13 => 6,
            14..=17 => 7,
            _ => 8,
        };
        counts[kind] += 1;
        let committed = match kind {
            0 => {
                engine
                    .stroke_begin(doc, brush(&mut rng), step as u64)
                    .unwrap();
                let n = 2 + rng.below(6);
                let samples: Vec<StrokeSample> = (0..n)
                    .map(|k| StrokeSample::mouse(rng.f(w as f64), rng.f(h as f64), k * 8))
                    .collect();
                engine.stroke_append(doc, &samples).unwrap();
                engine.stroke_commit(doc).unwrap();
                true
            }
            2 => {
                let radius = 1 + rng.below(3);
                engine
                    .dispatch(doc, "effect.blur.gaussian", json!({"radius": radius}))
                    .unwrap();
                true
            }
            3 => {
                engine
                    .dispatch(doc, "image.invert_colors", json!({}))
                    .unwrap();
                true
            }
            4 => {
                if layers < 4 {
                    engine
                        .dispatch(doc, "layer.add", json!({"name": format!("L{step}")}))
                        .unwrap();
                    true
                } else {
                    engine.dispatch(doc, "layer.delete", json!({})).unwrap();
                    true
                }
            }
            5 => {
                if layers > 1 {
                    engine.dispatch(doc, "layer.delete", json!({})).unwrap();
                    true
                } else {
                    engine
                        .dispatch(doc, "layer.add", json!({"name": "again"}))
                        .unwrap();
                    true
                }
            }
            6 => {
                let params = match rng.below(3) {
                    0 => (
                        "selection.rectangle",
                        json!({"x": rng.f(400.0), "y": rng.f(300.0), "width": 1.0 + rng.f(200.0), "height": 1.0 + rng.f(200.0), "combine": "replace"}),
                    ),
                    1 => ("selection.select_all", json!({})),
                    // Deselect is disabled with nothing selected.
                    _ if selected => ("selection.deselect", json!({})),
                    _ => ("selection.select_all", json!({})),
                };
                engine.dispatch(doc, params.0, params.1).unwrap();
                true
            }
            7 => {
                if rng.below(2) == 0 && index > 0 {
                    engine.dispatch(doc, "edit.undo", json!({})).unwrap();
                } else if index + 1 < len {
                    engine.dispatch(doc, "edit.redo", json!({})).unwrap();
                }
                false
            }
            _ => {
                // A jump anywhere, checked, and then often back to the newest
                // step (as a user browsing History does), so the history grows.
                let target = rng.below(len as u64);
                engine
                    .dispatch(doc, "history.jump", json!({"index": target}))
                    .unwrap();
                let now = doc_hash(&engine.snapshot(doc).unwrap().state);
                assert_eq!(now, made[target as usize], "step {step}: jump to {target}");
                if rng.below(4) != 0 {
                    engine
                        .dispatch(doc, "history.jump", json!({"index": len - 1}))
                        .unwrap();
                }
                false
            }
        };
        let snap = engine.snapshot(doc).unwrap();
        let now = doc_hash(&snap.state);
        if committed {
            assert_eq!(snap.history.index, index + 1, "step {step}: kind {kind}");
            made.truncate(index + 1);
            made.push(now);
        } else {
            assert_eq!(
                now, made[snap.history.index],
                "step {step}: index {} differs from when it was made",
                snap.history.index
            );
        }
        assert_eq!(made.len(), snap.history.labels.len(), "step {step}");
        let stats = store.stats();
        let memory = stats.resident_bytes + stats.compressed_bytes;
        peak_memory = peak_memory.max(memory);
        peak_spilled = peak_spilled.max(stats.spilled_bytes);
        longest = longest.max(made.len());
        assert!(
            !stats.over_budget && memory <= BUDGET,
            "step {step}: tile memory {memory} over the budget {BUDGET}: {stats:?}"
        );
    }
    let stats = store.stats();
    eprintln!(
        "history stress: {STEPS} steps {counts:?}, {} history steps at the end ({longest} at most), peak tile memory {:.1} MB (budget {:.1} MB), peak spilled {:.1} MB, tiles {}",
        made.len(),
        peak_memory as f64 / 1048576.0,
        BUDGET as f64 / 1048576.0,
        peak_spilled as f64 / 1048576.0,
        stats.tile_count
    );
    assert!(peak_spilled > 0, "history should have paged out: {stats:?}");
    assert!(longest >= 100, "the history should grow long");
    engine
        .dispatch(doc, "history.jump", json!({"index": 0}))
        .unwrap();
    assert_eq!(doc_hash(&engine.snapshot(doc).unwrap().state), original);
    // Redo all the way equals the last step as made.
    let last = made.len() - 1;
    engine
        .dispatch(doc, "history.jump", json!({"index": last}))
        .unwrap();
    assert_eq!(doc_hash(&engine.snapshot(doc).unwrap().state), made[last]);
    let mine: Vec<_> = spill_files()
        .into_iter()
        .filter(|f| !before.contains(f))
        .collect();
    assert!(!mine.is_empty(), "the document should have a spill file");
    drop(store);
    engine.close(doc).unwrap();
    drop(engine);
    let left: Vec<_> = spill_files()
        .into_iter()
        .filter(|f| mine.contains(f))
        .collect();
    assert!(left.is_empty(), "spill files left behind: {left:?}");
}
