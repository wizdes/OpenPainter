//! M57 review fixes for recovery records (items 1, 2, 3, 5 and 6 of the
//! review): a folder sync failing after the manifest rename, an older record
//! over a newer clean save, folders lost between a scan and an attach, clean-up
//! errors that were hidden, and a close that waited for a write.
use op_engine::recovery::{self, RecordHooks, RecordStage, RecoveryOptions};
use op_engine::{Engine, SaveFormat};
#[cfg(any(unix, windows))]
use op_engine::{Event, EventKind};
use serde_json::json;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

fn noise(w: u32, h: u32, seed: u32) -> Vec<u8> {
    let mut x = seed.wrapping_mul(2_654_435_761).max(1);
    (0..w * h * 4)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            x as u8
        })
        .collect()
}

fn manual() -> RecoveryOptions {
    RecoveryOptions {
        settle: Duration::from_secs(3600),
        max_interval: Duration::from_secs(3600),
        hooks: RecordHooks::default(),
    }
}

fn fast() -> RecoveryOptions {
    RecoveryOptions {
        settle: Duration::from_millis(30),
        max_interval: Duration::from_secs(60),
        hooks: RecordHooks::default(),
    }
}

fn wait<T>(mut f: impl FnMut() -> Option<T>) -> T {
    let start = Instant::now();
    loop {
        if let Some(v) = f() {
            return v;
        }
        assert!(start.elapsed() < Duration::from_secs(15), "timed out");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// A hook that stops the writer at `at` until `open` is called.
#[derive(Clone, Default)]
struct Gate {
    state: Arc<(Mutex<(bool, bool)>, Condvar)>,
}
impl Gate {
    fn hook(&self, at: RecordStage) -> Arc<dyn Fn(RecordStage) + Send + Sync> {
        let state = self.state.clone();
        Arc::new(move |stage| {
            let matches = match (at, stage) {
                (RecordStage::SnapshotBytes(_), RecordStage::SnapshotBytes(n)) => n > 2048,
                (a, b) => a == b,
            };
            if !matches {
                return;
            }
            let (lock, cv) = &*state;
            let mut s = lock.lock().unwrap();
            if s.1 {
                return;
            }
            s.0 = true;
            while !s.1 {
                s = cv.wait(s).unwrap();
            }
        })
    }
    fn reached(&self) -> bool {
        self.state.0.lock().unwrap().0
    }
    fn open(&self) {
        let (lock, cv) = &*self.state;
        lock.lock().unwrap().1 = true;
        cv.notify_all();
    }
}

#[test]
fn item1_a_failed_folder_sync_after_the_manifest_rename_keeps_the_new_record() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("sync");
    let e = Engine::new(None).unwrap();
    let doc = e.import_pixels(64, 48, &noise(64, 48, 1), None).unwrap();
    let fail = Arc::new(AtomicBool::new(false));
    let options = RecoveryOptions {
        hooks: RecordHooks {
            fail_dir_sync_after_manifest: Some(fail.clone()),
            ..RecordHooks::default()
        },
        ..manual()
    };
    e.recovery_attach(doc, &dir, None, options).unwrap();
    e.dispatch(doc, "image.invert_colors", json!({})).unwrap();
    e.recovery_flush(doc).unwrap();
    e.dispatch(doc, "image.invert_colors", json!({})).unwrap();
    let rev = e.snapshot(doc).unwrap().revision;
    fail.store(true, Ordering::SeqCst);
    let err = e.recovery_flush(doc).unwrap_err().to_string();
    assert!(err.contains("injected"), "{err}");
    // The renamed manifest names a snapshot that is still there.
    let m = recovery::read_record(&dir).unwrap();
    assert_eq!(m.revision, rev);
    assert!(dir.join(&m.snapshot).exists());
    assert!(
        e.recovery_status(doc)
            .unwrap()
            .error
            .unwrap()
            .contains("injected")
    );
}

#[test]
fn item2a_a_clean_save_during_a_record_write_never_leaves_the_older_record() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("race");
    let e = Engine::new(None).unwrap();
    let doc = e.import_pixels(64, 48, &noise(64, 48, 2), None).unwrap();
    let gate = Gate::default();
    let pause = gate.hook(RecordStage::SnapshotDone);
    // A crash right after the commit would leave whatever is on disk then,
    // so the test looks at the disk at that very moment.
    let saved = Arc::new(AtomicBool::new(false));
    let committed_after_save = Arc::new(AtomicBool::new(false));
    let (s2, c2, d2) = (saved.clone(), committed_after_save.clone(), dir.clone());
    let options = RecoveryOptions {
        hooks: RecordHooks {
            on_stage: Some(Arc::new(move |stage| {
                pause(stage);
                if stage == RecordStage::Committed
                    && s2.load(Ordering::SeqCst)
                    && d2.join(recovery::MANIFEST).exists()
                {
                    c2.store(true, Ordering::SeqCst);
                }
            })),
            ..RecordHooks::default()
        },
        ..fast()
    };
    e.recovery_attach(doc, &dir, None, options).unwrap();
    e.dispatch(doc, "image.invert_colors", json!({})).unwrap();
    // The writer has written revision A's snapshot and waits before its manifest.
    wait(|| gate.reached().then_some(()));
    let file = root.path().join("saved.opaint");
    e.save_document(doc, &file, SaveFormat::Opaint, true)
        .unwrap();
    saved.store(true, Ordering::SeqCst);
    // Once the save returns, no record may exist.
    assert!(!dir.join(recovery::MANIFEST).exists());
    gate.open();
    std::thread::sleep(Duration::from_millis(300));
    assert!(
        !committed_after_save.load(Ordering::SeqCst),
        "an older record was committed after a clean save"
    );
    assert!(recovery::read_record(&dir).is_err());
}

