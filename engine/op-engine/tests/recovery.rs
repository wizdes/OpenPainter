//! M57 recovery records: when the writer writes, what a save and a close do,
//! recover after a crash, a failed write (injected disk full) and a damaged
//! snapshot. The process-kill tests are in op-cli/tests/recovery_kill.rs.
use op_engine::recovery::{self, RecordHooks, RecordStage, RecoveryOptions};
use op_engine::{Engine, Event, EventKind, SaveFormat};
use serde_json::json;
use std::sync::{Arc, Mutex};
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

fn fast() -> RecoveryOptions {
    RecoveryOptions {
        settle: Duration::from_millis(40),
        max_interval: Duration::from_secs(60),
        hooks: RecordHooks::default(),
    }
}

/// Polls `f` until it returns Some or 10 s pass.
fn wait<T>(mut f: impl FnMut() -> Option<T>) -> T {
    let start = Instant::now();
    loop {
        if let Some(v) = f() {
            return v;
        }
        assert!(start.elapsed() < Duration::from_secs(10), "timed out");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn setup() -> (tempfile::TempDir, Engine, u64) {
    let root = tempfile::tempdir().unwrap();
    let engine = Engine::new(None).unwrap();
    let doc = engine
        .import_pixels(64, 48, &noise(64, 48, 1), None)
        .unwrap();
    (root, engine, doc)
}

fn revision(e: &Engine, doc: u64) -> u64 {
    e.snapshot(doc).unwrap().revision
}

#[test]
fn a_record_is_written_after_edits_settle_and_names_the_revision() {
    let (root, e, doc) = setup();
    let dir = root.path().join("doc-a");
    e.recovery_attach(doc, &dir, Some("file:///tmp/a.opaint".into()), fast())
        .unwrap();
    // A clean document has no record.
    std::thread::sleep(Duration::from_millis(150));
    assert!(!dir.join(recovery::MANIFEST).exists());
    e.dispatch(doc, "image.invert_colors", json!({})).unwrap();
    let rev = revision(&e, doc);
    let acked = wait(|| e.recovery_status(doc).unwrap().acknowledged);
    assert_eq!(acked, rev);
    let m = recovery::read_record(&dir).unwrap();
    assert_eq!(m.revision, rev);
    assert_eq!(m.original_url.as_deref(), Some("file:///tmp/a.opaint"));
    assert_eq!((m.width, m.height, m.layers), (64, 48, 1));
    assert!(m.time_unix > 1.0e9);
    // The snapshot holds the edited pixels.
    let reopened = e.open_opaint(dir.join(&m.snapshot)).unwrap();
    assert_eq!(
        e.snapshot(reopened).unwrap().pixels(false).unwrap(),
        e.snapshot(doc).unwrap().pixels(false).unwrap()
    );
    // doc.recovery reports the acknowledged revision.
    let q: serde_json::Value =
        serde_json::from_slice(&e.query(doc, "doc.recovery", json!({})).unwrap()).unwrap();
    assert_eq!(q["acknowledged_revision"], json!(rev));
    assert_eq!(q["error"], json!(null));
    // A live document's record is not offered for recovery.
    assert!(recovery::scan(root.path()).is_empty());
}

#[test]
fn continuous_edits_still_write_within_the_max_interval() {
    let (root, e, doc) = setup();
    let dir = root.path().join("doc-b");
    let options = RecoveryOptions {
        settle: Duration::from_secs(60),
        max_interval: Duration::from_millis(200),
        hooks: RecordHooks::default(),
    };
    e.recovery_attach(doc, &dir, None, options).unwrap();
    let start = Instant::now();
    while start.elapsed() < Duration::from_millis(900) {
        e.dispatch(doc, "image.invert_colors", json!({})).unwrap();
        std::thread::sleep(Duration::from_millis(30));
    }
    // Edits never settled for 60 s, yet a record exists.
    let status = e.recovery_status(doc).unwrap();
    assert!(status.writes >= 1, "{status:?}");
    assert!(recovery::read_record(&dir).is_ok());
}

#[test]
fn settle_delay_holds_back_a_write_while_edits_continue() {
    let (root, e, doc) = setup();
    let dir = root.path().join("doc-c");
    let options = RecoveryOptions {
        settle: Duration::from_millis(400),
        max_interval: Duration::from_secs(60),
        hooks: RecordHooks::default(),
    };
    e.recovery_attach(doc, &dir, None, options).unwrap();
    let start = Instant::now();
    while start.elapsed() < Duration::from_millis(600) {
        e.dispatch(doc, "image.invert_colors", json!({})).unwrap();
        std::thread::sleep(Duration::from_millis(40));
    }
    assert_eq!(e.recovery_status(doc).unwrap().writes, 0);
    wait(|| e.recovery_status(doc).unwrap().acknowledged);
}

#[test]
fn a_clean_save_and_undo_to_saved_delete_the_record_and_close_deletes_the_folder() {
    let (root, e, doc) = setup();
    let dir = root.path().join("doc-d");
    e.recovery_attach(doc, &dir, None, fast()).unwrap();
    e.dispatch(doc, "image.invert_colors", json!({})).unwrap();
    wait(|| e.recovery_status(doc).unwrap().acknowledged);
    let saved = root.path().join("saved.opaint");
    e.save_document(doc, &saved, SaveFormat::Opaint, true)
        .unwrap();
    wait(|| (!dir.join(recovery::MANIFEST).exists()).then_some(()));
    assert_eq!(e.recovery_status(doc).unwrap().acknowledged, None);
    // An edit writes a record again; undo back to the saved step clears it.
    e.dispatch(doc, "image.invert_colors", json!({})).unwrap();
    wait(|| e.recovery_status(doc).unwrap().acknowledged);
    e.dispatch(doc, "edit.undo", json!({})).unwrap();
    wait(|| (!dir.join(recovery::MANIFEST).exists()).then_some(()));
    // The manifest goes first, the snapshots right after (M57 review item 5).
    wait(|| {
        let snapshots = std::fs::read_dir(&dir)
            .unwrap()
            .filter(|f| {
                f.as_ref()
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with("snapshot-")
            })
            .count();
        (snapshots == 0).then_some(())
    });
    e.dispatch(doc, "image.invert_colors", json!({})).unwrap();
    wait(|| e.recovery_status(doc).unwrap().acknowledged);
    e.close(doc).unwrap();
    // Close returns at once (M57 review item 6); the folder goes right after.
    wait(|| (!dir.exists()).then_some(()));
}

#[test]
fn a_crashed_documents_record_is_found_and_recovers_unsaved_at_its_revision() {
    let (root, e, doc) = setup();
    let dir = root.path().join("doc-e");
    e.recovery_attach(doc, &dir, Some("file:///x/y.png".into()), fast())
        .unwrap();
    e.dispatch(doc, "image.invert_colors", json!({})).unwrap();
    e.dispatch(doc, "layer.add", json!({})).unwrap();
    let rev = revision(&e, doc);
    assert_eq!(e.recovery_flush(doc).unwrap(), Some(rev));
    let expected = e.snapshot(doc).unwrap().pixels(false).unwrap();
    // A crash: the writer stops and lets go of the folder without deleting it.
    e.recovery_detach(doc, false);
    let found = recovery::scan(root.path());
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].uuid, "doc-e");
    let m = found[0].manifest.clone().unwrap();
    assert_eq!(m.revision, rev);
    assert_eq!(m.layers, 2);
    let json = found[0].to_json();
    assert_eq!(json["valid"], json!(true));
    assert_eq!(json["original_url"], json!("file:///x/y.png"));

    let e2 = Engine::new(None).unwrap();
    let (rec, manifest) = e2.recovery_open(&dir, fast()).unwrap();
    assert_eq!(manifest.revision, rev);
    let snap = e2.snapshot(rec).unwrap();
    assert!(snap.history.dirty, "a recovered document is unsaved");
    assert_eq!(snap.state.layers.len(), 2);
    assert_eq!(snap.pixels(false).unwrap(), expected);
    // The recovered document owns the folder now, and its record stays until
    // a newer one replaces it.
    assert!(recovery::scan(root.path()).is_empty());
    assert_eq!(recovery::read_record(&dir).unwrap().revision, rev);
    e2.dispatch(rec, "image.invert_colors", json!({})).unwrap();
    let new_rev = revision(&e2, rec);
    wait(|| {
        e2.recovery_status(rec)
            .unwrap()
            .acknowledged
            .filter(|&r| r == new_rev)
    });
    // Discard of a live record is refused; after a close the folder is gone.
    assert!(recovery::discard(&dir).is_err());
    e2.close(rec).unwrap();
    // Close returns at once (M57 review item 6); the folder goes right after.
    wait(|| (!dir.exists()).then_some(()));
    drop(e);
}

