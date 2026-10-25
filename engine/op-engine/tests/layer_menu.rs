use op_color::{Profile, RenderingIntent, convert_rgba};
use op_core::tiles::{TILE_SIZE, TileCoord};
use op_engine::Engine;
use op_formats::{Decoded, Format, Metadata, Options, encode};
use serde_json::json;
use std::path::Path;

fn layer_pixels(e: &Engine, id: u64, index: usize) -> Vec<u8> {
    let s = e.snapshot(id).unwrap().state.clone();
    let mut out = vec![0; s.width as usize * s.height as usize * 4];
    for y in 0..s.height as usize {
        let coord_y = y / TILE_SIZE;
        for x in 0..s.width as usize {
            let tile = s
                .read_tile(
                    index,
                    TileCoord {
                        x: (x / TILE_SIZE) as i32,
                        y: coord_y as i32,
                    },
                )
                .unwrap();
            let src = ((y % TILE_SIZE) * TILE_SIZE + x % TILE_SIZE) * 4;
            out[(y * s.width as usize + x) * 4..][..4].copy_from_slice(&tile[src..src + 4]);
        }
    }
    out
}

fn png(path: &Path, w: u32, h: u32, pixels: Vec<u8>, icc: Option<Vec<u8>>) {
    let image = Decoded {
        width: w,
        height: h,
        rgba8: pixels,
        icc,
        metadata: Metadata::default(),
        notices: vec![],
    };
    std::fs::write(
        path,
        encode(Format::Png, &image, &Options::default()).unwrap(),
    )
    .unwrap();
}

#[test]
fn transforms_match_image_commands_and_undo() {
    let (w, h) = (263, 258);
    let pixels: Vec<u8> = (0..w * h)
        .flat_map(|i| [i as u8, (i / 7) as u8, (i / 13) as u8, 255])
        .collect();
    for (layer_cmd, image_cmd, label) in [
        (
            "layer.flip_horizontal",
            "image.flip_horizontal",
            "Flip Layer Horizontal",
        ),
        (
            "layer.flip_vertical",
            "image.flip_vertical",
            "Flip Layer Vertical",
        ),
        ("layer.rotate_180", "image.rotate_180", "Rotate Layer 180°"),
    ] {
        let e = Engine::new(None).unwrap();
        let id = e.import_pixels(w, h, &pixels, None).unwrap();
        let only = e.import_pixels(w, h, &pixels, None).unwrap();
        e.dispatch(id, "layer.duplicate", json!({})).unwrap();
        e.dispatch(id, "test.set_selection", json!({})).unwrap();
        let selection = e.snapshot(id).unwrap().state.selection.clone();
        let before = layer_pixels(&e, id, 1);
        let lower = layer_pixels(&e, id, 0);
        let index = e.snapshot(id).unwrap().history.index;
        e.dispatch(id, layer_cmd, json!({})).unwrap();
        e.dispatch(only, image_cmd, json!({})).unwrap();
        assert_eq!(
            layer_pixels(&e, id, 1),
            layer_pixels(&e, only, 0),
            "{layer_cmd}"
        );
        assert_eq!(layer_pixels(&e, id, 0), lower);
        assert_eq!(e.snapshot(id).unwrap().state.selection, selection);
        let snap = e.snapshot(id).unwrap();
        assert_eq!(snap.history.index, index + 1);
        assert_eq!(snap.history.labels.last().unwrap(), label);
        e.dispatch(id, "edit.undo", json!({})).unwrap();
        assert_eq!(layer_pixels(&e, id, 1), before);
    }
}

#[test]
fn imports_embedded_profile_and_undoes_once() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("adobe.png");
    let source = [55, 120, 210, 255, 200, 80, 35, 128];
    let adobe = Profile::builtin("adobe-rgb").unwrap();
    let srgb = Profile::builtin("srgb").unwrap();
    png(&path, 2, 1, source.to_vec(), Some(adobe.bytes().to_vec()));
    let e = Engine::new(None).unwrap();
    let id = e
        .import_pixels(3, 2, &[0; 24], Some(&srgb.bytes()))
        .unwrap();
    let before = e.snapshot(id).unwrap();
    e.dispatch(id, "layer.import_from_file", json!({"path":path}))
        .unwrap();
    let s = e.snapshot(id).unwrap();
    assert_eq!(s.state.layers.len(), 2);
    assert_eq!(s.state.active_layer, 1);
    assert_eq!(s.state.layers[1].name, "adobe.png");
    assert_eq!(s.history.index, before.history.index + 1);
    let mut expected = source;
    convert_rgba(
        &adobe,
        &srgb,
        RenderingIntent::Perceptual,
        true,
        &mut expected,
    )
    .unwrap();
    for (got, want) in layer_pixels(&e, id, 1)[..8].iter().zip(expected) {
        assert!(got.abs_diff(want) <= 1, "{got} vs {want}");
    }
    e.dispatch(id, "edit.undo", json!({})).unwrap();
    assert_eq!(e.snapshot(id).unwrap().state.layers.len(), 1);
}

