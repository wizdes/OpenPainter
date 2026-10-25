//! `cargo xtask fuzz`: a stable-Rust random-mutation fuzzer for every engine
//! reader (M57). cargo-fuzz needs a nightly toolchain, which is not installed.
//!
//! The readers run in a separate worker process (op-formats' `fuzz_worker`
//! example), so a crash, an abort or a hang ends only the worker. The driver
//! mutates seed inputs, sends each to the worker, and waits at most the
//! per-input time limit (1 s) for the answer. The worker caps its own heap at
//! 2 GiB (and asks for RLIMIT_AS). Crashing and hanging inputs are saved in
//! `target/fuzz/<target>/`.
//!
//! Usage: `cargo xtask fuzz [--seconds 600] [--jobs 3] [--seed 1]
//! [--timeout-ms 1000] [target ...]`; targets default to all of them.
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, ExitCode, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

pub const TARGETS: [&str; 10] = [
    "png", "bmp", "jpeg", "gif", "tiff", "webp", "opaint", "stackxml", "exif", "icc",
];

#[derive(Debug, Clone, PartialEq)]
pub struct Options {
    pub seconds: u64,
    pub jobs: usize,
    pub seed: u64,
    pub timeout: Duration,
    pub targets: Vec<String>,
}

pub fn parse_args(args: &[String]) -> Result<Options, String> {
    let mut o = Options {
        seconds: 600,
        jobs: 3,
        seed: 1,
        timeout: Duration::from_millis(1000),
        targets: Vec::new(),
    };
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut value = |name: &str| -> Result<u64, String> {
            it.next()
                .ok_or(format!("{name} needs a value"))?
                .parse()
                .map_err(|_| format!("{name} needs a number"))
        };
        match a.as_str() {
            "--seconds" => o.seconds = value("--seconds")?,
            "--jobs" => o.jobs = value("--jobs")?.max(1) as usize,
            "--seed" => o.seed = value("--seed")?,
            "--timeout-ms" => o.timeout = Duration::from_millis(value("--timeout-ms")?),
            t if TARGETS.contains(&t) || t.starts_with("selftest-") => o.targets.push(t.to_owned()),
            other => return Err(format!("unknown fuzz argument {other}")),
        }
    }
    if o.targets.is_empty() {
        o.targets = TARGETS.iter().map(|t| (*t).to_owned()).collect();
    }
    Ok(o)
}

pub struct Rng(pub u64);
impl Rng {
    pub fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    pub fn below(&mut self, n: usize) -> usize {
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

/// One to four random edits: bit flips, edge bytes, cuts, random inserts,
/// deletes, duplicated chunks, edge-case integers, and a splice from `other`.
pub fn mutate(data: &mut Vec<u8>, other: &[u8], rng: &mut Rng) {
    for _ in 0..1 + rng.below(4) {
        let len = data.len();
        if len == 0 {
            data.push(rng.next() as u8);
            continue;
        }
        let p = rng.below(len);
        match rng.below(8) {
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
            6 if !other.is_empty() => {
                let from = rng.below(other.len());
                let end = (from + 1 + rng.below(256)).min(other.len());
                let at = rng.below(data.len());
                let cut = (at + rng.below(256)).min(data.len());
                data.splice(at..cut, other[from..end].iter().copied());
            }
            _ => {
                let v = INTERESTING[rng.below(INTERESTING.len())];
                let n = [1, 2, 4][rng.below(3)].min(len - p);
                let mut field = v.to_le_bytes()[..n].to_vec();
                if rng.below(2) == 0 {
                    field.reverse();
                }
                data[p..p + n].copy_from_slice(&field);
            }
        }
    }
}

#[derive(Default, Debug)]
pub struct Tally {
    pub cases: u64,
    pub ok: u64,
    pub err: u64,
    pub crashes: u64,
    pub hangs: u64,
    pub aborts: u64,
    pub caught_in_dependency: u64,
    pub restarts: u64,
    pub slowest: Duration,
}
impl Tally {
    pub fn failed(&self) -> bool {
        self.crashes + self.hangs + self.aborts > 0
    }
}

struct Worker {
    child: Child,
    stdin: ChildStdin,
    replies: Receiver<u8>,
}

fn frames(bytes: &[u8]) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    let mut p = 0;
    while p + 4 <= bytes.len() {
        let n = u32::from_le_bytes(bytes[p..p + 4].try_into().unwrap()) as usize;
        p += 4;
        if p + n > bytes.len() {
            break;
        }
        out.push(bytes[p..p + n].to_vec());
        p += n;
    }
    out
}

fn spawn(worker: &Path, target: &str, log: &Path) -> std::io::Result<Worker> {
    let log = fs::OpenOptions::new().create(true).append(true).open(log)?;
    let mut child = Command::new(worker)
        .args(["run", target])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::from(log))
        .spawn()?;
    let stdin = child.stdin.take().expect("piped stdin");
    let mut stdout = child.stdout.take().expect("piped stdout");
    let (tx, replies) = mpsc::channel();
    std::thread::spawn(move || {
        let mut b = [0_u8; 1];
        while stdout.read_exact(&mut b).is_ok() {
            if tx.send(b[0]).is_err() {
                break;
            }
        }
    });
    Ok(Worker {
        child,
        stdin,
        replies,
    })
}

