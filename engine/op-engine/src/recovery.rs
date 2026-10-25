//! M57 autosave and crash recovery (docs/ARCHITECTURE.md, "Recovery records").
//!
//! Each document with recovery turned on has one writer thread. The writer
//! reads the document's published snapshot (an `arc-swap` load, so it never
//! waits for the actor or the UI) and writes a recovery record after edits
//! settle. A record is one folder:
//!
//! ```text
//! <root>/<doc-uuid>/owner.lock         held (flock) while a live process owns it
//! <root>/<doc-uuid>/manifest.json      the committed record
//! <root>/<doc-uuid>/snapshot-<rev>-<n>.opaint
//! ```
//!
//! A write goes snapshot first (temp, fsync, rename, fsync the folder), then
//! the manifest the same way. The manifest names the snapshot and its CRC-32,
//! so a torn snapshot never wins: until the new manifest is renamed into
//! place, the old manifest still names the old, complete snapshot. Only after
//! the new manifest is committed is the old snapshot deleted, and only then is
//! the revision acknowledged.
use super::{Engine, Error, Event, EventKind, Published, Result, bad, internal};
use arc_swap::ArcSwap;
use crossbeam_channel::{RecvTimeoutError, Sender, bounded, unbounded};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// The manifest's file name inside a record folder.
pub const MANIFEST: &str = "manifest.json";
/// The lock file a live process holds inside a record folder.
pub const LOCK: &str = "owner.lock";
/// The manifest format this engine writes and reads.
pub const FORMAT: u32 = 1;
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// One point inside a record write, for the kill tests' pause hook.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecordStage {
    /// The writer loaded the document state it will write.
    StateLoaded,
    /// The writer took the snapshot and is about to write.
    Begin,
    /// Part way through the snapshot's temp file, with the bytes written so far.
    SnapshotBytes(u64),
    /// The snapshot is renamed into place; the manifest still names the old one.
    SnapshotDone,
    /// The new manifest's temp file is written and synced, not renamed yet.
    ManifestTemp,
    /// The new manifest is renamed and the folder synced; not acknowledged yet.
    Committed,
    /// A clean-up (after a save, or a clean document) deleted the record.
    RecordDeleted,
}

/// Test hooks for a record write: a pause or a callback at each stage, and an
/// injected disk-full failure in the snapshot write.
#[derive(Clone, Default)]
pub struct RecordHooks {
    pub on_stage: Option<Arc<dyn Fn(RecordStage) + Send + Sync>>,
    /// The snapshot write fails with ENOSPC once it would pass this many bytes.
    pub fail_after_bytes: Option<u64>,
    /// While true, the folder sync right after the manifest rename fails.
    pub fail_dir_sync_after_manifest: Option<Arc<AtomicBool>>,
}

/// When the writer writes.
#[derive(Clone)]
pub struct RecoveryOptions {
    /// Write this long after the last change (default 30 s).
    pub settle: Duration,
    /// While edits go on, write at least this often (default 2 minutes).
    pub max_interval: Duration,
    pub hooks: RecordHooks,
}
impl Default for RecoveryOptions {
    fn default() -> Self {
        Self {
            settle: Duration::from_secs(30),
            max_interval: Duration::from_secs(120),
            hooks: RecordHooks::default(),
        }
    }
}

/// The committed record, as `manifest.json` holds it.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Manifest {
    pub format: u32,
    /// The document revision the snapshot holds (the writer's session).
    pub revision: u64,
    /// The file the document was opened from or last saved to, if any.
    pub original_url: Option<String>,
    /// Seconds since 1970 when the record was committed.
    pub time_unix: f64,
    /// The snapshot's file name in the record folder.
    pub snapshot: String,
    pub snapshot_bytes: u64,
    pub snapshot_crc32: u32,
    pub width: u32,
    pub height: u32,
    pub layers: usize,
    pub pid: u32,
    /// The original file's size and modification time when the document was
    /// last opened or saved: a file that differs now was saved after this
    /// record's base, so the record is older than the file.
    #[serde(default)]
    pub base_file: Option<FileStamp>,
}

/// A file's size and modification time.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileStamp {
    pub size: u64,
    pub modified_ns: u64,
}

/// One record found by `scan`.
#[derive(Clone, Debug)]
pub struct RecordInfo {
    pub uuid: String,
    pub dir: PathBuf,
    /// None when the manifest or snapshot cannot be read (`problem` says why).
    pub manifest: Option<Manifest>,
    /// Why the record cannot be recovered over its file, if it cannot.
    pub problem: Option<String>,
    /// The record reads, but its file was saved after it: it can only be
    /// recovered as a new untitled image, never over the newer file.
    pub stale: bool,
}
impl RecordInfo {
    /// The record can be recovered: it reads, and it is not older than its file.
    pub fn is_valid(&self) -> bool {
        self.manifest.is_some() && self.problem.is_none()
    }
    /// The record reads but is older than its file (Recover as Untitled only).
    pub fn is_stale(&self) -> bool {
        self.stale && self.manifest.is_some()
    }
    pub fn to_json(&self) -> serde_json::Value {
        let m = self.manifest.as_ref();
        json!({
            "uuid": self.uuid,
            "dir": self.dir.to_string_lossy(),
            "valid": self.is_valid(),
            "stale": self.is_stale(),
            "problem": self.problem,
            "revision": m.map(|m| m.revision),
            "original_url": m.and_then(|m| m.original_url.clone()),
            "time_unix": m.map(|m| m.time_unix),
            "width": m.map(|m| m.width),
            "height": m.map(|m| m.height),
            "layers": m.map(|m| m.layers),
            "snapshot_bytes": m.map(|m| m.snapshot_bytes),
        })
    }
}

