//! M31: the live Magic Wand and Paint Bucket session. A click begins a preview;
//! seed and option updates re-run the flood on the same frozen document; Finish
//! commits one step named as paint.net names the tool; Cancel restores. While
//! the session is open, the selection queries can read the live selection, any
//! other command finishes the session first, and Undo drops it.
use op_engine::Engine;
use serde_json::{Value, json};

/// 8 x 4: left half red with a darker red band, right half blue, a
/// half-transparent green pixel and a fully transparent pixel.
fn image() -> Vec<u8> {
    let mut px = Vec::new();
    for y in 0..4 {
        for x in 0..8 {
            let p: [u8; 4] = match (x, y) {
                (6, 1) => [0, 255, 0, 128],
                (7, 3) => [9, 9, 9, 0],
                (0..=3, 2) => [200, 0, 0, 255],
                (0..=3, _) => [255, 0, 0, 255],
                _ => [0, 0, 255, 255],
            };
            px.extend_from_slice(&p);
        }
    }
    px
}

fn open(engine: &Engine) -> u64 {
    engine.import_pixels(8, 4, &image(), None).unwrap()
}

fn mask(engine: &Engine, id: u64, preview: bool) -> Vec<u8> {
    engine
        .query(id, "doc.selection_mask", json!({ "preview": preview }))
        .unwrap()
}

fn pixels(engine: &Engine, id: u64) -> Vec<u8> {
    engine
        .query(id, "doc.pixels", json!({ "preview": false }))
        .unwrap()
}

fn labels(engine: &Engine, id: u64) -> Vec<String> {
    engine.snapshot(id).unwrap().history.labels.clone()
}

fn wand(x: i64, y: i64, tolerance: f64) -> Value {
    json!({"x":x,"y":y,"tolerance":tolerance,"contiguous":true,"alpha":"straight",
           "sampling":"layer","antialias":false,"combine":"replace"})
}

fn bucket(x: i64, y: i64, tolerance: f64) -> Value {
    json!({"x":x,"y":y,"tolerance":tolerance,"contiguous":true,"alpha":"premultiplied",
           "sampling":"image","antialias":true,"color":"10203080","blend":"normal"})
}

