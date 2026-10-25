// Release review items 3, 10, 11 and 12a: every .opaint the writer makes can
// be read back (layer count, name length, stack.xml size, decoded size, finite
// selection points), crafted files past the limits are refused before any
// decode, and a save through a symlink keeps the link and the permissions.
use op_core::doc::{DocState, PolygonSet, SelectionSource, SelectionState};
use op_core::tiles::TileStore;
use op_formats::opaint::{
    MAX_LAYER_NAME_CHARS, MAX_LAYERS, MAX_STACK_XML, load_opaint, save_opaint,
};
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::sync::Arc;
use zip::write::SimpleFileOptions;
use zip::{ZipArchive, ZipWriter};

fn doc(dir: &tempfile::TempDir, w: u32, h: u32) -> DocState {
    let store = Arc::new(TileStore::with_default_budget(dir.path()).unwrap());
    DocState::new(store, w, h, 72.0).unwrap()
}

fn rewrite(path: &std::path::Path, change: impl Fn(&str, Vec<u8>) -> Vec<u8>) {
    let original = std::fs::read(path).unwrap();
    let mut old = ZipArchive::new(std::io::Cursor::new(original)).unwrap();
    let mut writer = ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for i in 0..old.len() {
        let mut entry = old.by_index(i).unwrap();
        let mut data = Vec::new();
        entry.read_to_end(&mut data).unwrap();
        let data = change(entry.name(), data);
        writer
            .start_file(
                entry.name(),
                SimpleFileOptions::default().compression_method(entry.compression()),
            )
            .unwrap();
        writer.write_all(&data).unwrap();
    }
    std::fs::write(path, writer.finish().unwrap().into_inner()).unwrap();
}

fn stack_xml(path: &std::path::Path) -> String {
    let mut zip = ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
    let mut s = String::new();
    zip.by_name("stack.xml")
        .unwrap()
        .read_to_string(&mut s)
        .unwrap();
    s
}

/// The reproduced case: a readable file whose layer name is 3,145,728
/// quotation marks in a single-quoted attribute. Escaped as `&quot;` it
/// would need about 18 MiB, past the reader's 16 MiB stack.xml limit.
#[test]
fn a_huge_quoted_layer_name_loads_cut_and_saves_a_readable_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("q.opaint");
    save_opaint(&doc(&dir, 2, 2), &path).unwrap();
    rewrite(&path, |name, data| {
        if name != "stack.xml" {
            return data;
        }
        let xml = String::from_utf8(data).unwrap();
        let quotes = "\"".repeat(3_145_728);
        xml.replace("name=\"Background\"", &format!("name='{quotes}'"))
            .into_bytes()
    });
    let loaded = load_opaint(&path).expect("the crafted file is readable");
    let name = &loaded.layers[0].name;
    assert_eq!(name.chars().count(), MAX_LAYER_NAME_CHARS);
    assert!(name.chars().all(|c| c == '"'));
    let again = dir.path().join("again.opaint");
    save_opaint(&loaded, &again).expect("the save works");
    assert!(stack_xml(&again).len() <= MAX_STACK_XML);
    assert_eq!(load_opaint(&again).unwrap().layers[0].name, *name);
}

#[test]
fn renaming_and_duplicating_cut_names_to_the_cap() {
    let dir = tempfile::tempdir().unwrap();
    let d = doc(&dir, 2, 2).set_layer_name(0, "&".repeat(5000)).unwrap();
    assert_eq!(d.layers[0].name.chars().count(), MAX_LAYER_NAME_CHARS);
    let d = d.duplicate_layer(0).unwrap();
    assert_eq!(d.layers[1].name.chars().count(), MAX_LAYER_NAME_CHARS);
    let d = d.add_layer(0, "é".repeat(2000)).unwrap();
    assert_eq!(d.layers[0].name.chars().count(), MAX_LAYER_NAME_CHARS);
}

#[test]
fn the_engine_makes_at_most_max_layers_and_the_writer_refuses_more() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = doc(&dir, 1, 1);
    for i in 1..MAX_LAYERS {
        d = d.add_layer(i, "").unwrap();
    }
    assert!(d.add_layer(0, "").is_err(), "no layer past the cap");
    assert!(d.duplicate_layer(0).is_err(), "no duplicate past the cap");
    let path = dir.path().join("max.opaint");
    save_opaint(&d, &path).unwrap();
    assert_eq!(load_opaint(&path).unwrap().layers.len(), MAX_LAYERS);
    // A state built by hand with one more is refused, and the old file stays.
    let before = std::fs::read(&path).unwrap();
    d.layers.push(d.layers[0].clone());
    let err = save_opaint(&d, &path).unwrap_err().to_string();
    assert!(err.contains("at most 1000"), "{err}");
    assert_eq!(std::fs::read(&path).unwrap(), before);
}