#[test]
fn item2b_a_record_older_than_its_file_is_not_offered() {
    let root = tempfile::tempdir().unwrap();
    let rec = root.path().join("records");
    let file = root.path().join("work.opaint");
    let e = Engine::new(None).unwrap();
    let doc = e.import_pixels(64, 48, &noise(64, 48, 3), None).unwrap();
    e.save_document(doc, &file, SaveFormat::Opaint, true)
        .unwrap();
    let url = file_url(&file);
    let dir = rec.join("stale");
    e.recovery_attach(doc, &dir, Some(url.clone()), manual())
        .unwrap();
    e.dispatch(doc, "image.invert_colors", json!({})).unwrap();
    e.recovery_flush(doc).unwrap();
    e.recovery_detach(doc, false);
    // Untouched file: the record is offered.
    let found = recovery::scan(&rec);
    assert!(found[0].is_valid(), "{:?}", found[0].problem);
    // The file is saved again later (by this app before a crash, or by
    // another app): the record is older than the file.
    std::thread::sleep(Duration::from_millis(20));
    let other = e.import_pixels(32, 16, &noise(32, 16, 4), None).unwrap();
    e.save_document(other, &file, SaveFormat::Opaint, true)
        .unwrap();
    let found = recovery::scan(&rec);
    assert_eq!(found.len(), 1);
    assert!(!found[0].is_valid());
    assert_eq!(found[0].to_json()["valid"], json!(false));
    assert!(
        found[0]
            .problem
            .as_deref()
            .unwrap()
            .contains("saved after this copy"),
        "{:?}",
        found[0].problem
    );
    assert!(e.recovery_open(&dir, manual()).is_err());
}

#[test]
fn item3_concurrent_attaches_and_scans_lose_no_folder() {
    let root = tempfile::tempdir().unwrap();
    let rootp = root.path().to_path_buf();
    let stop = Arc::new(AtomicBool::new(false));
    let scanner = {
        let (stop, rootp) = (stop.clone(), rootp.clone());
        std::thread::spawn(move || {
            let mut scans = 0;
            while !stop.load(Ordering::SeqCst) {
                let _ = recovery::scan(&rootp);
                scans += 1;
            }
            scans
        })
    };
    let e = Engine::new(None).unwrap();
    let doc = e.import_pixels(16, 16, &noise(16, 16, 5), None).unwrap();
    let (failures, lost) = (AtomicUsize::new(0), AtomicUsize::new(0));
    for i in 0..400 {
        let dir = rootp.join(format!("f{}", i % 7));
        if let Err(err) = e.recovery_attach(doc, &dir, None, manual()) {
            failures.fetch_add(1, Ordering::SeqCst);
            eprintln!("attach {i}: {err}");
            continue;
        }
        e.dispatch(doc, "image.invert_colors", json!({})).unwrap();
        match e.recovery_flush(doc) {
            Ok(Some(_)) if dir.join(recovery::MANIFEST).exists() => {}
            other => {
                lost.fetch_add(1, Ordering::SeqCst);
                eprintln!("flush {i}: {other:?}");
            }
        }
        // Close-like stop: the folder is deleted.
        e.recovery_detach(doc, true);
    }
    stop.store(true, Ordering::SeqCst);
    let scans = scanner.join().unwrap();
    assert!(scans > 0);
    assert_eq!(failures.load(Ordering::SeqCst), 0, "attach failures");
    assert_eq!(lost.load(Ordering::SeqCst), 0, "lost folders");
}

#[test]
#[cfg(unix)] // Unix mode bits provide the permission failure injected by this test.
fn item5_a_failed_clean_up_is_reported_and_retried() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("clear");
    let events: Arc<Mutex<Vec<Event>>> = Arc::default();
    let sink = events.clone();
    let e = Engine::new(Some(Arc::new(move |ev: Event| {
        sink.lock().unwrap().push(ev)
    })))
    .unwrap();
    let doc = e.import_pixels(64, 48, &noise(64, 48, 6), None).unwrap();
    e.recovery_attach(doc, &dir, None, fast()).unwrap();
    e.dispatch(doc, "image.invert_colors", json!({})).unwrap();
    wait(|| e.recovery_status(doc).unwrap().acknowledged);
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555)).unwrap();
    e.save_document(doc, root.path().join("x.opaint"), SaveFormat::Opaint, true)
        .unwrap();
    let failure = wait(|| {
        events
            .lock()
            .unwrap()
            .iter()
            .find(|ev| matches!(ev.kind, EventKind::Recovery) && ev.json["ok"] == json!(false))
            .cloned()
    });
    assert!(
        failure.json["error"]
            .as_str()
            .unwrap()
            .contains("cannot delete"),
        "{}",
        failure.json
    );
    assert!(
        !events
            .lock()
            .unwrap()
            .iter()
            .any(|ev| ev.json["cleared"] == json!(true)),
        "a failed clean-up must not say it cleared"
    );
    assert!(dir.join(recovery::MANIFEST).exists());
    assert!(e.recovery_status(doc).unwrap().error.is_some());
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    wait(|| (!dir.join(recovery::MANIFEST).exists()).then_some(()));
    wait(|| {
        events
            .lock()
            .unwrap()
            .iter()
            .any(|ev| ev.json["cleared"] == json!(true))
            .then_some(())
    });
}

