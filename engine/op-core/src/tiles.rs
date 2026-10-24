//! Immutable RGBA8 tiles with a bounded memory cache and an append-only spill file.
//!
//! Where a tile lives (resident, compressed or spilled) is not part of its content, so
//! eviction is best effort: a failed eviction write never fails the caller's operation.
//! The tile keeps its previous state, the store counts the failure in `TileStats`, and
//! the next operation tries again. Only loads and invalid counts return errors.

use parking_lot::{Condvar, Mutex, MutexGuard};
use std::collections::{BTreeMap, HashMap};
use std::fs::{File, OpenOptions};
use std::io;
use std::ops::Deref;
use std::os::unix::fs::FileExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use thiserror::Error;

pub const TILE_SIZE: usize = 256;
pub const TILE_BYTES: usize = TILE_SIZE * TILE_SIZE * 4;
pub const DEFAULT_BUDGET: usize = 2 * 1024 * 1024 * 1024;
const CODEC_VERSION: u8 = 1;
const MAGIC: [u8; 4] = *b"OPTL";
const HEADER_LEN: usize = 17;
/// How many spill file names `TileStore::new` tries before it gives up.
const OPEN_ATTEMPTS: usize = 1024;
static FILE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TileCoord {
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TileId(pub u64);

#[derive(Debug, Error)]
pub enum TileError {
    #[error("tile {0:?} does not exist")]
    Missing(TileId),
    #[error("tile data must have exactly {TILE_BYTES} bytes; got {0}")]
    InvalidLength(usize),
    #[error("tile ownership or pin count is invalid")]
    InvalidCount,
    #[error("tile count overflow")]
    Overflow,
    #[error("spill I/O failed: {0}")]
    Io(#[from] io::Error),
    #[error("tile data or spill record is corrupt")]
    Corrupt,
}

pub type Result<T> = std::result::Result<T, TileError>;

#[derive(Debug, Clone)]
pub struct TileData(Box<[u8]>);

impl TileData {
    pub fn is_transparent_black(&self) -> bool {
        self.0.iter().all(|&byte| byte == 0)
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

impl TryFrom<Vec<u8>> for TileData {
    type Error = TileError;

    fn try_from(value: Vec<u8>) -> Result<Self> {
        if value.len() != TILE_BYTES {
            return Err(TileError::InvalidLength(value.len()));
        }
        Ok(Self(value.into_boxed_slice()))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TileStateKind {
    Resident,
    Compressed,
    Spilled,
}

/// Pixel and compressed buffers are shared, so handing one out or snapshotting it never copies.
/// A spilled tile's location is in `TileRecord::spill`.
#[derive(Clone)]
enum TileState {
    Resident(Arc<[u8]>),
    Compressed(Arc<[u8]>),
    Spilled,
}

impl TileState {
    fn memory_len(&self) -> usize {
        match self {
            TileState::Resident(bytes) | TileState::Compressed(bytes) => bytes.len(),
            TileState::Spilled => 0,
        }
    }
}

/// Where a tile's record sits in the spill file. Tiles are immutable, so a location stays
/// valid for the tile's whole life, including while the tile is resident again.
#[derive(Clone, Copy)]
struct SpillLoc {
    offset: u64,
    len: usize,
}

/// Marks a record that an eviction is compressing or writing without the main lock.
#[derive(Clone, Copy)]
struct MoveTicket {
    token: u64,
    bytes: usize,
}

struct TileRecord {
    state: TileState,
    spill: Option<SpillLoc>,
    /// Pins held by live `TileRef` guards.
    guard_pins: usize,
    /// Pins taken with `TileStore::pin` and given back with `TileStore::unpin`.
    explicit_pins: usize,
    owners: usize,
    byte_size: usize,
    checksum: u32,
    codec_version: u8,
    last_use: u64,
    /// Changes on every state change and every unpin, so a finished move can tell whether
    /// the tile was used while the move ran. Taking a pin does not change it; the pin
    /// itself blocks the publish.
    version: u64,
    moving: Option<MoveTicket>,
    /// A caller is reading and decoding this tile without the main lock.
    loading: bool,
}

impl TileRecord {
    fn pinned(&self) -> bool {
        self.guard_pins > 0 || self.explicit_pins > 0
    }

    fn is_eviction_candidate(&self) -> bool {
        !self.pinned()
            && self.moving.is_none()
            && !self.loading
            && !matches!(self.state, TileState::Spilled)
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct TileStats {
    pub resident_bytes: usize,
    pub compressed_bytes: usize,
    /// Spill-file bytes of tiles that are in the spilled state now.
    pub spilled_bytes: usize,
    pub freed_bytes: usize,
    pub tile_count: usize,
    /// Spill-file bytes that no live tile uses: records of freed tiles and writes whose
    /// tile was freed before the write finished. Nothing reclaims them yet.
    pub dead_spill_bytes: usize,
    /// How many eviction runs stopped because a spill write failed.
    pub evict_failures: u64,
    /// The text of the most recent eviction failure.
    pub last_evict_error: Option<String>,
    /// Resident plus compressed memory is above the budget right now, because every
    /// remaining tile is pinned or an eviction write failed.
    pub over_budget: bool,
}

/// Storage for spilled tiles. The spill file is a cache for one session: it is deleted
/// when the store drops and nothing reads it after a crash, so appends need no fsync.
/// An append reports success only after all bytes were handed to the file. A failed
/// append may leave unused bytes in the file, but the store never publishes its location.
/// Several threads may call at once, and a read must not wait for an append in progress.
pub trait SpillIo: Send + Sync {
    fn append(&self, bytes: &[u8]) -> io::Result<u64>;
    fn read_exact_at(&self, offset: u64, bytes: &mut [u8]) -> io::Result<()>;
}

struct FileIo {
    file: File,
    path: PathBuf,
    /// The next append offset. Only appends take this lock; reads use positioned reads.
    end: Mutex<u64>,
}

impl SpillIo for FileIo {
    fn append(&self, bytes: &[u8]) -> io::Result<u64> {
        let mut end = self.end.lock();
        let offset = *end;
        self.file.write_all_at(bytes, offset)?;
        *end = offset
            .checked_add(bytes.len() as u64)
            .ok_or_else(|| io::Error::other("spill file offset overflow"))?;
        Ok(offset)
    }

    fn read_exact_at(&self, offset: u64, bytes: &mut [u8]) -> io::Result<()> {
        self.file.read_exact_at(bytes, offset)
    }
}

impl Drop for FileIo {
    fn drop(&mut self) {
        // A destructor cannot return the error, so report it instead of hiding it.
        if let Err(error) = std::fs::remove_file(&self.path) {
            eprintln!(
                "op-core: cannot delete spill file {}: {error}",
                self.path.display()
            );
        }
    }
}

struct Inner {
    tiles: HashMap<TileId, TileRecord>,
    /// Eviction candidates keyed by `last_use`, oldest first. A record is listed exactly
    /// when `is_eviction_candidate` holds.
    lru: BTreeMap<u64, TileId>,
    next_id: u64,
    clock: u64,
    budget: usize,
    resident_bytes: usize,
    compressed_bytes: usize,
    spilled_bytes: usize,
    freed_bytes: usize,
    dead_spill_bytes: usize,
    /// Memory that unfinished moves expect to release; other callers do not evict it again.
    moving_bytes: usize,
    evict_failures: u64,
    last_evict_error: Option<String>,
    deferred_error: Option<TileError>,
}

/// One tile chosen for eviction, with everything needed to compress and write it unlocked.
struct Move {
    id: TileId,
    token: u64,
    version: u64,
    source: TileState,
    byte_size: usize,
    checksum: u32,
    codec_version: u8,
}

struct MovePlan {
    need: usize,
    moves: Vec<Move>,
}

/// The new storage a finished move offers to publish.
enum Moved {
    Compressed(Arc<[u8]>),
    Spilled(SpillLoc),
}

/// What a load must read, copied out so it can run without the main lock.
enum LoadSource {
    Compressed(Arc<[u8]>),
    Spilled(SpillLoc),
}

#[derive(Clone, Copy)]
enum PinKind {
    Guard,
    Explicit,
}

pub struct TileStore {
    inner: Mutex<Inner>,
    /// Woken whenever a load finishes, so callers waiting on the same tile retry.
    loaded: Condvar,
    /// Never called while `inner` is held.
    io: Box<dyn SpillIo>,
}

impl TileStore {
    /// Opens a new spill file in `directory`. A name that already exists is skipped.
    pub fn new(directory: impl AsRef<Path>, budget: usize) -> Result<Self> {
        for _ in 0..OPEN_ATTEMPTS {
            let sequence = FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let path = directory
                .as_ref()
                .join(format!("op-tiles-{}-{sequence}.spill", std::process::id()));
            match OpenOptions::new()
                .read(true)
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(file) => {
                    let io = FileIo {
                        file,
                        path,
                        end: Mutex::new(0),
                    };
                    return Ok(Self::with_io(budget, Box::new(io)));
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error.into()),
            }
        }
        Err(TileError::Io(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("no free spill file name after {OPEN_ATTEMPTS} attempts"),
        )))
    }

    pub fn with_default_budget(directory: impl AsRef<Path>) -> Result<Self> {
        Self::new(directory, DEFAULT_BUDGET)
    }

    pub fn with_io(budget: usize, io: Box<dyn SpillIo>) -> Self {
        Self {
            inner: Mutex::new(Inner {
                tiles: HashMap::new(),
                lru: BTreeMap::new(),
                next_id: 1,
                clock: 0,
                budget,
                resident_bytes: 0,
                compressed_bytes: 0,
                spilled_bytes: 0,
                freed_bytes: 0,
                dead_spill_bytes: 0,
                moving_bytes: 0,
                evict_failures: 0,
                last_evict_error: None,
                deferred_error: None,
            }),
            loaded: Condvar::new(),
            io,
        }
    }

    /// Adds a tile with one owner. Fails only when tile ids run out.
    pub fn insert(&self, data: TileData) -> Result<TileId> {
        let mut inner = self.inner.lock();
        let id = TileId(inner.next_id);
        inner.next_id = inner.next_id.checked_add(1).ok_or(TileError::Overflow)?;
        let checksum = crc32fast::hash(data.as_bytes());
        let last_use = inner.tick();
        let state = TileState::Resident(Arc::from(data.0));
        inner.add_bytes(&state, None);
        inner.tiles.insert(
            id,
            TileRecord {
                state,
                spill: None,
                guard_pins: 0,
                explicit_pins: 0,
                owners: 1,
                byte_size: TILE_BYTES,
                checksum,
                codec_version: CODEC_VERSION,
                last_use,
                version: last_use,
                moving: None,
                loading: false,
            },
        );
        inner.relist(id);
        self.enforce_budget(inner);
        Ok(id)
    }

    pub fn get(&self, id: TileId) -> Result<TileRef<'_>> {
        let bytes = self.acquire(id, PinKind::Guard)?;
        Ok(TileRef {
            store: self,
            id,
            bytes,
        })
    }

    /// Takes an explicit pin, loading the tile if needed. `unpin` gives it back.
    pub fn pin(&self, id: TileId) -> Result<()> {
        self.acquire(id, PinKind::Explicit).map(drop)
    }

    /// Sets the budget and evicts toward it. The new budget stays even if an eviction fails.
    pub fn set_budget(&self, budget: usize) {
        let mut inner = self.inner.lock();
        inner.budget = budget;
        self.enforce_budget(inner);
    }

    /// Gives back one explicit pin. Guards from `get` are not explicit pins.
    pub fn unpin(&self, id: TileId) -> Result<()> {
        let mut inner = self.inner.lock();
        let record = inner.tiles.get_mut(&id).ok_or(TileError::Missing(id))?;
        record.explicit_pins = record
            .explicit_pins
            .checked_sub(1)
            .ok_or(TileError::InvalidCount)?;
        inner.after_unpin(id);
        self.enforce_budget(inner);
        Ok(())
    }

    pub fn retain(&self, id: TileId) -> Result<()> {
        let mut inner = self.inner.lock();
        let record = inner.tiles.get_mut(&id).ok_or(TileError::Missing(id))?;
        record.owners = record.owners.checked_add(1).ok_or(TileError::Overflow)?;
        Ok(())
    }

    pub fn release(&self, id: TileId) -> Result<()> {
        let mut inner = self.inner.lock();
        let record = inner.tiles.get_mut(&id).ok_or(TileError::Missing(id))?;
        record.owners = record
            .owners
            .checked_sub(1)
            .ok_or(TileError::InvalidCount)?;
        inner.free_unowned(id);
        self.enforce_budget(inner);
        Ok(())
    }

    pub fn state(&self, id: TileId) -> Result<TileStateKind> {
        let inner = self.inner.lock();
        let record = inner.tiles.get(&id).ok_or(TileError::Missing(id))?;
        Ok(match record.state {
            TileState::Resident(_) => TileStateKind::Resident,
            TileState::Compressed(_) => TileStateKind::Compressed,
            TileState::Spilled => TileStateKind::Spilled,
        })
    }

    pub fn stats(&self) -> TileStats {
        let inner = self.inner.lock();
        TileStats {
            resident_bytes: inner.resident_bytes,
            compressed_bytes: inner.compressed_bytes,
            spilled_bytes: inner.spilled_bytes,
            freed_bytes: inner.freed_bytes,
            tile_count: inner.tiles.len(),
            dead_spill_bytes: inner.dead_spill_bytes,
            evict_failures: inner.evict_failures,
            last_evict_error: inner.last_evict_error.clone(),
            over_budget: inner.resident_bytes + inner.compressed_bytes > inner.budget,
        }
    }

    /// Recounts every record. Tests compare this with `stats`; normal code must not call it.
    /// Panics if the eviction order, the moving-byte total or a mark disagrees with the records.
    #[doc(hidden)]
    pub fn debug_recount_stats(&self) -> TileStats {
        let inner = self.inner.lock();
        let mut stats = TileStats {
            freed_bytes: inner.freed_bytes,
            tile_count: inner.tiles.len(),
            dead_spill_bytes: inner.dead_spill_bytes,
            evict_failures: inner.evict_failures,
            last_evict_error: inner.last_evict_error.clone(),
            ..TileStats::default()
        };
        let mut moving_bytes = 0;
        let mut candidates = BTreeMap::new();
        for (&id, record) in &inner.tiles {
            match &record.state {
                TileState::Resident(bytes) => stats.resident_bytes += bytes.len(),
                TileState::Compressed(bytes) => stats.compressed_bytes += bytes.len(),
                TileState::Spilled => {
                    stats.spilled_bytes += record.spill.expect("spilled tile has a location").len;
                }
            }
            moving_bytes += record.moving.map_or(0, |ticket| ticket.bytes);
            if record.is_eviction_candidate() {
                candidates.insert(record.last_use, id);
            }
        }
        stats.over_budget = stats.resident_bytes + stats.compressed_bytes > inner.budget;
        assert_eq!(inner.lru, candidates, "eviction order is out of sync");
        assert_eq!(
            inner.moving_bytes, moving_bytes,
            "moving bytes are out of sync"
        );
        stats
    }

    /// Returns the oldest eviction error not yet taken. Eviction errors never fail an
    /// operation; `stats` also counts them and keeps the latest text.
    pub fn take_deferred_error(&self) -> Option<TileError> {
        self.inner.lock().deferred_error.take()
    }

    /// Makes the tile resident, pins it and returns a shared handle to its pixels.
    /// A compressed or spilled tile is read and decoded without the main lock.
    fn acquire(&self, id: TileId, kind: PinKind) -> Result<Arc<[u8]>> {
        let mut inner = self.inner.lock();
        loop {
            let record = inner.tiles.get(&id).ok_or(TileError::Missing(id))?;
            let pins = match kind {
                PinKind::Guard => record.guard_pins,
                PinKind::Explicit => record.explicit_pins,
            }
            .checked_add(1)
            .ok_or(TileError::Overflow)?;
            if record.loading {
                // Another caller is loading this tile; wait for it and look again.
                self.loaded.wait(&mut inner);
                continue;
            }
            let source = match &record.state {
                TileState::Resident(bytes) => {
                    let bytes = bytes.clone();
                    inner.unlist(id);
                    let tick = inner.tick();
                    let record = inner.tiles.get_mut(&id).expect("checked tile");
                    match kind {
                        PinKind::Guard => record.guard_pins = pins,
                        PinKind::Explicit => record.explicit_pins = pins,
                    }
                    record.last_use = tick;
                    self.enforce_budget(inner);
                    return Ok(bytes);
                }
                TileState::Compressed(bytes) => LoadSource::Compressed(bytes.clone()),
                TileState::Spilled => {
                    LoadSource::Spilled(record.spill.expect("spilled tile has a location"))
                }
            };
            let (checksum, codec_version, byte_size) =
                (record.checksum, record.codec_version, record.byte_size);
            inner.unlist(id);
            inner.tiles.get_mut(&id).expect("checked tile").loading = true;
            drop(inner);
            let mark = LoadMark { store: self, id };
            let loaded = self.load(&source, checksum, codec_version, byte_size);
            inner = self.inner.lock();
            std::mem::forget(mark);
            let exists = inner.tiles.contains_key(&id);
            if exists {
                inner.tiles.get_mut(&id).expect("checked tile").loading = false;
            }
            self.loaded.notify_all();
            let raw = match loaded {
                Ok(raw) => raw,
                Err(error) => {
                    inner.relist(id);
                    return Err(error);
                }
            };
            // A release may have freed the tile while it loaded; the next pass reports it.
            if exists {
                // No move publishes while `loading` is set, so the state is still not resident.
                inner.set_state(id, TileState::Resident(raw));
                inner.relist(id);
            }
        }
    }

    /// Reads and checks a tile's stored form. Runs without the main lock.
    fn load(
        &self,
        source: &LoadSource,
        checksum: u32,
        codec_version: u8,
        byte_size: usize,
    ) -> Result<Arc<[u8]>> {
        match source {
            LoadSource::Compressed(bytes) => decode(bytes, checksum),
            LoadSource::Spilled(loc) => {
                let mut bytes = vec![0; loc.len];
                self.io
                    .read_exact_at(loc.offset, &mut bytes)
                    .map_err(|error| {
                        if error.kind() == io::ErrorKind::UnexpectedEof {
                            TileError::Corrupt
                        } else {
                            TileError::Io(error)
                        }
                    })?;
                if bytes.len() < HEADER_LEN
                    || bytes[..4] != MAGIC
                    || bytes[4] != codec_version
                    || u32::from_le_bytes(bytes[5..9].try_into().unwrap()) as usize != byte_size
                    || Some(u32::from_le_bytes(bytes[9..13].try_into().unwrap()) as usize)
                        != loc.len.checked_sub(HEADER_LEN)
                    || u32::from_le_bytes(bytes[13..17].try_into().unwrap()) != checksum
                {
                    return Err(TileError::Corrupt);
                }
                decode(&bytes[HEADER_LEN..], checksum)
            }
        }
    }

    /// Evicts toward the budget, then releases the lock. Tiles are compressed and written
    /// without the lock, so other callers keep working; results are published only for
    /// records that were not pinned or used in the meantime. A failed write stops this run,
    /// leaves the tile as it was and is recorded in the stats; the next operation tries again.
    fn enforce_budget<'s>(&'s self, mut inner: MutexGuard<'s, Inner>) {
        loop {
            let plan = inner.plan_moves();
            if plan.moves.is_empty() {
                return;
            }
            drop(inner);
            let mut mark = MoveMarks {
                store: self,
                plan: Some(plan),
            };
            let (results, error) = self.run_moves(mark.plan.as_ref().expect("plan is set"));
            inner = self.inner.lock();
            let plan = mark.plan.take().expect("plan is set");
            let failed = error.is_some();
            inner.finish_moves(plan, results, error);
            if failed {
                return;
            }
        }
    }

    /// Compresses and writes the planned tiles in order until the need is covered.
    /// A tile whose compressed form alone covers the rest of the need stays compressed.
    /// Stops at the first failed write and returns the results so far with the error.
    fn run_moves(&self, plan: &MovePlan) -> (Vec<Moved>, Option<TileError>) {
        let mut need = plan.need;
        let mut results = Vec::new();
        for tile in &plan.moves {
            if need == 0 {
                break;
            }
            let previous_len = tile.source.memory_len();
            let fresh;
            let compressed: &[u8] = match &tile.source {
                TileState::Resident(raw) => {
                    fresh = lz4_flex::block::compress(raw);
                    &fresh
                }
                TileState::Compressed(bytes) => bytes,
                TileState::Spilled => unreachable!("spilled tiles are not candidates"),
            };
            let resident = matches!(tile.source, TileState::Resident(_));
            if resident && compressed.len() + need <= previous_len {
                need = 0;
                results.push(Moved::Compressed(Arc::from(compressed)));
            } else {
                match self.write_spill(tile, compressed) {
                    Ok(loc) => results.push(Moved::Spilled(loc)),
                    Err(error) => return (results, Some(error)),
                }
                need = need.saturating_sub(previous_len);
            }
        }
        (results, None)
    }

    fn write_spill(&self, tile: &Move, compressed: &[u8]) -> Result<SpillLoc> {
        let mut bytes = Vec::with_capacity(HEADER_LEN + compressed.len());
        bytes.extend_from_slice(&MAGIC);
        bytes.push(tile.codec_version);
        bytes.extend_from_slice(&(tile.byte_size as u32).to_le_bytes());
        bytes.extend_from_slice(&(compressed.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&tile.checksum.to_le_bytes());
        bytes.extend_from_slice(compressed);
        let offset = self.io.append(&bytes)?;
        Ok(SpillLoc {
            offset,
            len: bytes.len(),
        })
    }
}

/// Clears a load mark if the load unwinds, so the tile is not stuck as loading.
/// A normal finish forgets this guard and clears the mark itself.
struct LoadMark<'a> {
    store: &'a TileStore,
    id: TileId,
}

impl Drop for LoadMark<'_> {
    fn drop(&mut self) {
        let mut inner = self.store.inner.lock();
        if let Some(record) = inner.tiles.get_mut(&self.id) {
            record.loading = false;
        }
        inner.relist(self.id);
        self.store.loaded.notify_all();
    }
}

/// Clears the moving marks of a plan if a move unwinds, so the tiles can be evicted later.
/// A normal finish takes the plan out first.
struct MoveMarks<'a> {
    store: &'a TileStore,
    plan: Option<MovePlan>,
}

impl Drop for MoveMarks<'_> {
    fn drop(&mut self) {
        if let Some(plan) = self.plan.take() {
            self.store.inner.lock().finish_moves(plan, Vec::new(), None);
        }
    }
}

pub struct TileRef<'a> {
    store: &'a TileStore,
    id: TileId,
    bytes: Arc<[u8]>,
}