/// What the writer last did, for `doc.recovery`.
#[derive(Clone, Debug, Default)]
pub struct RecoveryStatus {
    pub dir: PathBuf,
    /// The revision of the record on disk in this session, once acknowledged.
    pub acknowledged: Option<u64>,
    /// The last write's error; cleared by the next good write.
    pub error: Option<String>,
    pub writes: u64,
    pub failures: u64,
}
impl RecoveryStatus {
    pub fn to_json(&self) -> serde_json::Value {
        json!({
            "enabled": true,
            "dir": self.dir.to_string_lossy(),
            "acknowledged_revision": self.acknowledged,
            "error": self.error,
            "writes": self.writes,
            "failures": self.failures,
        })
    }
}

fn now_unix() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64())
}

fn io_text(what: &str, path: &Path, e: impl std::fmt::Display) -> String {
    format!("{what} {}: {e}", path.display())
}

fn sync_dir(dir: &Path) -> std::io::Result<()> {
    File::open(dir)?.sync_all()
}

fn parent_of(dir: &Path) -> &Path {
    dir.parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
}

/// The local path of a `file://` URL (percent escapes decoded).
fn url_path(url: &str) -> Option<PathBuf> {
    let rest = url.strip_prefix("file://")?;
    let rest = rest.strip_prefix("localhost").unwrap_or(rest);
    let bytes = rest.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Ok(v) = u8::from_str_radix(std::str::from_utf8(&bytes[i + 1..i + 3]).ok()?, 16)
        {
            out.push(v);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    use std::os::unix::ffi::OsStrExt;
    Some(PathBuf::from(std::ffi::OsStr::from_bytes(&out)))
}

/// The size and modification time of the file at `url` now (None: no file).
pub fn stamp_of(url: Option<&str>) -> Option<FileStamp> {
    let meta = fs::metadata(url_path(url?)?).ok()?;
    let modified = meta
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_nanos();
    Some(FileStamp {
        size: meta.len(),
        modified_ns: u64::try_from(modified).unwrap_or(u64::MAX),
    })
}

/// Why `m` must not be recovered over its file, if it must not: the file was
/// saved (here before a crash, or by another app) after the record's base.
pub fn older_than_file(m: &Manifest) -> Option<String> {
    let url = m.original_url.as_deref()?;
    let now = stamp_of(Some(url))?;
    if m.base_file == Some(now) {
        None
    } else {
        Some(
            "The file was saved after this copy was made, so this copy is older than the file."
                .into(),
        )
    }
}

/// Writes `bytes` to a synced temp file in `dir` and returns its path.
fn write_temp(dir: &Path, name: &str, bytes: &[u8]) -> std::io::Result<PathBuf> {
    let n = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let temp = dir.join(format!(".{name}.{}.{n}.tmp", std::process::id()));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()
    })();
    if let Err(e) = result {
        let _ = fs::remove_file(&temp);
        return Err(e);
    }
    Ok(temp)
}

/// The state a clean save and the writer share for one document.
#[derive(Default)]
pub(crate) struct Shared {
    original: Option<String>,
    base: Option<FileStamp>,
    /// True while a record's manifest is in the folder.
    on_disk: bool,
}

/// How a record write ended without an error.
pub enum WriteOutcome {
    Committed(Manifest),
    /// Abandoned before the commit: the document was saved meanwhile, or the
    /// writer was stopped. The previous record (if any) is untouched.
    Abandoned,
}

/// A failed record write. `committed` is true when the manifest was renamed
/// into place before the failure (a folder sync that failed): that record
/// stays, and is readable.
pub struct WriteError {
    pub text: String,
    pub committed: bool,
}

/// Writes one record of `state` at `revision` into `dir` (the protocol in the
/// module comment). The folder must exist and be locked by the caller.
fn write_record(
    dir: &Path,
    state: &op_core::doc::DocState,
    revision: u64,
    shared: &Mutex<Shared>,
    // The save number read before the state was loaded, and the live one.
    (epoch, saves): (u64, &AtomicU64),
    cancel: &AtomicBool,
    hooks: &RecordHooks,
) -> std::result::Result<WriteOutcome, WriteError> {
    let fail = |text: String| WriteError {
        text,
        committed: false,
    };
    let stage = |s: RecordStage| {
        if let Some(f) = &hooks.on_stage {
            f(s)
        }
    };
    stage(RecordStage::Begin);
    let n = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let name = format!("snapshot-{revision}-{}-{n}.opaint", std::process::id());
    let path = dir.join(&name);
    let mut on_write = |bytes: u64| stage(RecordStage::SnapshotBytes(bytes));
    let saved = op_formats::opaint::save_opaint_with_hooks(
        state,
        &path,
        op_formats::opaint::SaveHooks {
            fail_after_bytes: hooks.fail_after_bytes,
            on_write: Some(&mut on_write),
            cancel: Some(cancel),
        },
    );
    if cancel.load(Ordering::SeqCst) {
        let _ = fs::remove_file(&path);
        return Ok(WriteOutcome::Abandoned);
    }
    saved.map_err(|e| fail(io_text("cannot write", &path, e)))?;
    stage(RecordStage::SnapshotDone);
    let drop_snapshot = |e: WriteError| {
        let _ = fs::remove_file(&path);
        e
    };
    let (bytes, crc) =
        file_crc(&path).map_err(|e| drop_snapshot(fail(io_text("cannot read", &path, e))))?;
    let target = dir.join(MANIFEST);
    let guard = shared.lock();
    if saves.load(Ordering::SeqCst) != epoch || cancel.load(Ordering::SeqCst) {
        drop(guard);
        let _ = fs::remove_file(&path);
        return Ok(WriteOutcome::Abandoned);
    }
    let manifest = Manifest {
        format: FORMAT,
        revision,
        original_url: guard.original.clone(),
        time_unix: now_unix(),
        snapshot: name.clone(),
        snapshot_bytes: bytes,
        snapshot_crc32: crc,
        width: state.width,
        height: state.height,
        layers: state.layers.len(),
        pid: std::process::id(),
        base_file: guard.base,
    };
    let text =
        serde_json::to_vec_pretty(&manifest).map_err(|e| drop_snapshot(fail(e.to_string())))?;
    let temp = write_temp(dir, MANIFEST, &text)
        .map_err(|e| drop_snapshot(fail(io_text("cannot write", &target, e))))?;
    stage(RecordStage::ManifestTemp);
    if let Err(e) = fs::rename(&temp, &target) {
        let _ = fs::remove_file(&temp);
        return Err(drop_snapshot(fail(io_text("cannot write", &target, e))));
    }
    let mut guard = guard;
    guard.on_disk = true;
    // From here on the manifest names `name`: it is never deleted on error.
    let synced = match &hooks.fail_dir_sync_after_manifest {
        Some(f) if f.load(Ordering::SeqCst) => {
            Err(std::io::Error::other("injected folder sync failure"))
        }
        _ => sync_dir(dir),
    };
    drop(guard);
    if let Err(e) = synced {
        return Err(WriteError {
            text: io_text("cannot sync", dir, e),
            committed: true,
        });
    }
    stage(RecordStage::Committed);
    // A leftover old snapshot is harmless (the manifest names the new one);
    // the next write tries again.
    if let Err(e) = remove_stale(dir, Some(&name)) {
        eprintln!("op-engine: {e}");
    }
    Ok(WriteOutcome::Committed(manifest))
}

