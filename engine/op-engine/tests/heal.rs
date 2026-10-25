// Provisional OpenPainter healing behavior: paint.net 3.36 has no healing tool.
use op_core::doc::ToolCompositeMode;
use op_engine::Engine;
use op_tools::{BrushSettings, CloneSource, PressureMode, StrokeSample, Tool};
use serde_json::json;
fn cfg(tool: Tool) -> BrushSettings {
    BrushSettings {
        tool,
        width: 9.0,
        color: [255; 4],
        antialias: false,
        spacing: 1.0,
        pressure: PressureMode::None,
        blend: ToolCompositeMode::Normal,
    }
}
#[test]
fn heal_preview_commit_undo_and_cancel() {
    let e = Engine::new(None).unwrap();
    let mut pixels = vec![0u8; 64 * 32 * 4];
    for y in 0..32 {
        for x in 0..64 {
            pixels[(y * 64 + x) * 4..(y * 64 + x) * 4 + 4].copy_from_slice(if x < 24 {
                &[200, 20, 40, 255]
            } else {
                &[80, 110, 130, 255]
            });
        }
    }
    pixels[(15 * 64 + 40) * 4..(15 * 64 + 40) * 4 + 4].copy_from_slice(&[0, 0, 0, 255]);
    let id = e.import_pixels(64, 32, &pixels, None).unwrap();
    e.stroke_begin_heal(
        id,
        cfg(Tool::HealingBrush),
        1,
        CloneSource::Layer(0),
        (12, 15),
        None,
    )
    .unwrap();
    e.stroke_append(id, &[StrokeSample::mouse(40.5, 15.5, 0)])
        .unwrap();
    assert_eq!(e.snapshot(id).unwrap().pixels(false).unwrap(), pixels);
    assert_ne!(e.snapshot(id).unwrap().pixels(true).unwrap(), pixels);
    let offset = e.stroke_clone_offset(id).unwrap();
    assert_eq!(offset, Some((28, 0)));
    e.stroke_commit(id).unwrap();
    assert_eq!(e.snapshot(id).unwrap().history.labels.len(), 2);
    let healed = e.snapshot(id).unwrap().pixels(false).unwrap();
    e.dispatch(id, "edit.undo", json!({})).unwrap();
    assert_eq!(e.snapshot(id).unwrap().pixels(false).unwrap(), pixels);
    e.stroke_begin_heal(
        id,
        cfg(Tool::HealingBrush),
        1,
        CloneSource::Layer(0),
        (12, 15),
        Some((28, 0)),
    )
    .unwrap();
    e.stroke_append(id, &[StrokeSample::mouse(42.5, 15.5, 0)])
        .unwrap();
    e.stroke_cancel(id).unwrap();
    assert_eq!(e.snapshot(id).unwrap().pixels(false).unwrap(), pixels);
    assert_eq!(e.snapshot(id).unwrap().pixels(true).unwrap(), pixels);
    e.dispatch(id, "edit.redo", json!({})).unwrap();
    assert_eq!(e.snapshot(id).unwrap().pixels(false).unwrap(), healed);
}

#[test]
fn spot_heal_preview_and_cancel_keep_committed_bytes() {
    let e = Engine::new(None).unwrap();
    let mut pixels = vec![125u8; 48 * 48 * 4];
    for px in pixels.chunks_exact_mut(4) {
        px[3] = 255;
    }
    for y in 22..27 {
        for x in 22..27 {
            pixels[(y * 48 + x) * 4..(y * 48 + x) * 4 + 3].fill(0);
        }
    }
    let id = e.import_pixels(48, 48, &pixels, None).unwrap();
    e.stroke_begin_spot_heal(id, cfg(Tool::SpotHeal), 1)
        .unwrap();
    e.stroke_append(id, &[StrokeSample::mouse(24.5, 24.5, 0)])
        .unwrap();
    assert_eq!(e.snapshot(id).unwrap().pixels(false).unwrap(), pixels);
    assert_ne!(e.snapshot(id).unwrap().pixels(true).unwrap(), pixels);
    e.stroke_cancel(id).unwrap();
    assert_eq!(e.snapshot(id).unwrap().pixels(true).unwrap(), pixels);
    e.stroke_begin_spot_heal(id, cfg(Tool::SpotHeal), 1)
        .unwrap();
    e.stroke_append(id, &[StrokeSample::mouse(24.5, 24.5, 0)])
        .unwrap();
    e.stroke_commit(id).unwrap();
    assert_eq!(e.snapshot(id).unwrap().history.labels.len(), 2);
    e.dispatch(id, "edit.undo", json!({})).unwrap();
    assert_eq!(e.snapshot(id).unwrap().pixels(false).unwrap(), pixels);
}