#[test]
fn a_crafted_file_with_too_many_layers_is_refused_before_decoding() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("many.opaint");
    save_opaint(&doc(&dir, 2, 2), &path).unwrap();
    rewrite(&path, |name, data| {
        if name != "stack.xml" {
            return data;
        }
        let xml = String::from_utf8(data).unwrap();
        let start = xml.find("<layer ").unwrap();
        let end = xml[start..].find("/>").unwrap() + start + 2;
        let layer = &xml[start..end];
        let many: String = (0..100_000)
            .map(|i| layer.replace("op:id=\"1\"", &format!("op:id=\"{}\"", i + 1)))
            .collect();
        format!("{}{}{}", &xml[..start], many, &xml[end..]).into_bytes()
    });
    let t = std::time::Instant::now();
    let err = load_opaint(&path).err().expect("refused").to_string();
    assert!(err.contains("at most 1000"), "{err}");
    assert!(t.elapsed() < std::time::Duration::from_secs(5));
}

#[test]
fn a_crafted_file_past_the_decoded_size_cap_is_refused_before_decoding() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("big.opaint");
    save_opaint(&doc(&dir, 2, 2), &path).unwrap();
    // 17 layers of 16384 x 16384 (1 GiB each) all naming the one tiny PNG.
    rewrite(&path, |name, data| {
        if name != "stack.xml" {
            return data;
        }
        let xml = String::from_utf8(data)
            .unwrap()
            .replace("w=\"2\" h=\"2\"", "w=\"16384\" h=\"16384\"");
        let start = xml.find("<layer ").unwrap();
        let end = xml[start..].find("/>").unwrap() + start + 2;
        let layer = &xml[start..end];
        let many: String = (0..17)
            .map(|i| layer.replace("op:id=\"1\"", &format!("op:id=\"{}\"", i + 1)))
            .collect();
        format!("{}{}{}", &xml[..start], many, &xml[end..]).into_bytes()
    });
    let err = load_opaint(&path).err().expect("refused").to_string();
    assert!(err.contains("16 GiB"), "{err}");
}

#[test]
fn a_non_finite_selection_point_is_refused_and_the_old_file_stays() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nan.opaint");
    let d = doc(&dir, 4, 4);
    save_opaint(&d, &path).unwrap();
    let before = std::fs::read(&path).unwrap();
    for bad in [f64::NAN, f64::INFINITY] {
        let s = d.set_selection(SelectionState::Active(SelectionSource::Geometry(Arc::new(
            PolygonSet(vec![vec![(0.0, 0.0), (bad, 1.0), (1.0, 1.0)]]),
        ))));
        let err = save_opaint(&s, &path).unwrap_err().to_string();
        assert!(err.contains("not a finite number"), "{err}");
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }
}

#[test]
fn a_selection_outline_too_long_for_stack_xml_is_saved_as_its_mask() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("lasso.opaint");
    // About 1.2 million points: well past 16 MiB of JSON.
    let ring: Vec<(f64, f64)> = (0..1_200_000)
        .map(|i| {
            let t = i as f64 / 1_200_000.0 * std::f64::consts::TAU;
            (
                16.0 + 15.123_456_789 * t.cos(),
                16.0 + 15.987_654_321 * t.sin(),
            )
        })
        .collect();
    let d = doc(&dir, 32, 32).set_selection(SelectionState::Active(SelectionSource::Geometry(
        Arc::new(PolygonSet(vec![ring])),
    )));
    save_opaint(&d, &path).unwrap();
    assert!(stack_xml(&path).len() <= MAX_STACK_XML);
    let back = load_opaint(&path).unwrap();
    assert!(matches!(
        back.selection,
        SelectionState::Active(SelectionSource::Coverage(_))
    ));
    let a = d.rasterize_selection(op_core::selection::RasterMode::Antialiased);
    let b = back.rasterize_selection(op_core::selection::RasterMode::Antialiased);
    assert_eq!(a.bytes, b.bytes, "the mask is the outline's");
}

#[test]
fn metadata_too_large_for_stack_xml_is_refused_with_a_clear_error() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("meta.opaint");
    let d = doc(&dir, 2, 2);
    save_opaint(&d, &path).unwrap();
    let before = std::fs::read(&path).unwrap();
    let d = d.set_metadata("note".into(), "\"".repeat(3_000_000));
    let err = save_opaint(&d, &path).unwrap_err().to_string();
    assert!(err.contains("at most 16777216"), "{err}");
    assert_eq!(std::fs::read(&path).unwrap(), before);
}