#[test]
fn imports_no_profile_and_preserves_large_image_outside_canvas() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("plain.png");
    let pixels: Vec<u8> = (0..260 * 3).flat_map(|i| [i as u8, 30, 40, 255]).collect();
    png(&path, 260, 3, pixels.clone(), None);
    let e = Engine::new(None).unwrap();
    let id = e.import_pixels(2, 2, &[0; 16], None).unwrap();
    e.dispatch(id, "layer.import_from_file", json!({"path":path}))
        .unwrap();
    let s = e.snapshot(id).unwrap().state.clone();
    assert_eq!((s.width, s.height), (2, 2));
    assert_eq!(&layer_pixels(&e, id, 1)[..4], &pixels[..4]);
    assert!(s.layers[1].tiles.get(TileCoord { x: 1, y: 0 }).is_some());
    let mut expanded = s.clone();
    expanded.width = 260;
    expanded.height = 3;
    let far = expanded.read_tile(1, TileCoord { x: 1, y: 0 }).unwrap();
    assert_eq!(&far[..4], &pixels[256 * 4..256 * 4 + 4]);
    let bottom = expanded.read_tile(1, TileCoord { x: 0, y: 0 }).unwrap();
    assert_eq!(
        &bottom[(2 * TILE_SIZE + 3) * 4..][..4],
        &pixels[(2 * 260 + 3) * 4..][..4]
    );
    e.dispatch(id, "layer.flip_horizontal", json!({})).unwrap();
    let mut flipped = e.snapshot(id).unwrap().state.clone();
    flipped.width = 260;
    flipped.height = 3;
    let far = flipped.read_tile(1, TileCoord { x: 1, y: 0 }).unwrap();
    assert_eq!(&far[..4], &pixels[256 * 4..256 * 4 + 4]);
    let bottom = flipped.read_tile(1, TileCoord { x: 0, y: 0 }).unwrap();
    assert_eq!(
        &bottom[(2 * TILE_SIZE + 3) * 4..][..4],
        &pixels[(2 * 260 + 3) * 4..][..4]
    );
    let small = dir.path().join("small.png");
    png(&small, 1, 1, vec![1, 2, 3, 255], None);
    e.dispatch(id, "layer.import_from_file", json!({"path":small}))
        .unwrap();
    assert_eq!(e.snapshot(id).unwrap().state.layers[2].name, "small.png");
    assert_eq!(&layer_pixels(&e, id, 2)[..4], &[1, 2, 3, 255]);
    assert_eq!(&layer_pixels(&e, id, 2)[4..8], &[0, 0, 0, 0]);

    let adobe = Profile::builtin("adobe-rgb").unwrap();
    let id = e
        .import_pixels(1, 1, &[0; 4], Some(&adobe.bytes()))
        .unwrap();
    e.dispatch(id, "layer.import_from_file", json!({"path":small}))
        .unwrap();
    let mut expected = [1, 2, 3, 255];
    convert_rgba(
        &Profile::builtin("srgb").unwrap(),
        &adobe,
        RenderingIntent::Perceptual,
        true,
        &mut expected,
    )
    .unwrap();
    for (got, want) in layer_pixels(&e, id, 1).iter().zip(expected) {
        assert!(got.abs_diff(want) <= 1, "{got} vs {want}");
    }
}

#[test]
fn unreadable_import_changes_nothing() {
    let e = Engine::new(None).unwrap();
    let id = e.import_pixels(1, 1, &[1, 2, 3, 255], None).unwrap();
    let before = e.snapshot(id).unwrap();
    let err = e
        .dispatch(
            id,
            "layer.import_from_file",
            json!({"path":"/no/such/image.png"}),
        )
        .unwrap_err();
    assert!(err.to_string().contains("image.png"));
    let after = e.snapshot(id).unwrap();
    assert_eq!(after.history.index, before.history.index);
    assert_eq!(after.state.layers.len(), before.state.layers.len());
    assert_eq!(layer_pixels(&e, id, 0), [1, 2, 3, 255]);
    let path = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(path.path(), b"not an image").unwrap();
    let err = e
        .dispatch(id, "layer.import_from_file", json!({"path":path.path()}))
        .unwrap_err();
    assert!(err.to_string().contains("unknown image format"));
    assert_eq!(e.snapshot(id).unwrap().history.index, before.history.index);
}
