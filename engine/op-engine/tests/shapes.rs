use op_engine::Engine;
use serde_json::json;
#[test]
fn shape_session_preview_commit_cancel_and_catalog() {
    let e = Engine::new(None).unwrap();
    let original = [30, 40, 50, 255].repeat(40 * 40);
    let id = e.import_pixels(40, 40, &original, None).unwrap();
    let catalog: serde_json::Value =
        serde_json::from_slice(&e.query(0, "tool.shape.catalog", json!({})).unwrap()).unwrap();
    assert_eq!(catalog.as_array().unwrap().len(), 29);
    e.dispatch(id, "tool.shape.begin", json!({"shape":"heart","box":[[4,4],[35,35]],"primary":[255,0,0,255],"secondary":[0,0,255,255],"mode":"both"})).unwrap();
    e.dispatch(
        id,
        "tool.shape.update",
        json!({"rotation":0.5,"shape":"star_5","width":3.0}),
    )
    .unwrap();
    let preview = e.query(id, "doc.pixels", json!({"preview":true})).unwrap();
    assert_eq!(e.query(id, "doc.pixels", json!({})).unwrap(), original);
    e.dispatch(id, "tool.shape.finish", json!({})).unwrap();
    assert_eq!(e.query(id, "doc.pixels", json!({})).unwrap(), preview);
    assert_eq!(e.snapshot(id).unwrap().history.labels.len(), 2);
    e.dispatch(id, "tool.shape.begin", json!({"shape":"rectangle","box":[[0,0],[40,40]],"primary":[0,255,0,255],"secondary":[0,0,255,255]})).unwrap();
    e.dispatch(id, "tool.shape.cancel", json!({})).unwrap();
    assert_eq!(e.query(id, "doc.pixels", json!({})).unwrap(), preview);
    assert_eq!(e.snapshot(id).unwrap().history.labels.len(), 2);
}