/// Round trip: random layer stacks (names with characters XML escapes, long
/// names, unicode), random shape selections. Every save either fails with an
/// error (and leaves the old file) or makes a file that loads back equal.
#[test]
fn every_document_the_writer_saves_loads_back() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("rt.opaint");
    let mut seed = 0x1234_5678_9abc_def0_u64;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let alphabet: Vec<char> = "\"'&<>a é\u{1F600}\n\t".chars().collect();
    for _ in 0..40 {
        let mut d = doc(&dir, 1 + (next() % 40) as u32, 1 + (next() % 40) as u32);
        let layers = 1 + (next() % 12) as usize;
        for i in 0..layers {
            if i > 0 {
                d = d.add_layer(i, "").unwrap();
            }
            let len = (next() % 3000) as usize;
            let name: String = (0..len)
                .map(|_| alphabet[(next() % alphabet.len() as u64) as usize])
                .collect();
            d = d.set_layer_name(i, name).unwrap();
        }
        if next() % 2 == 0 {
            let n = 3 + (next() % 50) as usize;
            let ring = (0..n)
                .map(|_| ((next() % 4000) as f64 / 97.0, (next() % 4000) as f64 / 89.0))
                .collect();
            d = d.set_selection(SelectionState::Active(SelectionSource::Geometry(Arc::new(
                PolygonSet(vec![ring]),
            ))));
        }
        match save_opaint(&d, &path) {
            Ok(()) => {
                let back = load_opaint(&path).expect("a saved file loads");
                let names =
                    |s: &DocState| s.layers.iter().map(|l| l.name.clone()).collect::<Vec<_>>();
                assert_eq!(names(&back), names(&d));
                // Points come back within one unit in the last place
                // (serde_json parses without its float_roundtrip feature).
                let points = |s: &DocState| match &s.selection {
                    SelectionState::Active(SelectionSource::Geometry(p)) => {
                        p.0.iter().flatten().copied().collect::<Vec<_>>()
                    }
                    SelectionState::None => Vec::new(),
                    _ => panic!("the selection kind changed"),
                };
                let (a, b) = (points(&back), points(&d));
                assert_eq!(a.len(), b.len());
                for ((ax, ay), (bx, by)) in a.iter().zip(&b) {
                    assert!((ax - bx).abs() < 1e-12 && (ay - by).abs() < 1e-12);
                }
            }
            Err(e) => panic!("a document within the limits failed to save: {e}"),
        }
    }
}

#[test]
fn a_save_through_a_symlink_replaces_the_target_and_keeps_its_permissions() {
    let dir = tempfile::tempdir().unwrap();
    let real = dir.path().join("real.opaint");
    let link = dir.path().join("link.opaint");
    let d = doc(&dir, 2, 2);
    save_opaint(&d, &real).unwrap();
    std::fs::set_permissions(&real, std::fs::Permissions::from_mode(0o600)).unwrap();
    std::os::unix::fs::symlink("real.opaint", &link).unwrap();
    let renamed = d.set_layer_name(0, "Through the link").unwrap();
    save_opaint(&renamed, &link).unwrap();
    assert!(
        std::fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink(),
        "the link stays a link"
    );
    assert_eq!(
        load_opaint(&real).unwrap().layers[0].name,
        "Through the link"
    );
    let mode = std::fs::metadata(&real).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600, "the target keeps its permissions");
}

/// Item 5: the kept image metadata is its own ZIP entry, not stack.xml text.
#[test]
fn kept_image_metadata_is_its_own_entry() {
    use op_formats::opaint::IMAGE_METADATA_KEY;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("m.opaint");
    let text = "\"&<".repeat(1_000_000);
    let d = doc(&dir, 2, 2).set_metadata(IMAGE_METADATA_KEY.into(), text.clone());
    save_opaint(&d, &path).unwrap();
    assert!(stack_xml(&path).len() < 4096);
    let mut zip = ZipArchive::new(std::fs::File::open(&path).unwrap()).unwrap();
    let mut back = String::new();
    zip.by_name("data/image-metadata.txt")
        .unwrap()
        .read_to_string(&mut back)
        .unwrap();
    assert_eq!(back, text);
    assert_eq!(
        load_opaint(&path).unwrap().metadata.get(IMAGE_METADATA_KEY),
        Some(&text)
    );
}
