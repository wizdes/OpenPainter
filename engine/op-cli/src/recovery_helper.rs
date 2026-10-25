//! `opcli recovery-helper <record-dir> <stage>`: the M57 kill-test helper.
//! It edits a document, writes one recovery record and prints
//! `ACK <revision> <hash>`, edits again and prints `EDIT <revision> <hash>`,
//! then stops at `<stage>` and prints `PAUSED <stage>` until it is killed:
//!
//! - `before`: before the second record write starts
//! - `snapshot-mid`: part way through the second snapshot's temp file
//! - `snapshot-done`: the second snapshot is in place, the manifest is old
//! - `manifest-temp`: the new manifest's temp file is synced, not renamed
//! - `committed`: the new manifest is renamed, not yet acknowledged
//! - `after`: the second record is acknowledged (`ACK` printed again)
//!
//! The hash is FNV-1a over the layer count and the flattened pixels.
use op_engine::recovery::{RecordHooks, RecordStage, RecoveryOptions};
use op_engine::{Engine, EngineOptions};
use serde_json::json;
use std::io::Write;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

pub const STAGES: [&str; 6] = [
    "before",
    "snapshot-mid",
    "snapshot-done",
    "manifest-temp",
    "committed",
    "after",
];

/// FNV-1a 64 over the layer count and the flattened RGBA pixels.
pub fn doc_hash(layers: usize, pixels: &[u8]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325_u64;
    for b in (layers as u64).to_le_bytes().iter().chain(pixels) {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

fn pause(stage: &str) -> ! {
    println!("PAUSED {stage}");
    let _ = std::io::stdout().flush();
    loop {
        std::thread::sleep(Duration::from_secs(3600));
    }
}

fn matches(stage: &str, at: RecordStage) -> bool {
    match (stage, at) {
        // Well inside the snapshot, after its first entries are written.
        ("snapshot-mid", RecordStage::SnapshotBytes(n)) => n > 4096,
        ("snapshot-done", RecordStage::SnapshotDone) => true,
        ("manifest-temp", RecordStage::ManifestTemp) => true,
        ("committed", RecordStage::Committed) => true,
        _ => false,
    }
}

pub fn run(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let [dir, stage] = args else {
        return Err("usage: opcli recovery-helper <record-dir> <stage>".into());
    };
    if !STAGES.contains(&stage.as_str()) {
        return Err(format!("unknown stage {stage}").into());
    }
    let armed = Arc::new(AtomicBool::new(false));
    let hook_armed = armed.clone();
    let hook_stage = stage.clone();
    let options = RecoveryOptions {
        // Only explicit flushes write, so the stages are exact.
        settle: Duration::from_secs(3600),
        max_interval: Duration::from_secs(3600),
        hooks: RecordHooks {
            on_stage: Some(Arc::new(move |at| {
                if hook_armed.load(Ordering::SeqCst) && matches(&hook_stage, at) {
                    pause(&hook_stage);
                }
            })),
            fail_after_bytes: None,
            ..RecordHooks::default()
        },
    };
    let engine = Engine::with_options(
        None,
        EngineOptions {
            force_cpu: true,
            ..Default::default()
        },
    )?;
    let (w, h) = (160_u32, 120_u32);
    let mut x = 0x9e37_79b9_u32;
    let pixels: Vec<u8> = (0..w * h * 4)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            x as u8
        })
        .collect();
    let doc = engine.import_pixels(w, h, &pixels, None)?;
    engine.recovery_attach(doc, dir, Some("file:///kill/test.opaint".into()), options)?;
    let report = |label: &str| -> Result<(), Box<dyn std::error::Error>> {
        let snap = engine.snapshot(doc)?;
        let hash = doc_hash(snap.state.layers.len(), &snap.pixels(false)?);
        println!("{label} {} {hash}", snap.revision);
        std::io::stdout().flush()?;
        Ok(())
    };
    engine.dispatch(doc, "image.invert_colors", json!({}))?;
    engine
        .recovery_flush(doc)?
        .ok_or("the first record was not written")?;
    report("ACK")?;
    engine.dispatch(doc, "layer.add", json!({}))?;
    engine.dispatch(doc, "effect.blur.gaussian", json!({"radius": 3}))?;
    report("EDIT")?;
    match stage.as_str() {
        "before" => pause(stage),
        "after" => {
            engine.recovery_flush(doc)?;
            report("ACK")?;
            pause(stage)
        }
        _ => {
            armed.store(true, Ordering::SeqCst);
            engine.recovery_flush(doc)?;
            Err(format!("the write passed stage {stage} without pausing").into())
        }
    }
}
