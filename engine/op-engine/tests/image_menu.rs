use op_engine::Engine;
use serde_json::json;

#[test]
fn image_commands_change_all_layers_and_undo_once() {
    let engine = Engine::new(None).unwrap();
    let id = engine
        .import_pixels(2, 1, &[10, 0, 0, 255, 20, 0, 0, 255], None)
        .unwrap();
    let before = engine.query(id, "doc.pixels", json!({})).unwrap();
    engine
        .dispatch(id, "image.flip_horizontal", json!({}))
        .unwrap();
    assert_eq!(
        engine.query(id, "doc.pixels", json!({})).unwrap(),
        vec![20, 0, 0, 255, 10, 0, 0, 255]
    );
    assert_eq!(engine.snapshot(id).unwrap().history.index, 1);
    engine.dispatch(id, "edit.undo", json!({})).unwrap();
    assert_eq!(engine.query(id, "doc.pixels", json!({})).unwrap(), before);
}

#[test]
fn nearest_resize_and_canvas_anchor() {
    let engine = Engine::new(None).unwrap();
    let id = engine
        .import_pixels(2, 1, &[10, 0, 0, 255, 20, 0, 0, 255], None)
        .unwrap();
    engine.dispatch(id, "image.resize", json!({"width":4,"height":1,"resampling":"nearest","maintain_aspect":false,"gamma":false})).unwrap();
    assert_eq!(
        engine
            .query(id, "doc.pixels", json!({}))
            .unwrap()
            .chunks_exact(4)
            .map(|p| p[0])
            .collect::<Vec<_>>(),
        [10, 10, 20, 20]
    );
    engine
        .dispatch(
            id,
            "image.canvas_size",
            json!({"width":5,"height":1,"anchor":"right","fill":"black"}),
        )
        .unwrap();
    assert_eq!(
        engine
            .query(id, "doc.pixels", json!({}))
            .unwrap()
            .chunks_exact(4)
            .map(|p| p[0])
            .collect::<Vec<_>>(),
        [0, 10, 10, 20, 20]
    );
}

#[test]
fn every_anchor_and_fill() {
    let anchors = [
        ("top_left", (0, 0)),
        ("top", (1, 0)),
        ("top_right", (2, 0)),
        ("left", (0, 1)),
        ("center", (1, 1)),
        ("right", (2, 1)),
        ("bottom_left", (0, 2)),
        ("bottom", (1, 2)),
        ("bottom_right", (2, 2)),
    ];
    for (anchor, (ax, ay)) in anchors {
        for (fill, rgba) in [
            ("transparent", [0, 0, 0, 0]),
            ("primary", [1, 2, 3, 255]),
            ("secondary", [4, 5, 6, 255]),
            ("white", [255; 4]),
            ("black", [0, 0, 0, 255]),
        ] {
            let e = Engine::new(None).unwrap();
            let id = e.import_pixels(1, 1, &[9, 8, 7, 255], None).unwrap();
            e.dispatch(id,"image.canvas_size",json!({"width":3,"height":3,"anchor":anchor,"fill":fill,"primary":"010203ff","secondary":"040506ff"})).unwrap();
            let pixels = e.query(id, "doc.pixels", json!({})).unwrap();
            for y in 0..3 {
                for x in 0..3 {
                    let p = &pixels[(y * 3 + x) * 4..(y * 3 + x + 1) * 4];
                    assert_eq!(
                        p,
                        if (x, y) == (ax, ay) {
                            [9, 8, 7, 255]
                        } else {
                            rgba
                        },
                        "{anchor} {fill} {x} {y}"
                    );
                }
            }
        }
    }
}

#[test]
fn flips_rotations_and_crop_restore_on_undo() {
    let cases = [
        ("image.flip_horizontal", 2, 3, vec![3, 2, 1, 6, 5, 4]),
        ("image.flip_vertical", 2, 3, vec![4, 5, 6, 1, 2, 3]),
        ("image.rotate_90_cw", 3, 2, vec![4, 1, 5, 2, 6, 3]),
        ("image.rotate_90_ccw", 3, 2, vec![3, 6, 2, 5, 1, 4]),
        ("image.rotate_180", 2, 3, vec![6, 5, 4, 3, 2, 1]),
    ];
    let original: Vec<u8> = (1..=6).flat_map(|v| [v, 0, 0, 255]).collect();
    for (cmd, h, w, want) in cases {
        let e = Engine::new(None).unwrap();
        let id = e.import_pixels(3, 2, &original, None).unwrap();
        e.dispatch(id, cmd, json!({})).unwrap();
        let snap = e.snapshot(id).unwrap();
        assert_eq!((snap.state.width, snap.state.height), (w, h), "{cmd}");
        assert_eq!(
            e.query(id, "doc.pixels", json!({}))
                .unwrap()
                .chunks_exact(4)
                .map(|p| p[0])
                .collect::<Vec<_>>(),
            want,
            "{cmd}"
        );
        assert_eq!(snap.history.index, 1);
        e.dispatch(id, "edit.undo", json!({})).unwrap();
        assert_eq!(e.query(id, "doc.pixels", json!({})).unwrap(), original);
    }
}