#[test]
fn item6_close_during_a_record_write_returns_at_once() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("close");
    let e = Engine::new(None).unwrap();
    let doc = e
        .import_pixels(256, 256, &noise(256, 256, 7), None)
        .unwrap();
    let gate = Gate::default();
    let options = RecoveryOptions {
        hooks: RecordHooks {
            on_stage: Some(gate.hook(RecordStage::SnapshotBytes(0))),
            ..RecordHooks::default()
        },
        ..fast()
    };
    e.recovery_attach(doc, &dir, None, options).unwrap();
    e.dispatch(doc, "image.invert_colors", json!({})).unwrap();
    wait(|| gate.reached().then_some(()));
    let start = Instant::now();
    e.close(doc).unwrap();
    let took = start.elapsed();
    assert!(took < Duration::from_millis(200), "close took {took:?}");
    gate.open();
    // The abandoned write leaves no record: the folder goes.
    wait(|| (!dir.exists()).then_some(()));
}

#[test]
fn item6_a_stop_during_a_write_keeps_the_last_good_record() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("keep");
    let e = Engine::new(None).unwrap();
    let doc = e
        .import_pixels(256, 256, &noise(256, 256, 8), None)
        .unwrap();
    let gate = Gate::default();
    let armed = Arc::new(AtomicBool::new(false));
    let (g, a) = (gate.clone(), armed.clone());
    let inner = g.hook(RecordStage::SnapshotBytes(0));
    let options = RecoveryOptions {
        hooks: RecordHooks {
            on_stage: Some(Arc::new(move |s| {
                if a.load(Ordering::SeqCst) {
                    inner(s)
                }
            })),
            ..RecordHooks::default()
        },
        ..manual()
    };
    e.recovery_attach(doc, &dir, None, options).unwrap();
    e.dispatch(doc, "image.invert_colors", json!({})).unwrap();
    let good = e.recovery_flush(doc).unwrap().unwrap();
    e.dispatch(doc, "image.invert_colors", json!({})).unwrap();
    armed.store(true, Ordering::SeqCst);
    let flusher = {
        let e2 = Arc::new(e);
        let e3 = e2.clone();
        let t = std::thread::spawn(move || e3.recovery_flush(doc));
        wait(|| gate.reached().then_some(()));
        let start = Instant::now();
        e2.recovery_detach_nowait(doc, false);
        assert!(start.elapsed() < Duration::from_millis(200));
        gate.open();
        let _ = t.join();
        e2
    };
    std::thread::sleep(Duration::from_millis(200));
    let m = recovery::read_record(&dir).unwrap();
    assert_eq!(m.revision, good);
    let temps: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|f| f.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".tmp"))
        .collect();
    assert!(temps.is_empty(), "{temps:?}");
    drop(flusher);
}

/// A real file URL for this system: Windows paths need `file:///C:/...`.
fn file_url(path: &std::path::Path) -> String {
    if cfg!(windows) {
        format!("file:///{}", path.display().to_string().replace('\\', "/"))
    } else {
        format!("file://{}", path.display())
    }
}

