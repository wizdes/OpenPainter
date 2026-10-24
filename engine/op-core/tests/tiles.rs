use op_core::tiles::{
    SpillIo, TILE_BYTES, TileData, TileError, TileId, TileRef, TileStateKind, TileStore,
};
use proptest::prelude::*;
use proptest::test_runner::TestCaseError;
use std::collections::{BTreeMap, HashMap};
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

/// Seeded random bytes that LZ4 cannot shrink, so evicting this tile always spills it.
fn noise(seed: u64) -> Vec<u8> {
    let mut value = seed | 1;
    (0..TILE_BYTES).map(|_| next(&mut value) as u8).collect()
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
    /// Writes some junk bytes, then fails, like a write cut short by a full disk.
    Junk,
    PanicWrite,
    PanicRead,
}

struct MemoryIo {
    bytes: Vec<u8>,
    fault: Fault,
}

struct SharedIo(Arc<Mutex<MemoryIo>>);

impl SpillIo for SharedIo {
    fn append(&self, bytes: &[u8]) -> io::Result<u64> {
        let mut io = self.0.lock().unwrap();
        match io.fault {
            Fault::ShortWrite => {
                return Err(io::Error::new(io::ErrorKind::WriteZero, "short write"));
            }
            Fault::DiskFull => return Err(io::Error::from_raw_os_error(28)),
            Fault::Junk => {
                io.bytes.extend_from_slice(&[0xA5; 100]);
                return Err(io::Error::from_raw_os_error(28));
            }
            Fault::PanicWrite => {
                drop(io);
                panic!("injected panic in append");
            }
            _ => {}
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
        if matches!(io.fault, Fault::PanicRead) {
            drop(io);
            panic!("injected panic in read");
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
    for fault in [Fault::ShortWrite, Fault::DiskFull, Fault::Junk] {
        let (store, io) = memory_store(TILE_BYTES);
        let old = patterned(1);
        let id = store
            .insert(TileData::try_from(old.clone()).unwrap())
            .unwrap();
        io.lock().unwrap().fault = fault;
        // Eviction is best effort: the insert succeeds even though its eviction write fails.
        let newer = store.insert(TileData::try_from(noise(2)).unwrap()).unwrap();
        let stats = store.stats();
        assert_eq!(stats, store.debug_recount_stats());
        assert_eq!(stats.evict_failures, 1);
        assert!(stats.last_evict_error.is_some());
        assert!(stats.over_budget);
        assert_eq!(stats.resident_bytes, 2 * TILE_BYTES);
        assert_eq!(store.state(id).unwrap(), TileStateKind::Resident);
        assert_eq!(&*store.get(id).unwrap(), old);
        assert_eq!(&*store.get(newer).unwrap(), noise(2));
        assert!(matches!(
            store.take_deferred_error(),
            Some(TileError::Io(_))
        ));
        // Each of those gets retried the eviction and failed again.
        let failures = store.stats().evict_failures;
        assert!(failures > 1);
        // The next operation tries again and succeeds.
        io.lock().unwrap().fault = Fault::None;
        store.set_budget(TILE_BYTES);
        let stats = store.stats();
        assert!(!stats.over_budget);
        assert_eq!(stats.evict_failures, failures);
        assert_eq!(&*store.get(id).unwrap(), old);
        assert_eq!(&*store.get(newer).unwrap(), noise(2));
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
    // Both tiles are held before the budget drops, so neither has a spill record yet and
    // evicting `second` needs a real write.
    let (store, io) = memory_store(2 * TILE_BYTES);
    let first = store
        .insert(TileData::try_from(patterned(1)).unwrap())
        .unwrap();
    let held_first = store.get(first).unwrap();
    let second = store
        .insert(TileData::try_from(patterned(2)).unwrap())
        .unwrap();
    let held_second = store.get(second).unwrap();
    store.set_budget(TILE_BYTES);
    io.lock().unwrap().fault = Fault::DiskFull;
    drop(held_second);
    assert!(matches!(
        store.take_deferred_error(),
        Some(TileError::Io(_))
    ));
    assert_eq!(store.state(second).unwrap(), TileStateKind::Resident);
    io.lock().unwrap().fault = Fault::None;
    store.set_budget(TILE_BYTES);
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
        prop_assert_eq!(stats.clone(), store.debug_recount_stats(), "step {}", step);
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
    bytes: Mutex<Vec<u8>>,
    entered: mpsc::Sender<()>,
    release: Mutex<Option<mpsc::Receiver<()>>>,
}

impl SpillIo for GateIo {
    fn append(&self, bytes: &[u8]) -> io::Result<u64> {
        let release = self.release.lock().unwrap().take();
        if let Some(release) = release {
            self.entered.send(()).unwrap();
            release.recv().unwrap();
        }
        let mut stored = self.bytes.lock().unwrap();
        let offset = stored.len() as u64;
        stored.extend_from_slice(bytes);
        Ok(offset)
    }

    fn read_exact_at(&self, offset: u64, bytes: &mut [u8]) -> io::Result<()> {
        let end = offset as usize + bytes.len();
        bytes.copy_from_slice(
            self.bytes
                .lock()
                .unwrap()
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
            bytes: Mutex::new(Vec::new()),
            entered: entered_tx,
            release: Mutex::new(Some(release_rx)),
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

fn tile(bytes: &[u8]) -> TileData {
    TileData::try_from(bytes.to_vec()).unwrap()
}

fn spill_files(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    files.sort();
    files
}

#[test]
fn failed_eviction_during_unpin_keeps_the_unpin_done() {
    let (store, io) = memory_store(2 * TILE_BYTES);
    let held = store.insert(tile(&noise(1))).unwrap();
    store.pin(held).unwrap();
    let other = store.insert(tile(&noise(2))).unwrap();
    store.set_budget(TILE_BYTES / 2);
    assert_eq!(store.state(other).unwrap(), TileStateKind::Spilled);
    io.lock().unwrap().fault = Fault::DiskFull;
    // Unpinning makes the held tile the only eviction candidate, and its spill write fails.
    store.unpin(held).unwrap();
    assert!(matches!(store.unpin(held), Err(TileError::InvalidCount)));
    assert_eq!(store.state(held).unwrap(), TileStateKind::Resident);
    assert_eq!(&*store.get(held).unwrap(), noise(1));
    io.lock().unwrap().fault = Fault::None;
    store.set_budget(TILE_BYTES / 2);
    assert_eq!(store.state(held).unwrap(), TileStateKind::Spilled);
    assert_eq!(&*store.get(held).unwrap(), noise(1));
}

#[test]
fn unpin_does_not_take_a_guard_pin() {
    let (store, _) = memory_store(4 * TILE_BYTES);
    let id = store.insert(tile(&patterned(1))).unwrap();
    let guard = store.get(id).unwrap();
    assert!(matches!(store.unpin(id), Err(TileError::InvalidCount)));
    drop(guard);
    assert!(matches!(store.unpin(id), Err(TileError::InvalidCount)));
    assert_eq!(store.stats(), store.debug_recount_stats());
    store.release(id).unwrap();
    assert_eq!(store.stats().tile_count, 0);
}

#[test]
fn reloading_a_spilled_tile_does_not_grow_the_spill_file() {
    let (store, io) = memory_store(0);
    let id = store.insert(tile(&noise(3))).unwrap();
    assert_eq!(store.state(id).unwrap(), TileStateKind::Spilled);
    let written = io.lock().unwrap().bytes.len();
    for _ in 0..10 {
        assert_eq!(&*store.get(id).unwrap(), noise(3));
        assert_eq!(store.state(id).unwrap(), TileStateKind::Spilled);
    }
    assert_eq!(io.lock().unwrap().bytes.len(), written);
}

#[test]
fn spill_file_is_deleted_when_the_store_drops() {
    let dir = tempfile::tempdir().unwrap();
    let store = TileStore::new(dir.path(), 0).unwrap();
    store.insert(tile(&noise(4))).unwrap();
    assert_eq!(spill_files(dir.path()).len(), 1);
    drop(store);
    assert_eq!(spill_files(dir.path()), Vec::<std::path::PathBuf>::new());
}

#[test]
fn open_skips_a_spill_file_name_that_already_exists() {
    let dir = tempfile::tempdir().unwrap();
    let probe = TileStore::new(dir.path(), 0).unwrap();
    let name = spill_files(dir.path())[0]
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    drop(probe);
    // Names look like `op-tiles-<pid>-<sequence>.spill`; take the following names first.
    let stem = name.strip_suffix(".spill").unwrap();
    let (prefix, sequence) = stem.rsplit_once('-').unwrap();
    let sequence: u64 = sequence.parse().unwrap();
    for next in sequence + 1..=sequence + 256 {
        std::fs::write(dir.path().join(format!("{prefix}-{next}.spill")), b"old").unwrap();
    }
    let before = spill_files(dir.path());
    let store = TileStore::new(dir.path(), 0).unwrap();
    let id = store.insert(tile(&noise(5))).unwrap();
    assert_eq!(&*store.get(id).unwrap(), noise(5));
    for path in &before {
        assert_eq!(std::fs::read(path).unwrap(), b"old");
    }
    drop(store);
    assert_eq!(spill_files(dir.path()), before);
}

/// Blocks one append (or one read), once armed, until the test sends on the release channel.
struct BlockingIo {
    bytes: Arc<Mutex<Vec<u8>>>,
    gate: Gate,
    read_gate: Gate,
}

fn wait_at(gate: &Gate) {
    let armed = gate.lock().unwrap().take();
    if let Some((entered, release)) = armed {
        entered.send(()).unwrap();
        release.recv().unwrap();
    }
}

impl SpillIo for BlockingIo {
    fn append(&self, bytes: &[u8]) -> io::Result<u64> {
        wait_at(&self.gate);
        let mut stored = self.bytes.lock().unwrap();
        let offset = stored.len() as u64;
        stored.extend_from_slice(bytes);
        Ok(offset)
    }

    fn read_exact_at(&self, offset: u64, bytes: &mut [u8]) -> io::Result<()> {
        wait_at(&self.read_gate);
        let stored = self.bytes.lock().unwrap();
        let end = offset as usize + bytes.len();
        bytes.copy_from_slice(
            stored
                .get(offset as usize..end)
                .ok_or(io::ErrorKind::UnexpectedEof)?,
        );
        Ok(())
    }
}

type Gate = Arc<Mutex<Option<(mpsc::Sender<()>, mpsc::Receiver<()>)>>>;

struct Gates {
    append: Gate,
    read: Gate,
    bytes: Arc<Mutex<Vec<u8>>>,
}

fn gated_store(budget: usize) -> (TileStore, Gates) {
    let gates = Gates {
        append: Arc::new(Mutex::new(None)),
        read: Arc::new(Mutex::new(None)),
        bytes: Arc::new(Mutex::new(Vec::new())),
    };
    let io = BlockingIo {
        bytes: gates.bytes.clone(),
        gate: gates.append.clone(),
        read_gate: gates.read.clone(),
    };
    (TileStore::with_io(budget, Box::new(io)), gates)
}

fn blocking_store(budget: usize) -> (TileStore, Gate) {
    let (store, gates) = gated_store(budget);
    (store, gates.append)
}

/// Arms the gate and returns (entered, release) for the test side.
fn arm(gate: &Gate) -> (mpsc::Receiver<()>, mpsc::Sender<()>) {
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    *gate.lock().unwrap() = Some((entered_tx, release_rx));
    (entered_rx, release_tx)
}

const WAIT: Duration = Duration::from_secs(5);

#[test]
fn blocked_spill_write_does_not_block_a_get_of_a_spilled_tile() {
    let (store, gate) = blocking_store(TILE_BYTES);
    let spilled = store.insert(tile(&noise(1))).unwrap();
    let moving = store.insert(tile(&noise(2))).unwrap();
    assert_eq!(store.state(spilled).unwrap(), TileStateKind::Spilled);
    let (entered, release) = arm(&gate);
    let (done_tx, done_rx) = mpsc::channel();
    std::thread::scope(|scope| {
        let store = &store;
        // This insert pushes memory over budget, so `moving` is spilled and its write blocks.
        let writer = scope.spawn(move || store.insert(tile(&noise(3))).unwrap());
        entered
            .recv_timeout(WAIT)
            .expect("the spill write did not start");
        scope.spawn(move || {
            let same = store.get(spilled).map(|guard| guard[..] == noise(1)[..]);
            done_tx.send(same.is_ok_and(|same| same)).unwrap();
        });
        let got = done_rx.recv_timeout(WAIT);
        release.send(()).unwrap();
        writer.join().unwrap();
        assert_eq!(
            got,
            Ok(true),
            "a get of a spilled tile waited for a spill write"
        );
    });
    assert_eq!(&*store.get(moving).unwrap(), noise(2));
    assert_eq!(store.stats(), store.debug_recount_stats());
}

#[test]
fn panic_in_spill_io_leaves_no_stuck_marks() {
    let (store, io) = memory_store(TILE_BYTES);
    let first = store.insert(tile(&noise(1))).unwrap();
    io.lock().unwrap().fault = Fault::PanicWrite;
    let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = store.insert(tile(&noise(2)));
    }));
    assert!(panicked.is_err());
    io.lock().unwrap().fault = Fault::None;
    assert_eq!(store.stats(), store.debug_recount_stats());
    store.set_budget(0);
    assert_eq!(store.state(first).unwrap(), TileStateKind::Spilled);
    assert_eq!(&*store.get(first).unwrap(), noise(1));
    assert_eq!(store.stats(), store.debug_recount_stats());
}

#[test]
fn panic_in_a_load_leaves_no_stuck_mark() {
    let (store, io) = memory_store(0);
    let id = store.insert(tile(&noise(1))).unwrap();
    io.lock().unwrap().fault = Fault::PanicRead;
    let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = store.get(id);
    }));
    assert!(panicked.is_err());
    io.lock().unwrap().fault = Fault::None;
    // A detached thread on a leaked store, so a stuck mark fails the test instead of hanging it.
    let store: &'static TileStore = Box::leak(Box::new(store));
    let (done_tx, done_rx) = mpsc::channel();
    std::thread::spawn(move || {
        let same = store.get(id).map(|guard| guard[..] == noise(1)[..]);
        done_tx.send(same.is_ok_and(|same| same)).unwrap();
    });
    assert_eq!(
        done_rx.recv_timeout(WAIT),
        Ok(true),
        "a later get waited for a load that had panicked"
    );
    assert_eq!(store.stats(), store.debug_recount_stats());
}

#[test]
fn two_callers_loading_one_tile_both_get_its_bytes() {
    let (store, gates) = gated_store(0);
    let id = store.insert(tile(&noise(1))).unwrap();
    let written = gates.bytes.lock().unwrap().len();
    let (entered, release) = arm(&gates.read);
    std::thread::scope(|scope| {
        let store = &store;
        let first = scope.spawn(move || store.get(id).unwrap()[..] == noise(1)[..]);
        entered
            .recv_timeout(WAIT)
            .expect("the first load did not start");
        let second = scope.spawn(move || store.get(id).unwrap()[..] == noise(1)[..]);
        // Other tiles stay usable while the load is blocked.
        let other = store.insert(tile(&patterned(2))).unwrap();
        assert_eq!(&*store.get(other).unwrap(), patterned(2));
        release.send(()).unwrap();
        assert!(first.join().unwrap());
        assert!(second.join().unwrap());
    });
    assert_eq!(store.state(id).unwrap(), TileStateKind::Spilled);
    assert_eq!(store.stats(), store.debug_recount_stats());
    // The reloaded tile reused its spill record; only `other` was written.
    let records = gates.bytes.lock().unwrap().len() - written;
    assert!(
        records < TILE_BYTES / 4,
        "unexpected spill growth: {records}"
    );
}

/// Starts an insert whose eviction spills `target` and blocks in the write, runs `during`
/// while the write is blocked, then lets the write finish.
fn while_move_is_blocked(
    store: &TileStore,
    gate: &Gate,
    newcomer: Vec<u8>,
    during: impl FnOnce(),
) -> TileId {
    let (entered, release) = arm(gate);
    std::thread::scope(|scope| {
        let writer = scope.spawn(move || store.insert(tile(&newcomer)).unwrap());
        entered
            .recv_timeout(WAIT)
            .expect("the spill write did not start");
        during();
        release.send(()).unwrap();
        writer.join().unwrap()
    })
}

#[test]
fn a_tile_pinned_during_its_move_stays_resident() {
    let (store, gates) = gated_store(TILE_BYTES);
    let target = store.insert(tile(&noise(1))).unwrap();
    let newcomer = while_move_is_blocked(&store, &gates.append, noise(2), || {
        store.pin(target).unwrap();
    });
    // The move finished while the tile was pinned, so it must not publish.
    assert_eq!(store.state(target).unwrap(), TileStateKind::Resident);
    assert_eq!(store.stats(), store.debug_recount_stats());
    // With `target` pinned, the insert evicted `newcomer` instead.
    assert_eq!(store.state(newcomer).unwrap(), TileStateKind::Spilled);
    let written = gates.bytes.lock().unwrap().len();
    store.unpin(target).unwrap();
    store.set_budget(TILE_BYTES / 2);
    // The finished write was kept, so evicting again needs no new write.
    assert_eq!(store.state(target).unwrap(), TileStateKind::Spilled);
    assert_eq!(gates.bytes.lock().unwrap().len(), written);
    assert_eq!(&*store.get(target).unwrap(), noise(1));
    assert_eq!(&*store.get(newcomer).unwrap(), noise(2));
    assert_eq!(store.stats(), store.debug_recount_stats());
}

#[test]
fn a_tile_used_during_its_move_is_not_evicted_by_that_move() {
    let (store, gates) = gated_store(TILE_BYTES);
    let target = store.insert(tile(&noise(1))).unwrap();
    let newcomer = while_move_is_blocked(&store, &gates.append, noise(2), || {
        assert_eq!(&*store.get(target).unwrap(), noise(1));
    });
    // The use made `target` newer than `newcomer`, so `newcomer` is evicted instead.
    assert_eq!(store.state(target).unwrap(), TileStateKind::Resident);
    assert_eq!(store.state(newcomer).unwrap(), TileStateKind::Spilled);
    assert_eq!(store.stats(), store.debug_recount_stats());
    assert_eq!(&*store.get(newcomer).unwrap(), noise(2));
}

#[test]
fn a_tile_freed_during_its_move_stays_freed() {
    let (store, gates) = gated_store(TILE_BYTES);
    let target = store.insert(tile(&noise(1))).unwrap();
    let newcomer = while_move_is_blocked(&store, &gates.append, noise(2), || {
        store.release(target).unwrap();
    });
    assert!(matches!(store.state(target), Err(TileError::Missing(_))));
    assert!(matches!(store.get(target), Err(TileError::Missing(_))));
    let stats = store.stats();
    assert_eq!(stats, store.debug_recount_stats());
    assert_eq!(stats.tile_count, 1);
    assert_eq!(stats.freed_bytes, TILE_BYTES);
    // The finished write belongs to no tile.
    assert_eq!(stats.dead_spill_bytes, gates.bytes.lock().unwrap().len());
    assert_eq!(&*store.get(newcomer).unwrap(), noise(2));
}

#[test]
fn freed_spill_records_count_as_dead_bytes() {
    let (store, io) = memory_store(0);
    let id = store.insert(tile(&noise(1))).unwrap();
    let written = io.lock().unwrap().bytes.len();
    assert_eq!(&*store.get(id).unwrap(), noise(1));
    assert_eq!(store.stats().spilled_bytes, written);
    assert_eq!(store.stats().dead_spill_bytes, 0);
    store.release(id).unwrap();
    let stats = store.stats();
    assert_eq!(stats.spilled_bytes, 0);
    assert_eq!(stats.dead_spill_bytes, written);
    assert_eq!(stats, store.debug_recount_stats());
}

#[test]
fn failed_set_budget_keeps_the_new_budget() {
    let (store, io) = memory_store(4 * TILE_BYTES);
    let ids: Vec<_> = (1..=4)
        .map(|seed| store.insert(tile(&noise(seed))).unwrap())
        .collect();
    io.lock().unwrap().fault = Fault::DiskFull;
    store.set_budget(TILE_BYTES);
    let stats = store.stats();
    assert!(stats.over_budget);
    assert_eq!(stats.evict_failures, 1);
    assert!(stats.last_evict_error.unwrap().contains("spill I/O failed"));
    io.lock().unwrap().fault = Fault::None;
    // Any later operation evicts toward the new budget without another set_budget call.
    store.retain(ids[3]).unwrap();
    store.release(ids[3]).unwrap();
    let stats = store.stats();
    assert!(!stats.over_budget);
    assert!(stats.resident_bytes + stats.compressed_bytes <= TILE_BYTES);
    for (seed, id) in (1..=4).zip(ids) {
        assert_eq!(&*store.get(id).unwrap(), noise(seed));
    }
}

#[test]
fn short_write_junk_before_the_next_record_is_harmless() {
    let (store, io) = memory_store(TILE_BYTES);
    let first = store.insert(tile(&noise(1))).unwrap();
    io.lock().unwrap().fault = Fault::Junk;
    let second = store.insert(tile(&noise(2))).unwrap();
    assert_eq!(io.lock().unwrap().bytes, vec![0xA5; 100]);
    io.lock().unwrap().fault = Fault::None;
    store.set_budget(TILE_BYTES);
    assert_eq!(store.state(first).unwrap(), TileStateKind::Spilled);
    assert_eq!(&io.lock().unwrap().bytes[100..104], b"OPTL");
    assert_eq!(&*store.get(first).unwrap(), noise(1));
    assert_eq!(&*store.get(second).unwrap(), noise(2));
    assert_eq!(store.stats(), store.debug_recount_stats());
}

#[test]
fn truncated_spill_file_gives_a_clean_error() {
    for keep in [5, 100, 1000] {
        let dir = tempfile::tempdir().unwrap();
        let store = TileStore::new(dir.path(), 0).unwrap();
        let id = store.insert(tile(&noise(1))).unwrap();
        let path = spill_files(dir.path())[0].clone();
        std::fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .unwrap()
            .set_len(keep)
            .unwrap();
        let before = store.stats();
        assert!(matches!(store.get(id), Err(TileError::Corrupt)));
        assert!(matches!(store.pin(id), Err(TileError::Corrupt)));
        assert_eq!(store.stats(), before);
        assert_eq!(store.state(id).unwrap(), TileStateKind::Spilled);
        assert_eq!(store.stats(), store.debug_recount_stats());
    }
}

fn proptest_cases() -> u32 {
    std::env::var("OP_PROPTEST_CASES")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(64)
}

const MIB: usize = 1024 * 1024;
const POOL: usize = 160;

/// Tile contents shared by every case, so cases do not spend their time making bytes.
/// Entry `k` of the noise pool is incompressible; entry `k` of the patterned pool is not.
fn pooled(noisy: bool, index: usize) -> Arc<[u8]> {
    static POOLS: std::sync::OnceLock<Vec<std::sync::OnceLock<Arc<[u8]>>>> =
        std::sync::OnceLock::new();
    let pools = POOLS.get_or_init(|| (0..2 * POOL).map(|_| std::sync::OnceLock::new()).collect());
    let slot = usize::from(noisy) * POOL + index % POOL;
    pools[slot]
        .get_or_init(|| {
            let seed = 0x9E37_79B9_7F4A_7C15 ^ (slot as u64 + 1);
            if noisy {
                noise(seed).into()
            } else {
                patterned(seed).into()
            }
        })
        .clone()
}

#[derive(Debug, Clone)]
enum Op {
    Insert { noisy: bool },
    Get { slot: usize, hold: bool },
    DropHeld { slot: usize },
    Pin { slot: usize, count: u8 },
    Unpin { slot: usize },
    Retain { slot: usize },
    Release { slot: usize },
}

fn op_strategy() -> impl Strategy<Value = Op> {
    prop_oneof![
        8 => any::<bool>().prop_map(|noisy| Op::Insert { noisy }),
        3 => (any::<usize>(), any::<bool>()).prop_map(|(slot, hold)| Op::Get { slot, hold }),
        1 => any::<usize>().prop_map(|slot| Op::DropHeld { slot }),
        1 => (any::<usize>(), 1u8..=3).prop_map(|(slot, count)| Op::Pin { slot, count }),
        2 => any::<usize>().prop_map(|slot| Op::Unpin { slot }),
        1 => any::<usize>().prop_map(|slot| Op::Retain { slot }),
        2 => any::<usize>().prop_map(|slot| Op::Release { slot }),
    ]
}

struct LiveTile<'a> {
    bytes: Arc<[u8]>,
    owners: usize,
    pins: usize,
    held: Vec<TileRef<'a>>,
}

