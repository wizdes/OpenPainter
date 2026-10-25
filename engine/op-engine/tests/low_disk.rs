//! M57 low disk on a real full disk: a 20 MB disk image made with `hdiutil`,
//! filled up, then a Save (.opaint and PNG) and a recovery record write fail
//! with "No space left on device". The previous good file and the previous
//! record stay readable, the error text names the problem, and nothing is
//! lost from memory. After space is freed, both writes work again.
//!
//! It mounts a disk image, so it is ignored in the normal run (macOS only):
//! `cargo test -p op-engine --test low_disk -- --ignored --nocapture`
//! The image is detached by its own device path, even when the test fails.
#![cfg(target_os = "macos")]
use op_engine::recovery::{self, RecoveryOptions};
use op_engine::{Engine, SaveFormat};
use serde_json::json;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Detaches the image this test attached, by its device path.
struct Mounted {
    device: String,
    mount: PathBuf,
}
impl Drop for Mounted {
    fn drop(&mut self) {
        let out = Command::new("hdiutil")
            .args(["detach", &self.device, "-force"])
            .output();
        match out {
            Ok(o) if o.status.success() => {}
            other => eprintln!("low_disk: detach {} failed: {other:?}", self.device),
        }
    }
}

fn attach(dir: &Path) -> Mounted {
    let image = dir.join("low.dmg");
    let mount = dir.join("mnt");
    std::fs::create_dir_all(&mount).unwrap();
    let made = Command::new("hdiutil")
        .args([
            "create", "-size", "20m", "-fs", "HFS+", "-volname", "OPM57Low",
        ])
        .arg(&image)
        .output()
        .unwrap();
    assert!(made.status.success(), "{made:?}");
    let out = Command::new("hdiutil")
        .args([
            "attach",
            "-nobrowse",
            "-noautoopen",
            "-noverify",
            "-mountpoint",
        ])
        .arg(&mount)
        .arg(&image)
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let text = String::from_utf8_lossy(&out.stdout);
    // The first /dev/diskN in the output is the whole image; detaching it
    // detaches its volume too.
    let device = text
        .split_whitespace()
        .find(|w| w.starts_with("/dev/disk"))
        .expect("a device path")
        .to_owned();
    eprintln!("low_disk: attached {device} at {}", mount.display());
    Mounted { device, mount }
}

/// Writes 256 KB blocks until the disk is full. Returns the filler's path.
fn fill(dir: &Path) -> PathBuf {
    let path = dir.join("filler.bin");
    let mut f = std::fs::File::create(&path).unwrap();
    let block = vec![0x5a_u8; 256 * 1024];
    loop {
        match f.write_all(&block) {
            Ok(()) => {}
            Err(e) => {
                assert_eq!(e.raw_os_error(), Some(28), "{e}");
                break;
            }
        }
    }
    // Small writes until even those fail, so the disk is truly full.
    loop {
        if f.write_all(&[1; 512]).is_err() {
            break;
        }
    }
    let _ = f.sync_all();
    path
}

fn noise(w: u32, h: u32, seed: u32) -> Vec<u8> {
    let mut x = seed.max(1);
    (0..w * h * 4)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            x as u8
        })
        .collect()
}

#[test]
#[ignore = "mounts a 20 MB disk image; run alone with --ignored"]
fn a_full_disk_keeps_the_last_good_file_and_record_and_loses_nothing_in_memory() {
    let tmp = tempfile::tempdir().unwrap();
    let disk = attach(tmp.path());
    let engine = Engine::new(None).unwrap();
    let doc = engine
        .import_pixels(512, 512, &noise(512, 512, 3), None)
        .unwrap();
    let file = disk.mount.join("work.opaint");
    let png = disk.mount.join("work.png");
    engine
        .save_document(doc, &file, SaveFormat::Opaint, true)
        .unwrap();
    engine
        .save_document(doc, &png, SaveFormat::Png, false)
        .unwrap();
    let good_file = std::fs::read(&file).unwrap();
    let good_png = std::fs::read(&png).unwrap();
    let records = disk.mount.join("Recovery");
    let rec = records.join("0E9C1D2A-LOW");
    engine
        .recovery_attach(
            doc,
            &rec,
            Some("file:///work.opaint".into()),
            RecoveryOptions {
                settle: std::time::Duration::from_secs(3600),
                ..RecoveryOptions::default()
            },
        )
        .unwrap();
    engine
        .dispatch(doc, "image.invert_colors", json!({}))
        .unwrap();
    let good_rev = engine.recovery_flush(doc).unwrap().unwrap();

    let filler = fill(&disk.mount);
    engine.dispatch(doc, "layer.add", json!({})).unwrap();
    engine
        .dispatch(doc, "image.invert_colors", json!({}))
        .unwrap();
    let before = engine.snapshot(doc).unwrap();
    let pixels = before.pixels(false).unwrap();

    let err = engine
        .save_document(doc, &file, SaveFormat::Opaint, true)
        .unwrap_err()
        .to_string();
    eprintln!("low_disk: .opaint save error: {err}");
    assert!(err.contains("No space left on device"), "{err}");
    let shown = engine.last_file_error(doc).unwrap();
    assert!(shown.contains("No space left on device"), "{shown}");
    let err = engine
        .save_document(doc, &png, SaveFormat::Png, true)
        .unwrap_err()
        .to_string();
    eprintln!("low_disk: PNG save error: {err}");
    assert!(err.contains("No space left on device"), "{err}");
    let err = engine.recovery_flush(doc).unwrap_err().to_string();
    eprintln!("low_disk: record error: {err}");
    assert!(err.contains("No space left on device"), "{err}");

    // The last good file, PNG and record are untouched and readable.
    assert_eq!(std::fs::read(&file).unwrap(), good_file);
    assert_eq!(std::fs::read(&png).unwrap(), good_png);
    // Opened the way the app opens it (tiles go to the temp folder; the
    // full disk has no room for anything new).
    let old = engine.open_opaint(&file).unwrap();
    assert_eq!(engine.snapshot(old).unwrap().state.layers.len(), 1);
    engine.close(old).unwrap();
    assert!(op_formats::png::decode_png(&good_png).is_ok());
    assert_eq!(recovery::read_record(&rec).unwrap().revision, good_rev);
    let names: Vec<_> = std::fs::read_dir(&disk.mount)
        .unwrap()
        .chain(std::fs::read_dir(&rec).unwrap())
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".tmp"))
        .collect();
    assert!(
        names.is_empty(),
        "temp files left on the full disk: {names:?}"
    );

    // Nothing is lost from memory.
    let after = engine.snapshot(doc).unwrap();
    assert!(after.history.dirty);
    assert_eq!(after.history.labels, before.history.labels);
    assert_eq!(after.pixels(false).unwrap(), pixels);

    // With space again, the same save and record work.
    std::fs::remove_file(&filler).unwrap();
    engine
        .save_document(doc, &file, SaveFormat::Opaint, true)
        .unwrap();
    assert_eq!(engine.last_file_error(doc), None);
    let reopened = engine.open_opaint(&file).unwrap();
    assert_eq!(engine.snapshot(reopened).unwrap().state.layers.len(), 2);
    assert_eq!(
        engine.snapshot(reopened).unwrap().pixels(false).unwrap(),
        pixels
    );
    engine.close(reopened).unwrap();
    engine
        .dispatch(doc, "image.invert_colors", json!({}))
        .unwrap();
    let rev = engine.recovery_flush(doc).unwrap().unwrap();
    assert_eq!(recovery::read_record(&rec).unwrap().revision, rev);
    engine.close(doc).unwrap();
    drop(engine);
    drop(disk);
}
