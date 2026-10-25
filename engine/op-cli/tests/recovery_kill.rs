//! M57 kill tests: `opcli recovery-helper` writes one recovery record, edits
//! again, and is killed with SIGKILL before, during and after the second
//! record write. The record left behind must always open, hold the latest
//! acknowledged revision (or the newer one once its manifest is committed),
//! and a torn snapshot must never win.
use op_engine::Engine;
use op_engine::recovery::{self, RecoveryOptions};
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

fn doc_hash(layers: usize, pixels: &[u8]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325_u64;
    for b in (layers as u64).to_le_bytes().iter().chain(pixels) {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

struct Run {
    acks: Vec<(u64, u64)>,
    edit: (u64, u64),
    leftovers: Vec<String>,
}

/// Starts the helper, waits for `PAUSED`, kills it with SIGKILL and returns
/// what it printed and the temp files it left in `dir`.
fn kill_at(dir: &std::path::Path, stage: &str) -> Run {
    let mut child = Command::new(env!("CARGO_BIN_EXE_opcli"))
        .args(["recovery-helper", dir.to_str().unwrap(), stage])
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let stdout = child.stdout.take().unwrap();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let Ok(line) = line else { break };
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    let mut acks = Vec::new();
    let mut edit = None;
    loop {
        let line = match rx.recv_timeout(Duration::from_secs(60)) {
            Ok(line) => line,
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                panic!("helper at {stage} printed no PAUSED line: {e}");
            }
        };
        let parts: Vec<&str> = line.split_whitespace().collect();
        let pair = || (parts[1].parse().unwrap(), parts[2].parse().unwrap());
        match parts.first().copied() {
            Some("ACK") => acks.push(pair()),
            Some("EDIT") => edit = Some(pair()),
            Some("PAUSED") => {
                assert_eq!(parts[1], stage);
                break;
            }
            _ => {}
        }
    }
    // Child::kill sends SIGKILL on Unix: no destructor, no cleanup runs.
    child.kill().unwrap();
    let status = child.wait().unwrap();
    assert!(!status.success());
    let leftovers = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".tmp"))
        .collect();
    Run {
        acks,
        edit: edit.expect("EDIT line"),
        leftovers,
    }
}

#[test]
fn the_latest_acknowledged_revision_recovers_whenever_the_process_is_killed() {
    for round in 0..2 {
        for stage in [
            "before",
            "snapshot-mid",
            "snapshot-done",
            "manifest-temp",
            "committed",
            "after",
        ] {
            let root = tempfile::tempdir().unwrap();
            let dir = root.path().join(format!("kill-{stage}"));
            let run = kill_at(&dir, stage);
            let latest_ack = *run.acks.last().unwrap();
            if stage == "snapshot-mid" {
                assert!(
                    run.leftovers.iter().any(|n| n.starts_with(".snapshot-")),
                    "a torn snapshot temp file should be on disk: {:?}",
                    run.leftovers
                );
            }
            // Once the new manifest is renamed into place, the newer complete
            // record is the one to recover even though it was never acknowledged.
            let expected = match stage {
                "committed" | "after" => run.edit,
                _ => latest_ack,
            };
            let found = recovery::scan(root.path());
            assert_eq!(found.len(), 1, "round {round} {stage}: {found:?}");
            let manifest = found[0]
                .manifest
                .clone()
                .unwrap_or_else(|| panic!("{stage}: {:?}", found[0].problem));
            assert_eq!(manifest.revision, expected.0, "round {round} {stage}");
            assert!(manifest.revision >= latest_ack.0);
            let engine = Engine::new(None).unwrap();
            let (doc, _) = engine
                .recovery_open(&dir, RecoveryOptions::default())
                .unwrap();
            let snap = engine.snapshot(doc).unwrap();
            assert!(snap.history.dirty);
            assert_eq!(
                doc_hash(snap.state.layers.len(), &snap.pixels(false).unwrap()),
                expected.1,
                "round {round} {stage}: recovered pixels differ"
            );
            engine.close(doc).unwrap();
            // Close returns at once (M57 review item 6); the folder goes right after.
            let start = std::time::Instant::now();
            while dir.exists() {
                assert!(
                    start.elapsed() < Duration::from_secs(10),
                    "{stage}: folder stays"
                );
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    }
}

/// M57 review round 2, item 1: Don't Save while a slow record write is in
/// flight, then Quit at once. The next launch must not offer the record.
#[test]
fn dont_save_then_a_fast_quit_leaves_nothing_to_recover() {
    let root = tempfile::tempdir().unwrap();
    let status = Command::new(env!("CARGO_BIN_EXE_opcli"))
        .args(["recovery-quit", root.path().to_str().unwrap()])
        .status()
        .unwrap();
    assert!(status.success());
    let found = recovery::scan(root.path());
    assert!(found.is_empty(), "the next launch offers {found:?}");
}
