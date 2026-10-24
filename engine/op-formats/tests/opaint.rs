use op_core::doc::{DocState, LayerBlendMode, PolygonSet, SelectionSource, SelectionState};
use op_core::tiles::{TILE_BYTES, TileCoord, TileData, TileStore};
use op_formats::opaint::{load_opaint, save_opaint};
use std::io::{Read, Write};
use std::sync::Arc;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

fn one_layer(dir: &tempfile::TempDir) -> (DocState, std::path::PathBuf) {
    let store = Arc::new(TileStore::with_default_budget(dir.path()).unwrap());
    let doc = DocState::new(store, 2, 2, 72.0).unwrap();
    let path = dir.path().join("test.opaint");
    save_opaint(&doc, &path).unwrap();
    (doc, path)
}

fn rewrite(path: &std::path::Path, change: impl Fn(&str, Vec<u8>) -> Option<(String, Vec<u8>)>) {
    let original = std::fs::read(path).unwrap();
    let mut old = ZipArchive::new(std::io::Cursor::new(original)).unwrap();
    let output = std::io::Cursor::new(Vec::new());
    let mut writer = ZipWriter::new(output);
    for i in 0..old.len() {
        let mut entry = old.by_index(i).unwrap();
        let mut data = Vec::new();
        entry.read_to_end(&mut data).unwrap();
        if let Some((name, data)) = change(entry.name(), data) {
            writer
                .start_file(
                    name,
                    SimpleFileOptions::default().compression_method(entry.compression()),
                )
                .unwrap();
            writer.write_all(&data).unwrap();
        }
    }
    std::fs::write(path, writer.finish().unwrap().into_inner()).unwrap();
}

#[test]
fn ora_entries_and_independent_reader() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(TileStore::with_default_budget(dir.path()).unwrap());
    let mut doc = DocState::new(store, 1, 1, 72.0).unwrap();
    doc = doc.add_layer(1, "Top").unwrap();
    doc = doc.set_layer_blend(0, LayerBlendMode::Multiply).unwrap();
    doc = doc.set_layer_blend(1, LayerBlendMode::Normal).unwrap();
    for (i, color) in [[120, 40, 200, 255], [50, 200, 100, 255]]
        .into_iter()
        .enumerate()
    {
        let mut tile = vec![0; TILE_BYTES];
        tile[..4].copy_from_slice(&color);
        doc = doc
            .write_tile(
                i,
                TileCoord { x: 0, y: 0 },
                TileData::try_from(tile).unwrap(),
            )
            .unwrap();
    }
    let path = dir.path().join("ora.opaint");
    save_opaint(&doc, &path).unwrap();
    let file = std::fs::read(&path).unwrap();
    assert_eq!(&file[..4], b"PK\x03\x04");
    assert_eq!(u16::from_le_bytes([file[8], file[9]]), 0); // first local header: STORED
    assert_eq!(&file[30..38], b"mimetype");
    let mut zip = ZipArchive::new(std::io::Cursor::new(file)).unwrap();
    assert_eq!(zip.by_index(0).unwrap().name(), "mimetype");
    assert_eq!(
        zip.by_index(0).unwrap().compression(),
        CompressionMethod::Stored
    );
    let mut mime = Vec::new();
    zip.by_index(0).unwrap().read_to_end(&mut mime).unwrap();
    assert_eq!(mime, b"image/openraster");
    let expected_names = [
        "mimetype",
        "stack.xml",
        "data/layer0.png",
        "data/layer1.png",
        "mergedimage.png",
        "Thumbnails/thumbnail.png",
    ];
    let names: Vec<_> = (0..zip.len())
        .map(|i| zip.by_index(i).unwrap().name().to_owned())
        .collect();
    assert_eq!(names, expected_names);
    let mut xml = String::new();
    zip.by_name("stack.xml")
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
    let mut reader = quick_xml::Reader::from_str(&xml);
    let mut sources = Vec::new();
    loop {
        match reader.read_event().unwrap() {
            quick_xml::events::Event::Empty(e) if e.name().as_ref() == b"layer" => {
                let src = e
                    .attributes()
                    .find(|a| a.as_ref().unwrap().key.as_ref() == b"src")
                    .unwrap()
                    .unwrap();
                sources.push(String::from_utf8(src.value.to_vec()).unwrap());
            }
            quick_xml::events::Event::Eof => break,
            _ => {}
        }
    }
    assert_eq!(sources, ["data/layer0.png", "data/layer1.png"]);
    // This reader uses only the ORA layer order and PNG entries, not load_opaint.
    let mut top = Vec::new();
    zip.by_name(&sources[0])
        .unwrap()
        .read_to_end(&mut top)
        .unwrap();
    let mut bottom = Vec::new();
    zip.by_name(&sources[1])
        .unwrap()
        .read_to_end(&mut bottom)
        .unwrap();
    let top = op_formats::png::decode_png(&top).unwrap();
    let bottom = op_formats::png::decode_png(&bottom).unwrap();
    let expected = [
        ((u16::from(top.pixels[0]) * u16::from(bottom.pixels[0]) + 127) / 255) as u8,
        ((u16::from(top.pixels[1]) * u16::from(bottom.pixels[1]) + 127) / 255) as u8,
        ((u16::from(top.pixels[2]) * u16::from(bottom.pixels[2]) + 127) / 255) as u8,
        255,
    ];
    let mut merged = Vec::new();
    zip.by_name("mergedimage.png")
        .unwrap()
        .read_to_end(&mut merged)
        .unwrap();
    assert_eq!(
        &op_formats::png::decode_png(&merged).unwrap().pixels[..4],
        &expected
    );
}