const MAX_LIVE: usize = 120;

/// Runs one op sequence against a real file spill under an 8 MiB budget and checks every
/// live tile's bytes and the counters after every operation.
fn run_budget_case(base: usize, ops: &[Op]) -> std::result::Result<(), TestCaseError> {
    let dir = tempfile::tempdir().unwrap();
    let store = TileStore::new(dir.path(), 8 * MIB).unwrap();
    let mut live: BTreeMap<TileId, LiveTile<'_>> = BTreeMap::new();
    let mut inserted = 0;
    let mut peak_content = 0;
    for (step, op) in ops.iter().enumerate() {
        let pick = |slot: usize, live: &BTreeMap<TileId, LiveTile<'_>>| {
            (!live.is_empty()).then(|| *live.keys().nth(slot % live.len()).unwrap())
        };
        match *op {
            Op::Insert { noisy } => {
                if live.len() < MAX_LIVE {
                    let bytes = pooled(noisy, base + inserted);
                    inserted += 1;
                    let id = store.insert(tile(&bytes)).unwrap();
                    let entry = LiveTile {
                        bytes,
                        owners: 1,
                        pins: 0,
                        held: Vec::new(),
                    };
                    live.insert(id, entry);
                }
            }
            Op::Get { slot, hold } => {
                if let Some(id) = pick(slot, &live) {
                    let guard = store.get(id).unwrap();
                    let entry = live.get_mut(&id).unwrap();
                    prop_assert_eq!(&guard[..], &entry.bytes[..], "step {}", step);
                    if hold && entry.held.len() < 3 {
                        entry.held.push(guard);
                    }
                }
            }
            Op::DropHeld { slot } => {
                if let Some(id) = pick(slot, &live) {
                    let entry = live.get_mut(&id).unwrap();
                    entry.held.pop();
                    if entry.owners == 0 && entry.pins == 0 && entry.held.is_empty() {
                        live.remove(&id);
                    }
                }
            }
            Op::Pin { slot, count } => {
                if let Some(id) = pick(slot, &live) {
                    for _ in 0..count {
                        store.pin(id).unwrap();
                    }
                    live.get_mut(&id).unwrap().pins += usize::from(count);
                }
            }
            Op::Unpin { slot } => {
                if let Some(id) = pick(slot, &live) {
                    let entry = live.get_mut(&id).unwrap();
                    if entry.pins == 0 {
                        prop_assert!(matches!(store.unpin(id), Err(TileError::InvalidCount)));
                    } else {
                        store.unpin(id).unwrap();
                        entry.pins -= 1;
                        if entry.owners == 0 && entry.pins == 0 && entry.held.is_empty() {
                            live.remove(&id);
                        }
                    }
                }
            }
            Op::Retain { slot } => {
                if let Some(id) = pick(slot, &live) {
                    store.retain(id).unwrap();
                    live.get_mut(&id).unwrap().owners += 1;
                }
            }
            Op::Release { slot } => {
                if let Some(id) = pick(slot, &live) {
                    let entry = live.get_mut(&id).unwrap();
                    if entry.owners == 0 {
                        prop_assert!(matches!(store.release(id), Err(TileError::InvalidCount)));
                    } else {
                        store.release(id).unwrap();
                        entry.owners -= 1;
                        if entry.pins == 0 && entry.held.is_empty() && entry.owners == 0 {
                            live.remove(&id);
                        }
                    }
                }
            }
        }
        peak_content = peak_content.max(live.len() * TILE_BYTES);
        for (&id, entry) in &live {
            let guard = store.get(id).unwrap();
            prop_assert_eq!(&guard[..], &entry.bytes[..], "step {}, tile {:?}", step, id);
        }
        let stats = store.stats();
        prop_assert_eq!(stats.clone(), store.debug_recount_stats(), "step {}", step);
        prop_assert_eq!(stats.tile_count, live.len());
        prop_assert_eq!(stats.evict_failures, 0);
        if !stats.over_budget {
            prop_assert!(stats.resident_bytes + stats.compressed_bytes <= 8 * MIB);
        }
    }
    // Every case with more content than budget must have spilled to the real file.
    if peak_content > 9 * MIB {
        let stats = store.stats();
        prop_assert!(
            stats.spilled_bytes + stats.dead_spill_bytes > 0
                || std::fs::metadata(&spill_files(dir.path())[0])
                    .unwrap()
                    .len()
                    > 0,
            "content went over budget but nothing spilled"
        );
    }
    drop(live);
    let stats = store.stats();
    prop_assert_eq!(stats.clone(), store.debug_recount_stats());
    Ok(())
}