impl Deref for TileRef<'_> {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        &self.bytes
    }
}

impl Drop for TileRef<'_> {
    fn drop(&mut self) {
        let mut inner = self.store.inner.lock();
        let record = inner
            .tiles
            .get_mut(&self.id)
            .expect("a guarded tile is never freed");
        // Only guards change `guard_pins`, and this guard holds one of them.
        record.guard_pins = record
            .guard_pins
            .checked_sub(1)
            .expect("guard pin count covers this guard");
        inner.after_unpin(self.id);
        self.store.enforce_budget(inner);
    }
}

#[derive(Clone, Copy)]
enum Bucket {
    Resident,
    Compressed,
    Spilled,
}

impl Inner {
    fn tick(&mut self) -> u64 {
        self.clock = self.clock.wrapping_add(1);
        self.clock
    }

    fn bucket(state: &TileState, spill: Option<SpillLoc>) -> (Bucket, usize) {
        match state {
            TileState::Resident(bytes) => (Bucket::Resident, bytes.len()),
            TileState::Compressed(bytes) => (Bucket::Compressed, bytes.len()),
            TileState::Spilled => (
                Bucket::Spilled,
                spill.expect("spilled tile has a location").len,
            ),
        }
    }

    fn counter(&mut self, bucket: Bucket) -> &mut usize {
        match bucket {
            Bucket::Resident => &mut self.resident_bytes,
            Bucket::Compressed => &mut self.compressed_bytes,
            Bucket::Spilled => &mut self.spilled_bytes,
        }
    }

