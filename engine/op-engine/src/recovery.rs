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
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
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
}

/// Test hooks for a record write: a pause or a callback at each stage, and an
/// injected disk-full failure in the snapshot write.
#[derive(Clone, Default)]
pub struct RecordHooks {
    pub on_stage: Option<Arc<dyn Fn(RecordStage) + Send + Sync>>,
    /// The snapshot write fails with ENOSPC once it would pass this many bytes.
    pub fail_after_bytes: Option<u64>,
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
}

/// One record found by `scan`.
#[derive(Clone, Debug)]
pub struct RecordInfo {
    pub uuid: String,
    pub dir: PathBuf,
    /// None when the record cannot be used (`problem` says why).
    pub manifest: Option<Manifest>,
    pub problem: Option<String>,
}
impl RecordInfo {
    pub fn to_json(&self) -> serde_json::Value {
        let m = self.manifest.as_ref();
        json!({
            "uuid": self.uuid,
            "dir": self.dir.to_string_lossy(),
            "valid": m.is_some(),
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

/// Writes `bytes` to a temp file in `dir`, syncs it, renames it to `name` and
/// syncs the folder.
fn write_file_atomic(dir: &Path, name: &str, bytes: &[u8]) -> std::io::Result<PathBuf> {
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

/// Writes one record of `state` at `revision` into `dir` (the protocol in the
/// module comment). Returns the committed manifest.
pub fn write_record(
    dir: &Path,
    state: &op_core::doc::DocState,
    revision: u64,
    original_url: Option<&str>,
    hooks: &RecordHooks,
) -> std::result::Result<Manifest, String> {
    let stage = |s: RecordStage| {
        if let Some(f) = &hooks.on_stage {
            f(s)
        }
    };
    fs::create_dir_all(dir).map_err(|e| io_text("cannot make", dir, e))?;
    stage(RecordStage::Begin);
    let n = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let name = format!("snapshot-{revision}-{}-{n}.opaint", std::process::id());
    let path = dir.join(&name);
    let mut on_write = |bytes: u64| stage(RecordStage::SnapshotBytes(bytes));
    op_formats::opaint::save_opaint_with_hooks(
        state,
        &path,
        op_formats::opaint::SaveHooks {
            fail_after_bytes: hooks.fail_after_bytes,
            on_write: Some(&mut on_write),
        },
    )
    .map_err(|e| io_text("cannot write", &path, e))?;
    stage(RecordStage::SnapshotDone);
    let (bytes, crc) = file_crc(&path).map_err(|e| io_text("cannot read", &path, e))?;
    let manifest = Manifest {
        format: FORMAT,
        revision,
        original_url: original_url.map(str::to_owned),
        time_unix: now_unix(),
        snapshot: name.clone(),
        snapshot_bytes: bytes,
        snapshot_crc32: crc,
        width: state.width,
        height: state.height,
        layers: state.layers.len(),
        pid: std::process::id(),
    };
    let text = serde_json::to_vec_pretty(&manifest).map_err(|e| e.to_string())?;
    let target = dir.join(MANIFEST);
    let commit = (|| {
        let temp = write_file_atomic(dir, MANIFEST, &text)?;
        stage(RecordStage::ManifestTemp);
        if let Err(e) = fs::rename(&temp, &target) {
            let _ = fs::remove_file(&temp);
            return Err(e);
        }
        sync_dir(dir)
    })();
    if let Err(e) = commit {
        // The old manifest still names the old snapshot; drop the new one.
        let _ = fs::remove_file(&path);
        return Err(io_text("cannot write", &target, e));
    }
    stage(RecordStage::Committed);
    remove_stale(dir, Some(&name));
    Ok(manifest)
}

/// Deletes snapshots other than `keep` and leftover temp files.
fn remove_stale(dir: &Path, keep: Option<&str>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let stale_snapshot = name.starts_with("snapshot-") && Some(name.as_str()) != keep;
        let temp = name.starts_with('.') && name.ends_with(".tmp");
        if (stale_snapshot || temp)
            && let Err(e) = fs::remove_file(entry.path())
        {
            eprintln!("op-engine: cannot delete {}: {e}", entry.path().display());
        }
    }
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

/// Reads and checks the committed record in `dir`: the manifest parses, has a
/// known format, and its snapshot has the recorded size and CRC-32.
pub fn read_record(dir: &Path) -> std::result::Result<Manifest, String> {
    let path = dir.join(MANIFEST);
    let text = fs::read(&path).map_err(|e| io_text("cannot read", &path, e))?;
    let manifest: Manifest =
        serde_json::from_slice(&text).map_err(|e| io_text("bad manifest", &path, e))?;
    if manifest.format != FORMAT {
        return Err(format!("unknown record format {}", manifest.format));
    }
    if manifest.snapshot.contains('/') || !manifest.snapshot.starts_with("snapshot-") {
        return Err("bad snapshot name in the manifest".into());
    }
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

/// Takes the folder's lock without waiting. None when a live process holds it.
fn try_lock(dir: &Path) -> std::result::Result<Option<File>, String> {
    fs::create_dir_all(dir).map_err(|e| io_text("cannot make", dir, e))?;
    let path = dir.join(LOCK);
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path)
        .map_err(|e| io_text("cannot open", &path, e))?;
    match file.try_lock() {
        Ok(()) => Ok(Some(file)),
        Err(std::fs::TryLockError::WouldBlock) => Ok(None),
        Err(std::fs::TryLockError::Error(e)) => Err(io_text("cannot lock", &path, e)),
    }
}

/// Lists the records under `root` that no live process owns: each one is a
/// document that did not close cleanly. A folder with no manifest at all (a
/// crash before the first record) is cleaned up and not listed.
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
        let lock = match try_lock(&dir) {
            Ok(Some(lock)) => lock,
            Ok(None) => continue,
            Err(problem) => {
                out.push(RecordInfo {
                    uuid,
                    dir,
                    manifest: None,
                    problem: Some(problem),
                });
                continue;
            }
        };
        if !dir.join(MANIFEST).exists() {
            drop(lock);
            if let Err(e) = fs::remove_dir_all(&dir) {
                eprintln!("op-engine: cannot delete {}: {e}", dir.display());
            }
            continue;
        }
        let (manifest, problem) = match read_record(&dir) {
            Ok(m) => (Some(m), None),
            Err(p) => (None, Some(p)),
        };
        drop(lock);
        out.push(RecordInfo {
            uuid,
            dir,
            manifest,
            problem,
        });
    }
    out
}

/// Deletes a record folder (Discard). Refuses one a live process owns.
pub fn discard(dir: &Path) -> Result<()> {
    if !dir.exists() {
        return Ok(());
    }
    match try_lock(dir).map_err(internal)? {
        None => Err(Error::Busy),
        Some(lock) => {
            drop(lock);
            fs::remove_dir_all(dir).map_err(|e| internal(io_text("cannot delete", dir, e)))
        }
    }
}

enum Ctl {
    /// Write now if the document changed since the record (tests, helpers).
    Flush(Sender<std::result::Result<Option<u64>, String>>),
    /// Look at the document now (after a save).
    Wake,
    SetOriginal(Option<String>),
    /// Stop; with `true`, delete the record (a clean close).
    Stop(bool),
}

pub(crate) struct Writer {
    tx: Sender<Ctl>,
    join: Option<JoinHandle<()>>,
    status: Arc<Mutex<RecoveryStatus>>,
}

#[derive(Default)]
pub(crate) struct Registry {
    writers: Mutex<HashMap<u64, Writer>>,
    errors: Mutex<HashMap<u64, String>>,
}

struct WriterState {
    doc: u64,
    dir: PathBuf,
    original: Option<String>,
    published: Arc<ArcSwap<Published>>,
    options: RecoveryOptions,
    status: Arc<Mutex<RecoveryStatus>>,
    events: Sender<Event>,
    /// The revision the record on disk holds, written in this session.
    written: Option<u64>,
    /// True while a record (from any session) exists in `dir`.
    on_disk: bool,
    seen: u64,
    last_change: Option<Instant>,
    first_unrecorded: Option<Instant>,
    _lock: File,
}

impl WriterState {
    fn write_now(&mut self) -> std::result::Result<Option<u64>, String> {
        let snap = self.published.load_full();
        if !snap.history.dirty {
            self.clear();
            return Ok(None);
        }
        if self.written == Some(snap.revision) {
            return Ok(Some(snap.revision));
        }
        let result = write_record(
            &self.dir,
            &snap.state,
            snap.revision,
            self.original.as_deref(),
            &self.options.hooks,
        );
        let mut status = self.status.lock();
        match result {
            Ok(m) => {
                self.written = Some(m.revision);
                self.on_disk = true;
                self.first_unrecorded = None;
                status.acknowledged = Some(m.revision);
                status.error = None;
                status.writes += 1;
                drop(status);
                self.event(json!({"ok":true,"revision":m.revision}));
                Ok(Some(m.revision))
            }
            Err(e) => {
                status.error = Some(e.clone());
                status.failures += 1;
                drop(status);
                self.event(json!({"ok":false,"error":e}));
                Err(e)
            }
        }
    }

    /// The document is clean (saved): its record is no longer needed.
    fn clear(&mut self) {
        if self.on_disk {
            // The status changes first, so nobody sees the record gone while
            // its revision is still reported as acknowledged.
            self.status.lock().acknowledged = None;
            let _ = fs::remove_file(self.dir.join(MANIFEST));
            remove_stale(&self.dir, None);
            let _ = sync_dir(&self.dir);
            self.on_disk = false;
            self.written = None;
            self.event(json!({"ok":true,"revision":null,"cleared":true}));
        }
        self.first_unrecorded = None;
    }

    fn event(&self, json: serde_json::Value) {
        let _ = self.events.send(Event {
            kind: EventKind::Recovery,
            doc: self.doc,
            ticket: 0,
            json,
        });
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
            if self.on_disk {
                self.clear();
            }
            self.first_unrecorded = None;
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
                Ok(Ctl::Wake) => self.tick(),
                Ok(Ctl::SetOriginal(url)) => {
                    self.original = url;
                    // The next record carries the new file name.
                    self.written = None;
                }
                Ok(Ctl::Stop(delete)) => {
                    if delete {
                        self.on_disk = true;
                        self.clear();
                        drop(self._lock);
                        if let Err(e) = fs::remove_dir_all(&self.dir) {
                            eprintln!("op-engine: cannot delete {}: {e}", self.dir.display());
                        }
                    }
                    return;
                }
                Err(RecvTimeoutError::Timeout) => self.tick(),
                Err(RecvTimeoutError::Disconnected) => return,
            }
        }
    }
}

impl Registry {
    pub(crate) fn stop(&self, doc: u64, delete: bool) {
        let writer = self.writers.lock().remove(&doc);
        if let Some(mut w) = writer {
            let _ = w.tx.send(Ctl::Stop(delete));
            if let Some(join) = w.join.take() {
                let _ = join.join();
            }
        }
        self.errors.lock().remove(&doc);
    }
    fn send(&self, doc: u64, ctl: Ctl) -> bool {
        match self.writers.lock().get(&doc) {
            Some(w) => w.tx.send(ctl).is_ok(),
            None => false,
        }
    }
}

impl Engine {
    /// Turns on recovery records for `doc` in `dir` (one folder per document,
    /// `<root>/<doc-uuid>`). `original_url` is the file the document came from,
    /// if any. Calling it again for the same document only updates the URL.
    /// `adopt`: the document's current content is already the record in `dir`
    /// (a recovered document), so nothing is written until the next edit.
    pub fn recovery_attach(
        &self,
        doc: u64,
        dir: impl AsRef<Path>,
        original_url: Option<String>,
        options: RecoveryOptions,
    ) -> Result<()> {
        self.recovery_attach_impl(doc, dir.as_ref(), original_url, options, false)
    }

