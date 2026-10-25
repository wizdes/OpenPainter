//! M57: a 200-megapixel document (16384 x 12208). File > New, a brush
//! stroke, Gaussian Blur, Undo, Save as .opaint and open the file again,
//! with the process's peak memory under MEMORY_CAP thanks to 256x256 tiles,
//! LZ4 and the spill file (the tile store budget is 64 MB).
//!
//! Heavy (about 1 to 3 minutes, up to 2 GB of memory), so it is ignored in the
//! normal run. Run it alone:
//! `cargo test -p op-engine --test huge_document -- --ignored --nocapture`
use op_core::doc::ToolCompositeMode;
use op_core::tiles::TileCoord;
use op_engine::{Engine, SaveFormat};
use op_tools::{BrushSettings, PressureMode, StrokeSample, Tool};
use serde_json::json;
use std::time::Instant;

const WIDTH: u32 = 16384;
const HEIGHT: u32 = 12208;
/// The stated cap for the whole process (ARCHITECTURE.md: one 2 GB budget).
const MEMORY_CAP: u64 = 2 << 30;

#[repr(C)]
#[derive(Default)]
struct RUsage {
    times: [i64; 4],
    maxrss: i64,
    rest: [i64; 13],
}
unsafe extern "C" {
    fn getrusage(who: i32, usage: *mut RUsage) -> i32;
}

/// The process's peak resident memory in bytes (macOS reports bytes, Linux KiB).
fn peak_rss() -> u64 {
    let mut u = RUsage::default();
    assert_eq!(unsafe { getrusage(0, &mut u) }, 0);
    if cfg!(target_os = "macos") {
        u.maxrss as u64
    } else {
        u.maxrss as u64 * 1024
    }
}

fn mb(bytes: u64) -> f64 {
    bytes as f64 / 1048576.0
}

#[test]
#[ignore = "200 MP: run alone with --ignored (minutes, up to 2 GB)"]
fn a_200_megapixel_document_paints_blurs_undoes_and_saves_under_the_memory_cap() {
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::new(None).unwrap();
    let mut times = Vec::new();
    let mut lap = |name: &str, start: Instant| {
        let s = start.elapsed().as_secs_f64();
        eprintln!(
            "200 MP: {name}: {s:.2} s, peak RSS so far {:.0} MB",
            mb(peak_rss())
        );
        times.push((name.to_owned(), s));
    };

    let t = Instant::now();
    let doc = engine
        .new_document(WIDTH, HEIGHT, 300.0, [255, 255, 255, 255])
        .unwrap();
    lap("File > New (white)", t);

    let t = Instant::now();
    engine
        .stroke_begin(
            doc,
            BrushSettings {
                tool: Tool::Paintbrush,
                width: 60.0,
                color: [200, 20, 30, 255],
                antialias: true,
                spacing: 0.25,
                pressure: PressureMode::None,
                blend: ToolCompositeMode::Normal,
            },
            7,
        )
        .unwrap();
    let samples: Vec<StrokeSample> = (0..=40)
        .map(|i| {
            let f = f64::from(i) / 40.0;
            StrokeSample::mouse(
                100.0 + f * (f64::from(WIDTH) - 200.0),
                200.0 + f * (f64::from(HEIGHT) - 400.0),
                u64::from(i as u32) * 8,
            )
        })
        .collect();
    engine.stroke_append(doc, &samples).unwrap();
    engine.stroke_commit(doc).unwrap();
    lap("brush stroke corner to corner", t);
    // M57 review: a recovery record of the 200 MP document.
    let record = dir.path().join("Recovery").join("huge");
    engine
        .recovery_attach(
            doc,
            &record,
            None,
            op_engine::recovery::RecoveryOptions {
                settle: std::time::Duration::from_secs(3600),
                max_interval: std::time::Duration::from_secs(3600),
                ..Default::default()
            },
        )
        .unwrap();
    let t = Instant::now();
    let rev = engine.recovery_flush(doc).unwrap();
    lap("Recovery record write", t);
    let manifest = op_engine::recovery::read_record(&record).unwrap();
    assert_eq!(Some(manifest.revision), rev);
    assert_eq!((manifest.width, manifest.height), (WIDTH, HEIGHT));
    let painted = engine.snapshot(doc).unwrap().state.clone();
    let probe = TileCoord { x: 32, y: 24 };
    let painted_tile = painted.read_tile(0, probe).unwrap();

    let t = Instant::now();
    engine
        .dispatch(doc, "effect.blur.gaussian", json!({"radius": 8}))
        .unwrap();
    lap("Gaussian Blur r=8", t);
    let blurred = engine.snapshot(doc).unwrap();
    assert_eq!(blurred.history.labels.len(), 3);
    assert_ne!(blurred.state.read_tile(0, probe).unwrap(), painted_tile);
    let gpu = engine.query(doc, "doc.info", json!({})).unwrap();
    let gpu: serde_json::Value = serde_json::from_slice(&gpu).unwrap();
    eprintln!(
        "200 MP: blur ran with gpu {} ({}), runs {}",
        gpu["gpu"], gpu["gpu_reason"], gpu["gpu_effects"]
    );
    drop(blurred);

    // M57 review round 2: a recovery record write of the blurred document
    // overlapping a Save of the same document.
    let engine = std::sync::Arc::new(engine);
    let e2 = engine.clone();
    let t = Instant::now();
    let flusher = std::thread::spawn(move || e2.recovery_flush(doc));
    std::thread::sleep(std::time::Duration::from_millis(50));
    engine
        .save_document(
            doc,
            dir.path().join("overlap.opaint"),
            SaveFormat::Opaint,
            false,
        )
        .unwrap();
    let flushed = flusher.join().unwrap();
    lap("Record write overlapping a Save", t);
    assert!(flushed.is_ok(), "{flushed:?}");

    let t = Instant::now();
    engine.dispatch(doc, "edit.undo", json!({})).unwrap();
    lap("Undo", t);
    assert_eq!(
        engine
            .snapshot(doc)
            .unwrap()
            .state
            .read_tile(0, probe)
            .unwrap(),
        painted_tile
    );

    let path = dir.path().join("huge.opaint");
    let t = Instant::now();
    engine
        .save_document(doc, &path, SaveFormat::Opaint, true)
        .unwrap();
    lap("Save as .opaint", t);
    let size = std::fs::metadata(&path).unwrap().len();
    eprintln!("200 MP: file size {:.1} MB", mb(size));
    let stats = engine.tile_store(doc).unwrap().stats();
    eprintln!(
        "200 MP: tiles {}, resident {:.0} MB, compressed {:.0} MB, spilled {:.0} MB",
        stats.tile_count,
        mb(stats.resident_bytes as u64),
        mb(stats.compressed_bytes as u64),
        mb(stats.spilled_bytes as u64)
    );
    engine.close(doc).unwrap();

    let t = Instant::now();
    let reopened = engine.open_opaint(&path).unwrap();
    lap("Open the saved .opaint", t);
    let snap = engine.snapshot(reopened).unwrap();
    assert_eq!((snap.state.width, snap.state.height), (WIDTH, HEIGHT));
    assert_eq!(snap.state.read_tile(0, probe).unwrap(), painted_tile);
    drop(snap);
    engine.close(reopened).unwrap();

    let peak = peak_rss();
    eprintln!(
        "200 MP: peak RSS {:.0} MB (cap {:.0} MB); times {times:?}",
        mb(peak),
        mb(MEMORY_CAP)
    );
    assert!(peak < MEMORY_CAP, "peak RSS {} MB", mb(peak));
}