#[test]
fn standard_ora_modes_match_merged_image() {
    let dir = tempfile::tempdir().unwrap();
    let cases = [
        (LayerBlendMode::Normal, "svg:src-over"),
        (LayerBlendMode::Additive, "svg:plus"),
        (LayerBlendMode::Multiply, "svg:multiply"),
        (LayerBlendMode::Screen, "svg:screen"),
        (LayerBlendMode::Overlay, "svg:overlay"),
        (LayerBlendMode::Darken, "svg:darken"),
        (LayerBlendMode::Lighten, "svg:lighten"),
        (LayerBlendMode::ColorDodge, "svg:color-dodge"),
        (LayerBlendMode::ColorBurn, "svg:color-burn"),
        (LayerBlendMode::Difference, "svg:difference"),
    ];
    for (mode, expected_op) in cases {
        let store = Arc::new(TileStore::with_default_budget(dir.path()).unwrap());
        let mut doc = DocState::new(store, 1, 1, 72.0)
            .unwrap()
            .add_layer(0, "top")
            .unwrap();
        doc = doc.set_layer_blend(0, mode).unwrap();
        for (i, color) in [[102, 51, 204, 255], [153, 170, 85, 255]]
            .into_iter()
            .enumerate()
        {
            let mut tile = vec![0; TILE_BYTES];
            tile[..4].copy_from_slice(&color);
            doc = doc
                .write_tile(
                    i,
                    TileCoord { x: 0, y: 0 },
                    TileData::try_from(tile).unwrap(),
                )
                .unwrap();
        }
        let path = dir.path().join("mode.opaint");
        save_opaint(&doc, &path).unwrap();
        let mut zip = ZipArchive::new(std::fs::File::open(&path).unwrap()).unwrap();
        let mut xml = String::new();
        zip.by_name("stack.xml")
            .unwrap()
            .read_to_string(&mut xml)
            .unwrap();
        assert!(xml.contains(&format!("composite-op=\"{expected_op}\"")));
        let mut top_png = Vec::new();
        zip.by_name("data/layer0.png")
            .unwrap()
            .read_to_end(&mut top_png)
            .unwrap();
        let mut bottom_png = Vec::new();
        zip.by_name("data/layer1.png")
            .unwrap()
            .read_to_end(&mut bottom_png)
            .unwrap();
        let top = op_formats::png::decode_png(&top_png).unwrap();
        let bottom = op_formats::png::decode_png(&bottom_png).unwrap();
        let mut expected = [0; 4];
        for (c, out) in expected.iter_mut().enumerate().take(3) {
            let s = f32::from(top.pixels[c]) / 255.0;
            let d = f32::from(bottom.pixels[c]) / 255.0;
            let value = match expected_op {
                "svg:multiply" => d * s,
                "svg:plus" => (d + s).min(1.0),
                "svg:screen" => d + s - d * s,
                "svg:overlay" => {
                    if d <= 0.5 {
                        2.0 * d * s
                    } else {
                        1.0 - 2.0 * (1.0 - d) * (1.0 - s)
                    }
                }
                "svg:darken" => d.min(s),
                "svg:lighten" => d.max(s),
                "svg:color-dodge" => {
                    if s == 1.0 {
                        1.0
                    } else {
                        (d / (1.0 - s)).min(1.0)
                    }
                }
                "svg:color-burn" => {
                    if s == 0.0 {
                        0.0
                    } else {
                        1.0 - ((1.0 - d) / s).min(1.0)
                    }
                }
                "svg:difference" => (d - s).abs(),
                _ => s,
            };
            *out = (value * 255.0).round() as u8;
        }
        expected[3] = 255;
        let mut merged = Vec::new();
        zip.by_name("mergedimage.png")
            .unwrap()
            .read_to_end(&mut merged)
            .unwrap();
        assert_eq!(
            &op_formats::png::decode_png(&merged).unwrap().pixels[..4],
            &expected,
            "{expected_op}"
        );
    }
}

