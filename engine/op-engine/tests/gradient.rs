use op_engine::Engine;
use serde_json::json;
#[test]
fn gradient_commands_preview_commit_and_cancel() {
    let e = Engine::new(None).unwrap();
    let original = [40, 80, 120, 200].repeat(25);
    let id = e.import_pixels(5, 5, &original, None).unwrap();
    let params = json!({"shape":"linear","mode":"color","primary":[200,20,10,128],"secondary":[10,220,250,64],"antialias":false,"blend":"overwrite","start":[1.0,1.0],"end":[3.0,1.0]});
    e.dispatch(id, "tool.gradient.begin", params).unwrap();
    for i in 0..100 {
        e.dispatch(
            id,
            "tool.gradient.update",
            json!({"handle":"end","point":[3.0 + (i + 1) as f64 / 100.0,1.0]}),
        )
        .unwrap();
    }
    let preview = e.query(id, "doc.pixels", json!({"preview":true})).unwrap();
    assert_eq!(e.query(id, "doc.pixels", json!({})).unwrap(), original);
    e.dispatch(id, "tool.gradient.finish", json!({})).unwrap();
    assert_eq!(e.query(id, "doc.pixels", json!({})).unwrap(), preview);
    assert_eq!(e.snapshot(id).unwrap().history.labels.len(), 2);
    e.dispatch(id,"tool.gradient.begin",json!({"shape":"radial","mode":"transparency","primary":[0,0,0,255],"secondary":[0,0,0,0],"start":[0.0,0.0],"end":[4.0,0.0]})).unwrap();
    e.dispatch(id, "tool.gradient.cancel", json!({})).unwrap();
    assert_eq!(e.query(id, "doc.pixels", json!({})).unwrap(), preview);
    assert_eq!(e.snapshot(id).unwrap().history.labels.len(), 2);
}
