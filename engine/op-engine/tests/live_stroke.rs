//! M38 live strokes: brush options from JSON, a hardness stroke session, and the
//! tile diff and tile pixels the canvas view uses for partial uploads.
use op_core::tiles::TileCoord;
use op_engine::Engine;
use op_engine::live_stroke::{brush_from_json, changed_tiles, tile_rgba};
use op_tools::{PressureMode, StrokeSample, Tool};
use serde_json::json;

fn sample(x: f64, y: f64, pressure: f64, t: u64) -> StrokeSample {
    StrokeSample {
        x,
        y,
        pressure,
        timestamp_ms: t,
        tilt_x: 0.0,
        tilt_y: 0.0,
    }
}

#[test]
fn brush_json_maps_every_option() {
    let (s, h) = brush_from_json(&json!({
        "tool":"eraser","width":12.5,"color":"ff000080","opacity":127.5,"antialias":false,
        "blend":"overwrite","hardness":0.25,"pressure_size":true,"pressure_opacity":true
    }))
    .unwrap();
    assert_eq!(s.tool, Tool::Eraser);
    assert_eq!(s.width, 12.5);
    assert_eq!(s.color, [255, 0, 0, 64]);
    assert!(!s.antialias);
    assert_eq!(s.pressure, PressureMode::SizeAndOpacity);
    assert_eq!(h, 0.25);
    let (d, h) = brush_from_json(&json!({})).unwrap();
    assert_eq!(
        (d.tool, d.width, d.color, d.antialias, d.pressure, h),
        (
            Tool::Paintbrush,
            2.0,
            [0, 0, 0, 255],
            true,
            PressureMode::None,
            1.0
        )
    );
    let (s, _) = brush_from_json(&json!({"pressure_opacity":true})).unwrap();
    assert_eq!(s.pressure, PressureMode::Opacity);
    let (s, _) = brush_from_json(&json!({"pressure_size":true})).unwrap();
    assert_eq!(s.pressure, PressureMode::Size);
    for bad in [
        json!({"tool":"clone"}),
        json!({"width":0}),
        json!({"width":"x"}),
        json!({"hardness":2}),
        json!({"color":"red"}),
        json!({"blend":"multiply"}),
        json!({"antialias":1}),
    ] {
        assert!(brush_from_json(&bad).is_err(), "{bad}");
    }
}

#[test]
fn hard_session_commits_one_step_and_cancel_restores() {
    let e = Engine::new(None).unwrap();
    let id = e
        .import_pixels(600, 400, &vec![255; 600 * 400 * 4], None)
        .unwrap();
    let original = e.snapshot(id).unwrap().pixels(false).unwrap();
    let (settings, hardness) =
        brush_from_json(&json!({"width":30,"color":"0000ffff","hardness":0.5})).unwrap();
    e.stroke_begin_hard(id, settings.clone(), hardness, 1)
        .unwrap();
    for i in 0..20 {
        e.stroke_append(id, &[sample(20.0 + i as f64 * 20.0, 200.0, 1.0, i)])
            .unwrap();
    }
    assert_ne!(e.snapshot(id).unwrap().pixels(true).unwrap(), original);
    e.stroke_cancel(id).unwrap();
    let snap = e.snapshot(id).unwrap();
    assert_eq!(snap.pixels(true).unwrap(), original);
    assert_eq!(snap.history.labels.len(), 1);
    assert!(snap.preview.is_none());

    e.stroke_begin_hard(id, settings, hardness, 1).unwrap();
    for i in 0..20 {
        e.stroke_append(id, &[sample(20.0 + i as f64 * 20.0, 200.0, 1.0, i)])
            .unwrap();
    }
    e.stroke_commit(id).unwrap();
    let snap = e.snapshot(id).unwrap();
    assert_eq!(
        snap.history.labels,
        ["Open", "Paintbrush"].map(String::from)
    );
    assert_ne!(snap.pixels(false).unwrap(), original);
    // A bad hardness is refused before a session starts.
    let (s, _) = brush_from_json(&json!({})).unwrap();
    assert!(e.stroke_begin_hard(id, s, 1.5, 1).is_err());
    assert!(e.stroke_append(id, &[sample(1.0, 1.0, 1.0, 0)]).is_err());
}

#[test]
fn tile_diff_finds_only_stroked_tiles_and_tile_pixels_match() {
    let e = Engine::new(None).unwrap();
    let (w, h) = (700u32, 300u32);
    let id = e
        .import_pixels(w, h, &vec![200; (w * h * 4) as usize], None)
        .unwrap();
    let before = e.snapshot(id).unwrap().state.clone();
    let (settings, _) = brush_from_json(&json!({"width":10,"color":"ff0000ff"})).unwrap();
    e.stroke_begin_hard(id, settings, 1.0, 1).unwrap();
    // x 600..620 and y 270..280: only tile (2, 1).
    e.stroke_append(
        id,
        &[sample(600.0, 275.0, 1.0, 0), sample(620.0, 275.0, 1.0, 1)],
    )
    .unwrap();
    let snap = e.snapshot(id).unwrap();
    let preview = snap.preview.clone().unwrap();
    assert_eq!(
        changed_tiles(&before, &preview).unwrap(),
        vec![TileCoord { x: 2, y: 1 }]
    );
    assert_eq!(changed_tiles(&preview, &preview).unwrap(), vec![]);
    // The edge tile is cropped to the image: 700 - 512 = 188 by 300 - 256 = 44.
    let (x, y, tw, th, rgba) = tile_rgba(&preview, TileCoord { x: 2, y: 1 }).unwrap();
    assert_eq!((x, y, tw, th), (512, 256, 188, 44));
    let full = snap.pixels(true).unwrap();
    for row in 0..th as usize {
        let src = ((y as usize + row) * w as usize + x as usize) * 4;
        assert_eq!(
            &rgba[row * tw as usize * 4..(row + 1) * tw as usize * 4],
            &full[src..src + tw as usize * 4]
        );
    }
    assert!(tile_rgba(&preview, TileCoord { x: 3, y: 0 }).is_err());
    e.stroke_commit(id).unwrap();
    // Committing keeps the preview's tiles: nothing new to upload.
    let after = e.snapshot(id).unwrap().state.clone();
    assert_eq!(changed_tiles(&preview, &after).unwrap(), vec![]);
    // A layer property change needs a full redraw.
    e.dispatch(
        id,
        "layer.properties",
        json!({"index":0,"name":"Background","visible":true,"blend":"normal","opacity":100}),
    )
    .unwrap();
    let props = e.snapshot(id).unwrap().state.clone();
    assert!(changed_tiles(&after, &props).is_none());
}