#[cfg(windows)]
fn names_in(root: &std::path::Path) -> Vec<String> {
    let mut names: Vec<_> = std::fs::read_dir(root)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// A record left on disk with its writer stopped, as a crash would leave it.
#[cfg(windows)]
fn crashed_record(root: &std::path::Path, name: &str) -> std::path::PathBuf {
    let dir = root.join(name);
    let e = Engine::new(None).unwrap();
    let doc = e.import_pixels(64, 48, &noise(64, 48, 10), None).unwrap();
    e.recovery_attach(doc, &dir, None, manual()).unwrap();
    e.dispatch(doc, "image.invert_colors", json!({})).unwrap();
    e.recovery_flush(doc).unwrap();
    e.recovery_detach(doc, false);
    assert!(e.recovery_wait_stopped(Duration::from_secs(5)));
    dir
}

#[cfg(windows)]
#[test]
fn windows_clean_close_with_reader_does_not_offer_recovery() {
    use std::os::windows::fs::OpenOptionsExt;
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("reader-close");
    let e = Engine::new(None).unwrap();
    let doc = e.import_pixels(64, 48, &noise(64, 48, 9), None).unwrap();
    e.recovery_attach(doc, &dir, None, manual()).unwrap();
    e.dispatch(doc, "image.invert_colors", json!({})).unwrap();
    e.recovery_flush(doc).unwrap();
    let record = recovery::read_record(&dir).unwrap();
    let reader = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(dir.join(record.snapshot))
        .unwrap();
    e.close(doc).unwrap();
    assert!(e.recovery_wait_stopped(Duration::from_secs(5)));
    // The writer has stopped and the snapshot could not go. No false offer.
    assert!(dir.is_dir());
    assert!(root.path().join(".reader-close.closed").exists());
    assert!(recovery::scan(root.path()).is_empty());
    drop(reader);
    assert!(recovery::scan(root.path()).is_empty());
    assert_eq!(names_in(root.path()), Vec::<String>::new());
}

#[cfg(windows)]
#[test]
fn windows_a_manifest_held_open_at_close_makes_no_false_offer() {
    use std::os::windows::fs::OpenOptionsExt;
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("held-manifest");
    let e = Engine::new(None).unwrap();
    let doc = e.import_pixels(64, 48, &noise(64, 48, 12), None).unwrap();
    e.recovery_attach(doc, &dir, None, manual()).unwrap();
    e.dispatch(doc, "image.invert_colors", json!({})).unwrap();
    e.recovery_flush(doc).unwrap();
    let reader = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(dir.join(recovery::MANIFEST))
        .unwrap();
    e.close(doc).unwrap();
    assert!(e.recovery_wait_stopped(Duration::from_secs(5)));
    assert!(dir.join(recovery::MANIFEST).exists());
    // Only the marker stands between this manifest and a false offer.
    assert!(recovery::scan(root.path()).is_empty());
    drop(reader);
    assert!(recovery::scan(root.path()).is_empty());
    assert_eq!(names_in(root.path()), Vec::<String>::new());
}

/// Close used to delete the folder and lock before the writer stopped. A new
/// owner could then take the path, and the old writer deleted its snapshot.
#[cfg(windows)]
#[test]
fn windows_a_closing_writer_keeps_its_lock_until_it_stops() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("late-writer");
    let (paused_tx, paused_rx) = std::sync::mpsc::channel::<()>();
    let (resume_tx, resume_rx) = std::sync::mpsc::channel::<()>();
    let resume_rx = Mutex::new(resume_rx);
    let paused_tx = Mutex::new(paused_tx);
    let hooks = RecordHooks {
        on_stage: Some(Arc::new(move |stage| {
            if matches!(stage, RecordStage::Committed) {
                paused_tx.lock().unwrap().send(()).unwrap();
                resume_rx.lock().unwrap().recv().unwrap();
            }
        })),
        ..Default::default()
    };
    let e = Engine::new(None).unwrap();
    let doc = e.import_pixels(64, 48, &noise(64, 48, 13), None).unwrap();
    e.recovery_attach(
        doc,
        &dir,
        None,
        RecoveryOptions {
            settle: Duration::from_millis(5),
            max_interval: Duration::from_secs(60),
            hooks,
        },
    )
    .unwrap();
    e.dispatch(doc, "image.invert_colors", json!({})).unwrap();
    paused_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    e.close(doc).unwrap();
    // The old writer is still inside its write: the path must not be free.
    let other = Engine::new(None).unwrap();
    let doc2 = other
        .import_pixels(64, 48, &noise(64, 48, 14), None)
        .unwrap();
    assert!(other.recovery_attach(doc2, &dir, None, manual()).is_err());
    resume_tx.send(()).unwrap();
    assert!(e.recovery_wait_stopped(Duration::from_secs(5)));
    assert!(recovery::scan(root.path()).is_empty());
    assert_eq!(names_in(root.path()), Vec::<String>::new());
    // Now the path is free, and the new owner's record stays.
    other.recovery_attach(doc2, &dir, None, manual()).unwrap();
    other
        .dispatch(doc2, "image.invert_colors", json!({}))
        .unwrap();
    other.recovery_flush(doc2).unwrap();
    assert!(recovery::read_record(&dir).is_ok());
}

/// An unreadable manifest must never read as "no manifest", or a scan deletes
/// the user's last good record.
#[cfg(windows)]
#[test]
fn windows_a_manifest_that_cannot_be_checked_keeps_the_record() {
    let root = tempfile::tempdir().unwrap();
    let dir = crashed_record(root.path(), "denied");
    let icacls = |target: &std::path::Path, args: &[&str]| {
        let status = std::process::Command::new("icacls")
            .arg(target)
            .args(args)
            .stdout(std::process::Stdio::null())
            .status()
            .unwrap();
        assert!(status.success());
    };
    let everyone = "*S-1-1-0:(RD,RA,REA,X)";
    let manifest = dir.join(recovery::MANIFEST);
    icacls(&manifest, &["/deny", everyone]);
    icacls(&dir, &["/deny", everyone]);
    let found = recovery::scan(root.path());
    icacls(&dir, &["/remove:d", "*S-1-1-0"]);
    icacls(&manifest, &["/remove:d", "*S-1-1-0"]);
    assert!(found.is_empty(), "{found:?}");
    assert!(dir.join(recovery::MANIFEST).exists());
    let found = recovery::scan(root.path());
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].is_valid(), "{:?}", found[0].problem);
}

#[cfg(windows)]
#[test]
fn windows_a_lock_that_cannot_be_opened_hides_the_record_for_one_scan_only() {
    use std::os::windows::fs::OpenOptionsExt;
    let root = tempfile::tempdir().unwrap();
    let dir = crashed_record(root.path(), "busy");
    let held = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(root.path().join(".busy.owner.lock"))
        .unwrap();
    assert!(recovery::scan(root.path()).is_empty());
    assert!(dir.join(recovery::MANIFEST).exists());
    drop(held);
    let found = recovery::scan(root.path());
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].is_valid(), "{:?}", found[0].problem);
}