/// Deletes snapshots other than `keep` and leftover temp files. Returns the
/// first error.
fn remove_stale(dir: &Path, keep: Option<&str>) -> std::result::Result<(), String> {
    let entries = fs::read_dir(dir).map_err(|e| io_text("cannot list", dir, e))?;
    let mut first = Ok(());
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let stale_snapshot = name.starts_with("snapshot-") && Some(name.as_str()) != keep;
        let temp = name.starts_with('.') && name.ends_with(".tmp");
        if (stale_snapshot || temp)
            && let Err(e) = fs::remove_file(entry.path())
            && first.is_ok()
        {
            first = Err(io_text("cannot delete", &entry.path(), e));
        }
    }
    first
}

/// Deletes the manifest first, then the snapshots. On an error the record is
/// still there (or at least not half-gone) and the error is returned.
fn delete_record(dir: &Path, shared: &mut Shared) -> std::result::Result<bool, String> {
    let manifest = dir.join(MANIFEST);
    let existed = match fs::remove_file(&manifest) {
        Ok(()) => true,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
        Err(e) => return Err(io_text("cannot delete", &manifest, e)),
    };
    sync_dir(dir).map_err(|e| io_text("cannot sync", dir, e))?;
    shared.on_disk = false;
    remove_stale(dir, None)?;
    Ok(existed)
}

fn file_crc(path: &Path) -> std::io::Result<(u64, u32)> {
    let mut file = File::open(path)?;
    let mut hasher = crc32fast::Hasher::new();
    let mut buf = vec![0; 1 << 20];
    let mut total = 0_u64;
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        total += n as u64;
        hasher.update(&buf[..n]);
    }
    Ok((total, hasher.finalize()))
}

/// The most manifest bytes a reader accepts.
const MAX_MANIFEST: u64 = 64 * 1024;

/// Checks a manifest's bytes (format, snapshot name). The fuzz target reads here.
pub fn parse_manifest(text: &[u8]) -> std::result::Result<Manifest, String> {
    if text.len() as u64 > MAX_MANIFEST {
        return Err("the manifest is too large".into());
    }
    let manifest: Manifest =
        serde_json::from_slice(text).map_err(|e| format!("bad manifest: {e}"))?;
    if manifest.format != FORMAT {
        return Err(format!("unknown record format {}", manifest.format));
    }
    let name = &manifest.snapshot;
    if name.contains('/') || name.contains('\0') || !name.starts_with("snapshot-") {
        return Err("bad snapshot name in the manifest".into());
    }
    Ok(manifest)
}

