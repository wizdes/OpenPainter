//! M19 document lifecycle: New, open and save .opaint, PNG export, the saved
//! revision marker, and replacing a document's content (Revert).
use op_core::doc::{DocState, LayerBlendMode};
use op_core::tiles::{TILE_BYTES, TILE_SIZE, TileCoord, TileData, TileStore};
use op_engine::{Engine, Error, SaveFormat};
use serde_json::{Value, json};
use std::sync::Arc;

fn info(e: &Engine, id: u64) -> Value {
    serde_json::from_slice(&e.query(id, "doc.info", json!({})).unwrap()).unwrap()
}
fn dirty(e: &Engine, id: u64) -> bool {
    info(e, id)["dirty"].as_bool().unwrap()
}
fn revision(e: &Engine, id: u64) -> u64 {
    info(e, id)["revision"].as_u64().unwrap()
}

#[test]
fn new_document_fills_the_background_and_keeps_the_resolution() {
    let e = Engine::new(None).unwrap();
    for (color, label) in [
        ([255, 255, 255, 255], "white"),
        ([12, 34, 56, 255], "secondary"),
        ([0, 0, 0, 0], "transparent"),
    ] {
        let id = e.new_document(5, 3, 300.0, color).unwrap();
        let i = info(&e, id);
        assert_eq!(
            (i["width"].as_u64(), i["height"].as_u64()),
            (Some(5), Some(3))
        );
        assert_eq!(i["layers"].as_array().unwrap().len(), 1, "{label}");
        assert_eq!(i["layers"][0]["name"], "Background");
        assert!(!dirty(&e, id), "a new image is not modified");
        let px = e.query(id, "doc.pixels", json!({})).unwrap();
        assert_eq!(px.len(), 5 * 3 * 4);
        assert!(px.chunks_exact(4).all(|p| p == color), "{label}");
        let res: Value =
            serde_json::from_slice(&e.query(id, "doc.resolution", json!({})).unwrap()).unwrap();
        assert_eq!(res["dpi"], 300.0);
        e.close(id).unwrap();
    }
    assert!(e.new_document(0, 3, 96.0, [0; 4]).is_err());
    assert!(e.new_document(3, 3, 0.0, [0; 4]).is_err());
}

/// Five layers with their own pixels, names, opacity, blend mode and
/// visibility, plus a Display P3 profile.
fn five_layers(store: Arc<TileStore>) -> DocState {
    let w = TILE_SIZE as u32 + 7;
    let h = 9;
    let mut doc = DocState::new(store, w, h, 240.0).unwrap();
    let modes = [
        LayerBlendMode::Normal,
        LayerBlendMode::Multiply,
        LayerBlendMode::Screen,
        LayerBlendMode::Overlay,
        LayerBlendMode::Xor,
    ];
    for (i, &mode) in modes.iter().enumerate() {
        if i > 0 {
            doc = doc.add_layer(i, format!("Layer {i}")).unwrap();
        }
        for tx in 0..2 {
            let mut tile = vec![0; TILE_BYTES];
            for (n, p) in tile.chunks_exact_mut(4).enumerate() {
                p.copy_from_slice(&[
                    (n * 7 + i * 31) as u8,
                    (n * 3 + tx * 50) as u8,
                    (i * 40) as u8,
                    ((n + i) % 256) as u8,
                ]);
            }
            doc = doc
                .write_tile(
                    i,
                    TileCoord { x: tx as i32, y: 0 },
                    TileData::try_from(tile).unwrap(),
                )
                .unwrap();
        }
        doc = doc.set_layer_name(i, format!("Named “{i}” & <b>")).unwrap();
        doc = doc.set_layer_opacity(i, 50 + i as u8 * 40).unwrap();
        doc = doc.set_layer_blend(i, mode).unwrap();
        doc = doc.set_layer_visible(i, i != 2).unwrap();
    }
    let p3 = op_color::Profile::builtin("display-p3").unwrap().bytes();
    doc.set_profile(Some(p3))
}

