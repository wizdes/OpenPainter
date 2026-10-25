//! M57 review round 2: Don't Save then a fast Quit, records older than a
//! changed file (Recover as Untitled), the save-number race, saves that must
//! not wait for a record write, and lock retries with a time limit.
use op_engine::recovery::{self, RecordHooks, RecordStage, RecoveryOptions};
use op_engine::{Engine, SaveFormat};
use serde_json::json;
use std::sync::atomic::{AtomicBool, Ordering};
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

/// Stops the writer at the first `at` stage (once armed) until opened.
#[derive(Clone, Default)]
struct Gate {
    armed: Arc<AtomicBool>,
    state: Arc<(Mutex<(bool, bool)>, Condvar)>,
}
impl Gate {
    fn hook(&self, at: RecordStage) -> Arc<dyn Fn(RecordStage) + Send + Sync> {
        let (state, armed) = (self.state.clone(), self.armed.clone());
        Arc::new(move |stage| {
            if stage != at || !armed.load(Ordering::SeqCst) {
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
    fn arm(&self) {
        self.armed.store(true, Ordering::SeqCst);
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

/// Item 1, in process: close marks the folder discarded before it returns,
/// even while a write is still building its snapshot.
#[test]
fn item1_close_removes_the_folder_before_it_returns() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("doc");
    let e = Engine::new(None).unwrap();
    let doc = e
        .import_pixels(256, 256, &noise(256, 256, 1), None)
        .unwrap();
    let gate = Gate::default();
    let options = RecoveryOptions {
        hooks: RecordHooks {
            on_stage: Some(gate.hook(RecordStage::Begin)),
            ..RecordHooks::default()
        },
        ..manual()
    };
    e.recovery_attach(doc, &dir, None, options).unwrap();
    e.dispatch(doc, "image.invert_colors", json!({})).unwrap();
    e.recovery_flush(doc).unwrap();
    e.dispatch(doc, "image.invert_colors", json!({})).unwrap();
    gate.arm();
    let e = Arc::new(e);
    let e2 = e.clone();
    let flusher = std::thread::spawn(move || e2.recovery_flush(doc));
    wait(|| gate.reached().then_some(()));
    let start = Instant::now();
    e.close(doc).unwrap();
    assert!(start.elapsed() < Duration::from_millis(200));
    // What a launch right now would see: nothing.
    assert!(recovery::scan(root.path()).is_empty());
    assert!(!dir.exists());
    gate.open();
    let _ = flusher.join();
    // A quit waits (with a limit) for the stopped writer.
    assert!(e.recovery_wait_stopped(Duration::from_secs(5)));
    assert!(recovery::scan(root.path()).is_empty());
    let left: Vec<_> = std::fs::read_dir(root.path()).unwrap().collect();
    assert!(left.is_empty(), "{left:?}");
}

/// Item 2: a record older than its changed file can be recovered as a new
/// untitled image (never over the file).
#[test]
fn item2_a_record_older_than_its_file_recovers_as_untitled() {
    let root = tempfile::tempdir().unwrap();
    let rec = root.path().join("records");
    let file = root.path().join("work.opaint");
    let e = Engine::new(None).unwrap();
    let doc = e.import_pixels(64, 48, &noise(64, 48, 2), None).unwrap();
    e.save_document(doc, &file, SaveFormat::Opaint, true)
        .unwrap();
    let dir = rec.join("stale");
    e.recovery_attach(
        doc,
        &dir,
        Some(format!("file://{}", file.display())),
        manual(),
    )
    .unwrap();
    e.dispatch(doc, "image.invert_colors", json!({})).unwrap();
    e.recovery_flush(doc).unwrap();
    let expected = e.snapshot(doc).unwrap().pixels(false).unwrap();
    e.recovery_detach(doc, false);
    // Another app (or a sync service) changes the file.
    std::thread::sleep(Duration::from_millis(20));
    std::fs::write(&file, b"changed elsewhere").unwrap();
    let found = recovery::scan(&rec);
    assert!(!found[0].is_valid());
    assert!(found[0].is_stale());
    assert_eq!(found[0].to_json()["stale"], json!(true));
    assert!(e.recovery_open(&dir, manual()).is_err());
    let (copy, m) = e.recovery_open_untitled(&dir, manual()).unwrap();
    assert!(m.original_url.is_some());
    let snap = e.snapshot(copy).unwrap();
    assert!(snap.history.dirty);
    assert_eq!(snap.pixels(false).unwrap(), expected);
    // Its next record names no file.
    e.dispatch(copy, "image.invert_colors", json!({})).unwrap();
    e.recovery_flush(copy).unwrap();
    assert_eq!(recovery::read_record(&dir).unwrap().original_url, None);
    assert_eq!(std::fs::read(&file).unwrap(), b"changed elsewhere");
}

/// Item 3: an edit and a whole clean save land between the writer loading
/// the state and the commit: the older state must not be committed.
#[test]
fn item3_an_edit_and_save_after_the_state_load_is_not_committed_as_older() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("race");
    let file = root.path().join("f.opaint");
    let e = Engine::new(None).unwrap();
    let doc = e.import_pixels(64, 48, &noise(64, 48, 3), None).unwrap();
    let gate = Gate::default();
    let options = RecoveryOptions {
        hooks: RecordHooks {
            on_stage: Some(gate.hook(RecordStage::StateLoaded)),
            ..RecordHooks::default()
        },
        ..manual()
    };
    e.recovery_attach(
        doc,
        &dir,
        Some(format!("file://{}", file.display())),
        options,
    )
    .unwrap();
    e.dispatch(doc, "image.invert_colors", json!({})).unwrap();
    gate.arm();
    let e = Arc::new(e);
    let e2 = e.clone();
    let flusher = std::thread::spawn(move || e2.recovery_flush(doc));
    wait(|| gate.reached().then_some(()));
    e.dispatch(doc, "image.invert_colors", json!({})).unwrap();
    e.save_document(doc, &file, SaveFormat::Opaint, true)
        .unwrap();
    gate.open();
    let _ = flusher.join().unwrap();
    std::thread::sleep(Duration::from_millis(100));
    assert!(
        recovery::read_record(&dir).is_err(),
        "an older state was committed after the save"
    );
}

/// Item 4: a save (and its mark-saved) does not wait for a record write that
/// holds the record lock.
#[test]
fn item4_a_save_does_not_wait_for_a_record_write() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("slow");
    let e = Engine::new(None).unwrap();
    let doc = e.import_pixels(64, 48, &noise(64, 48, 4), None).unwrap();
    let gate = Gate::default();
    let options = RecoveryOptions {
        hooks: RecordHooks {
            on_stage: Some(gate.hook(RecordStage::ManifestTemp)),
            ..RecordHooks::default()
        },
        ..manual()
    };
    e.recovery_attach(doc, &dir, None, options).unwrap();
    e.dispatch(doc, "image.invert_colors", json!({})).unwrap();
    gate.arm();
    let e = Arc::new(e);
    let e2 = e.clone();
    let flusher = std::thread::spawn(move || e2.recovery_flush(doc));
    wait(|| gate.reached().then_some(()));
    let start = Instant::now();
    e.save_document(doc, root.path().join("s.opaint"), SaveFormat::Opaint, true)
        .unwrap();
    let took = start.elapsed();
    gate.open();
    let _ = flusher.join();
    assert!(
        took < Duration::from_millis(150),
        "the save waited {took:?}"
    );
    // The record still goes, in the background.
    wait(|| recovery::read_record(&dir).is_err().then_some(()));
}

/// Item 5: discard of a folder whose lock file keeps vanishing gives up
/// within its time limit instead of retrying forever.
#[test]
fn item5_lock_retries_stop_within_their_time_limit() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("flaky");
    std::fs::create_dir_all(&dir).unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let churn = {
        let (stop, dir) = (stop.clone(), dir.clone());
        std::thread::spawn(move || {
            while !stop.load(Ordering::SeqCst) {
                let lock = dir.join(recovery::LOCK);
                let _ = std::fs::remove_file(&lock);
                let _ = std::fs::write(&lock, b"");
            }
        })
    };
    let start = Instant::now();
    let _ = recovery::discard(&dir);
    let took = start.elapsed();
    stop.store(true, Ordering::SeqCst);
    churn.join().unwrap();
    assert!(took < Duration::from_secs(4), "discard took {took:?}");
}
