//! M57 review fixes for recovery records (items 1, 2, 3, 5 and 6 of the
//! review): a folder sync failing after the manifest rename, an older record
//! over a newer clean save, folders lost between a scan and an attach, clean-up
//! errors that were hidden, and a close that waited for a write.
use op_engine::recovery::{self, RecordHooks, RecordStage, RecoveryOptions};
use op_engine::{Engine, Event, EventKind, SaveFormat};
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
    let url = format!("file://{}", file.display());
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
