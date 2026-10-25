// tool.text.begin / update / finish / cancel with fixed run data (no fonts).
use op_engine::{Engine, Error};
use serde_json::{Value, json};

fn b64(bytes: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for c in bytes.chunks(3) {
        let n = u32::from(c[0]) << 16
            | u32::from(*c.get(1).unwrap_or(&0)) << 8
            | u32::from(*c.get(2).unwrap_or(&0));
        for i in 0..4 {
            if i <= c.len() {
                out.push(A[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}
fn cov_run(x: i32, y: i32, w: u32, h: u32, data: &[u8]) -> Value {
    json!({"kind":"coverage","x":x,"y":y,"width":w,"height":h,"data":b64(data)})
}
fn begin(origin: [i32; 2], runs: Value) -> Value {
    json!({"origin":origin,"runs":runs,"primary":[255,0,0,255],"blend":"normal","text":"Hi","caret":2})
}
fn pixels(e: &Engine, id: u64, preview: bool) -> Vec<u8> {
    e.query(id, "doc.pixels", json!({ "preview": preview }))
        .unwrap()
}

#[test]
fn begin_update_finish_commits_one_step_and_preview_equals_commit() {
    let e = Engine::new(None).unwrap();
    let original = [0, 0, 255, 255].repeat(8 * 4);
    let id = e.import_pixels(8, 4, &original, None).unwrap();
    let steps = e.snapshot(id).unwrap().history.labels.len();
    e.dispatch(
        id,
        "tool.text.begin",
        begin([1, 1], json!([cov_run(0, 0, 2, 1, &[255, 255])])),
    )
    .unwrap();
    // Typing: each key replaces the runs and the text.
    for n in 3..6u32 {
        e.dispatch(
            id,
            "tool.text.update",
            json!({"runs":[cov_run(0, 0, n, 1, &vec![255; n as usize])],"text":"Hi!","caret":3}),
        )
        .unwrap();
    }
    e.dispatch(
        id,
        "tool.text.update",
        json!({"origin":[2,2],"primary":[0,255,0,255]}),
    )
    .unwrap();
    let preview = pixels(&e, id, true);
    assert_eq!(
        pixels(&e, id, false),
        original,
        "the document is unchanged before Finish"
    );
    assert_eq!(
        &preview[(2 * 8 + 2) * 4..(2 * 8 + 2) * 4 + 4],
        [0, 255, 0, 255]
    );
    assert_eq!(
        e.snapshot(id).unwrap().history.labels.len(),
        steps,
        "no step while editing"
    );
    e.dispatch(id, "tool.text.finish", json!({})).unwrap();
    assert_eq!(pixels(&e, id, false), preview, "preview equals commit");
    let snap = e.snapshot(id).unwrap();
    assert_eq!(snap.history.labels.len(), steps + 1, "one history step");
    assert_eq!(snap.history.labels.last().unwrap(), "Text");
    assert_eq!(snap.history.commands.last().unwrap(), "tool.text.finish");
    e.dispatch(id, "edit.undo", json!({})).unwrap();
    assert_eq!(pixels(&e, id, false), original);
}

#[test]
fn cancel_leaves_bytes_and_history_unchanged() {
    let e = Engine::new(None).unwrap();
    let original = [10, 20, 30, 255].repeat(16);
    let id = e.import_pixels(4, 4, &original, None).unwrap();
    let before = e.snapshot(id).unwrap();
    e.dispatch(
        id,
        "tool.text.begin",
        begin([0, 0], json!([cov_run(0, 0, 4, 4, &[255; 16])])),
    )
    .unwrap();
    assert_ne!(pixels(&e, id, true), original);
    e.dispatch(id, "tool.text.cancel", json!({})).unwrap();
    let after = e.snapshot(id).unwrap();
    assert_eq!(pixels(&e, id, false), original);
    assert!(after.preview.is_none());
    assert_eq!(after.history.labels, before.history.labels);
    assert_eq!(after.history.index, before.history.index);
    assert_eq!(after.revision, before.revision);
    assert_eq!(
        e.dispatch(id, "tool.text.finish", json!({})),
        Err(Error::BadArgument("no text session".into()))
    );
}

#[test]
fn other_commands_wait_and_a_tool_switch_finishes() {
    let e = Engine::new(None).unwrap();
    let id = e.import_pixels(4, 4, &[0; 64], None).unwrap();
    let steps = e.snapshot(id).unwrap().history.labels.len();
    e.dispatch(
        id,
        "tool.text.begin",
        begin([0, 0], json!([cov_run(0, 0, 1, 1, &[255])])),
    )
    .unwrap();
    assert_eq!(
        e.dispatch(id, "tool.text.begin", begin([0, 0], json!([]))),
        Err(Error::Busy),
        "one text session at a time"
    );
    assert_eq!(
        e.dispatch(id, "selection.select_all", json!({})),
        Err(Error::Busy)
    );
    e.dispatch(id, "tool.select", json!({})).unwrap();
    assert_eq!(e.snapshot(id).unwrap().history.labels.len(), steps + 1);
    assert_eq!(&pixels(&e, id, false)[..4], [255, 0, 0, 255]);
    // Undo while editing drops the open text, like Line/Curve.
    e.dispatch(
        id,
        "tool.text.begin",
        begin([2, 2], json!([cov_run(0, 0, 1, 1, &[255])])),
    )
    .unwrap();
    e.dispatch(id, "edit.undo", json!({})).unwrap();
    assert!(e.snapshot(id).unwrap().preview.is_none());
    assert_eq!(e.snapshot(id).unwrap().history.labels.len(), steps + 1);
}

#[test]
fn selection_clips_and_bad_parameters_are_rejected() {
    let e = Engine::new(None).unwrap();
    let mut base = [0, 0, 0, 0].repeat(2);
    base.extend([9, 9, 9, 255].repeat(2));
    let id = e.import_pixels(4, 1, &base, None).unwrap();
    e.dispatch(
        id,
        "tool.magic_wand",
        json!({"x":3,"y":0,"tolerance":0.0,"contiguous":true,"sampling":"layer","antialias":false,"combine":"replace"}),
    )
    .unwrap();
    let sel: Value =
        serde_json::from_slice(&e.query(id, "doc.selection", json!({})).unwrap()).unwrap();
    assert_eq!(sel["active"], json!(true));
    e.dispatch(
        id,
        "tool.text.begin",
        begin([0, 0], json!([cov_run(0, 0, 4, 1, &[255; 4])])),
    )
    .unwrap();
    e.dispatch(id, "tool.text.finish", json!({})).unwrap();
    let px = pixels(&e, id, false);
    assert_eq!(&px[..8], [0; 8], "outside the selection");
    assert_eq!(&px[8..16], [255, 0, 0, 255].repeat(2));
    for bad in [
        json!({"origin":[0,0],"runs":[{"kind":"coverage","x":0,"y":0,"width":2,"height":1,"data":b64(&[1])}],"primary":[0,0,0,255]}),
        json!({"origin":[0,0],"runs":[{"kind":"glyph","x":0,"y":0,"width":1,"height":1,"data":b64(&[1])}],"primary":[0,0,0,255]}),
        json!({"origin":[0.5,0],"runs":[],"primary":[0,0,0,255]}),
        json!({"origin":[0,0],"runs":[],"primary":[0,0,0,255],"blend":"multiply"}),
        json!({"origin":[0,0],"runs":[],"primary":[0,0,0,255],"text":"é","caret":1}),
        json!({"origin":[0,0],"runs":[{"kind":"coverage","x":0,"y":0,"width":1,"height":1,"data":"A"}],"primary":[0,0,0,255]}),
    ] {
        assert!(
            matches!(
                e.dispatch(id, "tool.text.begin", bad.clone()),
                Err(Error::BadArgument(_))
            ),
            "{bad}"
        );
    }
}

#[test]
fn text_and_shape_sessions_finish_each_other() {
    // Starting one editable tool while the other is open commits the open one first.
    let e = Engine::new(None).unwrap();
    let id = e
        .import_pixels(8, 8, &[0, 0, 255, 255].repeat(64), None)
        .unwrap();
    let steps = e.snapshot(id).unwrap().history.labels.len();
    let shape = json!({"shape":"rectangle","box":[[1,1],[6,6]],"primary":[0,255,0,255],"secondary":[0,0,255,255]});
    e.dispatch(id, "tool.shape.begin", shape.clone()).unwrap();
    e.dispatch(
        id,
        "tool.text.begin",
        begin([0, 0], json!([cov_run(0, 0, 2, 1, &[255, 255])])),
    )
    .unwrap();
    assert_eq!(
        e.snapshot(id).unwrap().history.labels.len(),
        steps + 1,
        "shape committed"
    );
    e.dispatch(id, "tool.shape.begin", shape).unwrap();
    assert_eq!(
        e.snapshot(id).unwrap().history.labels.len(),
        steps + 2,
        "text committed"
    );
    e.dispatch(id, "tool.shape.cancel", json!({})).unwrap();
    assert_eq!(e.snapshot(id).unwrap().history.labels.len(), steps + 2);
}
