//! M15 main-window commands: selection, active layer, one-call strokes, effect
//! commands for every registered effect, and the catalog queries.
use op_engine::Engine;
use serde_json::{Value, json};

fn doc(e: &Engine) -> u64 {
    let mut px = vec![0u8; 32 * 32 * 4];
    for (i, p) in px.chunks_exact_mut(4).enumerate() {
        p.copy_from_slice(&[(i % 251) as u8, 40, 200, 255]);
    }
    e.import_pixels(32, 32, &px, None).unwrap()
}
fn query(e: &Engine, id: u64, q: &str) -> Value {
    serde_json::from_slice(&e.query(id, q, json!({})).unwrap()).unwrap()
}

#[test]
fn select_all_and_deselect() {
    let e = Engine::new(None).unwrap();
    let id = doc(&e);
    assert_eq!(query(&e, id, "doc.selection")["active"], json!(false));
    // Deselect with no selection is disabled.
    assert!(e.dispatch(id, "selection.deselect", json!({})).is_err());
    e.dispatch(id, "selection.select_all", json!({})).unwrap();
    let sel = query(&e, id, "doc.selection");
    assert_eq!(sel["active"], json!(true));
    assert_eq!(sel["bounds"], json!([0, 0, 32, 32]));
    e.dispatch(id, "selection.deselect", json!({})).unwrap();
    assert_eq!(query(&e, id, "doc.selection")["active"], json!(false));
    let labels = e.snapshot(id).unwrap().history.labels.clone();
    assert_eq!(labels, vec!["Open", "Select All", "Deselect"]);
    e.dispatch(id, "edit.undo", json!({})).unwrap();
    assert_eq!(query(&e, id, "doc.selection")["active"], json!(true));
}

#[test]
fn set_active_layer() {
    let e = Engine::new(None).unwrap();
    let id = doc(&e);
    e.dispatch(id, "layer.add", json!({"name":"Two"})).unwrap();
    // layer.add keeps the active layer where it was.
    assert_eq!(query(&e, id, "doc.info")["active_layer"], json!(0));
    e.dispatch(id, "layer.set_active", json!({"index":1}))
        .unwrap();
    assert_eq!(query(&e, id, "doc.info")["active_layer"], json!(1));
    e.dispatch(id, "layer.set_active", json!({"index":0}))
        .unwrap();
    assert_eq!(query(&e, id, "doc.info")["active_layer"], json!(0));
    assert!(
        e.dispatch(id, "layer.set_active", json!({"index":2}))
            .is_err()
    );
    assert_eq!(query(&e, id, "doc.info")["active_layer"], json!(0));
}

#[test]
fn stroke_paints_and_undoes() {
    let e = Engine::new(None).unwrap();
    let id = doc(&e);
    let before = e.snapshot(id).unwrap().pixels(false).unwrap();
    e.dispatch(
        id,
        "tool.stroke",
        json!({"tool":"pencil","width":1.0,"color":"ff0000ff","antialias":false,
               "points":"2.5 2.5 1 10.5 2.5 1"}),
    )
    .unwrap();
    let after = e.snapshot(id).unwrap().pixels(false).unwrap();
    let px = |x: usize, y: usize| &after[(y * 32 + x) * 4..(y * 32 + x) * 4 + 4];
    assert_eq!(px(5, 2), &[255, 0, 0, 255]);
    assert_eq!(px(5, 20), &before[(20 * 32 + 5) * 4..(20 * 32 + 5) * 4 + 4]);
    assert_eq!(
        e.snapshot(id).unwrap().history.labels.last().unwrap(),
        "Pencil"
    );
    e.dispatch(id, "edit.undo", json!({})).unwrap();
    assert_eq!(e.snapshot(id).unwrap().pixels(false).unwrap(), before);
    // Malformed points fail and change nothing.
    assert!(
        e.dispatch(id, "tool.stroke", json!({"points":"1 2"}))
            .is_err()
    );
    assert!(
        e.dispatch(id, "tool.stroke", json!({"points":"1 2 x"}))
            .is_err()
    );
}

#[test]
fn eraser_and_opacity() {
    let e = Engine::new(None).unwrap();
    let id = doc(&e);
    e.dispatch(
        id,
        "tool.stroke",
        json!({"tool":"eraser","width":4.0,"antialias":false,"points":"16 16 1"}),
    )
    .unwrap();
    let px = e.snapshot(id).unwrap().pixels(false).unwrap();
    assert_eq!(px[(16 * 32 + 16) * 4 + 3], 0);
    assert_eq!(
        e.snapshot(id).unwrap().history.labels.last().unwrap(),
        "Eraser"
    );
}

#[test]
fn every_effect_has_a_command_and_runs_with_defaults() {
    let e = Engine::new(None).unwrap();
    let list = query(&e, 0, "effects.list");
    let effects = list.as_array().unwrap();
    assert_eq!(effects.len(), op_ops::effect::registered().len());
    for (entry, spec) in effects.iter().zip(op_ops::effect::registered()) {
        assert_eq!(entry["id"], json!(spec.id));
        assert_eq!(entry["category"], json!(spec.category));
        assert!(!entry["label"].as_str().unwrap().is_empty());
        assert!(entry["schema"].is_array());
    }
    let commands = query(&e, doc(&e), "commands.list");
    let ids: Vec<&str> = commands
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["id"].as_str().unwrap())
        .collect();
    for spec in op_ops::effect::registered() {
        assert!(ids.contains(&spec.id), "{} has no command", spec.id);
    }
    for spec in op_ops::effect::registered() {
        if spec.id == "effect.fill.content_aware" {
            continue; // needs a selection; covered by op-ops tests
        }
        let id = doc(&e);
        e.dispatch(id, spec.id, json!({})).unwrap();
        let snap = e.snapshot(id).unwrap();
        assert_eq!(snap.history.labels.len(), 2, "{}", spec.id);
        assert_eq!(
            snap.history.labels[1],
            op_engine::effect_label(&spec),
            "{}",
            spec.id
        );
        e.close(id).unwrap();
    }
}

#[test]
fn commands_list_reports_enabled_state() {
    let e = Engine::new(None).unwrap();
    let id = doc(&e);
    let enabled = |e: &Engine, cmd: &str| {
        query(e, id, "commands.list")
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["id"] == json!(cmd))
            .unwrap()["enabled"]
            .as_bool()
            .unwrap()
    };
    assert!(!enabled(&e, "selection.deselect"));
    assert!(!enabled(&e, "layer.delete"));
    e.dispatch(id, "selection.select_all", json!({})).unwrap();
    assert!(enabled(&e, "selection.deselect"));
    assert!(
        !query(&e, id, "commands.list")
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["id"].as_str().unwrap().starts_with("test."))
    );
}

#[test]
fn effects_list_names_curve_and_level_parameter_kinds() {
    let e = Engine::new(None).unwrap();
    let list = query(&e, 0, "effects.list");
    let kinds = |id: &str| -> Vec<Value> {
        list.as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["id"] == json!(id))
            .unwrap_or_else(|| panic!("{id} missing"))["schema"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| f["kind"].clone())
            .collect()
    };
    assert!(kinds("effect.adjust.curves").contains(&json!("curve")));
    let levels = kinds("effect.adjust.levels");
    assert!(levels.contains(&json!("rgb_bytes")));
    assert!(levels.contains(&json!("rgb_gamma")));
}