#[test]
fn discard_deletes_a_crashed_record() {
    let (root, e, doc) = setup();
    let dir = root.path().join("doc-f");
    e.recovery_attach(doc, &dir, None, fast()).unwrap();
    e.dispatch(doc, "image.invert_colors", json!({})).unwrap();
    e.recovery_flush(doc).unwrap();
    e.recovery_detach(doc, false);
    assert_eq!(recovery::scan(root.path()).len(), 1);
    recovery::discard(&dir).unwrap();
    assert!(recovery::scan(root.path()).is_empty());
    assert!(!dir.exists());
}

#[test]
fn a_failed_record_write_keeps_the_previous_record_and_reports_the_error() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("doc-g");
    let events: Arc<Mutex<Vec<Event>>> = Arc::default();
    let sink = events.clone();
    let e = Engine::new(Some(Arc::new(move |ev: Event| {
        sink.lock().unwrap().push(ev)
    })))
    .unwrap();
    let doc = e.import_pixels(64, 48, &noise(64, 48, 2), None).unwrap();
    e.recovery_attach(doc, &dir, None, fast()).unwrap();
    e.dispatch(doc, "image.invert_colors", json!({})).unwrap();
    let good_rev = e.recovery_flush(doc).unwrap().unwrap();
    let good = recovery::read_record(&dir).unwrap();
    let good_bytes = std::fs::read(dir.join(&good.snapshot)).unwrap();
    e.recovery_detach(doc, false);
    // The disk fills up 1,000 bytes into the next snapshot.
    let failing = RecoveryOptions {
        hooks: RecordHooks {
            on_stage: None,
            fail_after_bytes: Some(1000),
            ..RecordHooks::default()
        },
        ..fast()
    };
    e.recovery_attach(doc, &dir, None, failing).unwrap();
    e.dispatch(doc, "image.invert_colors", json!({})).unwrap();
    let err = e.recovery_flush(doc).unwrap_err().to_string();
    assert!(err.contains("No space left on device"), "{err}");
    let status = e.recovery_status(doc).unwrap();
    assert!(status.error.as_deref().unwrap().contains("No space left"));
    // The writer may also have retried on its own after the settle delay.
    assert!(status.failures >= 1);
    // The previous record is still whole and readable.
    let still = recovery::read_record(&dir).unwrap();
    assert_eq!(still.revision, good_rev);
    assert_eq!(
        std::fs::read(dir.join(&still.snapshot)).unwrap(),
        good_bytes
    );
    let leftovers: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|f| f.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".tmp"))
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
    // Nothing is lost from memory: the edit is still there and still unsaved.
    let snap = e.snapshot(doc).unwrap();
    assert!(snap.history.dirty);
    assert_eq!(snap.history.labels.len(), 3);
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
            .contains("No space left")
    );
}