#[cfg(windows)]
#[test]
fn windows_a_writer_keeps_its_record_through_a_brief_lock_failure() {
    use std::os::windows::fs::OpenOptionsExt;
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("writer-brief");
    let e = Engine::new(None).unwrap();
    let doc = e.import_pixels(64, 48, &noise(64, 48, 15), None).unwrap();
    e.recovery_attach(doc, &dir, None, manual()).unwrap();
    e.dispatch(doc, "image.invert_colors", json!({})).unwrap();
    let first = e.recovery_flush(doc).unwrap();
    e.dispatch(doc, "image.invert_colors", json!({})).unwrap();
    // A handle that refuses readers makes the writer's lock check fail. The
    // writer's own write handle is allowed, because this one shares writes.
    const FILE_SHARE_WRITE: u32 = 2;
    const FILE_SHARE_DELETE: u32 = 4;
    let held = std::fs::OpenOptions::new()
        .write(true)
        .share_mode(FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .open(root.path().join(".writer-brief.owner.lock"))
        .unwrap();
    let err = e.recovery_flush(doc).unwrap_err().to_string();
    assert!(err.contains("cannot check lock"), "{err}");
    assert_eq!(e.recovery_status(doc).unwrap().acknowledged, first);
    assert!(recovery::read_record(&dir).is_ok());
    drop(held);
    let second = e.recovery_flush(doc).unwrap();
    assert!(second > first, "{second:?} after {first:?}");
}

#[cfg(windows)]
#[test]
fn windows_a_record_marked_closed_cannot_be_recovered() {
    let root = tempfile::tempdir().unwrap();
    let dir = crashed_record(root.path(), "marked");
    let marker = root.path().join(".marked.closed");
    std::fs::write(&marker, b"").unwrap();
    let e = Engine::new(None).unwrap();
    assert!(e.recovery_open(&dir, manual()).is_err());
    assert!(marker.exists());
    assert!(recovery::scan(root.path()).is_empty());
    assert_eq!(names_in(root.path()), Vec::<String>::new());
}

/// What each crash point leaves, built by hand, and what the next scan does.
#[cfg(windows)]
#[test]
fn windows_each_crash_state_scans_correctly() {
    let root = tempfile::tempdir().unwrap();
    // After the lock, before the folder.
    std::fs::write(root.path().join(".a.owner.lock"), b"").unwrap();
    // After the folder, before the first record.
    std::fs::create_dir(root.path().join("b")).unwrap();
    std::fs::write(root.path().join(".b.owner.lock"), b"").unwrap();
    // After the record: this one must be offered.
    crashed_record(root.path(), "c");
    // After the marker, before the manifest went.
    let d = crashed_record(root.path(), "d");
    std::fs::write(root.path().join(".d.closed"), b"").unwrap();
    assert!(d.join(recovery::MANIFEST).exists());
    // After the manifest went, before the folder.
    let e = crashed_record(root.path(), "e");
    std::fs::write(root.path().join(".e.closed"), b"").unwrap();
    std::fs::remove_file(e.join(recovery::MANIFEST)).unwrap();
    // After the folder, before the marker and lock.
    std::fs::write(root.path().join(".f.closed"), b"").unwrap();
    std::fs::write(root.path().join(".f.owner.lock"), b"").unwrap();
    // After close renamed the folder, before the writer deleted it.
    let hidden = root.path().join(".g.deleted-1-1");
    std::fs::create_dir(&hidden).unwrap();
    std::fs::write(hidden.join("snapshot-1-1-1.opaint"), b"x").unwrap();
    std::fs::write(root.path().join(".g.closed"), b"").unwrap();
    std::fs::write(root.path().join(".g.owner.lock"), b"").unwrap();
    let found = recovery::scan(root.path());
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].uuid, "c");
    assert!(found[0].is_valid(), "{:?}", found[0].problem);
    assert_eq!(names_in(root.path()), [".c.owner.lock", "c"]);
}

#[cfg(windows)]
#[test]
fn windows_old_layout_record_still_reads_and_its_lock_file_is_not_used() {
    let root = tempfile::tempdir().unwrap();
    let dir = crashed_record(root.path(), "old-layout");
    // A record as the old layout wrote it: a lock inside, none beside.
    std::fs::write(dir.join(recovery::LOCK), b"").unwrap();
    std::fs::remove_file(root.path().join(".old-layout.owner.lock")).unwrap();
    let found = recovery::scan(root.path());
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].is_valid(), "{:?}", found[0].problem);
    let e = Engine::new(None).unwrap();
    let (doc, _) = e.recovery_open(&dir, manual()).unwrap();
    // The owner holds the lock beside the folder, so others stay out.
    assert!(recovery::discard(&dir).is_err());
    e.close(doc).unwrap();
    assert!(e.recovery_wait_stopped(Duration::from_secs(5)));
    assert_eq!(names_in(root.path()), Vec::<String>::new());
}