#[test]
fn coverage_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let (doc, path) = one_layer(&dir);
    let mask = op_core::selection::MaskGrid::new(2, 2, vec![0, 127, 255, 50]).unwrap();
    let doc = doc.set_selection(SelectionState::Active(SelectionSource::Coverage(Arc::new(
        mask,
    ))));
    save_opaint(&doc, &path).unwrap();
    assert_eq!(load_opaint(&path).unwrap().selection, doc.selection);
}

#[test]
fn malformed_archives_return_errors() {
    let dir = tempfile::tempdir().unwrap();
    let (_, path) = one_layer(&dir);
    let original = std::fs::read(&path).unwrap();
    for (case, edit) in [
        ("missing mimetype", "drop_mime"),
        ("wrong mimetype", "bad_mime"),
        ("malformed XML", "bad_xml"),
        ("future major", "major"),
        ("PNG mismatch", "size"),
        ("path traversal", "traversal"),
        ("absolute path", "absolute"),
        ("missing merged image", "drop_merged"),
    ] {
        std::fs::write(&path, &original).unwrap();
        rewrite(&path, |name, bytes| match edit {
            "drop_mime" if name == "mimetype" => None,
            "bad_mime" if name == "mimetype" => Some((name.into(), b"bad".to_vec())),
            "bad_xml" if name == "stack.xml" => Some((name.into(), b"<image><stack>".to_vec())),
            "major" if name == "stack.xml" => Some((
                name.into(),
                String::from_utf8(bytes)
                    .unwrap()
                    .replace("op:version=\"1.0\"", "op:version=\"2.0\"")
                    .into_bytes(),
            )),
            "size" if name == "stack.xml" => Some((
                name.into(),
                String::from_utf8(bytes)
                    .unwrap()
                    .replace("w=\"2\"", "w=\"3\"")
                    .into_bytes(),
            )),
            "traversal" if name == "data/layer0.png" => Some(("../layer0.png".into(), bytes)),
            "absolute" if name == "data/layer0.png" => Some(("/layer0.png".into(), bytes)),
            "drop_merged" if name == "mergedimage.png" => None,
            _ => Some((name.into(), bytes)),
        });
        assert!(load_opaint(&path).is_err(), "{case}");
    }
    std::fs::write(&path, &original).unwrap();
    let mut bytes = original;
    let marker = b"PK\x01\x02";
    let pos = bytes.windows(4).position(|w| w == marker).unwrap();
    bytes[pos + 24..pos + 28].copy_from_slice(&u32::MAX.to_le_bytes());
    std::fs::write(&path, bytes).unwrap();
    assert!(load_opaint(&path).is_err(), "zip bomb");
}

#[test]
fn future_minor_has_notice() {
    let dir = tempfile::tempdir().unwrap();
    let (_, path) = one_layer(&dir);
    rewrite(&path, |name, bytes| {
        if name == "stack.xml" {
            Some((
                name.into(),
                String::from_utf8(bytes)
                    .unwrap()
                    .replace("op:version=\"1.0\"", "op:version=\"1.1\"")
                    .into_bytes(),
            ))
        } else {
            Some((name.into(), bytes))
        }
    });
    assert!(
        op_formats::opaint::load_opaint_with_notice(&path)
            .unwrap()
            .notice
            .is_some()
    );
}

#[test]
fn duplicate_entry_path_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let (_, path) = one_layer(&dir);
    let mut bytes = std::fs::read(&path).unwrap();
    let mut archive = ZipWriter::new_append(std::io::Cursor::new(&mut bytes)).unwrap();
    archive
        .start_file("stack.xmp", SimpleFileOptions::default())
        .unwrap();
    archive.write_all(b"duplicate").unwrap();
    archive.finish().unwrap();
    for i in 0..bytes.len() - 9 {
        if &bytes[i..i + 9] == b"stack.xmp" {
            bytes[i..i + 9].copy_from_slice(b"stack.xml");
        }
    }
    std::fs::write(&path, bytes).unwrap();
    assert!(load_opaint(&path).is_err());
}