#[test]
fn a_damaged_snapshot_is_listed_as_unusable() {
    let (root, e, doc) = setup();
    let dir = root.path().join("doc-h");
    e.recovery_attach(doc, &dir, None, fast()).unwrap();
    e.dispatch(doc, "image.invert_colors", json!({})).unwrap();
    e.recovery_flush(doc).unwrap();
    e.recovery_detach(doc, false);
    let m = recovery::read_record(&dir).unwrap();
    let path = dir.join(&m.snapshot);
    let mut bytes = std::fs::read(&path).unwrap();
    let mid = bytes.len() / 2;
    bytes[mid] ^= 0xff;
    std::fs::write(&path, bytes).unwrap();
    let found = recovery::scan(root.path());
    assert_eq!(found.len(), 1);
    assert!(found[0].manifest.is_none());
    assert!(
        found[0]
            .problem
            .as_deref()
            .unwrap()
            .contains("does not match its checksum")
    );
    assert!(
        Engine::new(None)
            .unwrap()
            .recovery_open(&dir, fast())
            .is_err()
    );
}

#[test]
fn a_folder_without_a_manifest_is_cleaned_up_by_scan() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("doc-i");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(".snapshot-1.opaint.9.0.tmp"), b"torn").unwrap();
    assert!(recovery::scan(root.path()).is_empty());
    assert!(!dir.exists());
}

#[test]
fn a_record_write_never_blocks_commands_or_queries() {
    let (root, e, doc) = setup();
    let dir = root.path().join("doc-j");
    let gate = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
    let paused = Arc::new(Mutex::new(false));
    let (g, p) = (gate.clone(), paused.clone());
    let options = RecoveryOptions {
        hooks: RecordHooks {
            on_stage: Some(Arc::new(move |stage| {
                if stage == RecordStage::Begin {
                    *p.lock().unwrap() = true;
                    let (lock, cv) = &*g;
                    let mut open = lock.lock().unwrap();
                    while !*open {
                        open = cv.wait(open).unwrap();
                    }
                }
            })),
            fail_after_bytes: None,
            ..RecordHooks::default()
        },
        ..fast()
    };
    e.recovery_attach(doc, &dir, None, options).unwrap();
    e.dispatch(doc, "image.invert_colors", json!({})).unwrap();
    wait(|| (*paused.lock().unwrap()).then_some(()));
    // The writer is stuck inside a write; the document still works.
    let start = Instant::now();
    for _ in 0..5 {
        e.dispatch(doc, "image.invert_colors", json!({})).unwrap();
        e.query(doc, "doc.info", json!({})).unwrap();
    }
    assert!(start.elapsed() < Duration::from_secs(2));
    let (lock, cv) = &*gate;
    *lock.lock().unwrap() = true;
    cv.notify_all();
    let final_rev = revision(&e, doc);
    wait(|| {
        e.recovery_status(doc)
            .unwrap()
            .acknowledged
            .filter(|&r| r == final_rev)
    });
}