/// Cases for the 8 MiB test. One case checks up to 120 live tiles after each of up to 400
/// operations (about 0.3 s in release), so release builds run OP_PROPTEST_CASES / 16 cases,
/// at least 4. Debug builds run LZ4 and CRC unoptimized, where one case takes about a
/// minute, so they run 1 case; the release run is the real check.
fn budget_cases() -> u32 {
    if cfg!(debug_assertions) {
        1
    } else {
        (proptest_cases() / 16).max(4)
    }
}

#[test]
fn eight_mib_budget_random_operations_keep_every_byte() {
    let mut runner = proptest::test_runner::TestRunner::new(proptest::test_runner::Config {
        cases: budget_cases(),
        failure_persistence: None,
        ..Default::default()
    });
    runner
        .run(
            &(0..POOL, proptest::collection::vec(op_strategy(), 1..400)),
            |(base, ops)| run_budget_case(base, &ops),
        )
        .unwrap();
}

/// A fixed sequence that is sure to go far past the budget, mixing both kinds of content.
#[test]
fn eight_mib_budget_spills_large_lz4_output_to_the_real_file() {
    let mut ops = Vec::new();
    for index in 0..MAX_LIVE {
        ops.push(Op::Insert {
            noisy: index.is_multiple_of(2),
        });
        if index.is_multiple_of(7) {
            ops.push(Op::Get {
                slot: index,
                hold: index.is_multiple_of(3),
            });
            ops.push(Op::Pin {
                slot: index / 2,
                count: 2,
            });
        }
        if index.is_multiple_of(11) {
            ops.push(Op::Unpin { slot: index / 2 });
            ops.push(Op::DropHeld { slot: index });
            ops.push(Op::Retain { slot: index * 3 });
            ops.push(Op::Release { slot: index * 5 });
        }
    }
    run_budget_case(0, &ops).unwrap();
    // Incompressible tiles make LZ4 output larger than a raw tile.
    assert!(lz4_flex::block::compress(&pooled(true, 0)).len() > TILE_BYTES);
}