fn save(dir: &Path, kind: &str, n: u64, data: &[u8]) {
    let path = dir.join(format!("{kind}-{n}.bin"));
    if let Err(e) = fs::write(&path, data) {
        eprintln!("fuzz: cannot save {}: {e}", path.display());
    }
}

/// Fuzzes one target until `deadline`. Returns the tally.
pub fn fuzz_target(
    worker: &Path,
    target: &str,
    out: &Path,
    seconds: u64,
    seed: u64,
    timeout: Duration,
) -> Result<Tally, String> {
    fs::create_dir_all(out).map_err(|e| format!("{}: {e}", out.display()))?;
    let seeds = Command::new(worker)
        .args(["seeds", target])
        .output()
        .map_err(|e| format!("cannot run {}: {e}", worker.display()))?;
    if !seeds.status.success() {
        return Err(format!(
            "{target}: seeds failed: {}",
            String::from_utf8_lossy(&seeds.stderr)
        ));
    }
    let mut corpus = frames(&seeds.stdout);
    if corpus.is_empty() {
        return Err(format!("{target}: no seed inputs"));
    }
    let seed_count = corpus.len();
    let corpus_dir = out.join("corpus");
    fs::create_dir_all(&corpus_dir).map_err(|e| e.to_string())?;
    for (i, s) in corpus.iter().enumerate() {
        save(&corpus_dir, "seed", i as u64, s);
    }
    let log = out.join("worker.log");
    let _ = fs::remove_file(&log);
    let mut rng = Rng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ fnv(target.as_bytes()) | 1);
    let mut t = Tally::default();
    let deadline = Instant::now() + Duration::from_secs(seconds);
    let mut w: Option<Worker> = None;
    while Instant::now() < deadline {
        if w.is_none() {
            w = Some(spawn(worker, target, &log).map_err(|e| e.to_string())?);
        }
        let worker_now = w.as_mut().expect("worker");
        let base = rng.below(corpus.len());
        let other = rng.below(corpus.len());
        let mut data = corpus[base].clone();
        let splice = corpus[other].clone();
        mutate(&mut data, &splice, &mut rng);
        let mut frame = (data.len() as u32).to_le_bytes().to_vec();
        frame.extend_from_slice(&data);
        let begin = Instant::now();
        let sent = worker_now
            .stdin
            .write_all(&frame)
            .and_then(|()| worker_now.stdin.flush());
        let reply = if sent.is_ok() {
            worker_now.replies.recv_timeout(timeout)
        } else {
            Err(RecvTimeoutError::Disconnected)
        };
        t.cases += 1;
        t.slowest = t.slowest.max(begin.elapsed());
        match reply {
            Ok(0) => {
                t.ok += 1;
                // Inputs that still read are kept, so later mutations go deeper.
                if corpus.len() < seed_count + 512 && rng.below(16) == 0 {
                    corpus.push(data);
                }
            }
            Ok(1) => t.err += 1,
            Ok(4) => t.caught_in_dependency += 1,
            Ok(_) => {
                t.crashes += 1;
                save(out, "crash", t.cases, &data);
            }
            Err(RecvTimeoutError::Timeout) => {
                t.hangs += 1;
                save(out, "hang", t.cases, &data);
                let mut dead = w.take().expect("worker");
                let _ = dead.child.kill();
                let _ = dead.child.wait();
                t.restarts += 1;
            }
            Err(RecvTimeoutError::Disconnected) => {
                let mut dead = w.take().expect("worker");
                let status = dead.child.wait().map_err(|e| e.to_string())?;
                t.aborts += 1;
                t.restarts += 1;
                save(out, "abort", t.cases, &data);
                eprintln!("fuzz {target}: worker ended ({status}) on case {}", t.cases);
            }
        }
    }
    if let Some(mut live) = w.take() {
        drop(live.stdin);
        let _ = live.child.wait();
    }
    Ok(t)
}

