//! Immutable RGBA8 tiles with a bounded memory cache and an append-only spill file.

use parking_lot::{Mutex, MutexGuard};
use std::collections::{BTreeMap, HashMap};
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::ops::Deref;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use thiserror::Error;

pub const TILE_SIZE: usize = 256;
pub const TILE_BYTES: usize = TILE_SIZE * TILE_SIZE * 4;
pub const DEFAULT_BUDGET: usize = 2 * 1024 * 1024 * 1024;
const CODEC_VERSION: u8 = 1;
const MAGIC: [u8; 4] = *b"OPTL";
const HEADER_LEN: usize = 17;
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
#[derive(Clone)]
enum TileState {
    Resident(Arc<[u8]>),
    Compressed(Arc<[u8]>),
    Spilled {
        offset: u64,
        len: usize,
        checksum: u32,
    },
}

impl TileState {
    fn memory_len(&self) -> usize {
        match self {
            TileState::Resident(bytes) | TileState::Compressed(bytes) => bytes.len(),
            TileState::Spilled { .. } => 0,
        }
    }
}

/// Marks a record that an eviction is compressing or writing without the main lock.
#[derive(Clone, Copy)]
struct MoveTicket {
    token: u64,
    bytes: usize,
}

struct TileRecord {
    state: TileState,
    pins: usize,
    owners: usize,
    byte_size: usize,
    checksum: u32,
    codec_version: u8,
    last_use: u64,
    /// Changes on every state change and every pin, so a finished move can tell whether
    /// the record still matches the snapshot it compressed.
    version: u64,
    moving: Option<MoveTicket>,
}

impl TileRecord {
    fn is_eviction_candidate(&self) -> bool {
        self.pins == 0 && self.moving.is_none() && !matches!(self.state, TileState::Spilled { .. })
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct TileStats {
    pub resident_bytes: usize,
    pub compressed_bytes: usize,
    pub spilled_bytes: usize,
    pub freed_bytes: usize,
    pub tile_count: usize,
}

/// An append must report success only after all bytes were written and flushed.
/// Failed appends may leave unused bytes in the file but must not publish a location.
pub trait SpillIo: Send {
    fn append(&mut self, bytes: &[u8]) -> io::Result<u64>;
    fn read_exact_at(&mut self, offset: u64, bytes: &mut [u8]) -> io::Result<()>;
}

struct FileIo(File);

impl SpillIo for FileIo {
    fn append(&mut self, bytes: &[u8]) -> io::Result<u64> {
        let offset = self.0.seek(SeekFrom::End(0))?;
        self.0.write_all(bytes)?;
        self.0.flush()?;
        Ok(offset)
    }

    fn read_exact_at(&mut self, offset: u64, bytes: &mut [u8]) -> io::Result<()> {
        self.0.seek(SeekFrom::Start(offset))?;
        self.0.read_exact(bytes)
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
    /// Memory that unfinished moves expect to release; other callers do not evict it again.
    moving_bytes: usize,
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

pub struct TileStore {
    inner: Mutex<Inner>,
    /// Lock order: `io` may be taken while holding `inner`, never the reverse.
    io: Mutex<Box<dyn SpillIo>>,
}

impl TileStore {
    pub fn new(directory: impl AsRef<Path>, budget: usize) -> Result<Self> {
        let sequence = FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = directory
            .as_ref()
            .join(format!("op-tiles-{}-{sequence}.spill", std::process::id()));
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(path)?;
        Ok(Self::with_io(budget, Box::new(FileIo(file))))
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
                moving_bytes: 0,
                deferred_error: None,
            }),
            io: Mutex::new(io),
        }
    }

    pub fn insert(&self, data: TileData) -> Result<TileId> {
        let mut inner = self.inner.lock();
        let id = TileId(inner.next_id);
        inner.next_id = inner.next_id.checked_add(1).ok_or(TileError::Overflow)?;
        let checksum = crc32fast::hash(data.as_bytes());
        let last_use = inner.tick();
        let state = TileState::Resident(Arc::from(data.0));
        inner.add_bytes(&state);
        inner.tiles.insert(
            id,
            TileRecord {
                state,
                pins: 0,
                owners: 1,
                byte_size: TILE_BYTES,
                checksum,
                codec_version: CODEC_VERSION,
                last_use,
                version: last_use,
                moving: None,
            },
        );
        inner.relist(id);
        if let Err(error) = self.enforce_budget(&mut inner) {
            let unseen = inner.tiles.get(&id).is_some_and(|record| record.pins == 0);
            inner.remove_record(id);
            // Reuse the id only if no other caller has pinned it in the meantime.
            if unseen && inner.next_id == id.0 + 1 {
                inner.next_id = id.0;
            }
            return Err(error);
        }
        Ok(id)
    }

    pub fn get(&self, id: TileId) -> Result<TileRef<'_>> {
        let bytes = self.acquire(id)?;
        Ok(TileRef {
            store: self,
            id,
            bytes,
        })
    }