/// Four threads share tiles and run random operations; afterwards every byte and every
/// counter must match, and dropping the shared owners must free every tile.
#[test]
fn four_threads_on_shared_tiles_keep_every_byte() {
    for seed in [1u64, 2, 3, 0xDEAD_BEEF] {
        let dir = tempfile::tempdir().unwrap();
        let store = TileStore::new(dir.path(), 4 * MIB).unwrap();
        let shared: Vec<(TileId, Arc<[u8]>)> = (0..40usize)
            .map(|index| {
                let bytes = pooled(index.is_multiple_of(3), index + seed as usize);
                (store.insert(tile(&bytes)).unwrap(), bytes)
            })
            .collect();
        std::thread::scope(|scope| {
            for thread in 0..4u64 {
                let (store, shared) = (&store, &shared);
                scope.spawn(move || {
                    let mut rng = (seed << 8 | thread).wrapping_mul(0x9E37_79B9) | 1;
                    let mut pins: Vec<TileId> = Vec::new();
                    let mut owners: Vec<TileId> = Vec::new();
                    let mut held: Vec<(TileRef<'_>, usize)> = Vec::new();
                    let mut private: Vec<(TileId, Arc<[u8]>)> = Vec::new();
                    for _ in 0..600 {
                        let index = next(&mut rng) as usize % shared.len();
                        let (id, bytes) = &shared[index];
                        match next(&mut rng) % 10 {
                            0..=2 => {
                                let guard = store.get(*id).unwrap();
                                assert_eq!(&guard[..], &bytes[..]);
                                if held.len() < 4 {
                                    held.push((guard, index));
                                }
                            }
                            3 => {
                                held.pop();
                            }
                            4 => {
                                store.pin(*id).unwrap();
                                pins.push(*id);
                            }
                            5 => {
                                if let Some(id) = pins.pop() {
                                    store.unpin(id).unwrap();
                                }
                            }
                            6 => {
                                store.retain(*id).unwrap();
                                owners.push(*id);
                            }
                            7 => {
                                if let Some(id) = owners.pop() {
                                    store.release(id).unwrap();
                                }
                            }
                            8 => {
                                let bytes = pooled(
                                    next(&mut rng).is_multiple_of(2),
                                    next(&mut rng) as usize,
                                );
                                private.push((store.insert(tile(&bytes)).unwrap(), bytes));
                            }
                            _ => {
                                if let Some((id, bytes)) = private.pop() {
                                    assert_eq!(&*store.get(id).unwrap(), &bytes[..]);
                                    store.release(id).unwrap();
                                }
                            }
                        }
                        for (guard, index) in &held {
                            assert_eq!(&guard[..], &shared[*index].1[..]);
                        }
                    }
                    drop(held);
                    for id in pins {
                        store.unpin(id).unwrap();
                    }
                    for id in owners {
                        store.release(id).unwrap();
                    }
                    for (id, bytes) in private {
                        assert_eq!(&*store.get(id).unwrap(), &bytes[..]);
                        store.release(id).unwrap();
                    }
                });
            }
        });
        for (id, bytes) in &shared {
            assert_eq!(&*store.get(*id).unwrap(), &bytes[..]);
        }
        let stats = store.stats();
        assert_eq!(stats, store.debug_recount_stats(), "seed {seed}");
        assert_eq!(stats.tile_count, shared.len());
        assert_eq!(stats.evict_failures, 0);
        assert!(stats.spilled_bytes > 0, "seed {seed} never spilled");
        for (id, _) in &shared {
            store.release(*id).unwrap();
        }
        let stats = store.stats();
        assert_eq!(stats.tile_count, 0, "seed {seed} leaked a pin or owner");
        assert_eq!(stats, store.debug_recount_stats());
    }
}