#[test]
fn flatten_composites_and_crops_selection_shape() {
    let e = Engine::new(None).unwrap();
    let id = e
        .import_pixels(
            2,
            2,
            &[255, 0, 0, 255, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            None,
        )
        .unwrap();
    e.dispatch(id, "layer.add", json!({"name":"Top"})).unwrap();
    let before_flatten = e.query(id, "doc.pixels", json!({})).unwrap();
    e.dispatch(id, "image.flatten", json!({})).unwrap();
    assert_eq!(e.snapshot(id).unwrap().state.layers.len(), 1);
    assert_eq!(
        e.query(id, "doc.pixels", json!({})).unwrap(),
        before_flatten
    );
    e.dispatch(id, "edit.undo", json!({})).unwrap();
    assert_eq!(e.snapshot(id).unwrap().state.layers.len(), 2);
    e.dispatch(id, "test.set_selection", json!({})).unwrap();
    e.dispatch(id, "image.crop_to_selection", json!({}))
        .unwrap();
    assert_eq!(
        (
            e.snapshot(id).unwrap().state.width,
            e.snapshot(id).unwrap().state.height
        ),
        (1, 1)
    );
    e.dispatch(id, "edit.undo", json!({})).unwrap();
    assert_eq!(
        (
            e.snapshot(id).unwrap().state.width,
            e.snapshot(id).unwrap().state.height
        ),
        (2, 2)
    );
}

#[test]
fn auto_crop_and_percent_resize_are_single_steps() {
    let e = Engine::new(None).unwrap();
    let mut pixels = vec![0; 4 * 3 * 4];
    pixels[24..28].copy_from_slice(&[17, 18, 19, 255]);
    let id = e.import_pixels(4, 3, &pixels, None).unwrap();
    e.dispatch(id, "image.auto_crop", json!({})).unwrap();
    let snap = e.snapshot(id).unwrap();
    assert_eq!((snap.state.width, snap.state.height), (1, 1));
    assert_eq!(snap.history.index, 1);
    e.dispatch(id, "edit.undo", json!({})).unwrap();
    assert_eq!(e.query(id, "doc.pixels", json!({})).unwrap(), pixels);
    e.dispatch(
        id,
        "image.resize",
        json!({"percent":50.0,"resampling":"nearest","gamma":false}),
    )
    .unwrap();
    let snap = e.snapshot(id).unwrap();
    assert_eq!((snap.state.width, snap.state.height), (2, 2));
    assert_eq!(snap.history.index, 1);
}

#[test]
fn selection_follows_canvas_and_rotation() {
    use op_core::selection::RasterMode;
    let e = Engine::new(None).unwrap();
    let id = e.import_pixels(2, 2, &[255; 16], None).unwrap();
    e.dispatch(id, "test.set_selection", json!({})).unwrap();
    let old = e
        .snapshot(id)
        .unwrap()
        .state
        .rasterize_selection(RasterMode::Pixelated);
    e.dispatch(
        id,
        "image.canvas_size",
        json!({"width":4,"height":4,"anchor":"bottom_right","fill":"transparent"}),
    )
    .unwrap();
    let new = e
        .snapshot(id)
        .unwrap()
        .state
        .rasterize_selection(RasterMode::Pixelated);
    assert_eq!(new.bytes[(2 * 4 + 2) as usize], old.bytes[0]);
    e.dispatch(id, "image.rotate_90_cw", json!({})).unwrap();
    let rotated = e
        .snapshot(id)
        .unwrap()
        .state
        .rasterize_selection(RasterMode::Pixelated);
    assert_eq!(rotated.bytes[(2 * 4 + 1) as usize], old.bytes[0]);
    e.dispatch(id, "edit.undo", json!({})).unwrap();
    assert_eq!(
        e.snapshot(id)
            .unwrap()
            .state
            .rasterize_selection(RasterMode::Pixelated)
            .bytes,
        new.bytes
    );
}

#[test]
fn crop_to_nonrectangular_selection_clears_outside_pixels() {
    let mut pixels = vec![0; 3 * 3 * 4];
    for y in 0..3 {
        for x in 0..3 {
            let i = (y * 3 + x) * 4;
            pixels[i..i + 4].copy_from_slice(if x == y {
                &[200, 0, 0, 255]
            } else {
                &[0, 0, 200, 255]
            });
        }
    }
    let e = Engine::new(None).unwrap();
    let id = e.import_pixels(3, 3, &pixels, None).unwrap();
    e.dispatch(
        id,
        "tool.magic_wand",
        json!({"x":0,"y":0,"contiguous":false,"tolerance":0.0,"antialias":false}),
    )
    .unwrap();
    e.dispatch(id, "image.crop_to_selection", json!({}))
        .unwrap();
    let out = e.query(id, "doc.pixels", json!({})).unwrap();
    assert_eq!(
        (
            e.snapshot(id).unwrap().state.width,
            e.snapshot(id).unwrap().state.height
        ),
        (3, 3)
    );
    for y in 0..3 {
        for x in 0..3 {
            let p = &out[(y * 3 + x) * 4..(y * 3 + x + 1) * 4];
            assert_eq!(
                p,
                if x == y {
                    &[200, 0, 0, 255]
                } else {
                    &[0, 0, 0, 0]
                }
            );
        }
    }
    e.dispatch(id, "edit.undo", json!({})).unwrap();
    assert_eq!(e.query(id, "doc.pixels", json!({})).unwrap(), pixels);
}

#[test]
fn each_image_command_is_one_undo_step_and_restores_source() {
    let pixels: Vec<u8> = (0..6)
        .flat_map(|i| [i as u8 + 10, 20, 30, if i == 0 { 0 } else { 255 }])
        .collect();
    let cases = [
        (
            "image.resize",
            json!({"width":4,"height":3,"maintain_aspect":false,"gamma":false,"resampling":"bicubic"}),
        ),
        (
            "image.canvas_size",
            json!({"width":4,"height":3,"anchor":"bottom_right","fill":"white"}),
        ),
        ("image.flip_horizontal", json!({})),
        ("image.flip_vertical", json!({})),
        ("image.rotate_90_cw", json!({})),
        ("image.rotate_90_ccw", json!({})),
        ("image.rotate_180", json!({})),
        ("image.flatten", json!({})),
        ("image.auto_crop", json!({})),
        ("image.crop_to_selection", json!({})),
    ];
    for (command, params) in cases {
        let e = Engine::new(None).unwrap();
        let id = e.import_pixels(3, 2, &pixels, None).unwrap();
        if command == "image.flatten" {
            e.dispatch(id, "layer.add", json!({"name":"Top"})).unwrap();
        }
        if command == "image.crop_to_selection" {
            e.dispatch(id, "test.set_selection", json!({})).unwrap();
        }
        let old = e.snapshot(id).unwrap();
        let old_pixels = e.query(id, "doc.pixels", json!({})).unwrap();
        e.dispatch(id, command, params).unwrap();
        assert_eq!(
            e.snapshot(id).unwrap().history.index,
            old.history.index + 1,
            "{command}"
        );
        e.dispatch(id, "edit.undo", json!({})).unwrap();
        let now = e.snapshot(id).unwrap();
        assert_eq!(
            (now.state.width, now.state.height),
            (old.state.width, old.state.height),
            "{command}"
        );
        assert_eq!(now.state.layers.len(), old.state.layers.len(), "{command}");
        assert_eq!(
            e.query(id, "doc.pixels", json!({})).unwrap(),
            old_pixels,
            "{command}"
        );
    }
}

#[test]
fn every_anchor_crops_expected_source_pixel() {
    let source: Vec<u8> = (0..9).flat_map(|i| [i as u8, 0, 0, 255]).collect();
    let anchors = [
        ("top_left", 0),
        ("top", 1),
        ("top_right", 2),
        ("left", 3),
        ("center", 4),
        ("right", 5),
        ("bottom_left", 6),
        ("bottom", 7),
        ("bottom_right", 8),
    ];
    for (anchor, want) in anchors {
        let e = Engine::new(None).unwrap();
        let id = e.import_pixels(3, 3, &source, None).unwrap();
        e.dispatch(
            id,
            "image.canvas_size",
            json!({"width":1,"height":1,"anchor":anchor,"fill":"white"}),
        )
        .unwrap();
        assert_eq!(
            e.query(id, "doc.pixels", json!({})).unwrap(),
            vec![want, 0, 0, 255],
            "{anchor}"
        );
    }
}

#[test]
fn maintain_aspect_uses_width_to_set_height() {
    let e = Engine::new(None).unwrap();
    let id = e.import_pixels(4, 2, &[255; 32], None).unwrap();
    e.dispatch(
        id,
        "image.resize",
        json!({"width":8,"height":9,"maintain_aspect":true,"resampling":"nearest"}),
    )
    .unwrap();
    let snap = e.snapshot(id).unwrap();
    assert_eq!((snap.state.width, snap.state.height), (8, 4));
}
