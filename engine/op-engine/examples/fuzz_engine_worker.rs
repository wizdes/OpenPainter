//! The M57 fuzz worker for engine readers, driven by `cargo xtask fuzz`
//! (same protocol as op-formats' `fuzz_worker`: `seeds <target>` writes
//! length-prefixed seeds; `run <target>` answers each input with a status byte:
//! 0 read, 1 clean error, 2 panic escaped, 3 panic in our code).
//!
//! Targets:
//! - `manifest`: the recovery manifest reader (`recovery::read_record`, and
//!   the check against the original file) on a mutated `manifest.json` next
//!   to a valid snapshot.
//! - `command`: the command JSON path of `op_dispatch` (parse the JSON, then
//!   `Engine::dispatch`) on a small document. An input is `<command id>\n<json>`.
#[path = "../../op-formats/examples/common/capped_alloc.rs"]
mod capped_alloc;

use op_engine::recovery::{self, RecoveryOptions};
use op_engine::{Engine, EngineOptions};
use serde_json::json;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::{Mutex, PoisonError};

static PANICS: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn engine() -> Engine {
    Engine::with_options(
        None,
        EngineOptions {
            force_cpu: true,
            ..Default::default()
        },
    )
    .expect("engine")
}

fn small_doc(e: &Engine) -> u64 {
    let px: Vec<u8> = (0..48 * 32 * 4).map(|i| (i * 7 % 251) as u8).collect();
    e.import_pixels(48, 32, &px, None).expect("doc")
}

fn work_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("op-fuzz-engine-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("work dir");
    dir
}

/// A real record folder (manifest plus snapshot) to read seeds and run in.
/// It stays between `seeds` and `run`, so the seed manifest's checksum
/// matches the snapshot the runs read.
fn record(e: &Engine) -> PathBuf {
    let dir = std::env::temp_dir().join("op-fuzz-engine-record");
    if recovery::read_record(&dir).is_ok() {
        return dir;
    }
    let _ = std::fs::remove_dir_all(&dir);
    let doc = small_doc(e);
    let options = RecoveryOptions {
        settle: std::time::Duration::from_secs(3600),
        max_interval: std::time::Duration::from_secs(3600),
        ..Default::default()
    };
    e.recovery_attach(
        doc,
        &dir,
        Some("file:///tmp/op-fuzz/x.opaint".into()),
        options,
    )
    .expect("attach");
    e.dispatch(doc, "image.invert_colors", json!({}))
        .expect("edit");
    e.recovery_flush(doc).expect("flush");
    e.recovery_detach(doc, false);
    dir
}

fn command_seeds(e: &Engine) -> Vec<Vec<u8>> {
    let doc = small_doc(e);
    let list: Vec<serde_json::Value> =
        serde_json::from_slice(&e.query(doc, "commands.list", json!({})).unwrap()).unwrap();
    let mut out: Vec<Vec<u8>> = list
        .iter()
        .filter_map(|c| c["id"].as_str())
        .map(|id| format!("{id}\n{{}}").into_bytes())
        .collect();
    for (id, params) in [
        (
            "selection.rectangle",
            json!({"x":2.5,"y":3,"width":20,"height":10,"combine":"add"}),
        ),
        (
            "selection.ellipse",
            json!({"x":1,"y":1,"width":30,"height":20}),
        ),
        ("effect.blur.gaussian", json!({"radius":3,"gamma":"srgb"})),
        ("layer.add", json!({"name":"Layer 2"})),
        (
            "layer.properties",
            json!({"name":"x","opacity":128,"blend":"Multiply","visible":false}),
        ),
        (
            "image.resize",
            json!({"width":24,"height":16,"resampling":"bicubic"}),
        ),
        (
            "image.canvas_size",
            json!({"width":64,"height":40,"anchor":"center"}),
        ),
        ("history.jump", json!({"index":0})),
        (
            "tool.paint_bucket",
            json!({"x":4,"y":4,"tolerance":50,"color":"ff0000ff"}),
        ),
    ] {
        out.push(format!("{id}\n{params}").into_bytes());
    }
    out
}