fn layer_pixels(doc: &DocState, i: usize) -> Vec<u8> {
    let mut out = Vec::new();
    for tx in 0..doc.width.div_ceil(TILE_SIZE as u32) {
        let t = doc.read_tile(i, TileCoord { x: tx as i32, y: 0 }).unwrap();
        out.extend_from_slice(&t[..]);
    }
    out
}

#[test]
fn opaint_round_trip_keeps_five_layers_and_the_profile() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(TileStore::with_default_budget(dir.path()).unwrap());
    let original = five_layers(store.clone());
    let first = dir.path().join("first.opaint");
    op_formats::opaint::save_opaint(&original, &first).unwrap();

    let e = Engine::new(None).unwrap();
    let id = e.open_opaint(&first).unwrap();
    let i = info(&e, id);
    assert_eq!(i["layers"].as_array().unwrap().len(), 5);
    assert_eq!(i["profile_embedded"], true);
    assert!(!dirty(&e, id));
    let second = dir.path().join("second.opaint");
    e.save_document(id, &second, SaveFormat::Opaint, true)
        .unwrap();
    let back = op_formats::opaint::load_opaint_with_store(&second, store)
        .unwrap()
        .doc;
    assert_eq!(
        (back.width, back.height, back.dpi),
        (original.width, original.height, 240.0)
    );
    assert_eq!(back.profile, original.profile, "profile bytes kept");
    assert_eq!(back.layers.len(), 5);
    for n in 0..5 {
        let (a, b) = (&original.layers[n], &back.layers[n]);
        assert_eq!(a.name, b.name, "name {n}");
        assert_eq!(a.opacity, b.opacity, "opacity {n}");
        assert_eq!(a.blend, b.blend, "blend {n}");
        assert_eq!(a.visible, b.visible, "visibility {n}");
        assert_eq!(
            layer_pixels(&original, n),
            layer_pixels(&back, n),
            "pixels {n}"
        );
    }
    e.close(id).unwrap();
}

#[test]
fn opening_a_bad_opaint_fails_without_a_document() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bad.opaint");
    std::fs::write(&path, b"not a zip").unwrap();
    let e = Engine::new(None).unwrap();
    assert!(matches!(e.open_opaint(&path), Err(Error::BadArgument(_))));
    assert!(e.open_opaint(dir.path().join("missing.opaint")).is_err());
}

