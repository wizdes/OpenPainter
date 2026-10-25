use op_core::doc::ToolCompositeMode;
use op_engine::Engine;
use op_tools::flood::AlphaMode;
use op_tools::{BrushSettings, CloneSource, PressureMode, RecolorOptions, StrokeSample, Tool};
use serde_json::json;

fn cfg(tool: Tool) -> BrushSettings {
    BrushSettings {
        tool,
        width: 1.0,
        color: [255; 4],
        antialias: false,
        spacing: 1.0,
        pressure: PressureMode::None,
        blend: ToolCompositeMode::Normal,
    }
}
#[test]
fn clone_and_recolor_each_make_one_undoable_step() {
    let e = Engine::new(None).unwrap();
    let mut pixels = vec![0; 8 * 4];
    pixels[0..4].copy_from_slice(&[100, 100, 100, 255]);
    let id = e.import_pixels(8, 1, &pixels, None).unwrap();
    e.stroke_begin_clone(
        id,
        cfg(Tool::CloneStamp),
        1,
        CloneSource::Layer(0),
        (0, 0),
        None,
    )
    .unwrap();
    e.stroke_append(id, &[StrokeSample::mouse(2.5, 0.5, 0)])
        .unwrap();
    assert_eq!(e.snapshot(id).unwrap().pixels(false).unwrap(), pixels);
    assert_ne!(e.snapshot(id).unwrap().pixels(true).unwrap(), pixels);
    assert_eq!(e.stroke_clone_offset(id).unwrap(), Some((2, 0)));
    e.stroke_commit(id).unwrap();
    let cloned = e.snapshot(id).unwrap().pixels(false).unwrap();
    assert_eq!(&cloned[8..12], &[100, 100, 100, 255]);
    e.stroke_begin_recolor(
        id,
        cfg(Tool::Recolor),
        1,
        RecolorOptions {
            primary: [120, 80, 200, 255],
            secondary: [100, 100, 100, 255],
            tolerance: 0.0,
            alpha: AlphaMode::Straight,
        },
    )
    .unwrap();
    e.stroke_append(id, &[StrokeSample::mouse(2.5, 0.5, 0)])
        .unwrap();
    e.stroke_commit(id).unwrap();
    let snap = e.snapshot(id).unwrap();
    assert_eq!(snap.history.labels.len(), 3);
    assert_eq!(&snap.pixels(false).unwrap()[8..12], &[120, 80, 200, 255]);
    e.dispatch(id, "edit.undo", json!({})).unwrap();
    assert_eq!(e.snapshot(id).unwrap().pixels(false).unwrap(), cloned);
    e.dispatch(id, "edit.undo", json!({})).unwrap();
    assert_eq!(e.snapshot(id).unwrap().pixels(false).unwrap(), pixels);
}

#[test]
fn active_stroke_rejects_a_second_begin_as_busy() {
    let e = Engine::new(None).unwrap();
    let id = e.import_pixels(1, 1, &[1, 2, 3, 255], None).unwrap();
    e.stroke_begin_clone(
        id,
        cfg(Tool::CloneStamp),
        1,
        CloneSource::Layer(0),
        (0, 0),
        None,
    )
    .unwrap();
    let error = e
        .stroke_begin_recolor(
            id,
            cfg(Tool::Paintbrush),
            1,
            RecolorOptions {
                primary: [0; 4],
                secondary: [0; 4],
                tolerance: 0.0,
                alpha: AlphaMode::Straight,
            },
        )
        .unwrap_err();
    assert!(matches!(error, op_engine::Error::Busy), "{error:?}");
    e.stroke_cancel(id).unwrap();
}