fn seeds(target: &str) -> Vec<Vec<u8>> {
    let e = engine();
    match target {
        "manifest" => {
            let dir = record(&e);
            vec![std::fs::read(dir.join(recovery::MANIFEST)).expect("manifest")]
        }
        "command" => command_seeds(&e),
        _ => panic!("unknown target {target}"),
    }
}

fn read_frame(input: &mut impl Read) -> Option<Vec<u8>> {
    let mut len = [0; 4];
    input.read_exact(&mut len).ok()?;
    let mut data = vec![0; u32::from_le_bytes(len) as usize];
    input.read_exact(&mut data).ok()?;
    Some(data)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (mode, target) = match args.as_slice() {
        [m, t] => (m.clone(), t.clone()),
        _ => {
            eprintln!("usage: fuzz_engine_worker seeds|run <target>");
            std::process::exit(2);
        }
    };
    let mut out = std::io::stdout().lock();
    if mode == "seeds" {
        for s in seeds(&target) {
            out.write_all(&(s.len() as u32).to_le_bytes()).unwrap();
            out.write_all(&s).unwrap();
        }
        let _ = std::fs::remove_dir_all(work_dir());
        return;
    }
    eprintln!(
        "fuzz_engine_worker {target}: heap cap {} MiB, {}",
        capped_alloc::MEMORY_LIMIT >> 20,
        capped_alloc::set_rlimit()
    );
    std::panic::set_hook(Box::new(|info| {
        let at = info
            .location()
            .map_or("unknown".into(), |l| format!("{}:{}", l.file(), l.line()));
        PANICS
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(format!("{at}: {info}"));
    }));
    let e = engine();
    // Each run works on its own copy, so parallel workers do not collide.
    let rec = if target == "manifest" {
        let shared = record(&e);
        let mine = work_dir().join("record");
        std::fs::create_dir_all(&mine).expect("record copy");
        for f in std::fs::read_dir(&shared).expect("record").flatten() {
            let name = f.file_name().to_string_lossy().into_owned();
            if name.starts_with("snapshot-") || name == recovery::MANIFEST {
                std::fs::copy(f.path(), mine.join(&name)).expect("copy");
            }
        }
        Some(mine)
    } else {
        None
    };
    let mut doc = small_doc(&e);
    let mut count = 0_u64;
    let mut input = std::io::stdin().lock();
    while let Some(data) = read_frame(&mut input) {
        PANICS
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clear();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match &rec {
            Some(dir) => {
                std::fs::write(dir.join(recovery::MANIFEST), &data).expect("write manifest");
                let _ = recovery::parse_manifest(&data);
                match recovery::read_record(dir) {
                    Ok(m) => {
                        let _ = recovery::older_than_file(&m);
                        true
                    }
                    Err(_) => false,
                }
            }
            None => {
                let text = data.splitn(2, |b| *b == b'\n').collect::<Vec<_>>();
                let id = String::from_utf8_lossy(text[0]).into_owned();
                let json = text.get(1).copied().unwrap_or(b"{}");
                match serde_json::from_slice::<serde_json::Value>(json) {
                    // op_dispatch passes an empty body as an empty object.
                    Ok(params) if !id.starts_with("test.") => e.dispatch(doc, &id, params).is_ok(),
                    _ => false,
                }
            }
        }));
        let ours = PANICS
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .any(|s| s.contains("op-") || s.contains("op_"));
        let status = match result {
            Err(_) => 2,
            Ok(_) if ours => 3,
            Ok(true) => 0,
            Ok(false) => 1,
        };
        if status >= 2 {
            for site in PANICS.lock().unwrap_or_else(PoisonError::into_inner).iter() {
                eprintln!("fuzz_engine_worker {target}: panic at {site}");
            }
        }
        count += 1;
        // A fresh document now and then, so history does not grow forever.
        if rec.is_none() && (count.is_multiple_of(200) || e.is_read_only(doc).unwrap_or(true)) {
            let _ = e.close(doc);
            doc = small_doc(&e);
        }
        if out.write_all(&[status]).and_then(|()| out.flush()).is_err() {
            break;
        }
    }
    let _ = std::fs::remove_dir_all(work_dir());
}
