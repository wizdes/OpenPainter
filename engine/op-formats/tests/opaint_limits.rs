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
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::sync::Arc;
use zip::write::SimpleFileOptions;
use zip::{ZipArchive, ZipWriter};

#[cfg(windows)]
#[test]
fn windows_saves_replace_an_existing_opaint_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("repeat.opaint");
    let first = doc(&dir, 2, 2);
    save_opaint(&first, &path).unwrap();
    let second = first.set_layer_name(0, "Second save").unwrap();
    save_opaint(&second, &path).unwrap();
    assert_eq!(load_opaint(&path).unwrap().layers[0].name, "Second save");
}

/// A named stream on `path`. A replace that makes a new file would lose it.
#[cfg(windows)]
fn stream_of(path: &std::path::Path) -> std::path::PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(":note");
    name.into()
}

#[cfg(windows)]
fn acl_of(path: &std::path::Path) -> String {
    let out = std::process::Command::new("icacls")
        .arg(path)
        .output()
        .unwrap();
    assert!(out.status.success());
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[cfg(windows)]
fn no_temp_left(dir: &std::path::Path) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let name = entry.unwrap().file_name().to_string_lossy().into_owned();
        assert!(
            !name.ends_with(".tmp") && !name.ends_with(".old"),
            "left behind: {name}"
        );
    }
}

#[cfg(windows)]
#[test]
fn windows_a_save_keeps_access_rules_streams_and_creation_time() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("private.opaint");
    let first = doc(&dir, 2, 2);
    save_opaint(&first, &path).unwrap();
    let user = std::env::var("USERNAME").unwrap();
    let status = std::process::Command::new("icacls")
        .arg(&path)
        .args(["/inheritance:r", "/grant:r", &format!("{user}:F")])
        .output()
        .unwrap();
    assert!(status.status.success());
    std::fs::write(stream_of(&path), b"kept").unwrap();
    let acl = acl_of(&path);
    let created = std::fs::metadata(&path).unwrap().created().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(50));
    let second = first.set_layer_name(0, "Second").unwrap();
    save_opaint(&second, &path).unwrap();
    assert_eq!(load_opaint(&path).unwrap().layers[0].name, "Second");
    assert_eq!(acl_of(&path), acl);
    assert_eq!(std::fs::read(stream_of(&path)).unwrap(), b"kept");
    assert_eq!(
        std::fs::metadata(&path).unwrap().created().unwrap(),
        created
    );
    no_temp_left(dir.path());
}

#[cfg(windows)]
#[test]
fn windows_a_read_only_target_is_refused_and_kept() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("locked.opaint");
    let first = doc(&dir, 2, 2);
    save_opaint(&first, &path).unwrap();
    let old = std::fs::read(&path).unwrap();
    let mut p = std::fs::metadata(&path).unwrap().permissions();
    p.set_readonly(true);
    std::fs::set_permissions(&path, p.clone()).unwrap();
    let error = save_opaint(&first.set_layer_name(0, "No").unwrap(), &path).unwrap_err();
    let text = error.to_string();
    assert!(
        text.contains("read-only") && text.contains("locked.opaint"),
        "{text}"
    );
    assert_eq!(std::fs::read(&path).unwrap(), old);
    no_temp_left(dir.path());
    #[allow(clippy::permissions_set_readonly_false)]
    p.set_readonly(false);
    std::fs::set_permissions(&path, p).unwrap();
}

#[cfg(windows)]
#[test]
fn windows_a_plain_reader_does_not_block_a_save() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("read.opaint");
    let first = doc(&dir, 2, 2);
    save_opaint(&first, &path).unwrap();
    let reader = std::fs::File::open(&path).unwrap();
    save_opaint(&first.set_layer_name(0, "Saved").unwrap(), &path).unwrap();
    drop(reader);
    assert_eq!(load_opaint(&path).unwrap().layers[0].name, "Saved");
}

#[cfg(windows)]
#[test]
fn windows_a_reader_sharing_read_only_delays_a_save_until_it_lets_go() {
    use std::os::windows::fs::OpenOptionsExt;
    const FILE_SHARE_READ: u32 = 1;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("shared.opaint");
    let first = doc(&dir, 2, 2);
    save_opaint(&first, &path).unwrap();
    let held = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .open(&path)
        .unwrap();
    let release = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(500));
        drop(held);
    });
    let started = std::time::Instant::now();
    save_opaint(&first.set_layer_name(0, "After").unwrap(), &path).unwrap();
    assert!(
        started.elapsed() >= std::time::Duration::from_millis(300),
        "the reader had to delay the save"
    );
    release.join().unwrap();
    assert_eq!(load_opaint(&path).unwrap().layers[0].name, "After");
    no_temp_left(dir.path());
}

#[cfg(windows)]
fn hold_with_no_sharing(path: &std::path::Path) -> std::fs::File {
    use std::os::windows::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(path)
        .unwrap()
}

#[cfg(windows)]
#[test]
fn windows_a_save_waits_for_a_program_that_lets_go() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("held.opaint");
    let first = doc(&dir, 2, 2);
    save_opaint(&first, &path).unwrap();
    let held = hold_with_no_sharing(&path);
    let release = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(500));
        drop(held);
    });
    save_opaint(&first.set_layer_name(0, "After").unwrap(), &path).unwrap();
    release.join().unwrap();
    assert_eq!(load_opaint(&path).unwrap().layers[0].name, "After");
    no_temp_left(dir.path());
}

#[cfg(windows)]
#[test]
fn windows_a_held_target_fails_with_its_name_and_stays_whole() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("stuck.opaint");
    let first = doc(&dir, 2, 2);
    save_opaint(&first, &path).unwrap();
    let old = std::fs::read(&path).unwrap();
    let held = hold_with_no_sharing(&path);
    let error = save_opaint(&first.set_layer_name(0, "No").unwrap(), &path).unwrap_err();
    drop(held);
    let text = error.to_string();
    assert!(
        text.contains("stuck.opaint") && text.contains("another program"),
        "{text}"
    );
    assert_eq!(std::fs::read(&path).unwrap(), old);
    no_temp_left(dir.path());
}

#[cfg(windows)]
#[test]
fn windows_a_save_works_past_the_old_path_limit() {
    let root = tempfile::tempdir().unwrap();
    let folder = root.path().join("a".repeat(120)).join("b".repeat(120));
    std::fs::create_dir_all(&folder).unwrap();
    let path = folder.join("long.opaint");
    assert!(path.as_os_str().len() > 260);
    let first = doc(&root, 2, 2);
    save_opaint(&first, &path).unwrap();
    save_opaint(&first.set_layer_name(0, "Again").unwrap(), &path).unwrap();
    assert_eq!(load_opaint(&path).unwrap().layers[0].name, "Again");
}

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
    #[cfg(unix)]
    std::fs::set_permissions(&real, std::fs::Permissions::from_mode(0o600)).unwrap();
    #[cfg(windows)]
    std::fs::write(stream_of(&real), b"kept").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink("real.opaint", &link).unwrap();
    #[cfg(windows)]
    std::os::windows::fs::symlink_file("real.opaint", &link).unwrap();
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
    #[cfg(unix)]
    {
        let mode = std::fs::metadata(&real).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "the target keeps its permissions");
    }
    #[cfg(windows)]
    assert_eq!(
        std::fs::read(stream_of(&real)).unwrap(),
        b"kept",
        "the target keeps its extra stream"
    );
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