    pub fn pin(&self, id: TileId) -> Result<()> {
        self.acquire(id).map(drop)
    }

    pub fn set_budget(&self, budget: usize) -> Result<()> {
        let mut inner = self.inner.lock();
        let previous = inner.budget;
        inner.budget = budget;
        if let Err(error) = self.enforce_budget(&mut inner) {
            inner.budget = previous;
            return Err(error);
        }
        Ok(())
    }

    pub fn unpin(&self, id: TileId) -> Result<()> {
        let mut inner = self.inner.lock();
        let record = inner.tiles.get(&id).ok_or(TileError::Missing(id))?;
        if record.pins == 0 {
            return Err(TileError::InvalidCount);
        }
        inner.unpin_one(id);
        self.enforce_budget(&mut inner)
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
        if record.owners == 0 {
            return Err(TileError::InvalidCount);
        }
        record.owners -= 1;
        inner.free_unowned(id);
        Ok(())
    }

    pub fn state(&self, id: TileId) -> Result<TileStateKind> {
        let inner = self.inner.lock();
        let record = inner.tiles.get(&id).ok_or(TileError::Missing(id))?;
        Ok(match record.state {
            TileState::Resident(_) => TileStateKind::Resident,
            TileState::Compressed(_) => TileStateKind::Compressed,
            TileState::Spilled { .. } => TileStateKind::Spilled,
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
        }
    }

    /// Recounts every record. Tests compare this with `stats`; normal code must not call it.
    /// Panics if the eviction order or the moving-byte total disagrees with the records.
    #[doc(hidden)]
    pub fn debug_recount_stats(&self) -> TileStats {
        let inner = self.inner.lock();
        let mut stats = TileStats {
            freed_bytes: inner.freed_bytes,
            tile_count: inner.tiles.len(),
            ..TileStats::default()
        };
        let mut moving_bytes = 0;
        let mut candidates = BTreeMap::new();
        for (&id, record) in &inner.tiles {
            match &record.state {
                TileState::Resident(bytes) => stats.resident_bytes += bytes.len(),
                TileState::Compressed(bytes) => stats.compressed_bytes += bytes.len(),
                TileState::Spilled { len, .. } => stats.spilled_bytes += len,
            }
            moving_bytes += record.moving.map_or(0, |ticket| ticket.bytes);
            if record.is_eviction_candidate() {
                candidates.insert(record.last_use, id);
            }
        }
        assert_eq!(inner.lru, candidates, "eviction order is out of sync");
        assert_eq!(
            inner.moving_bytes, moving_bytes,
            "moving bytes are out of sync"
        );
        stats
    }

    /// Returns an eviction error from releasing a `TileRef`, whose destructor cannot return it.
    pub fn take_deferred_error(&self) -> Option<TileError> {
        self.inner.lock().deferred_error.take()
    }

    /// Makes the tile resident, pins it and returns a shared handle to its pixels.
    fn acquire(&self, id: TileId) -> Result<Arc<[u8]>> {
        let mut inner = self.inner.lock();
        let record = inner.tiles.get(&id).ok_or(TileError::Missing(id))?;
        let pins = record.pins.checked_add(1).ok_or(TileError::Overflow)?;
        let previous = self.ensure_resident(&mut inner, id)?;
        inner.unlist(id);
        let tick = inner.tick();
        let record = inner.tiles.get_mut(&id).expect("checked tile");
        record.pins = pins;
        record.last_use = tick;
        record.version = tick;
        let bytes = match &record.state {
            TileState::Resident(bytes) => bytes.clone(),
            _ => unreachable!("loaded tile is resident"),
        };
        if let Err(error) = self.enforce_budget(&mut inner) {
            let record = inner.tiles.get(&id).expect("pinned tile");
            // Restore the old storage only if nobody else used the tile while the lock was free.
            let untouched = record.pins == 1 && record.version == tick;
            inner.unpin_one(id);
            if let Some(previous) = previous
                && untouched
                && inner.tiles.contains_key(&id)
            {
                inner.unlist(id);
                inner.set_state(id, previous);
                inner.relist(id);
            }
            return Err(error);
        }
        Ok(bytes)
    }

    /// Brings memory within budget. The lock is released while tiles are compressed and
    /// written, so other callers keep working; results are published only for records
    /// that did not change in the meantime. On error no record changes state.
    fn enforce_budget(&self, inner: &mut MutexGuard<'_, Inner>) -> Result<()> {
        while let Some(plan) = inner.plan_moves() {
            let results = MutexGuard::unlocked(inner, || self.run_moves(&plan));
            inner.finish_moves(plan, results)?;
        }
        Ok(())
    }

    /// Compresses and writes the planned tiles in order until the need is covered.
    /// A tile whose compressed form alone covers the rest of the need stays compressed.
    fn run_moves(&self, plan: &MovePlan) -> Result<Vec<TileState>> {
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
                TileState::Spilled { .. } => unreachable!("spilled tiles are not candidates"),
            };
            let resident = matches!(tile.source, TileState::Resident(_));
            if resident && compressed.len() + need <= previous_len {
                need = 0;
                results.push(TileState::Compressed(Arc::from(compressed)));
            } else {
                results.push(self.write_spill(tile, compressed)?);
                need = need.saturating_sub(previous_len);
            }
        }
        Ok(results)
    }

    fn write_spill(&self, tile: &Move, compressed: &[u8]) -> Result<TileState> {
        let mut bytes = Vec::with_capacity(HEADER_LEN + compressed.len());
        bytes.extend_from_slice(&MAGIC);
        bytes.push(tile.codec_version);
        bytes.extend_from_slice(&(tile.byte_size as u32).to_le_bytes());
        bytes.extend_from_slice(&(compressed.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&tile.checksum.to_le_bytes());
        bytes.extend_from_slice(compressed);
        let offset = self.io.lock().append(&bytes)?;
        Ok(TileState::Spilled {
            offset,
            len: bytes.len(),
            checksum: tile.checksum,
        })
    }

    /// Loads a compressed or spilled tile into memory and returns its previous state.
    fn ensure_resident(&self, inner: &mut Inner, id: TileId) -> Result<Option<TileState>> {
        let record = inner.tiles.get(&id).ok_or(TileError::Missing(id))?;
        let raw = match &record.state {
            TileState::Resident(_) => return Ok(None),
            TileState::Compressed(bytes) => decode(bytes, record.checksum)?,
            TileState::Spilled {
                offset,
                len,
                checksum,
            } => {
                let mut bytes = vec![0; *len];
                self.io
                    .lock()
                    .read_exact_at(*offset, &mut bytes)
                    .map_err(|error| {
                        if error.kind() == io::ErrorKind::UnexpectedEof {
                            TileError::Corrupt
                        } else {
                            TileError::Io(error)
                        }
                    })?;
                if bytes.len() < HEADER_LEN
                    || bytes[..4] != MAGIC
                    || bytes[4] != record.codec_version
                    || u32::from_le_bytes(bytes[5..9].try_into().unwrap()) as usize
                        != record.byte_size
                    || u32::from_le_bytes(bytes[9..13].try_into().unwrap()) as usize
                        != len - HEADER_LEN
                    || u32::from_le_bytes(bytes[13..17].try_into().unwrap()) != *checksum
                    || *checksum != record.checksum
                {
                    return Err(TileError::Corrupt);
                }
                decode(&bytes[HEADER_LEN..], *checksum)?
            }
        };
        // The caller immediately holds this tile resident. Other tiles can be evicted later.
        Ok(Some(inner.set_state(id, TileState::Resident(raw))))
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
        inner.unpin_one(self.id);
        if let Err(error) = self.store.enforce_budget(&mut inner)
            && inner.deferred_error.is_none()
        {
            inner.deferred_error = Some(error);
        }
    }
}

