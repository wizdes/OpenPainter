use op_core::tiles::{SpillIo, TILE_BYTES, TileData, TileError, TileId, TileStateKind, TileStore};
use proptest::prelude::*;
use proptest::test_runner::TestCaseError;
use std::collections::HashMap;
use std::io;
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

fn patterned(seed: u64) -> Vec<u8> {
    let mut value = seed;
    let mut block = [0u8; 4096];
    for byte in &mut block {
        value ^= value << 13;
        value ^= value >> 7;
        value ^= value << 17;
        *byte = value as u8;
    }
    block.repeat(TILE_BYTES / block.len())
}

fn next(value: &mut u64) -> u64 {
    *value ^= *value << 13;
    *value ^= *value >> 7;
    *value ^= *value << 17;
    *value
}

#[test]
fn tile_keeps_hidden_color() {
    let dir = tempfile::tempdir().unwrap();
    let store = TileStore::new(dir.path(), TILE_BYTES).unwrap();
    let mut pixels = vec![0; TILE_BYTES];
    pixels[..4].copy_from_slice(&[91, 42, 7, 0]);
    let id = store
        .insert(TileData::try_from(pixels.clone()).unwrap())
        .unwrap();
    assert_eq!(&*store.get(id).unwrap(), pixels.as_slice());
}

#[derive(Clone, Copy)]
enum Fault {
    None,
    Read,
    ShortWrite,
    DiskFull,
    Truncate,
    Flip,
}

struct MemoryIo {
    bytes: Vec<u8>,
    fault: Fault,
}

struct SharedIo(Arc<Mutex<MemoryIo>>);

impl SpillIo for SharedIo {
    fn append(&mut self, bytes: &[u8]) -> io::Result<u64> {
        let mut io = self.0.lock().unwrap();
        match io.fault {
            Fault::ShortWrite => {
                return Err(io::Error::new(io::ErrorKind::WriteZero, "short write"));
            }
            Fault::DiskFull => return Err(io::Error::from_raw_os_error(28)),
            _ => {}
        }
        let offset = io.bytes.len() as u64;
        io.bytes.extend_from_slice(bytes);
        Ok(offset)
    }