/// Close used to delete whatever was at the path, even after losing the lock.
#[cfg(windows)]
#[test]
fn windows_closing_a_former_owner_keeps_the_new_owners_record() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("former");
    let old = Engine::new(None).unwrap();
    let old_doc = old.import_pixels(64, 48, &noise(64, 48, 21), None).unwrap();
    old.recovery_attach(old_doc, &dir, None, manual()).unwrap();
    old.dispatch(old_doc, "image.invert_colors", json!({}))
        .unwrap();
    old.recovery_flush(old_doc).unwrap();
    // Something outside deletes the lock file. A new owner then takes over.
    std::fs::remove_file(root.path().join(".former.owner.lock")).unwrap();
    let new = Engine::new(None).unwrap();
    let new_doc = new.import_pixels(64, 48, &noise(64, 48, 22), None).unwrap();
    new.recovery_attach(new_doc, &dir, None, manual()).unwrap();
    new.dispatch(new_doc, "image.invert_colors", json!({}))
        .unwrap();
    new.recovery_flush(new_doc).unwrap();
    old.close(old_doc).unwrap();
    assert!(old.recovery_wait_stopped(Duration::from_secs(5)));
    let record = recovery::read_record(&dir).expect("the new owner's record");
    assert!(dir.join(record.snapshot).exists());
    new.dispatch(new_doc, "image.invert_colors", json!({}))
        .unwrap();
    new.recovery_flush(new_doc).unwrap();
}

/// Another program holding a file is not damage. Listing the record as damaged
/// would steer the user to Discard.
#[cfg(windows)]
#[test]
fn windows_a_held_file_does_not_list_the_record_as_damaged() {
    use std::os::windows::fs::OpenOptionsExt;
    let root = tempfile::tempdir().unwrap();
    let dir = crashed_record(root.path(), "held-file");
    let record = recovery::read_record(&dir).unwrap();
    for name in [record.snapshot.as_str(), recovery::MANIFEST] {
        let held = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(dir.join(name))
            .unwrap();
        let found = recovery::scan(root.path());
        assert!(found.is_empty(), "{name}: {found:?}");
        drop(held);
        let found = recovery::scan(root.path());
        assert_eq!(found.len(), 1, "{name}: {found:?}");
        assert!(found[0].is_valid(), "{name}: {:?}", found[0].problem);
    }
}

/// A lock that can never be opened: attach gives the real reason, and a scan
/// does not wait the full 2 s on each record.
#[cfg(windows)]
#[test]
fn windows_a_lock_that_stays_denied_names_the_cause_and_scan_stays_quick() {
    let root = tempfile::tempdir().unwrap();
    let dir = crashed_record(root.path(), "denied-lock");
    let lock = root.path().join(".denied-lock.owner.lock");
    let mut perms = std::fs::metadata(&lock).unwrap().permissions();
    perms.set_readonly(true);
    std::fs::set_permissions(&lock, perms.clone()).unwrap();
    let start = Instant::now();
    assert!(recovery::scan(root.path()).is_empty());
    assert!(
        start.elapsed() < Duration::from_secs(1),
        "{:?}",
        start.elapsed()
    );
    assert!(dir.join(recovery::MANIFEST).exists());
    let e = Engine::new(None).unwrap();
    let doc = e.import_pixels(64, 48, &noise(64, 48, 24), None).unwrap();
    let err = e
        .recovery_attach(doc, &dir, None, manual())
        .unwrap_err()
        .to_string();
    assert!(err.contains("cannot open"), "{err}");
    #[allow(clippy::permissions_set_readonly_false)]
    perms.set_readonly(false);
    std::fs::set_permissions(&lock, perms).unwrap();
}

/// The same check as item5_lock_retries_stop_within_their_time_limit, on the
/// lock file Windows really uses.
#[cfg(windows)]
#[test]
fn windows_lock_churn_stops_within_the_time_limit() {
    let root = tempfile::tempdir().unwrap();
    let dir = crashed_record(root.path(), "churn");
    let lock = root.path().join(".churn.owner.lock");
    let stop = Arc::new(AtomicBool::new(false));
    let (flag, path) = (stop.clone(), lock.clone());
    let churn = std::thread::spawn(move || {
        while !flag.load(Ordering::SeqCst) {
            let _ = std::fs::remove_file(&path);
            let _ = std::fs::write(&path, b"");
        }
    });
    let e = Engine::new(None).unwrap();
    let doc = e.import_pixels(64, 48, &noise(64, 48, 25), None).unwrap();
    let start = Instant::now();
    let result = e.recovery_attach(doc, &dir, None, manual());
    assert!(
        start.elapsed() < Duration::from_secs(8),
        "{:?}",
        start.elapsed()
    );
    stop.store(true, Ordering::SeqCst);
    churn.join().unwrap();
    // Either it got through, or it says what stopped it. Never a panic, and
    // the record survives either way.
    if let Err(error) = &result {
        assert!(!error.to_string().is_empty());
    }
    assert!(dir.join(recovery::MANIFEST).exists());
}

