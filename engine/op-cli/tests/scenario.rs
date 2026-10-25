//! The committed M28 scenario runs through `opcli scenario` and meets its own
//! expectations (the Mac test, Phase2EndToEndTests, compares the window with
//! the same run).
use std::path::Path;
use std::process::Command;

#[test]
fn m28_phase2_scenario_runs_and_writes_its_files() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let script = root.join("tests/scenarios/m28-phase2.json");
    let out = std::env::temp_dir().join(format!("opcli-m28-{}", std::process::id()));
    let run = Command::new(env!("CARGO_BIN_EXE_opcli"))
        .args([
            "scenario",
            script.to_str().unwrap(),
            "-o",
            out.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    let summary: serde_json::Value = serde_json::from_slice(&run.stdout).unwrap();
    assert_eq!(summary["width"], 256);
    assert_eq!(summary["height"], 171);
    assert_eq!(summary["history_index"], 2);
    assert!(summary["decoder_check"]["max_diff"].as_u64().unwrap() <= 3);
    for name in ["result.opaint", "result.png", "result.jpg"] {
        assert!(out.join(name).metadata().unwrap().len() > 0, "{name}");
    }
    // A step that breaks the expectation fails the run.
    let mut broken: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&script).unwrap()).unwrap();
    broken["expect"]["history_index"] = serde_json::json!(3);
    broken["input"]["path"] = serde_json::json!(
        root.join("tests/fixtures/m28/phase2-exif6-p3.jpg")
            .to_str()
            .unwrap()
    );
    broken["input"]["pixels"] = serde_json::json!(
        root.join("tests/fixtures/m28/phase2-exif6-p3.imageio.png")
            .to_str()
            .unwrap()
    );
    let bad = out.join("broken.json");
    std::fs::write(&bad, broken.to_string()).unwrap();
    let run = Command::new(env!("CARGO_BIN_EXE_opcli"))
        .args([
            "scenario",
            bad.to_str().unwrap(),
            "-o",
            out.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(!run.status.success());
    assert!(
        String::from_utf8_lossy(&run.stderr).contains("expected history_index 3, got 2"),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    std::fs::remove_dir_all(&out).unwrap();
}
