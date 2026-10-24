use op_engine::Engine;
use serde_json::json;

#[test]
fn wand_live_tolerance_commits_one_history_step() {
    let engine = Engine::new(None).unwrap();
    let id = engine
        .import_pixels(3, 1, &[0, 0, 0, 255, 128, 0, 0, 255, 255, 0, 0, 255], None)
        .unwrap();
    engine
        .preview_begin(id, "tool.magic_wand", json!({"x":0,"y":0,"tolerance":0.0}))
        .unwrap();
    engine.preview_update(id, json!({"tolerance":0.5})).unwrap();
    engine.preview_update(id, json!({"x":2})).unwrap();
    engine.preview_update(id, json!({"x":0})).unwrap();
    engine.preview_commit(id).unwrap();
    let (state, _, _, _) = engine.actor_probe(id).unwrap();
    let mask = state.rasterize_selection(op_core::selection::RasterMode::Pixelated);
    assert_eq!(mask.bytes.as_ref(), &[255, 255, 0]);
    assert_eq!(engine.snapshot(id).unwrap().history.labels.len(), 2);
}

#[test]
fn bucket_detects_on_frozen_source_and_cancel_keeps_document() {
    let engine = Engine::new(None).unwrap();
    let id = engine
        .import_pixels(3, 1, &[1, 0, 0, 255, 1, 0, 0, 255, 2, 0, 0, 255], None)
        .unwrap();
    let args = json!({"x":0,"y":0,"tolerance":0.0,"color":"020000ff","blend":"overwrite"});
    engine
        .preview_begin(id, "tool.paint_bucket", args.clone())
        .unwrap();
    engine.preview_cancel(id).unwrap();
    assert_eq!(engine.snapshot(id).unwrap().history.labels.len(), 1);
    engine.dispatch(id, "tool.paint_bucket", args).unwrap();
    let bytes = engine
        .actor_probe(id)
        .unwrap()
        .0
        .read_tile(0, op_core::tiles::TileCoord { x: 0, y: 0 })
        .unwrap();
    assert_eq!(&bytes[..12], &[2, 0, 0, 255, 2, 0, 0, 255, 2, 0, 0, 255]);
    assert_eq!(engine.snapshot(id).unwrap().history.labels.len(), 2);
}