/// Two writers on one folder, the first having lost its lock to something
/// outside. Returns the former owner, its document, and the new owner.
#[cfg(windows)]
fn takeover(root: &std::path::Path, name: &str) -> (Engine, u64, Engine, u64) {
    let dir = root.join(name);
    let old = Engine::new(None).unwrap();
    let old_doc = old.import_pixels(64, 48, &noise(64, 48, 31), None).unwrap();
    old.recovery_attach(old_doc, &dir, None, manual()).unwrap();
    old.dispatch(old_doc, "image.invert_colors", json!({}))
        .unwrap();
    old.recovery_flush(old_doc).unwrap();
    std::fs::remove_file(root.join(format!(".{name}.owner.lock"))).unwrap();
    let new = Engine::new(None).unwrap();
    let new_doc = new.import_pixels(64, 48, &noise(64, 48, 32), None).unwrap();
    new.recovery_attach(new_doc, &dir, None, manual()).unwrap();
    new.dispatch(new_doc, "image.invert_colors", json!({}))
        .unwrap();
    new.recovery_flush(new_doc).unwrap();
    (old, old_doc, new, new_doc)
}

#[cfg(windows)]
#[test]
fn windows_saving_a_former_owner_keeps_the_new_owners_record() {
    let root = tempfile::tempdir().unwrap();
    let (old, old_doc, _new, _new_doc) = takeover(root.path(), "saved");
    let dir = root.path().join("saved");
    old.save_document(
        old_doc,
        root.path().join("x.opaint"),
        SaveFormat::Opaint,
        true,
    )
    .unwrap();
    // A flush runs after the writer handled the save.
    let _ = old.recovery_flush(old_doc);
    let record = recovery::read_record(&dir).expect("the new owner's record");
    assert!(dir.join(record.snapshot).exists());
}

#[cfg(windows)]
#[test]
fn windows_a_former_owner_past_its_check_keeps_the_new_owners_record() {
    for pause_at in ["ManifestTemp", "Committed"] {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("late");
        let (paused_tx, paused_rx) = std::sync::mpsc::channel::<()>();
        let (resume_tx, resume_rx) = std::sync::mpsc::channel::<()>();
        let (paused_tx, resume_rx) = (Mutex::new(paused_tx), Mutex::new(resume_rx));
        let armed = Arc::new(AtomicBool::new(false));
        let trigger = armed.clone();
        let hooks = RecordHooks {
            on_stage: Some(Arc::new(move |stage| {
                let hit = match pause_at {
                    "ManifestTemp" => matches!(stage, RecordStage::ManifestTemp),
                    _ => matches!(stage, RecordStage::Committed),
                };
                if hit && trigger.swap(false, Ordering::SeqCst) {
                    paused_tx.lock().unwrap().send(()).unwrap();
                    resume_rx.lock().unwrap().recv().unwrap();
                }
            })),
            ..Default::default()
        };
        let old = Arc::new(Engine::new(None).unwrap());
        let old_doc = old.import_pixels(64, 48, &noise(64, 48, 33), None).unwrap();
        old.recovery_attach(
            old_doc,
            &dir,
            None,
            RecoveryOptions {
                settle: Duration::from_secs(600),
                max_interval: Duration::from_secs(600),
                hooks,
            },
        )
        .unwrap();
        old.dispatch(old_doc, "image.invert_colors", json!({}))
            .unwrap();
        armed.store(true, Ordering::SeqCst);
        let flusher = old.clone();
        let flushed = std::thread::spawn(move || flusher.recovery_flush(old_doc).map(|_| ()));
        paused_rx.recv_timeout(Duration::from_secs(10)).unwrap();
        std::fs::remove_file(root.path().join(".late.owner.lock")).unwrap();
        let new = Engine::new(None).unwrap();
        let new_doc = new.import_pixels(80, 60, &noise(80, 60, 34), None).unwrap();
        new.recovery_attach(new_doc, &dir, None, manual()).unwrap();
        new.dispatch(new_doc, "image.invert_colors", json!({}))
            .unwrap();
        new.recovery_flush(new_doc).unwrap();
        resume_tx.send(()).unwrap();
        let _ = flushed.join().unwrap();
        let record = recovery::read_record(&dir).unwrap_or_else(|e| panic!("{pause_at}: {e}"));
        assert_eq!(record.width, 80, "{pause_at}");
        assert!(dir.join(record.snapshot).exists(), "{pause_at}");
    }
}

/// A brief denial of the lock during close must not leave a record to offer.
#[cfg(windows)]
#[test]
fn windows_a_brief_lock_denial_during_close_makes_no_offer() {
    use std::os::windows::fs::OpenOptionsExt;
    const FILE_SHARE_WRITE: u32 = 2;
    const FILE_SHARE_DELETE: u32 = 4;
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("denied-close");
    let e = Engine::new(None).unwrap();
    let doc = e.import_pixels(64, 48, &noise(64, 48, 35), None).unwrap();
    e.recovery_attach(doc, &dir, None, manual()).unwrap();
    e.dispatch(doc, "image.invert_colors", json!({})).unwrap();
    e.recovery_flush(doc).unwrap();
    let held = std::fs::OpenOptions::new()
        .write(true)
        .share_mode(FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .open(root.path().join(".denied-close.owner.lock"))
        .unwrap();
    e.close(doc).unwrap();
    std::thread::sleep(Duration::from_millis(300));
    drop(held);
    assert!(e.recovery_wait_stopped(Duration::from_secs(10)));
    assert!(recovery::scan(root.path()).is_empty());
    assert_eq!(names_in(root.path()), Vec::<String>::new());
}

/// A lock file deleted from outside, with nobody else taking over.
#[cfg(windows)]
#[test]
fn windows_a_lost_lock_with_no_new_owner_makes_no_offer_after_close() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("lock-gone");
    let e = Engine::new(None).unwrap();
    let doc = e.import_pixels(64, 48, &noise(64, 48, 36), None).unwrap();
    e.recovery_attach(doc, &dir, None, manual()).unwrap();
    e.dispatch(doc, "image.invert_colors", json!({})).unwrap();
    e.recovery_flush(doc).unwrap();
    std::fs::remove_file(root.path().join(".lock-gone.owner.lock")).unwrap();
    e.close(doc).unwrap();
    assert!(e.recovery_wait_stopped(Duration::from_secs(5)));
    assert!(recovery::scan(root.path()).is_empty());
    assert_eq!(names_in(root.path()), Vec::<String>::new());
}

