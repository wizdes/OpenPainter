//! The M57 fuzz worker that `cargo xtask fuzz` drives (cargo-fuzz needs a
//! nightly toolchain, which this Mac does not have).
//!
//! `fuzz_worker seeds <target>` writes the target's seed inputs to stdout,
//! each as a u32 little-endian length and the bytes.
//! `fuzz_worker run <target>` reads inputs the same way from stdin and
//! answers each with one status byte on stdout:
//! 0 = read, 1 = clean error, 2 = a panic escaped the reader,
//! 3 = a panic started in our code (op-formats, op-color, op-core),
//! 4 = a panic inside a third-party codec that the reader caught.
//! An input that needs more than the memory limit (2 GiB, counted by the
//! allocator below) aborts the worker; the driver reports it as out of memory.
//! The driver kills the worker when one input takes longer than 1 s.
//!
//! Targets: png, bmp, jpeg, gif, tiff, webp (the engine's own decoders),
//! opaint (a whole .opaint ZIP), stackxml (stack.xml inside an otherwise
//! valid .opaint), exif (the EXIF IFD reader), icc (ICC parse through op-color).
//! `selftest-oom` and `selftest-hang` break the memory and time limits on
//! purpose, to check that the driver catches both.
use op_core::doc::SelectionState;
use op_core::doc::{DocState, LayerBlendMode, MaskGrid, PolygonSet, SelectionSource};
use op_core::tiles::{TileCoord, TileData, TileStore};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};

#[path = "common/capped_alloc.rs"]
mod capped_alloc;
use capped_alloc::{MEMORY_LIMIT, set_rlimit};

static PANICS: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn fixtures() -> Vec<(String, Vec<u8>)> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/io");
    let mut out: Vec<_> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .map(|e| e.unwrap().path())
        .map(|p| {
            (
                p.file_name().unwrap().to_string_lossy().into_owned(),
                std::fs::read(&p).unwrap(),
            )
        })
        .collect();
    out.sort();
    out
}

fn store() -> Arc<TileStore> {
    Arc::new(TileStore::new(std::env::temp_dir(), 64 << 20).expect("tile store"))
}

fn sample_docs(store: &Arc<TileStore>) -> Vec<DocState> {
    let mut docs = Vec::new();
    let base = DocState::new(store.clone(), 70, 40, 96.0).unwrap();
    let mut tile = vec![0_u8; op_core::tiles::TILE_BYTES];
    for (i, b) in tile.iter_mut().enumerate() {
        *b = (i * 7 % 251) as u8;
    }
    let painted = base
        .write_tile(
            0,
            TileCoord { x: 0, y: 0 },
            TileData::try_from(tile).unwrap(),
        )
        .unwrap();
    docs.push(painted.clone());
    let layered = painted
        .add_layer(1, "Top & <odd> \"name\"")
        .unwrap()
        .set_layer_blend(1, LayerBlendMode::Multiply)
        .unwrap()
        .set_layer_opacity(1, 128)
        .unwrap()
        .set_layer_visible(0, false)
        .unwrap()
        .set_profile(Some(
            op_color::Profile::builtin("display-p3").unwrap().bytes(),
        ));
    docs.push(layered.clone());
    let poly = PolygonSet(vec![vec![(1.0, 1.0), (30.0, 2.0), (12.0, 20.0)]]);
    docs.push(
        layered.set_selection(SelectionState::Active(SelectionSource::Geometry(Arc::new(
            poly,
        )))),
    );
    let mask = MaskGrid {
        width: 70,
        height: 40,
        bytes: (0..70 * 40)
            .map(|i| (i % 256) as u8)
            .collect::<Vec<_>>()
            .into(),
    };
    docs.push(
        painted.set_selection(SelectionState::Active(SelectionSource::Coverage(Arc::new(
            mask,
        )))),
    );
    docs
}

fn opaint_bytes(doc: &DocState) -> Vec<u8> {
    let dir = std::env::temp_dir().join(format!("op-fuzz-seed-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("seed.opaint");
    op_formats::opaint::save_opaint(doc, &path).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    bytes
}

fn zip_entry(zip_bytes: &[u8], name: &str) -> Vec<u8> {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(zip_bytes)).unwrap();
    let mut out = Vec::new();
    zip.by_name(name).unwrap().read_to_end(&mut out).unwrap();
    out
}

/// An .opaint ZIP whose stack.xml is `xml`, with the entries the reader
/// wants (one 4x4 layer PNG per possible layer index 0..4).
fn wrap_stack(xml: &[u8], template: &[u8]) -> Vec<u8> {
    use zip::write::SimpleFileOptions;
    let mut out = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let stored = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    out.start_file("mimetype", stored).unwrap();
    out.write_all(b"image/openraster").unwrap();
    out.start_file("stack.xml", stored).unwrap();
    out.write_all(xml).unwrap();
    let mut src = zip::ZipArchive::new(std::io::Cursor::new(template)).unwrap();
    for i in 0..src.len() {
        let mut entry = src.by_index(i).unwrap();
        let name = entry.name().to_owned();
        if name == "mimetype" || name == "stack.xml" {
            continue;
        }
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).unwrap();
        out.start_file(name, stored).unwrap();
        out.write_all(&bytes).unwrap();
    }
    out.finish().unwrap().into_inner()
}