#[test]
fn full_state_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    for count in [1, 3, 20] {
        let store = Arc::new(TileStore::with_default_budget(dir.path()).unwrap());
        let mut doc = DocState::new(store, 257, 259, 144.5).unwrap();
        for i in 1..count {
            doc = doc.add_layer(i, format!("Layer {i} & <>\"")).unwrap();
        }
        let modes = [
            LayerBlendMode::Normal,
            LayerBlendMode::Multiply,
            LayerBlendMode::Additive,
            LayerBlendMode::ColorBurn,
            LayerBlendMode::ColorDodge,
            LayerBlendMode::Reflect,
            LayerBlendMode::Glow,
            LayerBlendMode::Overlay,
            LayerBlendMode::Difference,
            LayerBlendMode::Negation,
            LayerBlendMode::Lighten,
            LayerBlendMode::Darken,
            LayerBlendMode::Screen,
            LayerBlendMode::Xor,
        ];
        for i in 0..count {
            doc = doc.set_layer_blend(i, modes[i % modes.len()]).unwrap();
            doc = doc.set_layer_opacity(i, (i * 11) as u8).unwrap();
            doc = doc.set_layer_visible(i, i % 4 != 0).unwrap();
            let mut bytes = vec![0; TILE_BYTES];
            bytes[..4].copy_from_slice(&[i as u8, 32, 77, 0]);
            bytes[4..8].copy_from_slice(&[200, i as u8, 5, 123]);
            doc = doc
                .write_tile(
                    i,
                    TileCoord { x: 1, y: 1 },
                    TileData::try_from(bytes).unwrap(),
                )
                .unwrap();
        }
        doc = doc.set_active_layer(count - 1).unwrap();
        doc = doc.set_profile(Some(Arc::from(&b"icc-bytes"[..])));
        doc = doc.set_metadata("author".into(), "A & B".into());
        doc = doc.set_selection(SelectionState::Active(SelectionSource::Geometry(Arc::new(
            PolygonSet::rectangle(0.5, 1.0, 22.0, 30.0),
        ))));
        let path = dir.path().join(format!("{count}.opaint"));
        save_opaint(&doc, &path).unwrap();
        let mut archive = ZipArchive::new(std::fs::File::open(&path).unwrap()).unwrap();
        assert!(archive.by_name("data/profile.icc").is_ok());
        let mut thumb_png = Vec::new();
        archive
            .by_name("Thumbnails/thumbnail.png")
            .unwrap()
            .read_to_end(&mut thumb_png)
            .unwrap();
        let thumb = op_formats::png::decode_png(&thumb_png).unwrap();
        assert!(thumb.width <= 256 && thumb.height <= 256);
        let loaded = load_opaint(&path).unwrap();
        assert_eq!(
            (loaded.width, loaded.height, loaded.dpi),
            (doc.width, doc.height, doc.dpi)
        );
        assert_eq!(loaded.profile, doc.profile);
        assert_eq!(loaded.metadata, doc.metadata);
        assert_eq!(loaded.active_layer, doc.active_layer);
        assert_eq!(loaded.selection, doc.selection);
        for i in 0..count {
            assert_eq!(loaded.layers[i].id, doc.layers[i].id);
            assert_eq!(loaded.layers[i].name, doc.layers[i].name);
            assert_eq!(loaded.layers[i].blend, doc.layers[i].blend);
            assert_eq!(loaded.layers[i].visible, doc.layers[i].visible);
            assert_eq!(loaded.layers[i].opacity, doc.layers[i].opacity);
            for y in 0..2 {
                for x in 0..2 {
                    let c = TileCoord { x, y };
                    assert_eq!(
                        loaded.read_tile(i, c).unwrap(),
                        doc.read_tile(i, c).unwrap()
                    );
                }
            }
        }
    }
}

#[test]
fn loaded_sparse_layer_ids_are_never_reused() {
    // Layers with ids 1, 7 and 9 (others deleted). After loading, new layers
    // must get ids above 9, never an id already in the document.
    let dir = tempfile::tempdir().unwrap();
    let (mut doc, path) = one_layer(&dir);
    for i in 1..9 {
        doc = doc.add_layer(i, format!("l{i}")).unwrap();
    }
    let keep = [1u64, 7, 9];
    let mut i = 0;
    while i < doc.layers.len() {
        if keep.contains(&doc.layers[i].id.0) {
            i += 1;
        } else {
            doc = doc.delete_layer(i).unwrap();
        }
    }
    let ids: Vec<u64> = doc.layers.iter().map(|l| l.id.0).collect();
    assert_eq!(ids, keep);
    save_opaint(&doc, &path).unwrap();
    let mut loaded = load_opaint(&path).unwrap();
    for n in 0..4 {
        loaded = loaded
            .add_layer(loaded.layers.len(), format!("new{n}"))
            .unwrap();
        let new_id = loaded.layers.last().unwrap().id.0;
        assert!(
            new_id > 9,
            "new layer reused or went below a loaded id: {new_id}"
        );
    }
}