    fn recovery_attach_impl(
        &self,
        doc: u64,
        dir: &Path,
        original_url: Option<String>,
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
            let _ = w.tx.send(Ctl::SetOriginal(original_url));
            return Ok(());
        }
        let lock = try_lock(dir)
            .map_err(internal)?
            .ok_or_else(|| bad("another process owns this recovery folder"))?;
        let on_disk = dir.join(MANIFEST).exists();
        let current = d.published.load();
        let status = Arc::new(Mutex::new(RecoveryStatus {
            dir: dir.to_path_buf(),
            acknowledged: adopt.then_some(current.revision),
            ..Default::default()
        }));
        let state = WriterState {
            doc,
            dir: dir.to_path_buf(),
            original: original_url,
            published: d.published.clone(),
            options,
            status: status.clone(),
            events: self.events_tx.clone(),
            written: adopt.then_some(current.revision),
            on_disk,
            seen: current.revision,
            last_change: None,
            first_unrecorded: None,
            _lock: lock,
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
                join: Some(join),
                status,
            },
        );
        Ok(())
    }

    /// Opens the record in `dir` as a new document marked unsaved, and keeps
    /// writing its records into the same folder, so the old record stays until
    /// a newer one replaces it. Returns the document id and the manifest.
    pub fn recovery_open(
        &self,
        dir: impl AsRef<Path>,
        options: RecoveryOptions,
    ) -> Result<(u64, Manifest)> {
        let dir = dir.as_ref();
        let manifest = read_record(dir).map_err(bad)?;
        let doc = self.open_opaint(dir.join(&manifest.snapshot))?;
        let opened = (|| {
            self.mark_unsaved(doc)?;
            self.recovery_attach_impl(doc, dir, manifest.original_url.clone(), options, true)
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

    /// Stops recovery for `doc`; with `delete`, deletes its record folder.
    pub fn recovery_detach(&self, doc: u64, delete: bool) {
        self.recovery.stop(doc, delete);
    }

    /// The last save or mark-saved error for `doc`, as text.
    pub fn last_file_error(&self, doc: u64) -> Option<String> {
        self.recovery.errors.lock().get(&doc).cloned()
    }

    pub(crate) fn note_file_result(&self, doc: u64, result: &Result<()>) {
        match result {
            Ok(()) => {
                self.recovery.errors.lock().remove(&doc);
                self.recovery.send(doc, Ctl::Wake);
            }
            Err(e) => {
                self.recovery.errors.lock().insert(doc, e.to_string());
            }
        }
    }
}