    fn add_bytes(&mut self, state: &TileState, spill: Option<SpillLoc>) {
        let (bucket, len) = Self::bucket(state, spill);
        let counter = self.counter(bucket);
        *counter = counter.checked_add(len).expect("byte counter overflow");
    }

    fn sub_bytes(&mut self, state: &TileState, spill: Option<SpillLoc>) {
        let (bucket, len) = Self::bucket(state, spill);
        let counter = self.counter(bucket);
        *counter = counter.checked_sub(len).expect("byte counter underflow");
    }

    /// Replaces a record's state, keeps the byte counters exact and returns the old state.
    /// The caller keeps the eviction order in sync.
    fn set_state(&mut self, id: TileId, state: TileState) -> TileState {
        let version = self.tick();
        let spill = self.tiles.get(&id).expect("existing tile").spill;
        self.add_bytes(&state, spill);
        let record = self.tiles.get_mut(&id).expect("existing tile");
        record.version = version;
        let previous = std::mem::replace(&mut record.state, state);
        self.sub_bytes(&previous, spill);
        previous
    }

    fn unlist(&mut self, id: TileId) {
        if let Some(record) = self.tiles.get(&id)
            && self.lru.get(&record.last_use) == Some(&id)
        {
            self.lru.remove(&record.last_use);
        }
    }