/// A byte-range lock lets a file open but not read. That is not damage.
#[cfg(windows)]
#[test]
fn windows_a_byte_range_lock_does_not_list_the_record_as_damaged() {
    let root = tempfile::tempdir().unwrap();
    let dir = crashed_record(root.path(), "range");
    let record = recovery::read_record(&dir).unwrap();
    for name in [record.snapshot.as_str(), recovery::MANIFEST] {
        let held = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(dir.join(name))
            .unwrap();
        held.lock().unwrap();
        let found = recovery::scan(root.path());
        assert!(found.is_empty(), "{name}: {found:?}");
        drop(held);
        let found = recovery::scan(root.path());
        assert_eq!(found.len(), 1, "{name}: {found:?}");
        assert!(found[0].is_valid(), "{name}: {:?}", found[0].problem);
    }
}

/// A truly bad record is still listed, so Discard stays reachable.
#[cfg(windows)]
#[test]
fn windows_a_really_damaged_record_is_still_listed() {
    let root = tempfile::tempdir().unwrap();
    let dir = crashed_record(root.path(), "bad");
    let record = recovery::read_record(&dir).unwrap();
    std::fs::write(dir.join(record.snapshot), b"not a snapshot").unwrap();
    let found = recovery::scan(root.path());
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(!found[0].is_valid());
}

/// A new document taking over a folder marked closed must not bring the old
/// record back if it crashes before its own first write.
#[cfg(windows)]
#[test]
fn windows_a_new_document_over_a_marked_folder_removes_the_old_record() {
    let root = tempfile::tempdir().unwrap();
    let dir = crashed_record(root.path(), "reused");
    std::fs::write(root.path().join(".reused.closed"), b"").unwrap();
    let e = Engine::new(None).unwrap();
    let doc = e.import_pixels(64, 48, &noise(64, 48, 37), None).unwrap();
    e.recovery_attach(doc, &dir, None, manual()).unwrap();
    assert!(!root.path().join(".reused.closed").exists());
    assert!(!dir.join(recovery::MANIFEST).exists());
    e.recovery_detach(doc, false);
    assert!(e.recovery_wait_stopped(Duration::from_secs(5)));
    assert!(recovery::scan(root.path()).is_empty());
}

#[cfg(windows)]
#[test]
fn windows_discard_removes_everything() {
    let root = tempfile::tempdir().unwrap();
    let dir = crashed_record(root.path(), "discard-all");
    recovery::discard(&dir).unwrap();
    assert_eq!(names_in(root.path()), Vec::<String>::new());
}

#[cfg(windows)]
#[test]
fn item5_a_failed_clean_up_is_reported_and_retried_on_windows() {
    use std::os::windows::fs::OpenOptionsExt;
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("clear-windows");
    let events: Arc<Mutex<Vec<Event>>> = Arc::default();
    let sink = events.clone();
    let e = Engine::new(Some(Arc::new(move |ev: Event| {
        sink.lock().unwrap().push(ev)
    })))
    .unwrap();
    let doc = e.import_pixels(64, 48, &noise(64, 48, 11), None).unwrap();
    e.recovery_attach(doc, &dir, None, fast()).unwrap();
    e.dispatch(doc, "image.invert_colors", json!({})).unwrap();
    wait(|| e.recovery_status(doc).unwrap().acknowledged);
    let held = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(dir.join(recovery::MANIFEST))
        .unwrap();
    e.save_document(doc, root.path().join("x.opaint"), SaveFormat::Opaint, true)
        .unwrap();
    let failure = wait(|| {
        events
            .lock()
            .unwrap()
            .iter()
            .find(|ev| matches!(ev.kind, EventKind::Recovery) && ev.json["ok"] == json!(false))
            .cloned()
    });
    assert!(
        failure.json["error"]
            .as_str()
            .unwrap()
            .contains("cannot delete"),
        "{}",
        failure.json
    );
    assert!(dir.join(recovery::MANIFEST).exists());
    assert!(e.recovery_status(doc).unwrap().error.is_some());
    assert!(
        !events
            .lock()
            .unwrap()
            .iter()
            .any(|ev| ev.json["cleared"] == json!(true))
    );
    drop(held);
    wait(|| (!dir.join(recovery::MANIFEST).exists()).then_some(()));
    wait(|| {
        events
            .lock()
            .unwrap()
            .iter()
            .any(|ev| ev.json["cleared"] == json!(true))
            .then_some(())
    });
}