    fn read_exact_at(&mut self, offset: u64, bytes: &mut [u8]) -> io::Result<()> {
        let io = self.0.lock().unwrap();
        if matches!(io.fault, Fault::Read) {
            return Err(io::Error::other("injected read failure"));
        }
        let end = offset as usize + bytes.len();
        let source = io
            .bytes
            .get(offset as usize..end)
            .ok_or(io::ErrorKind::UnexpectedEof)?;
        if matches!(io.fault, Fault::Truncate) {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        bytes.copy_from_slice(source);
        if matches!(io.fault, Fault::Flip) {
            bytes[20] ^= 1;
        }
        Ok(())
    }
}

fn memory_store(budget: usize) -> (TileStore, Arc<Mutex<MemoryIo>>) {
    let io = Arc::new(Mutex::new(MemoryIo {
        bytes: Vec::new(),
        fault: Fault::None,
    }));
    (
        TileStore::with_io(budget, Box::new(SharedIo(io.clone()))),
        io,
    )
}

#[test]
fn write_faults_leave_old_tile_unchanged() {
    for fault in [Fault::ShortWrite, Fault::DiskFull] {
        let (store, io) = memory_store(TILE_BYTES);
        let old = patterned(1);
        let id = store
            .insert(TileData::try_from(old.clone()).unwrap())
            .unwrap();
        io.lock().unwrap().fault = fault;
        let before = store.stats();
        assert!(matches!(
            store.insert(TileData::try_from(patterned(2)).unwrap()),
            Err(TileError::Io(_))
        ));
        assert_eq!(store.stats(), before);
        assert_eq!(store.state(id).unwrap(), TileStateKind::Resident);
        assert_eq!(&*store.get(id).unwrap(), old);
    }
}

#[test]
fn read_faults_and_corruption_leave_spilled_state() {
    for fault in [Fault::Read, Fault::Truncate, Fault::Flip] {
        let (store, io) = memory_store(0);
        let expected = patterned(5);
        let id = store
            .insert(TileData::try_from(expected.clone()).unwrap())
            .unwrap();
        assert_eq!(store.state(id).unwrap(), TileStateKind::Spilled);
        let before = store.stats();
        io.lock().unwrap().fault = fault;
        let result = store.get(id);
        assert!(matches!(
            result,
            Err(TileError::Io(_)) | Err(TileError::Corrupt)
        ));
        assert_eq!(store.stats(), before);
        assert_eq!(store.state(id).unwrap(), TileStateKind::Spilled);
        io.lock().unwrap().fault = Fault::None;
        assert_eq!(&*store.get(id).unwrap(), expected);
    }
}

#[test]
fn hidden_color_survives_spill_and_reload() {
    let dir = tempfile::tempdir().unwrap();
    let store = TileStore::new(dir.path(), 0).unwrap();
    let mut pixels = vec![0; TILE_BYTES];
    pixels[..4].copy_from_slice(&[19, 82, 233, 0]);
    let data = TileData::try_from(pixels.clone()).unwrap();
    assert!(!data.is_transparent_black());
    let id = store.insert(data).unwrap();
    assert_eq!(store.state(id).unwrap(), TileStateKind::Spilled);
    assert_eq!(&*store.get(id).unwrap(), pixels);
}

#[test]
fn pins_keep_tiles_resident_under_pressure() {
    let dir = tempfile::tempdir().unwrap();
    let store = TileStore::new(dir.path(), TILE_BYTES).unwrap();
    let id = store
        .insert(TileData::try_from(patterned(1)).unwrap())
        .unwrap();
    store.pin(id).unwrap();
    for seed in 2..10 {
        store
            .insert(TileData::try_from(patterned(seed)).unwrap())
            .unwrap();
        assert_eq!(store.state(id).unwrap(), TileStateKind::Resident);
    }
    store.unpin(id).unwrap();
}

#[test]
fn reload_keeps_memory_within_budget() {
    let dir = tempfile::tempdir().unwrap();
    let store = TileStore::new(dir.path(), 8 * 1024 * 1024).unwrap();
    let mut ids = Vec::new();
    for seed in 1..=48 {
        ids.push(
            store
                .insert(TileData::try_from(patterned(seed)).unwrap())
                .unwrap(),
        );
    }
    for id in ids {
        assert_eq!(store.get(id).unwrap().len(), TILE_BYTES);
        let stats = store.stats();
        assert!(stats.resident_bytes + stats.compressed_bytes <= 8 * 1024 * 1024);
    }
}

#[test]
fn two_hundred_tiles_survive_budget_pressure() {
    let dir = tempfile::tempdir().unwrap();
    let store = TileStore::new(dir.path(), 8 * 1024 * 1024).unwrap();
    let mut model = HashMap::new();
    for seed in 1..=200 {
        let bytes = patterned(seed);
        let id = store
            .insert(TileData::try_from(bytes.clone()).unwrap())
            .unwrap();
        model.insert(id, bytes);
    }
    for (id, bytes) in model {
        assert_eq!(&*store.get(id).unwrap(), bytes);
    }
    let stats = store.stats();
    assert!(stats.spilled_bytes > 0);
    assert!(stats.resident_bytes + stats.compressed_bytes <= 8 * 1024 * 1024);
}

#[test]
fn stats_track_each_storage_state_and_freed_bytes() {
    let (store, _) = memory_store(TILE_BYTES + 100);
    let first = store
        .insert(TileData::try_from(vec![0; TILE_BYTES]).unwrap())
        .unwrap();
    let second = store
        .insert(TileData::try_from(patterned(3)).unwrap())
        .unwrap();
    let stats = store.stats();
    assert_eq!(stats.resident_bytes, TILE_BYTES);
    assert_eq!(stats.compressed_bytes, 0);
    assert_eq!(
        stats.spilled_bytes,
        17 + lz4_flex::block::compress(&vec![0; TILE_BYTES]).len()
    );
    assert_eq!(stats.tile_count, 2);
    assert_eq!(store.state(first).unwrap(), TileStateKind::Spilled);
    store.release(first).unwrap();
    let stats = store.stats();
    assert_eq!(stats.freed_bytes, TILE_BYTES);
    assert_eq!(stats.spilled_bytes, 0);
    store.release(second).unwrap();
    assert_eq!(store.stats().freed_bytes, TILE_BYTES * 2);
}

#[test]
fn compressed_bytes_are_exact() {
    let (store, _) = memory_store(TILE_BYTES + 2000);
    let first = store
        .insert(TileData::try_from(vec![0; TILE_BYTES]).unwrap())
        .unwrap();
    let second = store
        .insert(TileData::try_from(vec![1; TILE_BYTES]).unwrap())
        .unwrap();
    assert_eq!(store.state(first).unwrap(), TileStateKind::Compressed);
    assert_eq!(store.state(second).unwrap(), TileStateKind::Resident);
    assert_eq!(
        store.stats().compressed_bytes,
        lz4_flex::block::compress(&vec![0; TILE_BYTES]).len()
    );
}

#[test]
fn owner_and_pin_counts_control_freeing() {
    let (store, _) = memory_store(TILE_BYTES);
    let id = store
        .insert(TileData::try_from(patterned(4)).unwrap())
        .unwrap();
    store.retain(id).unwrap();
    store.pin(id).unwrap();
    store.release(id).unwrap();
    store.release(id).unwrap();
    assert_eq!(store.state(id).unwrap(), TileStateKind::Resident);
    store.unpin(id).unwrap();
    assert!(matches!(store.get(id), Err(TileError::Missing(_))));
}

#[test]
fn held_tile_allows_other_store_calls() {
    let (store, _) = memory_store(2 * TILE_BYTES);
    let first = store
        .insert(TileData::try_from(patterned(1)).unwrap())
        .unwrap();
    let second = store
        .insert(TileData::try_from(patterned(2)).unwrap())
        .unwrap();
    let first_ref = store.get(first).unwrap();
    let second_ref = store.get(second).unwrap();
    assert_eq!(first_ref[0], patterned(1)[0]);
    assert_eq!(second_ref[0], patterned(2)[0]);
    assert_eq!(store.stats().tile_count, 2);
}

#[test]
fn dropping_last_handle_reclaims_budget() {
    let (store, _) = memory_store(TILE_BYTES);
    let id = store
        .insert(TileData::try_from(patterned(1)).unwrap())
        .unwrap();
    let held = store.get(id).unwrap();
    let second = store
        .insert(TileData::try_from(patterned(2)).unwrap())
        .unwrap();
    let held_second = store.get(second).unwrap();
    assert!(store.stats().resident_bytes > TILE_BYTES);
    drop(held_second);
    let stats = store.stats();
    assert!(stats.resident_bytes + stats.compressed_bytes <= TILE_BYTES);
    drop(held);
}

#[test]
fn failed_eviction_on_handle_drop_is_reported() {
    let (store, io) = memory_store(TILE_BYTES);
    let first = store
        .insert(TileData::try_from(patterned(1)).unwrap())
        .unwrap();
    let held_first = store.get(first).unwrap();
    let second = store
        .insert(TileData::try_from(patterned(2)).unwrap())
        .unwrap();
    let held_second = store.get(second).unwrap();
    io.lock().unwrap().fault = Fault::DiskFull;
    drop(held_second);
    assert!(matches!(
        store.take_deferred_error(),
        Some(TileError::Io(_))
    ));
    assert_eq!(store.state(second).unwrap(), TileStateKind::Resident);
    io.lock().unwrap().fault = Fault::None;
    store.set_budget(TILE_BYTES).unwrap();
    assert_eq!(store.state(second).unwrap(), TileStateKind::Spilled);
    drop(held_first);
}

#[test]
fn random_operations_preserve_every_live_tile() {
    let cases = std::env::var("OP_PROPTEST_CASES")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(64);
    let mut runner = proptest::test_runner::TestRunner::new(proptest::test_runner::Config {
        cases,
        failure_persistence: None,
        ..Default::default()
    });
    runner
        .run(&any::<u64>(), |seed| {
            let dir = tempfile::tempdir().unwrap();
            let store = TileStore::new(dir.path(), 8 * 1024 * 1024).unwrap();
            let mut rng = seed | 1;
            let mut inserted = 0u64;
            let mut live: HashMap<TileId, (Vec<u8>, usize, usize)> = HashMap::new();
            for step in 0..2000 {
                let roll = next(&mut rng) % 16;
                if live.is_empty() || (roll < 4 && inserted < 200 && live.len() < 2) {
                    inserted += 1;
                    let bytes = patterned(next(&mut rng));
                    let id = store
                        .insert(TileData::try_from(bytes.clone()).unwrap())
                        .unwrap();
                    live.insert(id, (bytes, 1, 0));
                } else {
                    let index = next(&mut rng) as usize % live.len();
                    let id = *live.keys().nth(index).unwrap();
                    let entry = live.get_mut(&id).unwrap();
                    match roll {
                        13..=15 => {
                            assert_eq!(&*store.get(id).unwrap(), entry.0);
                        }
                        10 if entry.2 == 0 => {
                            store.pin(id).unwrap();
                            entry.2 += 1;
                        }
                        11 if entry.2 > 0 => {
                            store.unpin(id).unwrap();
                            entry.2 -= 1;
                            if entry.1 == 0 && entry.2 == 0 {
                                live.remove(&id);
                            }
                        }
                        12 if entry.1 < 2 => {
                            store.retain(id).unwrap();
                            entry.1 += 1;
                        }
                        4..=9 if entry.1 > 0 => {
                            store.release(id).unwrap();
                            entry.1 -= 1;
                            if entry.1 == 0 && entry.2 == 0 {
                                live.remove(&id);
                            }
                        }
                        _ => {}
                    }
                }
                for (&id, (bytes, _, _)) in &live {
                    prop_assert_eq!(
                        &*store.get(id).unwrap(),
                        bytes,
                        "step {}, tile {:?}",
                        step,
                        id
                    );
                }
            }
            prop_assert!(inserted >= 200);
            Ok(())
        })
        .unwrap();
}

#[test]
fn resident_gets_share_one_allocation() {
    let (store, _) = memory_store(4 * TILE_BYTES);
    let id = store
        .insert(TileData::try_from(patterned(7)).unwrap())
        .unwrap();
    let first = store.get(id).unwrap();
    let second = store.get(id).unwrap();
    assert_eq!(first.as_ptr(), second.as_ptr());
    let pointer = first.as_ptr();
    drop(first);
    drop(second);
    assert_eq!(store.state(id).unwrap(), TileStateKind::Resident);
    assert_eq!(store.get(id).unwrap().as_ptr(), pointer);
}

/// Runs the same operation mix as `random_operations_preserve_every_live_tile` and checks
/// after every step that the running counters equal a full recount.
fn random_sequence_keeps_counters_exact(
    seed: u64,
    budget: usize,
    max_live: usize,
) -> std::result::Result<(), TestCaseError> {
    let (store, _) = memory_store(budget);
    let mut rng = seed | 1;
    let mut inserted = 0u64;
    let mut live: HashMap<TileId, (Vec<u8>, usize, usize)> = HashMap::new();
    let (mut saw_compressed, mut saw_spilled) = (false, false);
    for step in 0..2000 {
        let roll = next(&mut rng) % 16;
        if live.is_empty() || (roll < 4 && inserted < 200 && live.len() < max_live) {
            inserted += 1;
            let bytes = patterned(next(&mut rng));
            let id = store
                .insert(TileData::try_from(bytes.clone()).unwrap())
                .unwrap();
            live.insert(id, (bytes, 1, 0));
        } else {
            let index = next(&mut rng) as usize % live.len();
            let id = *live.keys().nth(index).unwrap();
            let entry = live.get_mut(&id).unwrap();
            match roll {
                13..=15 => {
                    prop_assert_eq!(&*store.get(id).unwrap(), entry.0.as_slice());
                }
                10 if entry.2 == 0 => {
                    store.pin(id).unwrap();
                    entry.2 += 1;
                }
                11 if entry.2 > 0 => {
                    store.unpin(id).unwrap();
                    entry.2 -= 1;
                    if entry.1 == 0 && entry.2 == 0 {
                        live.remove(&id);
                    }
                }
                12 if entry.1 < 2 => {
                    store.retain(id).unwrap();
                    entry.1 += 1;
                }
                4..=9 if entry.1 > 0 => {
                    store.release(id).unwrap();
                    entry.1 -= 1;
                    if entry.1 == 0 && entry.2 == 0 {
                        live.remove(&id);
                    }
                }
                _ => {}
            }
        }
        let stats = store.stats();
        prop_assert_eq!(stats, store.debug_recount_stats(), "step {}", step);
        saw_compressed |= stats.compressed_bytes > 0;
        saw_spilled |= stats.spilled_bytes > 0;
    }
    if budget < 2 * max_live * TILE_BYTES {
        prop_assert!(
            saw_compressed && saw_spilled,
            "the tight budget never evicted"
        );
    }
    prop_assert!(store.take_deferred_error().is_none());
    for (&id, (bytes, _, pins)) in &live {
        if *pins > 0 {
            prop_assert_eq!(store.state(id).unwrap(), TileStateKind::Resident);
        }
        prop_assert_eq!(&*store.get(id).unwrap(), bytes.as_slice());
    }
    prop_assert_eq!(store.stats(), store.debug_recount_stats());
    prop_assert_eq!(store.stats().tile_count, live.len());
    Ok(())
}

#[test]
fn random_operations_keep_counters_equal_to_recount() {
    let cases = std::env::var("OP_PROPTEST_CASES")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(64);
    let mut runner = proptest::test_runner::TestRunner::new(proptest::test_runner::Config {
        cases,
        failure_persistence: None,
        ..Default::default()
    });
    runner
        .run(&any::<u64>(), |seed| {
            // The original budget and live limit, then a tight budget that compresses and spills.
            random_sequence_keeps_counters_exact(seed, 8 * 1024 * 1024, 2)?;
            random_sequence_keeps_counters_exact(seed, 2 * TILE_BYTES + 8192, 8)
        })
        .unwrap();
}

/// Blocks the first append until the test sends on `release`.
struct GateIo {
    bytes: Vec<u8>,
    entered: mpsc::Sender<()>,
    release: Option<mpsc::Receiver<()>>,
}

impl SpillIo for GateIo {
    fn append(&mut self, bytes: &[u8]) -> io::Result<u64> {
        if let Some(release) = self.release.take() {
            self.entered.send(()).unwrap();
            release.recv().unwrap();
        }
        let offset = self.bytes.len() as u64;
        self.bytes.extend_from_slice(bytes);
        Ok(offset)
    }

