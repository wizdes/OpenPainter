//! M20 Layers and History panel commands: choosing the active layer adds no
//! history step, duplicate, merge down, reorder, visibility, Layer Properties
//! as a preview session, per-layer content keys and thumbnails.
use op_engine::Engine;
use serde_json::{Value, json};
use std::time::{Duration, Instant};

fn doc(e: &Engine) -> u64 {
    let mut px = vec![0u8; 40 * 24 * 4];
    for (i, p) in px.chunks_exact_mut(4).enumerate() {
        p.copy_from_slice(&[(i % 251) as u8, 40, 200, 255]);
    }
    e.import_pixels(40, 24, &px, None).unwrap()
}
fn info(e: &Engine, id: u64) -> Value {
    serde_json::from_slice(&e.query(id, "doc.info", json!({})).unwrap()).unwrap()
}
fn labels(e: &Engine, id: u64) -> Vec<String> {
    e.snapshot(id).unwrap().history.labels.clone()
}
fn layer(e: &Engine, id: u64, index: usize) -> Value {
    info(e, id)["layers"][index].clone()
}
fn names(e: &Engine, id: u64) -> Vec<String> {
    info(e, id)["layers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| l["name"].as_str().unwrap().to_owned())
        .collect()
}
fn pixels(e: &Engine, id: u64) -> Vec<u8> {
    e.snapshot(id).unwrap().pixels(false).unwrap()
}
/// Paints a half-transparent red line on the active layer.
fn paint(e: &Engine, id: u64) {
    e.dispatch(
        id,
        "tool.stroke",
        json!({"color":"ff000080","width":6.0,"points":"5 5 1 15 5 1"}),
    )
    .unwrap();
}
fn wait(mut f: impl FnMut() -> bool) -> bool {
    let end = Instant::now() + Duration::from_secs(10);
    while Instant::now() < end {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    f()
}

#[test]
fn selecting_a_layer_adds_no_history_step_and_keeps_the_document_clean() {
    let e = Engine::new(None).unwrap();
    let id = doc(&e);
    e.dispatch(id, "layer.add", json!({"name":"Two"})).unwrap();
    let before = labels(&e, id);
    e.dispatch(id, "layer.set_active", json!({"index":1}))
        .unwrap();
    assert_eq!(info(&e, id)["active_layer"], json!(1));
    assert_eq!(labels(&e, id), before, "no Select Layer step");
    assert_eq!(info(&e, id)["history_index"], json!(1));
    // A saved-clean document stays clean: undo back to step 0 (the opened
    // file), then choose a layer there.
    let id2 = doc(&e);
    assert_eq!(info(&e, id2)["dirty"], json!(false));
    e.dispatch(id2, "layer.set_active", json!({"index":0}))
        .unwrap();
    assert_eq!(info(&e, id2)["dirty"], json!(false));
    assert_eq!(labels(&e, id2), ["Open"]);
    // Pixels do not change.
    let px = pixels(&e, id);
    e.dispatch(id, "layer.set_active", json!({"index":0}))
        .unwrap();
    assert_eq!(pixels(&e, id), px);
    // The next edit lands on the chosen layer.
    e.dispatch(id, "layer.set_active", json!({"index":1}))
        .unwrap();
    paint(&e, id);
    let s = e.snapshot(id).unwrap();
    assert!(s.state.layers[1].tiles.iter().count() > 0);
    assert_eq!(labels(&e, id), ["Open", "layer.add", "Paintbrush"]);
}

#[test]
fn duplicate_merge_down_and_reorder() {
    let e = Engine::new(None).unwrap();
    let id = doc(&e);
    let base = pixels(&e, id);
    e.dispatch(id, "layer.duplicate", json!({})).unwrap();
    assert_eq!(names(&e, id), ["Background", "Background copy"]);
    assert_eq!(info(&e, id)["active_layer"], json!(1), "the copy is active");
    assert_eq!(pixels(&e, id), base);
    // Merge Layer Down is off for the bottom layer.
    e.dispatch(id, "layer.set_active", json!({"index":0}))
        .unwrap();
    assert!(e.dispatch(id, "layer.merge_down", json!({})).is_err());
    assert!(e.dispatch(id, "layer.move_down", json!({})).is_err());
    e.dispatch(id, "layer.add", json!({"name":"Top"})).unwrap();
    e.dispatch(id, "layer.set_active", json!({"index":2}))
        .unwrap();
    assert!(e.dispatch(id, "layer.move_up", json!({})).is_err());
    paint(&e, id);
    // Blend mode and opacity of the top layer apply when it merges.
    e.dispatch(
        id,
        "layer.properties",
        json!({"index":2,"name":"Top","visible":true,"blend":"multiply","opacity":200}),
    )
    .unwrap();
    let composite = pixels(&e, id);
    e.dispatch(id, "layer.move_down", json!({})).unwrap();
    assert_eq!(names(&e, id), ["Background", "Top", "Background copy"]);
    assert_eq!(
        info(&e, id)["active_layer"],
        json!(1),
        "the moved layer stays active"
    );
    e.dispatch(id, "layer.move_up", json!({})).unwrap();
    assert_eq!(names(&e, id), ["Background", "Background copy", "Top"]);
    e.dispatch(id, "layer.merge_down", json!({})).unwrap();
    assert_eq!(names(&e, id), ["Background", "Background copy"]);
    assert_eq!(info(&e, id)["active_layer"], json!(1));
    let merged = layer(&e, id, 1);
    assert_eq!(
        merged["blend"],
        json!("normal"),
        "the lower layer keeps its own mode"
    );
    assert_eq!(merged["opacity"], json!(255));
    // The merged picture equals the picture before the merge, within 1/255
    // (the merge rounds once between the two layers).
    let after = pixels(&e, id);
    let worst = composite
        .iter()
        .zip(&after)
        .map(|(a, b)| a.abs_diff(*b))
        .max()
        .unwrap();
    assert!(worst <= 1, "merge changed a byte by {worst}");
    // Drag reorder: any position to any position.
    e.dispatch(id, "layer.move", json!({"from":1,"to":0}))
        .unwrap();
    assert_eq!(names(&e, id), ["Background copy", "Background"]);
    assert!(
        e.dispatch(id, "layer.move", json!({"from":0,"to":2}))
            .is_err()
    );
    assert_eq!(
        labels(&e, id),
        [
            "Open",
            "Duplicate Layer",
            // layer.add keeps its id as its label (tests/move.rs checks it).
            "layer.add",
            "Paintbrush",
            "Layer Properties",
            "Move Layer Down",
            "Move Layer Up",
            "Merge Layer Down",
            "Move Layer",
        ]
    );
    // Undo all the way gives back the opened document.
    e.dispatch(id, "history.jump", json!({"index":0})).unwrap();
    assert_eq!(pixels(&e, id), base);
    assert_eq!(names(&e, id), ["Background"]);
}

#[test]
fn hidden_top_layer_merges_as_nothing() {
    let e = Engine::new(None).unwrap();
    let id = doc(&e);
    e.dispatch(id, "layer.add", json!({"name":"Top"})).unwrap();
    e.dispatch(id, "layer.set_active", json!({"index":1}))
        .unwrap();
    paint(&e, id);
    e.dispatch(id, "layer.set_visible", json!({"index":1,"visible":false}))
        .unwrap();
    let hidden = pixels(&e, id);
    e.dispatch(id, "layer.merge_down", json!({})).unwrap();
    assert_eq!(pixels(&e, id), hidden);
}

#[test]
fn visibility_is_one_history_step_each() {
    let e = Engine::new(None).unwrap();
    let id = doc(&e);
    let shown = pixels(&e, id);
    e.dispatch(id, "layer.set_visible", json!({"index":0,"visible":false}))
        .unwrap();
    assert_eq!(layer(&e, id, 0)["visible"], json!(false));
    assert!(pixels(&e, id).iter().all(|&b| b == 0));
    e.dispatch(id, "layer.set_visible", json!({"index":0,"visible":true}))
        .unwrap();
    assert_eq!(pixels(&e, id), shown);
    assert_eq!(labels(&e, id), ["Open", "Hide Layer", "Show Layer"]);
    // Setting the state it already has is refused: no empty step.
    assert!(
        e.dispatch(id, "layer.set_visible", json!({"index":0,"visible":true}))
            .is_err()
    );
    assert!(
        e.dispatch(id, "layer.set_visible", json!({"index":3,"visible":true}))
            .is_err()
    );
    assert_eq!(labels(&e, id).len(), 3);
}

#[test]
fn layer_properties_preview_commit_is_one_step_and_cancel_changes_nothing() {
    let e = Engine::new(None).unwrap();
    let id = doc(&e);
    e.dispatch(id, "layer.duplicate", json!({})).unwrap();
    let px = pixels(&e, id);
    let before = info(&e, id);
    let props = |opacity: u64, blend: &str| json!({"index":1,"name":"Renamed","visible":true,"blend":blend,"opacity":opacity});
    e.preview_begin(id, "layer.properties", props(128, "difference"))
        .unwrap();
    // The preview shows the change live; the document itself does not change.
    assert!(wait(|| {
        let s = e.snapshot(id).unwrap();
        s.preview.is_some() && s.pixels(true).unwrap() != px
    }));
    for o in [10, 60, 200] {
        e.preview_update(id, props(o, "screen")).unwrap();
    }
    assert!(wait(|| {
        let s = e.snapshot(id).unwrap();
        s.preview
            .as_ref()
            .is_some_and(|p| p.layers[1].opacity == 200)
    }));
    assert_eq!(pixels(&e, id), px);
    assert_eq!(info(&e, id)["history_labels"], before["history_labels"]);
    e.preview_cancel(id).unwrap();
    assert!(e.snapshot(id).unwrap().preview.is_none());
    assert_eq!(pixels(&e, id), px);
    let mut now = info(&e, id);
    let mut was = before.clone();
    // The revision counter may move; everything a user can see may not.
    now["revision"] = json!(0);
    was["revision"] = json!(0);
    assert_eq!(now, was, "Cancel leaves the document and history unchanged");

    // OK: one history step with every field.
    e.preview_begin(id, "layer.properties", props(128, "difference"))
        .unwrap();
    e.preview_update(id, props(77, "xor")).unwrap();
    e.preview_commit(id).unwrap();
    assert_eq!(
        labels(&e, id),
        ["Open", "Duplicate Layer", "Layer Properties"]
    );
    let l = layer(&e, id, 1);
    assert_eq!(l["name"], json!("Renamed"));
    assert_eq!(l["blend"], json!("xor"));
    assert_eq!(l["opacity"], json!(77));
    assert!(e.snapshot(id).unwrap().preview.is_none());
    // No change at all is refused, so OK with nothing changed adds no step.
    assert!(
        e.dispatch(
            id,
            "layer.properties",
            json!({"index":1,"name":"Renamed","visible":true,"blend":"xor","opacity":77})
        )
        .is_err()
    );
    // Every one of the 14 modes is accepted.
    for mode in [
        "normal",
        "multiply",
        "additive",
        "color_burn",
        "color_dodge",
        "reflect",
        "glow",
        "overlay",
        "difference",
        "negation",
        "lighten",
        "darken",
        "screen",
        "xor",
    ] {
        e.dispatch(id, "layer.properties", props(1, mode)).unwrap();
        assert_eq!(layer(&e, id, 1)["blend"], json!(mode));
    }
    assert!(
        e.dispatch(id, "layer.properties", props(1, "hard_light"))
            .is_err()
    );
}

#[test]
fn layer_content_keys_follow_edits_and_history() {
    let e = Engine::new(None).unwrap();
    let id = doc(&e);
    e.dispatch(id, "layer.add", json!({"name":"Top"})).unwrap();
    let k0 = layer(&e, id, 0)["revision"].clone();
    let k1 = layer(&e, id, 1)["revision"].clone();
    assert!(k0.is_u64() && k1.is_u64());
    assert_ne!(k0, k1);
    e.dispatch(id, "layer.set_active", json!({"index":1}))
        .unwrap();
    paint(&e, id);
    assert_eq!(
        layer(&e, id, 0)["revision"],
        k0,
        "untouched layer keeps its key"
    );
    let painted = layer(&e, id, 1)["revision"].clone();
    assert_ne!(painted, k1);
    // A property change is not a pixel change.
    e.dispatch(id, "layer.set_visible", json!({"index":1,"visible":false}))
        .unwrap();
    assert_eq!(layer(&e, id, 1)["revision"], painted);
    e.dispatch(id, "history.jump", json!({"index":1})).unwrap();
    assert_eq!(
        layer(&e, id, 1)["revision"],
        k1,
        "a jump brings the old key back"
    );
    // History commands name each step's command for the History icons.
    assert_eq!(
        info(&e, id)["history_commands"],
        json!([
            "document.open",
            "layer.add",
            "tool.stroke",
            "layer.set_visible"
        ])
    );
}

#[test]
fn layer_thumbnail_is_a_box_filtered_copy_of_one_layer() {
    let e = Engine::new(None).unwrap();
    let id = doc(&e);
    e.dispatch(id, "layer.add", json!({"name":"Top"})).unwrap();
    let thumb = |index: usize, max: u64| {
        let id_of = layer(&e, id, index)["id"].clone();
        e.query(id, "layer.thumbnail", json!({"id":id_of,"max":max}))
            .unwrap()
    };
    // 40 x 24 into a 20 pixel box: 20 x 12, a header (size and content key),
    // then RGBA rows.
    let t = thumb(0, 20);
    assert_eq!(u32::from_le_bytes(t[0..4].try_into().unwrap()), 20);
    assert_eq!(u32::from_le_bytes(t[4..8].try_into().unwrap()), 12);
    assert_eq!(
        json!(u64::from_le_bytes(t[8..16].try_into().unwrap())),
        layer(&e, id, 0)["revision"]
    );
    assert_eq!(t.len(), 16 + 20 * 12 * 4);
    let t = &t[8..];
    // Pixel (0,0) averages source pixels 0, 1, 40 and 41.
    let reds = [0u32, 1, 40, 41].map(|i| i % 251);
    assert_eq!(
        &t[8..12],
        &[((reds.iter().sum::<u32>() + 2) / 4) as u8, 40, 200, 255]
    );
    // The empty top layer is transparent; a bigger box never upscales.
    let top = thumb(1, 200);
    assert_eq!(u32::from_le_bytes(top[0..4].try_into().unwrap()), 40);
    assert!(top[16..].iter().all(|&b| b == 0));
    assert!(
        e.query(id, "layer.thumbnail", json!({"id":999,"max":20}))
            .is_err()
    );
}

#[test]
fn preview_serial_changes_with_each_shown_preview() {
    let e = Engine::new(None).unwrap();
    let id = doc(&e);
    assert_eq!(e.snapshot(id).unwrap().preview_serial, 0);
    e.preview_begin(
        id,
        "layer.properties",
        json!({"index":0,"name":"B","visible":true,"blend":"normal","opacity":100}),
    )
    .unwrap();
    assert!(wait(|| e.snapshot(id).unwrap().preview.is_some()));
    let first = e.snapshot(id).unwrap().preview_serial;
    assert_ne!(first, 0);
    e.preview_update(
        id,
        json!({"index":0,"name":"B","visible":true,"blend":"normal","opacity":50}),
    )
    .unwrap();
    assert!(wait(|| {
        let s = e.snapshot(id).unwrap();
        s.preview.is_some() && s.preview_serial != first
    }));
    e.preview_cancel(id).unwrap();
    assert_eq!(e.snapshot(id).unwrap().preview_serial, 0);
}