/// Reads and checks the committed record in `dir`: the manifest parses, has a
/// known format, and its snapshot has the recorded size and CRC-32.
pub fn read_record(dir: &Path) -> std::result::Result<Manifest, String> {
    let path = dir.join(MANIFEST);
    let mut text = Vec::new();
    File::open(&path)
        .and_then(|f| f.take(MAX_MANIFEST + 1).read_to_end(&mut text))
        .map_err(|e| io_text("cannot read", &path, e))?;
    let manifest = parse_manifest(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    let snapshot = dir.join(&manifest.snapshot);
    let (bytes, crc) = file_crc(&snapshot).map_err(|e| io_text("cannot read", &snapshot, e))?;
    if bytes != manifest.snapshot_bytes || crc != manifest.snapshot_crc32 {
        return Err(format!(
            "the snapshot {} does not match its checksum",
            snapshot.display()
        ));
    }
    Ok(manifest)
}

fn same_file(a: &std::fs::Metadata, b: &std::fs::Metadata) -> bool {
    a.dev() == b.dev() && a.ino() == b.ino()
}

/// A record folder's lock (`owner.lock`, flock). While it is held no other
/// attach, scan or discard touches the folder, and the folder is deleted only
/// while it is held (renamed to a tombstone first).
pub(crate) struct FolderLock {
    file: File,
    dir: PathBuf,
}

impl FolderLock {
    /// Takes the lock, waiting up to `wait` while another holder (such as a
    /// scan) has it. With `create`, makes the folder (and syncs its parent).
    /// None: still held by someone else, or (without `create`) no folder.
    fn take(dir: &Path, create: bool, wait: Duration) -> std::result::Result<Option<Self>, String> {
        let start = Instant::now();
        // Retries after a vanished folder or a lost lock stop after this.
        let limit = wait + Duration::from_secs(2);
        let path = dir.join(LOCK);
        let mut first = true;
        loop {
            if !first && start.elapsed() >= limit {
                return Err(format!(
                    "the recovery folder {} kept changing while it was being locked",
                    dir.display()
                ));
            }
            first = false;
            if !dir.is_dir() {
                if !create {
                    return Ok(None);
                }
                // A new folder is made under a hidden name with its lock
                // already held, then renamed into place: a scan never sees it
                // empty and unlocked (it would delete it as a crash leftover).
                if let Some(lock) = Self::create_locked(dir)? {
                    return Ok(Some(lock));
                }
                // Someone else made the folder meanwhile: lock theirs.
                continue;
            }
            let file = match OpenOptions::new()
                .create(true)
                .truncate(false)
                .write(true)
                .open(&path)
            {
                Ok(f) => f,
                // The folder went away between the check and the open.
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => return Err(io_text("cannot open", &path, e)),
            };
            match file.try_lock() {
                Ok(()) => {}
                Err(std::fs::TryLockError::WouldBlock) => {
                    if start.elapsed() >= wait {
                        return Ok(None);
                    }
                    std::thread::sleep(Duration::from_millis(2));
                    continue;
                }
                Err(std::fs::TryLockError::Error(e)) => {
                    return Err(io_text("cannot lock", &path, e));
                }
            }
            let lock = Self {
                file,
                dir: dir.to_path_buf(),
            };
            // The folder may have been renamed away (deleted) while we waited.
            if lock.held() {
                return Ok(Some(lock));
            }
        }
    }

    /// Makes `dir` with its lock held (see `take`). None: `dir` appeared
    /// meanwhile.
    fn create_locked(dir: &Path) -> std::result::Result<Option<Self>, String> {
        let parent = parent_of(dir);
        fs::create_dir_all(parent).map_err(|e| io_text("cannot make", parent, e))?;
        let n = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let name = dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let fresh = parent.join(format!(".{name}.new-{}-{n}", std::process::id()));
        fs::create_dir(&fresh).map_err(|e| io_text("cannot make", &fresh, e))?;
        let made = (|| {
            let path = fresh.join(LOCK);
            let file = OpenOptions::new()
                .create(true)
                .truncate(false)
                .write(true)
                .open(&path)
                .map_err(|e| io_text("cannot open", &path, e))?;
            file.try_lock()
                .map_err(|e| io_text("cannot lock", &path, format!("{e:?}")))?;
            Ok::<File, String>(file)
        })();
        let file = match made {
            Ok(f) => f,
            Err(e) => {
                let _ = fs::remove_dir_all(&fresh);
                return Err(e);
            }
        };
        // rename(2) of a folder onto an existing non-empty folder fails, so
        // a folder someone else made in between is never replaced.
        if dir.exists() || fs::rename(&fresh, dir).is_err() {
            drop(file);
            let _ = fs::remove_dir_all(&fresh);
            if dir.is_dir() {
                return Ok(None);
            }
            return Err(format!("cannot make {}", dir.display()));
        }
        sync_dir(parent).map_err(|e| io_text("cannot sync", parent, e))?;
        Ok(Some(Self {
            file,
            dir: dir.to_path_buf(),
        }))
    }

    /// The lock file at the folder's path is still the one we hold.
    fn held(&self) -> bool {
        #[cfg(test)]
        if tests::FORCE_LOST.load(Ordering::SeqCst) {
            return false;
        }
        match (fs::metadata(self.dir.join(LOCK)), self.file.metadata()) {
            (Ok(a), Ok(b)) => same_file(&a, &b),
            _ => false,
        }
    }

    /// Deletes the folder while still holding its lock: renames it to a
    /// hidden tombstone (so the path is free at once), then deletes that.
    fn delete_folder(self) -> std::result::Result<(), String> {
        if !self.held() {
            return Ok(());
        }
        let n = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let name = self
            .dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let parent = parent_of(&self.dir).to_path_buf();
        let tomb = parent.join(format!(".{name}.deleted-{}-{n}", std::process::id()));
        fs::rename(&self.dir, &tomb).map_err(|e| io_text("cannot delete", &self.dir, e))?;
        let _ = sync_dir(&parent);
        let result = fs::remove_dir_all(&tomb).map_err(|e| io_text("cannot delete", &tomb, e));
        drop(self.file);
        result
    }
}

/// Moves a record folder to a hidden tombstone name next to it and syncs the
/// parent. Scans skip tombstones (and delete leftover ones).
fn tombstone(dir: &Path) -> std::result::Result<PathBuf, String> {
    let n = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let name = dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let parent = parent_of(dir).to_path_buf();
    let tomb = parent.join(format!(".{name}.deleted-{}-{n}", std::process::id()));
    fs::rename(dir, &tomb).map_err(|e| io_text("cannot delete", dir, e))?;
    sync_dir(&parent).map_err(|e| io_text("cannot sync", &parent, e))?;
    Ok(tomb)
}

/// Lists the records under `root` that no live process owns: each one is a
/// document that did not close cleanly. A folder with no manifest at all (a
/// crash before the first record) and leftover tombstones are deleted.
pub fn scan(root: &Path) -> Vec<RecordInfo> {
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir(root) else {
        return out;
    };
    let mut dirs: Vec<_> = entries
        .flatten()
        .filter(|e| e.path().is_dir())
        .map(|e| e.path())
        .collect();
    dirs.sort();
    for dir in dirs {
        let uuid = dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if uuid.starts_with('.') {
            if uuid.contains(".deleted-")
                && let Ok(Some(lock)) = FolderLock::take(&dir, false, Duration::ZERO)
            {
                drop(lock);
                let _ = fs::remove_dir_all(&dir);
            }
            continue;
        }
        let lock = match FolderLock::take(&dir, false, Duration::ZERO) {
            Ok(Some(lock)) => lock,
            Ok(None) => continue,
            Err(problem) => {
                out.push(RecordInfo {
                    uuid,
                    dir,
                    manifest: None,
                    problem: Some(problem),
                    stale: false,
                });
                continue;
            }
        };
        if !dir.join(MANIFEST).exists() {
            if let Err(e) = lock.delete_folder() {
                eprintln!("op-engine: {e}");
            }
            continue;
        }
        // The lock is held only for the check above: reading the record
        // (the snapshot's checksum can take long) happens without it, so an
        // attach to this folder never waits behind a scan's reads.
        drop(lock);
        let read = match read_record(&dir) {
            Ok(m) => Ok(m),
            // It changed meanwhile: a live owner took it (skip it), or read
            // it again under the lock.
            Err(_) => match FolderLock::take(&dir, false, Duration::ZERO) {
                Ok(Some(lock)) => {
                    let again = read_record(&dir);
                    drop(lock);
                    again
                }
                _ => continue,
            },
        };
        // A live owner may have taken the folder while it was read: skip it.
        match FolderLock::take(&dir, false, Duration::ZERO) {
            Ok(Some(lock)) => drop(lock),
            _ => continue,
        }
        let (manifest, problem) = match read {
            Ok(m) => {
                let stale = older_than_file(&m);
                (Some(m), stale)
            }
            Err(p) => (None, Some(p)),
        };
        let stale = manifest.is_some() && problem.is_some();
        out.push(RecordInfo {
            uuid,
            dir,
            manifest,
            problem,
            stale,
        });
    }
    out
}

/// Deletes a record folder (Discard). Refuses one a live process owns.
pub fn discard(dir: &Path) -> Result<()> {
    if !dir.exists() {
        return Ok(());
    }
    match FolderLock::take(dir, false, Duration::from_millis(200)).map_err(internal)? {
        None if !dir.exists() => Ok(()),
        None => Err(Error::Busy),
        Some(lock) => lock.delete_folder().map_err(internal),
    }
}

enum Ctl {
    /// Write now if the document changed since the record (tests, helpers).
    Flush(Sender<std::result::Result<Option<u64>, String>>),
    /// A clean save happened: take the file's stamp again and delete the record.
    Saved,
    /// The document's file changed (Save As): its URL and stamp.
    SetOriginal(Option<String>, Option<FileStamp>),
    /// Stop. `delete`: a clean close; `tomb` is where close already moved
    /// the folder (deleted here), or None to delete it from its own path.
    Stop { delete: bool, tomb: Option<PathBuf> },
}

pub(crate) struct Writer {
    tx: Sender<Ctl>,
    status: Arc<Mutex<RecoveryStatus>>,
    /// Clean saves so far; a write that began before a save never commits.
    saves: Arc<AtomicU64>,
    cancel: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
}

#[derive(Default)]
pub(crate) struct Registry {
    writers: Mutex<HashMap<u64, Writer>>,
    errors: Mutex<HashMap<u64, String>>,
    /// Writer threads told to stop; Engine's drop waits for them.
    retired: Mutex<Vec<JoinHandle<()>>>,
}

struct WriterState {
    doc: u64,
    dir: PathBuf,
    published: Arc<ArcSwap<Published>>,
    options: RecoveryOptions,
    status: Arc<Mutex<RecoveryStatus>>,
    shared: Arc<Mutex<Shared>>,
    saves: Arc<AtomicU64>,
    cancel: Arc<AtomicBool>,
    events: Sender<Event>,
    /// The revision the record on disk holds, written in this session.
    written: Option<u64>,
    seen: u64,
    last_change: Option<Instant>,
    first_unrecorded: Option<Instant>,
    /// After a failed clean-up, the next try waits until then.
    retry_clear_at: Option<Instant>,
    lock: Option<FolderLock>,
}

fn event_to(events: &Sender<Event>, doc: u64, json: serde_json::Value) {
    let _ = events.send(Event {
        kind: EventKind::Recovery,
        doc,
        ticket: 0,
        json,
    });
}

impl WriterState {
    fn fail(&mut self, e: String) -> String {
        let mut status = self.status.lock();
        status.error = Some(e.clone());
        status.failures += 1;
        drop(status);
        self.event(json!({"ok":false,"error":e}));
        e
    }

    fn write_now(&mut self) -> std::result::Result<Option<u64>, String> {
        // The save number is read before the state: a save after this point
        // (with an edit before it) makes the commit below abandon.
        let epoch = self.saves.load(Ordering::SeqCst);
        let snap = self.published.load_full();
        if let Some(f) = &self.options.hooks.on_stage {
            f(RecordStage::StateLoaded);
        }
        if !snap.history.dirty {
            return self.clear().map(|()| None);
        }
        if self.written == Some(snap.revision) {
            return Ok(Some(snap.revision));
        }
        match &self.lock {
            Some(lock) if lock.held() => {}
            _ => {
                // Never recreate a folder this writer no longer owns.
                self.lock = None;
                return Err(self.fail(format!(
                    "the recovery folder {} was removed",
                    self.dir.display()
                )));
            }
        }
        let result = write_record(
            &self.dir,
            &snap.state,
            snap.revision,
            &self.shared,
            (epoch, &self.saves),
            &self.cancel,
            &self.options.hooks,
        );
        match result {
            Ok(WriteOutcome::Committed(m)) => {
                self.written = Some(m.revision);
                self.first_unrecorded = None;
                let mut status = self.status.lock();
                status.acknowledged = Some(m.revision);
                status.error = None;
                status.writes += 1;
                drop(status);
                self.event(json!({"ok":true,"revision":m.revision}));
                Ok(Some(m.revision))
            }
            Ok(WriteOutcome::Abandoned) => Ok(None),
            Err(e) => Err(self.fail(e.text)),
        }
    }

    /// The document is clean (saved): its record is no longer needed. On an
    /// error the record stays tracked and the clean-up is tried again later.
    fn clear(&mut self) -> std::result::Result<(), String> {
        self.first_unrecorded = None;
        if !self.shared.lock().on_disk {
            self.written = None;
            return Ok(());
        }
        if self.retry_clear_at.is_some_and(|t| Instant::now() < t) {
            return Err("waiting to retry the clean-up".into());
        }
        // The status changes first, so it never names a revision whose
        // record is already gone; it is put back if the delete fails.
        let acknowledged = self.status.lock().acknowledged.take();
        let deleted = {
            let mut shared = self.shared.lock();
            delete_record(&self.dir, &mut shared)
        };
        if deleted.is_ok() {
            self.stage(RecordStage::RecordDeleted);
        } else {
            self.status.lock().acknowledged = acknowledged;
        }
        match deleted {
            Ok(_) => {
                self.retry_clear_at = None;
                self.written = None;
                let mut status = self.status.lock();
                status.acknowledged = None;
                status.error = None;
                drop(status);
                self.event(json!({"ok":true,"revision":null,"cleared":true}));
                Ok(())
            }
            Err(e) => {
                self.retry_clear_at = Some(Instant::now() + self.options.settle);
                Err(self.fail(e))
            }
        }
    }

    fn event(&self, json: serde_json::Value) {
        event_to(&self.events, self.doc, json);
    }

    fn stage(&self, s: RecordStage) {
        if let Some(f) = &self.options.hooks.on_stage {
            f(s)
        }
    }

    /// A clean save happened (on the writer thread, so the save itself never
    /// waits for a record write): the file's stamp is taken again and the
    /// record deleted, manifest first.
    fn saved(&mut self) {
        let mut shared = self.shared.lock();
        shared.base = stamp_of(shared.original.as_deref());
        if !shared.on_disk {
            return;
        }
        // The status changes first, so it never names a revision whose
        // record is already gone; it is put back if the delete fails.
        let acknowledged = self.status.lock().acknowledged.take();
        let deleted = delete_record(&self.dir, &mut shared);
        drop(shared);
        if deleted.is_err() {
            self.status.lock().acknowledged = acknowledged;
        }
        if deleted.is_ok() {
            self.stage(RecordStage::RecordDeleted);
        }
        match deleted {
            Ok(_) => {
                self.written = None;
                self.retry_clear_at = None;
                let mut status = self.status.lock();
                status.acknowledged = None;
                status.error = None;
                drop(status);
                self.event(json!({"ok":true,"revision":null,"cleared":true}));
            }
            Err(e) => {
                self.retry_clear_at = Some(Instant::now() + self.options.settle);
                let _ = self.fail(e);
            }
        }
    }

    /// Notes a new revision and writes when edits settled or waited too long.
    fn tick(&mut self) {
        let snap = self.published.load();
        let now = Instant::now();
        if snap.revision != self.seen {
            self.seen = snap.revision;
            self.last_change = Some(now);
            if self.written != Some(snap.revision) && self.first_unrecorded.is_none() {
                self.first_unrecorded = Some(now);
            }
        }
        if !snap.history.dirty {
            drop(snap);
            let _ = self.clear();
            return;
        }
        if self.written == Some(snap.revision) {
            self.first_unrecorded = None;
            return;
        }
        let first = *self.first_unrecorded.get_or_insert(now);
        let settled = self
            .last_change
            .is_none_or(|t| now.duration_since(t) >= self.options.settle);
        let overdue = now.duration_since(first) >= self.options.max_interval;
        drop(snap);
        if (settled || overdue) && self.write_now().is_err() {
            // Try again after another settle period; the old record stays.
            self.first_unrecorded = Some(now);
            self.last_change = Some(now);
        }
    }

    fn run(mut self, rx: crossbeam_channel::Receiver<Ctl>) {
        let poll = (self.options.settle.min(self.options.max_interval) / 4)
            .min(Duration::from_millis(500))
            .max(Duration::from_millis(5));
        loop {
            match rx.recv_timeout(poll) {
                Ok(Ctl::Flush(reply)) => {
                    let _ = reply.send(self.write_now());
                }
                Ok(Ctl::Saved) => self.saved(),
                Ok(Ctl::SetOriginal(url, base)) => {
                    let mut shared = self.shared.lock();
                    shared.original = url;
                    shared.base = base;
                }
                Ok(Ctl::Stop { delete, tomb }) => {
                    if let Some(tomb) = tomb {
                        drop(self.lock.take());
                        if let Err(e) = fs::remove_dir_all(&tomb) {
                            eprintln!("op-engine: cannot delete {}: {e}", tomb.display());
                        }
                    } else if delete
                        && let Some(lock) = self.lock.take()
                        && let Err(e) = lock.delete_folder()
                    {
                        eprintln!("op-engine: {e}");
                    }
                    return;
                }
                Err(RecvTimeoutError::Timeout) => {
                    if self.cancel.load(Ordering::SeqCst) {
                        continue;
                    }
                    self.tick()
                }
                Err(RecvTimeoutError::Disconnected) => return,
            }
        }
    }
}

impl Registry {
    /// Tells the writer to stop and returns at once. The writer abandons a
    /// write in progress (the last good record stays, or the folder goes when
    /// `delete`), then deletes the folder if asked.
    pub(crate) fn stop(&self, doc: u64, delete: bool) {
        self.stop_impl(doc, delete, false);
    }

    fn stop_impl(&self, doc: u64, delete: bool, wait: bool) {
        let writer = self.writers.lock().remove(&doc);
        if let Some(mut w) = writer {
            w.cancel.store(true, Ordering::SeqCst);
            // A clean close moves the folder out of the way before it returns
            // (one rename and a folder sync), so a Quit right after can never
            // leave a record to offer; the writer deletes the moved folder.
            let tomb = if delete {
                let dir = w.status.lock().dir.clone();
                match tombstone(&dir) {
                    Ok(t) => Some(t),
                    Err(e) => {
                        eprintln!("op-engine: {e}");
                        None
                    }
                }
            } else {
                None
            };
            let _ = w.tx.send(Ctl::Stop { delete, tomb });
            if wait {
                if let Some(join) = w.join.take() {
                    let _ = join.join();
                }
            } else if let Some(join) = w.join.take() {
                let mut retired = self.retired.lock();
                retired.retain(|j| !j.is_finished());
                retired.push(join);
            }
        }
        self.errors.lock().remove(&doc);
    }

    /// Waits for every stopped writer (the engine is going away).
    pub(crate) fn join_retired(&self) {
        let handles: Vec<_> = self.retired.lock().drain(..).collect();
        for h in handles {
            let _ = h.join();
        }
    }

    fn send(&self, doc: u64, ctl: Ctl) -> bool {
        match self.writers.lock().get(&doc) {
            Some(w) => w.tx.send(ctl).is_ok(),
            None => false,
        }
    }

    /// A clean save happened: no record older than it may appear. The save
    /// number moves on at once (a write in progress abandons its commit);
    /// the writer thread takes the file's stamp again and deletes the record,
    /// so the caller (the save's completion on the main thread) never waits.
    /// A crash before that delete leaves a record whose stamp no longer
    /// matches the file: it is offered only as "Recover as Untitled".
    fn saved(&self, doc: u64) {
        let writers = self.writers.lock();
        let Some(w) = writers.get(&doc) else {
            return;
        };
        w.saves.fetch_add(1, Ordering::SeqCst);
        let _ = w.tx.send(Ctl::Saved);
    }
}

impl Engine {
    /// Turns on recovery records for `doc` in `dir` (one folder per document,
    /// `<root>/<doc-uuid>`). `original_url` is the file the document came from,
    /// if any. Calling it again for the same document only updates the URL
    /// (and the file stamp records are compared with).
    pub fn recovery_attach(
        &self,
        doc: u64,
        dir: impl AsRef<Path>,
        original_url: Option<String>,
        options: RecoveryOptions,
    ) -> Result<()> {
        let base = stamp_of(original_url.as_deref());
        self.recovery_attach_impl(doc, dir.as_ref(), original_url, base, options, false)
    }

    fn recovery_attach_impl(
        &self,
        doc: u64,
        dir: &Path,
        original_url: Option<String>,
        base: Option<FileStamp>,
        options: RecoveryOptions,
        adopt: bool,
    ) -> Result<()> {
        let d = self.doc(doc)?;
        let mut writers = self.recovery.writers.lock();
        if let Some(w) = writers.get(&doc) {
            if w.status.lock().dir != dir {
                return Err(bad(
                    "recovery is already on for this document in another folder",
                ));
            }
            // Applied by the writer, so this never waits for a record write.
            let _ = w.tx.send(Ctl::SetOriginal(original_url, base));
            return Ok(());
        }
        // A scan may hold the folder for a moment; wait for it.
        let lock = FolderLock::take(dir, true, Duration::from_secs(2))
            .map_err(internal)?
            .ok_or_else(|| bad("another process owns this recovery folder"))?;
        let on_disk = dir.join(MANIFEST).exists();
        let current = d.published.load();
        let status = Arc::new(Mutex::new(RecoveryStatus {
            dir: dir.to_path_buf(),
            acknowledged: adopt.then_some(current.revision),
            ..Default::default()
        }));
        let saves = Arc::new(AtomicU64::new(0));
        let shared = Arc::new(Mutex::new(Shared {
            original: original_url,
            base,
            on_disk,
        }));
        let cancel = Arc::new(AtomicBool::new(false));
        let state = WriterState {
            doc,
            dir: dir.to_path_buf(),
            published: d.published.clone(),
            options,
            status: status.clone(),
            shared,
            saves: saves.clone(),
            cancel: cancel.clone(),
            events: self.events_tx.clone(),
            written: adopt.then_some(current.revision),
            seen: current.revision,
            last_change: None,
            first_unrecorded: None,
            retry_clear_at: None,
            lock: Some(lock),
        };
        drop(current);
        let (tx, rx) = unbounded();
        let join = std::thread::Builder::new()
            .name(format!("op-recovery-{doc}"))
            .spawn(move || state.run(rx))
            .map_err(internal)?;
        writers.insert(
            doc,
            Writer {
                tx,
                status,
                saves,
                cancel,
                join: Some(join),
            },
        );
        Ok(())
    }

    /// Opens the record in `dir` as a new document marked unsaved, and keeps
    /// writing its records into the same folder, so the old record stays until
    /// a newer one replaces it. Refuses a damaged record and one older than
    /// its file. Returns the document id and the manifest.
    pub fn recovery_open(
        &self,
        dir: impl AsRef<Path>,
        options: RecoveryOptions,
    ) -> Result<(u64, Manifest)> {
        let dir = dir.as_ref();
        let manifest = read_record(dir).map_err(bad)?;
        if let Some(problem) = older_than_file(&manifest) {
            return Err(bad(problem));
        }
        let doc = self.open_opaint(dir.join(&manifest.snapshot))?;
        let opened = (|| {
            self.mark_unsaved(doc)?;
            self.recovery_attach_impl(
                doc,
                dir,
                manifest.original_url.clone(),
                manifest.base_file,
                options,
                true,
            )
        })();
        if let Err(e) = opened {
            let _ = self.close(doc);
            return Err(e);
        }
        Ok((doc, manifest))
    }

    /// Writes a record now if the document changed since the last one.
    /// Returns the acknowledged revision (None: the document is clean, so
    /// there is no record).
    pub fn recovery_flush(&self, doc: u64) -> Result<Option<u64>> {
        let (tx, rx) = bounded(1);
        if !self.recovery.send(doc, Ctl::Flush(tx)) {
            return Err(bad("recovery is off for this document"));
        }
        rx.recv().map_err(internal)?.map_err(internal)
    }

    /// What the writer last did (None: recovery is off for `doc`).
    pub fn recovery_status(&self, doc: u64) -> Option<RecoveryStatus> {
        self.recovery
            .writers
            .lock()
            .get(&doc)
            .map(|w| w.status.lock().clone())
    }

    /// Stops recovery for `doc` and waits for the writer (a write in progress
    /// is abandoned; with `delete`, the record folder is deleted). Tests and
    /// helpers use it; `close` stops without waiting.
    pub fn recovery_detach(&self, doc: u64, delete: bool) {
        self.recovery.stop_impl(doc, delete, true);
    }

    /// Stops recovery for `doc` at once; the writer abandons a write in
    /// progress and (with `delete`) deletes the folder in the background.
    pub fn recovery_detach_nowait(&self, doc: u64, delete: bool) {
        self.recovery.stop(doc, delete);
    }

    /// The last save or mark-saved error for `doc`, as text.
    pub fn last_file_error(&self, doc: u64) -> Option<String> {
        self.recovery.errors.lock().get(&doc).cloned()
    }

    /// `marked`: the operation made the current step the saved one.
    pub(crate) fn note_file_result(&self, doc: u64, result: &Result<()>, marked: bool) {
        match result {
            Ok(()) => {
                self.recovery.errors.lock().remove(&doc);
                if marked {
                    self.recovery.saved(doc);
                }
            }
            Err(e) => {
                self.recovery.errors.lock().insert(doc, e.to_string());
            }
        }
    }

    /// Waits up to `limit` for stopped writers to finish (the app's Quit).
    /// True when all of them finished.
    pub fn recovery_wait_stopped(&self, limit: Duration) -> bool {
        let start = Instant::now();
        loop {
            let mut retired = self.recovery.retired.lock();
            retired.retain(|j| !j.is_finished());
            if retired.is_empty() {
                return true;
            }
            drop(retired);
            if start.elapsed() >= limit {
                return false;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// Opens a record as a new untitled document, even one older than its
    /// file: it names no file, so it can never be saved over the newer one.
    pub fn recovery_open_untitled(
        &self,
        dir: impl AsRef<Path>,
        options: RecoveryOptions,
    ) -> Result<(u64, Manifest)> {
        let dir = dir.as_ref();
        let manifest = read_record(dir).map_err(bad)?;
        let doc = self.open_opaint(dir.join(&manifest.snapshot))?;
        let opened = (|| {
            self.mark_unsaved(doc)?;
            self.recovery_attach_impl(doc, dir, None, None, options, true)
        })();
        if let Err(e) = opened {
            let _ = self.close(doc);
            return Err(e);
        }
        Ok((doc, manifest))
    }

    pub(crate) fn recovery_join(&self) {
        self.recovery.join_retired();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Makes every lock look lost (taken over by another holder).
    pub(super) static FORCE_LOST: AtomicBool = AtomicBool::new(false);
    /// FORCE_LOST is process-wide: the lock tests run one at a time.
    static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn taking_a_lock_that_keeps_being_lost_gives_up_within_its_time_limit() {
        let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let root = std::env::temp_dir().join(format!("op-lock-limit-{}", std::process::id()));
        let dir = root.join("x");
        // An existing folder: taking it checks that the lock is still held.
        fs::create_dir_all(&dir).unwrap();
        FORCE_LOST.store(true, Ordering::SeqCst);
        let (tx, rx) = std::sync::mpsc::channel();
        let d = dir.clone();
        std::thread::spawn(move || {
            let start = Instant::now();
            let r = FolderLock::take(&d, true, Duration::ZERO).map(|l| l.is_some());
            let _ = tx.send((r, start.elapsed()));
        });
        let got = rx.recv_timeout(Duration::from_secs(10));
        FORCE_LOST.store(false, Ordering::SeqCst);
        let _ = fs::remove_dir_all(&root);
        let (result, took) = got.expect("take() kept retrying past its time limit");
        assert!(result.is_err(), "{result:?}");
        assert!(took < Duration::from_secs(5), "{took:?}");
    }

    #[test]
    fn a_new_folder_is_never_visible_without_its_lock_held() {
        let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let root = std::env::temp_dir().join(format!("op-lock-new-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let stolen = Arc::new(AtomicU64::new(0));
        let watcher = {
            let (stop, stolen, root) = (stop.clone(), stolen.clone(), root.clone());
            std::thread::spawn(move || {
                while !stop.load(Ordering::SeqCst) {
                    for i in 0..4 {
                        let dir = root.join(format!("d{i}"));
                        // What a scan does: lock a folder that has no record.
                        if let Ok(Some(lock)) = FolderLock::take(&dir, false, Duration::ZERO) {
                            stolen.fetch_add(1, Ordering::SeqCst);
                            drop(lock);
                        }
                    }
                }
            })
        };
        for n in 0..2000 {
            let dir = root.join(format!("d{}", n % 4));
            let lock = FolderLock::take(&dir, true, Duration::from_secs(2))
                .unwrap()
                .unwrap();
            // Released by deleting the folder, as a close does.
            lock.delete_folder().unwrap();
        }
        stop.store(true, Ordering::SeqCst);
        watcher.join().unwrap();
        let _ = fs::remove_dir_all(&root);
        assert_eq!(
            stolen.load(Ordering::SeqCst),
            0,
            "a scan locked a folder an attach had just made"
        );
    }
}
