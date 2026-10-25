use op_engine::Engine;
use serde_json::json;

fn open() -> (Engine, u64, Vec<u8>) {
    let e = Engine::new(None).unwrap();
    let original = [30, 40, 50, 255].repeat(32 * 32);
    let id = e.import_pixels(32, 32, &original, None).unwrap();
    (e, id, original)
}
fn begin(e: &Engine, id: u64) {
    e.dispatch(
        id,
        "tool.line.begin",
        json!({"points":[[3,4],[27,25]],"primary":[240,20,10,255],"width":3.0}),
    )
    .unwrap();
}
#[test]
fn preview_edits_finish_once_and_escape() {
    let (e, id, original) = open();
    begin(&e, id);
    for i in 0..5 {
        e.dispatch(
            id,
            "tool.line.update",
            json!({"handle":1,"point":[27,i+10]}),
        )
        .unwrap();
    }
    let preview = e.query(id, "doc.pixels", json!({"preview":true})).unwrap();
    assert_eq!(e.query(id, "doc.pixels", json!({})).unwrap(), original);
    e.dispatch(id, "tool.line.finish", json!({})).unwrap();
    e.dispatch(id, "tool.line.finish", json!({})).unwrap();
    assert_eq!(e.query(id, "doc.pixels", json!({})).unwrap(), preview);
    assert_eq!(e.snapshot(id).unwrap().history.labels.len(), 2);
    begin(&e, id);
    e.dispatch(id, "tool.line.cancel", json!({})).unwrap();
    assert_eq!(e.query(id, "doc.pixels", json!({})).unwrap(), preview);
    assert_eq!(e.snapshot(id).unwrap().history.labels.len(), 2);
}
#[test]
fn undo_switch_and_delete_resolve_editing() {
    let (e, id, original) = open();
    begin(&e, id);
    e.dispatch(id, "edit.undo", json!({})).unwrap();
    assert_eq!(e.query(id, "doc.pixels", json!({})).unwrap(), original);
    begin(&e, id);
    e.dispatch(id, "tool.select", json!({"tool":"paintbrush"}))
        .unwrap();
    assert_eq!(e.snapshot(id).unwrap().history.labels.len(), 2);
    e.dispatch(id, "layer.add", json!({})).unwrap();
    begin(&e, id);
    e.dispatch(id, "layer.delete", json!({})).unwrap();
    assert!(e.snapshot(id).unwrap().preview.is_none());
}

#[test]
fn invalid_combined_update_keeps_session_unchanged() {
    let (e, id, _) = open();
    begin(&e, id);
    let preview = e.query(id, "doc.pixels", json!({"preview":true})).unwrap();
    assert!(
        e.dispatch(
            id,
            "tool.line.update",
            json!({"handle":1,"point":[4,25],"width":-1.0})
        )
        .is_err()
    );
    e.dispatch(id, "tool.line.finish", json!({})).unwrap();
    assert_eq!(e.query(id, "doc.pixels", json!({})).unwrap(), preview);
}

#[test]
fn tool_select_without_editing_is_a_no_op() {
    let (e, id, original) = open();
    e.dispatch(id, "tool.select", json!({"tool":"line"}))
        .unwrap();
    assert_eq!(e.query(id, "doc.pixels", json!({})).unwrap(), original);
    assert_eq!(e.snapshot(id).unwrap().history.labels.len(), 1);
}