    fn relist(&mut self, id: TileId) {
        if let Some(record) = self.tiles.get(&id)
            && record.is_eviction_candidate()
        {
            self.lru.insert(record.last_use, id);
        }
    }

    /// Records a use after a pin count dropped, then lists or frees the tile.
    fn after_unpin(&mut self, id: TileId) {
        self.unlist(id);
        let tick = self.tick();
        if let Some(record) = self.tiles.get_mut(&id) {
            record.last_use = tick;
            record.version = tick;
        }
        self.relist(id);
        self.free_unowned(id);
    }

    fn free_unowned(&mut self, id: TileId) {
        if !self
            .tiles
            .get(&id)
            .is_some_and(|record| !record.pinned() && record.owners == 0)
        {
            return;
        }
        self.unlist(id);
        let record = self.tiles.remove(&id).expect("checked above");
        self.sub_bytes(&record.state, record.spill);
        if let Some(ticket) = record.moving {
            self.moving_bytes = self
                .moving_bytes
                .checked_sub(ticket.bytes)
                .expect("moving bytes cover the ticket");
        }
        if let Some(loc) = record.spill {
            self.dead_spill_bytes += loc.len;
        }
        self.freed_bytes += record.byte_size;
    }

    /// Evicts the oldest candidates until memory, minus moves already under way, is within
    /// budget. A tile that already has a spill record goes straight back to spilled under
    /// the lock. The others are marked moving and returned for an unlocked move.
    fn plan_moves(&mut self) -> MovePlan {
        let moving_before = self.moving_bytes;
        let mut moves = Vec::new();
        loop {
            let used =
                (self.resident_bytes + self.compressed_bytes).saturating_sub(self.moving_bytes);
            if used <= self.budget {
                break;
            }
            let Some((_, id)) = self.lru.pop_first() else {
                break;
            };
            if self.tiles.get(&id).expect("listed tile").spill.is_some() {
                self.set_state(id, TileState::Spilled);
                continue;
            }
            let token = self.tick();
            let record = self.tiles.get_mut(&id).expect("listed tile");
            let bytes = record.state.memory_len();
            record.moving = Some(MoveTicket { token, bytes });
            moves.push(Move {
                id,
                token,
                version: record.version,
                source: record.state.clone(),
                byte_size: record.byte_size,
                checksum: record.checksum,
                codec_version: record.codec_version,
            });
            self.moving_bytes += bytes;
        }
        // What the planned moves must free, after the instant spills above.
        let need = (self.resident_bytes + self.compressed_bytes)
            .saturating_sub(moving_before)
            .saturating_sub(self.budget);
        MovePlan { need, moves }
    }