/// Waits for the actor so the preview render of the last update has landed.
fn settle(engine: &Engine, id: u64) {
    for _ in 0..400 {
        if engine.snapshot(id).unwrap().preview.is_some() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    panic!("no preview arrived");
}

#[test]
fn finish_names_the_step_magic_wand_or_paint_bucket() {
    let engine = Engine::new(None).unwrap();
    let id = open(&engine);
    engine
        .preview_begin(id, "tool.magic_wand", wand(0, 0, 0.0))
        .unwrap();
    engine.preview_commit(id).unwrap();
    engine
        .preview_begin(id, "tool.paint_bucket", bucket(5, 0, 0.0))
        .unwrap();
    engine.preview_commit(id).unwrap();
    // One-shot commands use the same names.
    engine
        .dispatch(id, "tool.magic_wand", wand(0, 0, 0.0))
        .unwrap();
    engine
        .dispatch(id, "tool.paint_bucket", bucket(0, 0, 0.0))
        .unwrap();
    assert_eq!(
        labels(&engine, id)[1..],
        ["Magic Wand", "Paint Bucket", "Magic Wand", "Paint Bucket"]
    );
}

#[test]
fn the_selection_queries_read_the_live_wand_selection() {
    let engine = Engine::new(None).unwrap();
    let id = open(&engine);
    engine
        .preview_begin(id, "tool.magic_wand", wand(0, 0, 0.0))
        .unwrap();
    settle(&engine, id);
    let live = mask(&engine, id, true);
    let expected: Vec<u8> = (0..32)
        .map(|i| {
            let (x, y) = (i % 8, i / 8);
            if x < 4 && y != 2 && y < 2 { 255 } else { 0 }
        })
        .collect();
    assert_eq!(
        live, expected,
        "contiguous: the band at row 2 stops the fill"
    );
    // The committed state has no selection yet.
    let path: Value = serde_json::from_slice(
        &engine
            .query(id, "doc.selection_path", json!({"preview": false}))
            .unwrap(),
    )
    .unwrap();
    assert_eq!(path["active"], json!(false));
    let path: Value = serde_json::from_slice(
        &engine
            .query(id, "doc.selection_path", json!({"preview": true}))
            .unwrap(),
    )
    .unwrap();
    assert_eq!(path["active"], json!(true));
    let sel: Value = serde_json::from_slice(
        &engine
            .query(id, "doc.selection", json!({"preview": true}))
            .unwrap(),
    )
    .unwrap();
    assert_eq!(sel["active"], json!(true));
    assert_eq!(sel["bounds"], json!([0, 0, 4, 2]));
    // A higher tolerance takes the band too, and global mode the far side.
    engine
        .preview_update(id, json!({"tolerance": 0.5}))
        .unwrap();
    settle(&engine, id);
    let wide = mask(&engine, id, true);
    assert_eq!(wide.iter().filter(|&&b| b == 255).count(), 16);
    engine.preview_cancel(id).unwrap();
    assert_eq!(mask(&engine, id, true), mask(&engine, id, false));
    assert_eq!(labels(&engine, id).len(), 1);
}

#[test]
fn every_update_equals_one_plain_command_with_the_final_options() {
    // Each option is changed on its own; the committed bytes and selection
    // equal one plain command with the same final options on a fresh image.
    let changes = [
        json!({"tolerance": 0.4}),
        json!({"x": 6, "y": 0}),
        json!({"contiguous": false}),
        json!({"alpha": "straight"}),
        json!({"sampling": "layer"}),
        json!({"antialias": false}),
        json!({"blend": "overwrite"}),
        json!({"color": "ff00ff40"}),
    ];
    for change in &changes {
        let engine = Engine::new(None).unwrap();
        let id = open(&engine);
        let before = pixels(&engine, id);
        engine
            .preview_begin(id, "tool.paint_bucket", bucket(1, 1, 0.1))
            .unwrap();
        engine.preview_update(id, change.clone()).unwrap();
        settle(&engine, id);
        let shown = engine.snapshot(id).unwrap().pixels(true).unwrap();
        assert_eq!(pixels(&engine, id), before, "{change}: preview only");
        engine.preview_commit(id).unwrap();
        let mut last = bucket(1, 1, 0.1);
        for (k, v) in change.as_object().unwrap() {
            last[k] = v.clone();
        }
        let reference = Engine::new(None).unwrap();
        let rid = open(&reference);
        reference.dispatch(rid, "tool.paint_bucket", last).unwrap();
        assert_eq!(pixels(&engine, id), pixels(&reference, rid), "{change}");
        assert_eq!(
            shown,
            pixels(&reference, rid),
            "{change}: preview equals result"
        );
        let l = labels(&engine, id);
        assert_eq!(l.len(), 2, "{change}: one step");
        assert_eq!(l[1], "Paint Bucket");
    }
    // Wand: the selection mode is live too.
    for combine in ["replace", "add", "subtract", "intersect", "invert"] {
        let engine = Engine::new(None).unwrap();
        let id = open(&engine);
        engine
            .dispatch(
                id,
                "selection.rectangle",
                json!({"x":2.0,"y":0.0,"width":4.0,"height":4.0}),
            )
            .unwrap();
        engine
            .preview_begin(id, "tool.magic_wand", wand(0, 0, 0.0))
            .unwrap();
        engine
            .preview_update(id, json!({"combine": combine, "tolerance": 0.3}))
            .unwrap();
        settle(&engine, id);
        let live = mask(&engine, id, true);
        engine.preview_commit(id).unwrap();
        let reference = Engine::new(None).unwrap();
        let rid = open(&reference);
        reference
            .dispatch(
                rid,
                "selection.rectangle",
                json!({"x":2.0,"y":0.0,"width":4.0,"height":4.0}),
            )
            .unwrap();
        let mut p = wand(0, 0, 0.3);
        p["combine"] = json!(combine);
        reference.dispatch(rid, "tool.magic_wand", p).unwrap();
        assert_eq!(
            mask(&engine, id, false),
            mask(&reference, rid, false),
            "{combine}"
        );
        assert_eq!(live, mask(&reference, rid, false), "{combine}: live");
        assert_eq!(labels(&engine, id).len(), 3);
    }
}

#[test]
fn another_command_finishes_the_session_and_undo_drops_it() {
    let engine = Engine::new(None).unwrap();
    let id = open(&engine);
    engine
        .preview_begin(id, "tool.paint_bucket", bucket(0, 0, 0.0))
        .unwrap();
    engine
        .dispatch(id, "layer.add", json!({"name": "Layer 2"}))
        .unwrap();
    assert_eq!(labels(&engine, id)[1], "Paint Bucket");
    assert_eq!(labels(&engine, id).len(), 3);
    // Undo while a session is open drops the session only.
    let before = pixels(&engine, id);
    engine
        .preview_begin(id, "tool.magic_wand", wand(0, 0, 0.0))
        .unwrap();
    engine.dispatch(id, "edit.undo", json!({})).unwrap();
    assert_eq!(labels(&engine, id).len(), 3);
    assert_eq!(engine.snapshot(id).unwrap().history.index, 2);
    assert!(engine.snapshot(id).unwrap().preview.is_none());
    assert_eq!(pixels(&engine, id), before);
    assert!(engine.preview_commit(id).is_err(), "nothing left to commit");
}
