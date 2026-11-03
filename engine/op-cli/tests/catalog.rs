//! `opcli dump-catalog` against the three files that hold it to the Swift.
//!
//! No Swift builds on Windows, so these tests use the Mac's own checked-in
//! records. `MenuTests` on the Mac keeps `menu-skeleton.txt` equal to the live
//! menu bar, and `KeyboardAccessibilityTests` keeps `docs/shortcuts.md` equal to
//! the key table. Any disagreement fails here and names the item.
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;

const CHECKED_IN: &str = "windows/assets/catalog/mac-catalog.json";
const SKELETON: &str = "mac/Tests/OpenPainterKitTests/menu-skeleton.txt";
const SHORTCUTS: &str = "docs/shortcuts.md";

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn dump_text() -> String {
    let run = Command::new(env!("CARGO_BIN_EXE_opcli"))
        .arg("dump-catalog")
        .output()
        .expect("opcli starts");
    assert!(
        run.status.success(),
        "dump-catalog failed: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8(run.stdout).expect("the catalog is UTF-8")
}

fn dump() -> Value {
    serde_json::from_str(&dump_text()).expect("the catalog is JSON")
}

fn read(path: &str) -> String {
    std::fs::read_to_string(root().join(path))
        .unwrap_or_else(|e| panic!("{path}: {e}"))
        .replace("\r\n", "\n")
}

/// `MenuSkeleton.describe`'s notation, from the JSON.
fn describe(items: &[Value], lines: &mut Vec<String>) {
    for item in items {
        let path = item["path"].as_str().expect("every row has a path");
        match item["kind"].as_str() {
            Some("menu") => {
                lines.push(path.to_owned());
                describe(item["items"].as_array().expect("a menu has items"), lines);
            }
            Some("separator") => lines.push(path.to_owned()),
            Some("item") => {
                let mut line = path.to_owned();
                if let Some(key) = item["shortcut"].as_str() {
                    line += &format!(" [{key}]");
                }
                if item["off"] == Value::Bool(true) {
                    line += " (off)";
                }
                lines.push(line);
            }
            other => panic!("{path}: unknown kind {other:?}"),
        }
    }
}

#[test]
fn the_catalog_menus_equal_the_mac_skeleton_list_line_by_line() {
    let mut got = Vec::new();
    describe(dump()["menus"].as_array().expect("menus"), &mut got);
    let text = read(SKELETON);
    let want: Vec<&str> = text
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect();
    assert!(
        want.len() > 200,
        "{SKELETON} looks cut short: {}",
        want.len()
    );
    let missing: Vec<&&str> = want
        .iter()
        .filter(|line| !got.iter().any(|g| g == *line))
        .collect();
    let extra: Vec<&String> = got
        .iter()
        .filter(|line| !want.contains(&line.as_str()))
        .collect();
    assert!(
        missing.is_empty() && extra.is_empty(),
        "dump-catalog and {SKELETON} disagree\nonly in the skeleton list: {missing:#?}\nonly in the catalog: {extra:#?}"
    );
    for (index, (got, want)) in got.iter().zip(&want).enumerate() {
        assert_eq!(
            got,
            want,
            "{SKELETON}: line {} of the items is out of order",
            index + 1
        );
    }
    assert_eq!(got.len(), want.len());
}

#[test]
fn the_checked_in_catalog_equals_what_dump_catalog_writes() {
    let fresh = dump_text();
    let kept = read(CHECKED_IN);
    if fresh == kept {
        return;
    }
    let first = fresh
        .lines()
        .zip(kept.lines())
        .position(|(a, b)| a != b)
        .unwrap_or_else(|| fresh.lines().count().min(kept.lines().count()));
    panic!(
        "{CHECKED_IN} is out of date at line {}.\nwrites: {:?}\nkept:   {:?}\nRun: cargo run -p op-cli -- dump-catalog -o ../{CHECKED_IN}",
        first + 1,
        fresh.lines().nth(first),
        kept.lines().nth(first),
    );
}

#[test]
fn every_key_row_is_in_the_mac_shortcut_table() {
    let doc = read(SHORTCUTS);
    let catalog = dump();
    let rows = catalog["shortcuts"].as_array().expect("shortcuts");
    assert!(rows.len() > 30, "only {} key rows", rows.len());
    let cell = |row: &Value, name: &str| {
        let text = row[name].as_str().expect("a key row field");
        if text.is_empty() {
            "–".to_owned()
        } else {
            text.replace('|', "\\|")
        }
    };
    let missing: Vec<String> = rows
        .iter()
        .map(|row| {
            format!(
                "| {} | {} | {} | {} | {} |",
                cell(row, "action"),
                cell(row, "win"),
                cell(row, "mac"),
                cell(row, "scope"),
                cell(row, "note"),
            )
        })
        .filter(|line| !doc.contains(line.as_str()))
        .collect();
    assert!(missing.is_empty(), "not in {SHORTCUTS}: {missing:#?}");
}

#[test]
fn every_tool_has_its_words_its_key_and_a_recorded_icon() {
    let catalog = dump();
    let tools = catalog["tools"].as_array().expect("tools");
    assert_eq!(tools.len(), 19, "the Mac has 19 tools");
    assert_eq!(tools[0]["label"], "Rectangle Select");
    assert_eq!(tools[0]["key"], "s");
    assert_eq!(tools[1]["label"], "Move Selected Pixels");
    for tool in tools {
        let id = tool["id"].as_str().expect("a tool id");
        assert!(tool["label"].is_string(), "{id}: no label");
        assert_eq!(tool["key"].as_str().map(str::len), Some(1), "{id}: key");
        assert!(tool["icon"].is_string(), "{id}: no row in icons.toml");
    }
}

#[test]
fn the_panels_come_with_their_order_keys_and_icons() {
    let catalog = dump();
    let panels = catalog["panels"].as_array().expect("panels");
    let titles: Vec<&str> = panels
        .iter()
        .map(|p| p["title"].as_str().unwrap())
        .collect();
    assert_eq!(titles, ["Tools", "History", "Layers", "Colors"]);
    assert_eq!(panels[3]["shortcut"], "F8");
    assert!(panels.iter().all(|panel| panel["icon"].is_string()));
}

#[test]
fn the_output_file_option_writes_the_same_bytes() {
    let dir = tempfile::tempdir().expect("a temporary folder");
    let out = dir.path().join("catalog.json");
    let run = Command::new(env!("CARGO_BIN_EXE_opcli"))
        .arg("dump-catalog")
        .arg("-o")
        .arg(&out)
        .output()
        .expect("opcli starts");
    assert!(run.status.success());
    assert!(run.stdout.is_empty(), "with -o nothing goes to the screen");
    assert_eq!(
        std::fs::read_to_string(&out).expect("the file"),
        dump_text()
    );
}

#[test]
fn wrong_arguments_fail_and_say_how_to_use_it() {
    let run = Command::new(env!("CARGO_BIN_EXE_opcli"))
        .args(["dump-catalog", "--wrong"])
        .output()
        .expect("opcli starts");
    assert!(!run.status.success());
    let text = String::from_utf8_lossy(&run.stderr);
    assert!(text.contains("usage: opcli dump-catalog"), "{text}");
}