    fn read_exact_at(&mut self, offset: u64, bytes: &mut [u8]) -> io::Result<()> {
        let end = offset as usize + bytes.len();
        bytes.copy_from_slice(
            self.bytes
                .get(offset as usize..end)
                .ok_or(io::ErrorKind::UnexpectedEof)?,
        );
        Ok(())
    }
}

#[test]
fn blocked_spill_write_does_not_block_other_gets() {
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let (done_tx, done_rx) = mpsc::channel();
    let store = TileStore::with_io(
        2 * TILE_BYTES,
        Box::new(GateIo {
            bytes: Vec::new(),
            entered: entered_tx,
            release: Some(release_rx),
        }),
    );
    let pixels: Vec<_> = (1..=3).map(patterned).collect();
    let moving = store
        .insert(TileData::try_from(pixels[0].clone()).unwrap())
        .unwrap();
    let other = store
        .insert(TileData::try_from(pixels[1].clone()).unwrap())
        .unwrap();
    let timeout = Duration::from_secs(5);
    let newest = std::thread::scope(|scope| {
        let (store, pixels) = (&store, &pixels);
        // The third tile pushes memory over budget, so the oldest tile is spilled.
        let writer =
            scope.spawn(move || store.insert(TileData::try_from(pixels[2].clone()).unwrap()));
        entered_rx
            .recv_timeout(timeout)
            .expect("the spill write did not start");
        scope.spawn(move || {
            for (id, expected) in [(other, &pixels[1]), (moving, &pixels[0])] {
                let tile = store.get(id).unwrap();
                let same = tile[..] == expected[..];
                drop(tile);
                done_tx.send(same).unwrap();
            }
        });
        let other_get = done_rx.recv_timeout(timeout);
        let moving_get = other_get.is_ok().then(|| done_rx.recv_timeout(timeout));
        let moving_state = moving_get
            .as_ref()
            .is_some_and(Result::is_ok)
            .then(|| store.state(moving).unwrap());
        release_tx.send(()).unwrap();
        let newest = writer.join().unwrap().unwrap();
        assert_eq!(
            other_get,
            Ok(true),
            "a get of another resident tile waited for the spill write"
        );
        assert_eq!(
            moving_get,
            Some(Ok(true)),
            "a get of the tile being spilled failed or waited"
        );
        assert_eq!(moving_state, Some(TileStateKind::Resident));
        newest
    });
    for (id, expected) in [moving, other, newest].into_iter().zip(&pixels) {
        assert_eq!(&*store.get(id).unwrap(), expected.as_slice());
    }
    let stats = store.stats();
    assert_eq!(stats, store.debug_recount_stats());
    assert!(stats.resident_bytes + stats.compressed_bytes <= 2 * TILE_BYTES);
    assert!(store.take_deferred_error().is_none());
}
