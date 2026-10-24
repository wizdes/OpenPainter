use op_core::doc::ToolCompositeMode;
use op_engine::Engine;
use op_tools::{BrushSettings, PressureMode, StrokeSample, Tool};
use serde_json::json;
fn cfg() -> BrushSettings {
    BrushSettings {
        tool: Tool::Paintbrush,
        width: 8.0,
        color: [255, 0, 0, 255],
        antialias: true,
        spacing: 0.1,
        pressure: PressureMode::None,
        blend: ToolCompositeMode::Normal,
    }
}
#[test]
fn stroke_preview_cancel_commit_undo() {
    let e = Engine::new(None).unwrap();
    let id = e
        .import_pixels(32, 32, &vec![0; 32 * 32 * 4], None)
        .unwrap();
    let original = e.snapshot(id).unwrap().pixels(false).unwrap();
    e.stroke_begin(id, cfg(), 99).unwrap();
    e.stroke_append(id, &[StrokeSample::mouse(10.5, 10.5, 0)])
        .unwrap();
    assert_ne!(e.snapshot(id).unwrap().pixels(true).unwrap(), original);
    assert_eq!(e.snapshot(id).unwrap().pixels(false).unwrap(), original);
    e.stroke_cancel(id).unwrap();
    assert_eq!(e.snapshot(id).unwrap().pixels(true).unwrap(), original);
    e.stroke_begin(id, cfg(), 99).unwrap();
    e.stroke_append(id, &[StrokeSample::mouse(10.5, 10.5, 0)])
        .unwrap();
    e.stroke_commit(id).unwrap();
    let snap = e.snapshot(id).unwrap();
    assert_eq!(snap.history.labels.len(), 2);
    assert_eq!(snap.history.labels[1], "Paintbrush");
    assert_ne!(snap.pixels(false).unwrap(), original);
    e.dispatch(id, "edit.undo", json!({})).unwrap();
    assert_eq!(e.snapshot(id).unwrap().pixels(false).unwrap(), original);
}