fn seeds(target: &str) -> Vec<Vec<u8>> {
    let prefix = |p: &str| -> Vec<Vec<u8>> {
        fixtures()
            .into_iter()
            .filter(|(n, _)| n.starts_with(p))
            .map(|(_, b)| b)
            .collect()
    };
    match target {
        "png" | "bmp" | "jpeg" | "gif" | "tiff" | "webp" => {
            let mut s = prefix(&format!("{target}_"));
            if target == "png" {
                let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures");
                for name in ["alpha_edge_64.png", "ramp_256x16.png", "impulse_64.png"] {
                    s.push(std::fs::read(dir.join(name)).unwrap());
                }
            }
            s
        }
        "opaint" => sample_docs(&store()).iter().map(opaint_bytes).collect(),
        "stackxml" => sample_docs(&store())
            .iter()
            .map(|d| zip_entry(&opaint_bytes(d), "stack.xml"))
            .collect(),
        "exif" => {
            let mut s = prefix("tiff_");
            for (name, bytes) in fixtures() {
                if name.starts_with("jpeg_exif")
                    && let Some(at) = bytes.windows(6).position(|w| w == b"Exif\0\0")
                {
                    s.push(bytes[at..(at + 4096).min(bytes.len())].to_vec());
                }
            }
            s
        }
        "icc" => {
            let mut s: Vec<Vec<u8>> = ["srgb", "display-p3", "adobe-rgb", "prophoto"]
                .iter()
                .map(|n| op_color::Profile::builtin(n).unwrap().bytes().to_vec())
                .collect();
            for (_, bytes) in fixtures() {
                if let Ok(d) = op_formats::decode(&bytes)
                    && let Some(icc) = d.icc
                {
                    s.push(icc);
                }
            }
            s
        }
        t if t.starts_with("selftest-") => vec![b"x".to_vec()],
        _ => panic!("unknown target {target}"),
    }
}

fn run_one(target: &str, data: &[u8], store: &Arc<TileStore>, template: &[u8]) -> u8 {
    PANICS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clear();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match target {
        "png" => {
            let a = op_formats::png::decode_png(data).is_ok();
            let b = op_formats::decode(data).is_ok();
            a || b
        }
        "bmp" | "jpeg" | "gif" | "tiff" | "webp" => op_formats::decode(data).is_ok(),
        "opaint" => op_formats::opaint::load_opaint_from_bytes(data, store.clone()).is_ok(),
        "stackxml" => {
            op_formats::opaint::load_opaint_from_bytes(&wrap_stack(data, template), store.clone())
                .is_ok()
        }
        "exif" => op_formats::fuzz_exif_ifd(data),
        "icc" => op_color::Profile::from_icc(data).is_ok(),
        // Checks of the limits themselves: past the heap cap, and past 1 s.
        "selftest-oom" => std::hint::black_box(vec![1_u8; 3 << 30]).len() > data.len(),
        // A C allocation past the cap (not seen by the Rust allocator): the
        // driver's resident-size watchdog must kill the worker.
        "selftest-coom" => unsafe {
            unsafe extern "C" {
                fn malloc(size: usize) -> *mut u8;
            }
            let n = 3_usize << 30;
            let p = malloc(n);
            if !p.is_null() {
                for i in (0..n).step_by(4096) {
                    std::ptr::write_volatile(p.add(i), 1);
                }
            }
            !p.is_null()
        },
        "selftest-hang" => {
            std::thread::sleep(std::time::Duration::from_secs(5));
            true
        }
        _ => unreachable!(),
    }));
    let sites: Vec<String> = PANICS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .drain(..)
        .collect();
    let ours = sites
        .iter()
        .any(|s| s.contains("op-formats") || s.contains("op-color") || s.contains("op-core"));
    match result {
        Err(_) => 2,
        Ok(_) if ours => 3,
        Ok(_) if !sites.is_empty() => 4,
        Ok(true) => 0,
        Ok(false) => 1,
    }
}

fn read_frame(input: &mut impl Read) -> Option<Vec<u8>> {
    let mut len = [0; 4];
    input.read_exact(&mut len).ok()?;
    let mut data = vec![0; u32::from_le_bytes(len) as usize];
    input.read_exact(&mut data).ok()?;
    Some(data)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (mode, target) = match args.as_slice() {
        [m, t] => (m.as_str(), t.as_str()),
        _ => {
            eprintln!("usage: fuzz_worker seeds|run <target>");
            std::process::exit(2);
        }
    };
    let mut out = std::io::stdout().lock();
    if mode == "seeds" {
        for s in seeds(target) {
            out.write_all(&(s.len() as u32).to_le_bytes()).unwrap();
            out.write_all(&s).unwrap();
        }
        return;
    }
    eprintln!(
        "fuzz_worker {target}: heap cap {} MiB, {}",
        MEMORY_LIMIT >> 20,
        set_rlimit()
    );
    std::panic::set_hook(Box::new(|info| {
        let at = info
            .location()
            .map_or("unknown".into(), |l| format!("{}:{}", l.file(), l.line()));
        PANICS
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(format!("{at}: {info}"));
    }));
    let store = store();
    let template = opaint_bytes(&sample_docs(&store)[1]);
    let mut input = std::io::stdin().lock();
    while let Some(data) = read_frame(&mut input) {
        let status = run_one(target, &data, &store, &template);
        if status == 2 || status == 3 {
            for site in PANICS.lock().unwrap_or_else(PoisonError::into_inner).iter() {
                eprintln!("fuzz_worker {target}: panic at {site}");
            }
        }
        if out.write_all(&[status]).and_then(|()| out.flush()).is_err() {
            return;
        }
    }
}