    /// Clears the moving marks and publishes each result whose record is unpinned and
    /// unused since planning. A new spill location is kept on the record even when the
    /// state cannot change, because it stays valid for the tile's life. An error is counted.
    fn finish_moves(&mut self, plan: MovePlan, results: Vec<Moved>, error: Option<TileError>) {
        let mut results = results.into_iter();
        for tile in plan.moves {
            let result = results.next();
            let Some(record) = self.tiles.get_mut(&tile.id) else {
                if let Some(Moved::Spilled(loc)) = result {
                    self.dead_spill_bytes += loc.len;
                }
                continue;
            };
            let Some(ticket) = record.moving.filter(|ticket| ticket.token == tile.token) else {
                continue;
            };
            record.moving = None;
            let unchanged = !record.pinned() && !record.loading && record.version == tile.version;
            self.moving_bytes = self
                .moving_bytes
                .checked_sub(ticket.bytes)
                .expect("moving bytes cover the ticket");
            match result {
                Some(Moved::Spilled(loc)) => {
                    let record = self.tiles.get_mut(&tile.id).expect("checked tile");
                    if record.spill.is_none() {
                        record.spill = Some(loc);
                    } else {
                        self.dead_spill_bytes += loc.len;
                    }
                    if unchanged {
                        self.set_state(tile.id, TileState::Spilled);
                    }
                }
                Some(Moved::Compressed(bytes)) if unchanged => {
                    self.set_state(tile.id, TileState::Compressed(bytes));
                }
                _ => {}
            }
            self.relist(tile.id);
        }
        if let Some(error) = error {
            self.evict_failures += 1;
            self.last_evict_error = Some(error.to_string());
            if self.deferred_error.is_none() {
                self.deferred_error = Some(error);
            }
        }
    }
}

fn decode(compressed: &[u8], checksum: u32) -> Result<Arc<[u8]>> {
    let mut raw: Arc<[u8]> = std::iter::repeat_n(0, TILE_BYTES).collect();
    let output = Arc::get_mut(&mut raw).expect("new buffer is unshared");
    let len =
        lz4_flex::block::decompress_into(compressed, output).map_err(|_| TileError::Corrupt)?;
    if len != TILE_BYTES || crc32fast::hash(&raw) != checksum {
        return Err(TileError::Corrupt);
    }
    Ok(raw)
}