impl Inner {
    fn tick(&mut self) -> u64 {
        self.clock = self.clock.wrapping_add(1);
        self.clock
    }

    fn counter(&mut self, state: &TileState) -> (&mut usize, usize) {
        match state {
            TileState::Resident(bytes) => (&mut self.resident_bytes, bytes.len()),
            TileState::Compressed(bytes) => (&mut self.compressed_bytes, bytes.len()),
            TileState::Spilled { len, .. } => (&mut self.spilled_bytes, *len),
        }
    }

    fn add_bytes(&mut self, state: &TileState) {
        let (counter, len) = self.counter(state);
        *counter += len;
    }

    fn sub_bytes(&mut self, state: &TileState) {
        let (counter, len) = self.counter(state);
        *counter -= len;
    }

    /// Replaces a record's state, keeps the byte counters exact and returns the old state.
    /// The caller keeps the eviction order in sync.
    fn set_state(&mut self, id: TileId, state: TileState) -> TileState {
        let version = self.tick();
        self.add_bytes(&state);
        let record = self.tiles.get_mut(&id).expect("existing tile");
        record.version = version;
        let previous = std::mem::replace(&mut record.state, state);
        self.sub_bytes(&previous);
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

    fn unpin_one(&mut self, id: TileId) {
        if let Some(record) = self.tiles.get_mut(&id) {
            record.pins -= 1;
        }
        self.relist(id);
        self.free_unowned(id);
    }

    fn remove_record(&mut self, id: TileId) -> Option<TileRecord> {
        self.unlist(id);
        let record = self.tiles.remove(&id)?;
        self.sub_bytes(&record.state);
        if let Some(ticket) = record.moving {
            self.moving_bytes -= ticket.bytes;
        }
        Some(record)
    }

    fn free_unowned(&mut self, id: TileId) {
        if self
            .tiles
            .get(&id)
            .is_some_and(|record| record.pins == 0 && record.owners == 0)
        {
            let record = self.remove_record(id).expect("checked above");
            self.freed_bytes += record.byte_size;
        }
    }

    /// Picks the oldest candidates whose memory covers the overage and marks them moving.
    /// Returns `None` when memory, minus moves already under way, is within budget.
    fn plan_moves(&mut self) -> Option<MovePlan> {
        let used = (self.resident_bytes + self.compressed_bytes).saturating_sub(self.moving_bytes);
        if used <= self.budget {
            return None;
        }
        let need = used - self.budget;
        let mut remaining = need;
        let mut moves = Vec::new();
        while remaining > 0 {
            let Some((_, id)) = self.lru.pop_first() else {
                break;
            };
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
            remaining = remaining.saturating_sub(bytes);
        }
        (!moves.is_empty()).then_some(MovePlan { need, moves })
    }

    /// Clears the moving marks and publishes each result whose record is unpinned and
    /// unchanged since planning. A failed run publishes nothing.
    fn finish_moves(&mut self, plan: MovePlan, results: Result<Vec<TileState>>) -> Result<()> {
        let (results, error) = match results {
            Ok(results) => (results, None),
            Err(error) => (Vec::new(), Some(error)),
        };
        let mut results = results.into_iter();
        for tile in plan.moves {
            let result = results.next();
            let Some(record) = self.tiles.get_mut(&tile.id) else {
                continue;
            };
            let Some(ticket) = record.moving.filter(|ticket| ticket.token == tile.token) else {
                continue;
            };
            record.moving = None;
            let unchanged = record.pins == 0 && record.version == tile.version;
            self.moving_bytes -= ticket.bytes;
            if let Some(state) = result
                && unchanged
            {
                self.set_state(tile.id, state);
            }
            self.relist(tile.id);
        }
        error.map_or(Ok(()), Err)
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
