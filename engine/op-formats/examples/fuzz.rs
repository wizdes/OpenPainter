//! Mutation fuzzer for `op_formats::decode` (stands in for cargo-fuzz, whose
//! libfuzzer-sys crate is not available offline).
//!
//! Run: `cargo run -p op-formats --example fuzz -- [mutations per format] [seed]`
//! (defaults: 10000 and 1). Each run takes the files in `tests/fixtures/io`,
//! groups them by format, and decodes that many mutated copies per format.
//!
//! `decode` turns a panic inside a codec into an error, so a panic hook
//! counts every panic, even caught ones, with its source location. The run
//! fails (exit code 1) if a panic escapes `decode` or starts in op-formats
//! code. Caught panics inside third-party codecs are listed but do not fail
//! the run, because the caller still gets a clean error.
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{Mutex, PoisonError},
    time::{Duration, Instant},
};

static PANICS: Mutex<Vec<String>> = Mutex::new(Vec::new());

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

const INTERESTING: [u32; 10] = [
    0,
    1,
    0x7f,
    0x80,
    0xff,
    0x7fff,
    0xffff,
    0x7fff_ffff,
    0xffff_ffff,
    100_001,
];

fn mutate(data: &mut Vec<u8>, rng: &mut Rng) {
    for _ in 0..1 + rng.below(4) {
        let len = data.len();
        if len == 0 {
            data.push(rng.next() as u8);
            continue;
        }
        let p = rng.below(len);
        match rng.below(7) {
            0 => data[p] ^= 1 << rng.below(8),
            1 => data[p] = INTERESTING[rng.below(5)] as u8,
            2 => data.truncate(p),
            3 => {
                let n = 1 + rng.below(16);
                let bytes: Vec<u8> = (0..n).map(|_| rng.next() as u8).collect();
                data.splice(p..p, bytes);
            }
            4 => {
                let end = (p + 1 + rng.below(16)).min(len);
                data.drain(p..end);
            }
            5 => {
                let end = (p + 1 + rng.below(64)).min(len);
                let chunk = data[p..end].to_vec();
                let at = rng.below(data.len());
                data.splice(at..at, chunk);
            }
            _ => {
                // Write an edge-case integer, 1, 2 or 4 bytes wide, in either order.
                let v = INTERESTING[rng.below(INTERESTING.len())];
                let n = [1, 2, 4][rng.below(3)].min(len - p);
                let le = v.to_le_bytes();
                let mut field = le[..n].to_vec();
                if rng.below(2) == 0 {
                    field.reverse();
                }
                data[p..p + n].copy_from_slice(&field);
            }
        }
    }
}

#[derive(Default)]
struct Tally {
    runs: usize,
    ok: usize,
    err: usize,
    caught_dep: usize,
    ours: usize,
    escaped: usize,
    slowest: Duration,
}

fn format_of(name: &str) -> &'static str {
    match name.split('_').next() {
        Some("png") => "png",
        Some("jpeg") => "jpeg",
        Some("bmp") => "bmp",
        Some("gif") => "gif",
        Some("tiff") => "tiff",
        Some("webp") => "webp",
        _ => "other",
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let per_format: usize = args.next().map_or(10_000, |a| a.parse().expect("count"));
    let seed: u64 = args.next().map_or(1, |a| a.parse().expect("seed"));
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/io");
    let mut corpus: BTreeMap<&str, Vec<(String, Vec<u8>)>> = BTreeMap::new();
    let mut entries: Vec<_> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .map(|e| e.unwrap().path())
        .collect();
    entries.sort();
    for path in entries {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        if format_of(&name) == "other" {
            continue;
        }
        corpus
            .entry(format_of(&name))
            .or_default()
            .push((name, std::fs::read(&path).unwrap()));
    }

    std::panic::set_hook(Box::new(|info| {
        let at = info
            .location()
            .map_or("unknown".into(), |l| format!("{}:{}", l.file(), l.line()));
        PANICS
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(at);
    }));

    let mut rng = Rng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1);
    let mut failed = false;
    let mut dep_sites: BTreeMap<String, usize> = BTreeMap::new();
    println!(
        "seed {seed}, {per_format} mutations per format, corpus {}",
        dir.display()
    );
    for (format, files) in &corpus {
        let mut t = Tally::default();
        let start = Instant::now();
        for i in 0..per_format {
            let (name, original) = &files[i % files.len()];
            let mut data = original.clone();
            mutate(&mut data, &mut rng);
            PANICS
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clear();
            let begin = Instant::now();
            let result = std::panic::catch_unwind(|| op_formats::decode(&data));
            t.slowest = t.slowest.max(begin.elapsed());
            t.runs += 1;
            match result {
                Ok(Ok(_)) => t.ok += 1,
                Ok(Err(_)) => t.err += 1,
                Err(_) => {
                    t.escaped += 1;
                    failed = true;
                    println!("  ESCAPED PANIC: {format} #{i} from {name}");
                }
            }
            for site in PANICS
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .drain(..)
            {
                if site.contains("op-formats") {
                    t.ours += 1;
                    failed = true;
                    println!("  PANIC IN OP-FORMATS: {site} ({format} #{i} from {name})");
                } else {
                    t.caught_dep += 1;
                    *dep_sites.entry(site).or_default() += 1;
                }
            }
        }
        println!(
            "{format:5} runs {:6} ok {:6} err {:6} | panics: op-formats {} escaped {} caught-in-dependency {} | slowest {:.1} ms, total {:.1} s",
            t.runs,
            t.ok,
            t.err,
            t.ours,
            t.escaped,
            t.caught_dep,
            t.slowest.as_secs_f64() * 1e3,
            start.elapsed().as_secs_f64()
        );
    }
    for (site, n) in &dep_sites {
        println!("  dependency panic caught by decode: {site} x{n}");
    }
    if failed {
        println!("FAIL");
        std::process::exit(1);
    }
    println!("PASS: no panic escaped decode and none started in op-formats");
}
