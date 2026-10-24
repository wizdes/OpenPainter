use op_core::doc::DocState;
use op_core::history::History;
use op_core::tiles::{TILE_BYTES, TileCoord, TileData, TileStateKind, TileStore};
use std::sync::Arc;
use std::time::{Duration, Instant};

const VIEWPORT: (u32, u32, u32, u32) = (0, 0, 1920, 1080);

fn tile(seed: u64) -> TileData {
    let mut n = seed;
    let mut bytes = vec![0; TILE_BYTES];
    for byte in &mut bytes {
        n ^= n << 13;
        n ^= n >> 7;
        n ^= n << 17;
        *byte = n as u8;
    }
    TileData::try_from(bytes).unwrap()
}

/// Counts the viewport tiles of a state that are spilled, and all its viewport tiles.
fn spilled_viewport_tiles(store: &TileStore, state: &DocState) -> (usize, usize) {
    let (mut spilled, mut total) = (0, 0);
    for layer in &state.layers {
        for y in 0..VIEWPORT.3.div_ceil(256) as i32 {
            for x in 0..VIEWPORT.2.div_ceil(256) as i32 {
                if let Some(id) = layer.tiles.get(TileCoord { x, y }) {
                    total += 1;
                    spilled += usize::from(store.state(id).unwrap() == TileStateKind::Spilled);
                }
            }
        }
    }
    (spilled, total)
}

fn main() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(TileStore::new(dir.path(), 8 * 1024 * 1024).unwrap());
    let mut state = DocState::new(store.clone(), 6000, 4000, 72.0).unwrap();
    state = state
        .add_layer(1, "Middle")
        .unwrap()
        .add_layer(2, "Top")
        .unwrap();
    for layer in 0..3 {
        for y in 0..16 {
            for x in 0..24 {
                state = state
                    .write_tile(
                        layer,
                        TileCoord { x, y },
                        tile((layer * 10000 + y as usize * 24 + x as usize + 1) as u64),
                    )
                    .unwrap();
            }
        }
    }
    let mut history = History::new(state);
    for step in 0..50 {
        let next = history
            .current_state()
            .write_tile(
                step % 3,
                TileCoord {
                    x: (step % 24) as i32,
                    y: ((step / 24) % 16) as i32,
                },
                tile((step + 100000) as u64),
            )
            .unwrap();
        history.commit("paint", "paint", next);
    }

    // Pure jump: a pointer swap that touches no pixel bytes.
    let mut jumps = Vec::new();
    for _ in 0..10 {
        let start = Instant::now();
        history.jump(0).unwrap();
        std::hint::black_box(history.current_state().revision);
        history.jump(50).unwrap();
        std::hint::black_box(history.current_state().revision);
        jumps.push(start.elapsed());
    }
    let slowest = *jumps.iter().max().unwrap();
    println!(
        "50-step resident jump to 0 and back: best {:?}, worst {:?} (10 runs)",
        jumps.iter().min().unwrap(),
        slowest
    );

    // Read tiles outside the viewport so the budget spills step 0's viewport tiles.
    for x in 0..24 {
        for y in 10..12 {
            std::hint::black_box(
                history
                    .current_state()
                    .read_tile(0, TileCoord { x, y })
                    .unwrap(),
            );
        }
    }
    let (spilled, total) = spilled_viewport_tiles(&store, history.state(0).unwrap());
    let start = Instant::now();
    history.jump(0).unwrap();
    let jumped = start.elapsed();
    let view = history.ensure_resident(VIEWPORT).unwrap();
    let loaded = start.elapsed();
    assert_eq!(view.len(), total);
    println!(
        "50-step jump, then ensure_resident 1920x1080 ({spilled} of {total} viewport tiles spilled): \
         total {loaded:?} (jump {jumped:?}, load {:?})",
        loaded - jumped
    );
    println!("tiles: {}, {:?}", store.stats().tile_count, store.stats());
    assert_eq!(
        spilled, total,
        "step 0's viewport tiles were not all spilled"
    );
    drop(view);
    assert!(
        slowest < Duration::from_millis(200),
        "resident jump exceeds 200 ms"
    );
    drop(history);
    assert_eq!(store.stats().tile_count, 0);
    assert_eq!(op_core::doc::release_failures(), 0);
}