fn fnv(bytes: &[u8]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325_u64;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

pub fn main(engine: &Path, args: &[String]) -> ExitCode {
    let o = match parse_args(args) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("{e}");
            eprintln!(
                "Usage: cargo xtask fuzz [--seconds 600] [--jobs 3] [--seed 1] [--timeout-ms 1000] [{}]",
                TARGETS.join("|")
            );
            return ExitCode::from(2);
        }
    };
    let built = Command::new("cargo")
        .args(["build", "-p", "op-formats", "--example", "fuzz_worker"])
        .current_dir(engine)
        .status();
    if !matches!(built, Ok(s) if s.success()) {
        eprintln!("fuzz: building fuzz_worker failed");
        return ExitCode::FAILURE;
    }
    let target_dir =
        std::env::var_os("CARGO_TARGET_DIR").map_or_else(|| engine.join("target"), PathBuf::from);
    let worker = target_dir.join("debug/examples/fuzz_worker");
    let out_root = target_dir.join("fuzz");
    println!(
        "fuzz: {} targets, {} s each, {} at a time, seed {}, {} ms per input, heap cap 2 GiB",
        o.targets.len(),
        o.seconds,
        o.jobs,
        o.seed,
        o.timeout.as_millis()
    );
    let mut failed = false;
    for chunk in o.targets.chunks(o.jobs) {
        let handles: Vec<_> = chunk
            .iter()
            .map(|target| {
                let (worker, out, target) = (worker.clone(), out_root.join(target), target.clone());
                let (seconds, seed, timeout) = (o.seconds, o.seed, o.timeout);
                std::thread::spawn(move || {
                    let r = fuzz_target(&worker, &target, &out, seconds, seed, timeout);
                    (target, out, r)
                })
            })
            .collect();
        for h in handles {
            let (target, out, result) = h.join().expect("fuzz thread");
            match result {
                Ok(t) => {
                    failed |= t.failed();
                    println!(
                        "{target:8} cases {:9} ok {:8} err {:9} | crashes {} hangs {} aborts {} | caught-in-dependency {} | slowest {:.1} ms | {}",
                        t.cases,
                        t.ok,
                        t.err,
                        t.crashes,
                        t.hangs,
                        t.aborts,
                        t.caught_in_dependency,
                        t.slowest.as_secs_f64() * 1e3,
                        if t.failed() {
                            format!("FAIL, inputs in {}", out.display())
                        } else {
                            "PASS".into()
                        }
                    );
                }
                Err(e) => {
                    failed = true;
                    println!("{target:8} ERROR {e}");
                }
            }
        }
    }
    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arguments_default_to_ten_minutes_for_every_target() {
        let o = parse_args(&[]).unwrap();
        assert_eq!(o.seconds, 600);
        assert_eq!(o.timeout, Duration::from_millis(1000));
        assert_eq!(o.targets.len(), TARGETS.len());
        let o = parse_args(&["--seconds".into(), "5".into(), "icc".into()]).unwrap();
        assert_eq!((o.seconds, o.targets.clone()), (5, vec!["icc".to_owned()]));
        assert!(parse_args(&["nope".into()]).is_err());
        assert!(parse_args(&["--seconds".into()]).is_err());
    }

    #[test]
    fn frames_round_trip_and_mutation_changes_the_input() {
        let mut bytes = Vec::new();
        for s in [&b"abc"[..], b"", b"hello"] {
            bytes.extend_from_slice(&(s.len() as u32).to_le_bytes());
            bytes.extend_from_slice(s);
        }
        assert_eq!(
            frames(&bytes),
            vec![b"abc".to_vec(), vec![], b"hello".to_vec()]
        );
        let mut rng = Rng(7);
        let mut changed = 0;
        for _ in 0..100 {
            let mut d = b"0123456789abcdef".to_vec();
            mutate(&mut d, b"zz", &mut rng);
            changed += usize::from(d != b"0123456789abcdef");
        }
        assert!(changed > 90);
    }
}