#[test]
fn png_export_is_the_flattened_image_with_the_profile() {
    let dir = tempfile::tempdir().unwrap();
    let e = Engine::new(None).unwrap();
    let p3 = op_color::Profile::builtin("display-p3").unwrap().bytes();
    let id = e
        .import_pixels(2, 1, &[200, 10, 10, 255, 0, 0, 0, 0], Some(&p3))
        .unwrap();
    e.dispatch(id, "layer.add", json!({"name": "Top"})).unwrap();
    e.dispatch(id, "effect.adjust.invert_colors", json!({}))
        .unwrap();
    let path = dir.path().join("flat.png");
    e.save_document(id, &path, SaveFormat::Png, true).unwrap();
    let decoded = op_formats::png::decode_png(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!((decoded.width, decoded.height), (2, 1));
    assert_eq!(
        decoded.pixels,
        e.query(id, "doc.pixels", json!({})).unwrap()
    );
    assert_eq!(decoded.icc.as_deref(), Some(&p3[..]));
    assert_eq!(
        info(&e, id)["layers"].as_array().unwrap().len(),
        2,
        "the document stays layered"
    );
    assert!(!dirty(&e, id));
    // No temporary files are left next to the image.
    let names: Vec<_> = std::fs::read_dir(dir.path()).unwrap().collect();
    assert_eq!(names.len(), 1);
}

#[test]
fn saved_marker_follows_undo_redo_and_new_edits() {
    let dir = tempfile::tempdir().unwrap();
    let e = Engine::new(None).unwrap();
    let id = e.new_document(4, 4, 96.0, [255; 4]).unwrap();
    let path = dir.path().join("m.opaint");
    assert!(!dirty(&e, id));
    e.dispatch(id, "image.invert_colors", json!({})).unwrap();
    assert!(dirty(&e, id), "an edit sets the mark");
    e.save_document(id, &path, SaveFormat::Opaint, true)
        .unwrap();
    assert!(!dirty(&e, id), "save clears it");
    e.dispatch(id, "edit.undo", json!({})).unwrap();
    assert!(dirty(&e, id), "undo past the saved step sets it");
    e.dispatch(id, "edit.redo", json!({})).unwrap();
    assert!(!dirty(&e, id), "redo back to the saved step clears it");
    e.dispatch(id, "layer.add", json!({})).unwrap();
    assert!(dirty(&e, id));
    e.dispatch(id, "edit.undo", json!({})).unwrap();
    assert!(!dirty(&e, id), "undo back to the saved step clears it");
    e.dispatch(id, "edit.undo", json!({})).unwrap();
    e.dispatch(id, "layer.add", json!({})).unwrap();
    assert!(dirty(&e, id), "a new edit after undo sets it");
    e.dispatch(id, "edit.undo", json!({})).unwrap();
    assert!(dirty(&e, id), "step 0 was never the saved step");
    // The saved step was cut off by the new edit: no step equals it any more.
    for _ in 0..3 {
        e.dispatch(id, "edit.redo", json!({})).unwrap();
        assert!(dirty(&e, id));
    }
    // Save a Copy writes the file but leaves the mark.
    let copy = dir.path().join("copy.opaint");
    e.save_document(id, &copy, SaveFormat::Opaint, false)
        .unwrap();
    assert!(copy.exists());
    assert!(dirty(&e, id), "save a copy does not clear the mark");
    e.save_document(id, &path, SaveFormat::Opaint, true)
        .unwrap();
    assert!(!dirty(&e, id));
}

#[test]
fn mark_saved_needs_the_current_revision() {
    let e = Engine::new(None).unwrap();
    let id = e.new_document(2, 2, 96.0, [255; 4]).unwrap();
    e.dispatch(id, "image.invert_colors", json!({})).unwrap();
    let r = revision(&e, id);
    e.dispatch(id, "image.invert_colors", json!({})).unwrap();
    assert_eq!(
        e.mark_saved(id, r),
        Err(Error::Busy),
        "the document changed after the read"
    );
    assert!(dirty(&e, id));
    e.mark_saved(id, revision(&e, id)).unwrap();
    assert!(!dirty(&e, id));
}

#[test]
fn replace_content_resets_history_and_keeps_the_document_id() {
    let e = Engine::new(None).unwrap();
    let id = e.new_document(2, 2, 96.0, [255; 4]).unwrap();
    e.dispatch(id, "image.invert_colors", json!({})).unwrap();
    e.dispatch(id, "layer.add", json!({})).unwrap();
    let before = revision(&e, id);
    let source = e
        .import_pixels(3, 1, &[1, 2, 3, 255, 4, 5, 6, 255, 7, 8, 9, 255], None)
        .unwrap();
    e.replace_content(id, source).unwrap();
    let i = info(&e, id);
    assert_eq!(
        (i["width"].as_u64(), i["height"].as_u64()),
        (Some(3), Some(1))
    );
    assert_eq!(i["history_labels"].as_array().unwrap().len(), 1);
    assert_eq!(i["layers"].as_array().unwrap().len(), 1);
    assert!(!dirty(&e, id));
    assert!(revision(&e, id) > before);
    assert_eq!(
        e.query(id, "doc.pixels", json!({})).unwrap(),
        vec![1, 2, 3, 255, 4, 5, 6, 255, 7, 8, 9, 255]
    );
    // The source is still open and unchanged; the caller closes it.
    e.close(source).unwrap();
    assert_eq!(e.query(id, "doc.pixels", json!({})).unwrap().len(), 12);
    assert_eq!(e.replace_content(id, 999), Err(Error::NoSuchDoc));
}
