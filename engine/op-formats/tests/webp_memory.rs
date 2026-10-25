//! WebP decodes under the fuzz worker's memory rules (M58).
//!
//! `cargo xtask fuzz webp` found 43 small WebP files that made the worker pass
//! its 2 GiB memory limit. Each one is a lossy WebP (one "VP8 " chunk) whose
//! header claims a huge canvas, up to 16383 x 16383, with a corrupt bitstream
//! after it. Before the codec read one byte of pixel data, it took about
//! 1.5 bytes per pixel for its YUV planes and `decode` took 3 more for the
//! output, up to 1.15 GiB for a file of 190 bytes. A few of those in a row
//! left the worker's footprint over 2 GiB.
//!
//! `decode` must now refuse a WebP whose decode needs more than
//! `WEBP_DECODE_BUDGET` (1 GiB, counted from the headers) before any pixel
//! buffer exists. This file has its own counting allocator, so it holds one
//! test only: tests running side by side would add to the same count.
use op_formats::decode;
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

/// The fuzz worker's heap cap: an allocation past it aborts the test.
const MEMORY_LIMIT: usize = 2 << 30;
/// The most a refused or corrupt WebP may allocate in this test.
const REFUSAL_PEAK: usize = 64 << 20;
const WEBP_DECODE_BUDGET: usize = 1 << 30;

static IN_USE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

struct Counting;
impl Counting {
    fn add(n: usize) -> bool {
        let now = IN_USE.fetch_add(n, Ordering::Relaxed) + n;
        if now > MEMORY_LIMIT {
            IN_USE.fetch_sub(n, Ordering::Relaxed);
            return false;
        }
        PEAK.fetch_max(now, Ordering::Relaxed);
        true
    }
}
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if !Self::add(layout.size()) {
            return std::ptr::null_mut();
        }
        let p = unsafe { System.alloc(layout) };
        if p.is_null() {
            IN_USE.fetch_sub(layout.size(), Ordering::Relaxed);
        }
        p
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        if !Self::add(layout.size()) {
            return std::ptr::null_mut();
        }
        let p = unsafe { System.alloc_zeroed(layout) };
        if p.is_null() {
            IN_USE.fetch_sub(layout.size(), Ordering::Relaxed);
        }
        p
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) };
        IN_USE.fetch_sub(layout.size(), Ordering::Relaxed);
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let old = layout.size();
        if new_size > old && !Self::add(new_size - old) {
            return std::ptr::null_mut();
        }
        let p = unsafe { System.realloc(ptr, layout, new_size) };
        if p.is_null() {
            if new_size > old {
                IN_USE.fetch_sub(new_size - old, Ordering::Relaxed);
            }
        } else if new_size < old {
            IN_USE.fetch_sub(old - new_size, Ordering::Relaxed);
        }
        p
    }
}
#[global_allocator]
static GLOBAL: Counting = Counting;

/// Decodes `bytes` and returns the result as text and the heap peak it took.
fn decode_counted(bytes: &[u8]) -> (Result<(u32, u32), String>, usize) {
    let before = IN_USE.load(Ordering::Relaxed);
    PEAK.store(before, Ordering::Relaxed);
    let result = decode(bytes)
        .map(|d| (d.width, d.height))
        .map_err(|e| e.to_string());
    (result, PEAK.load(Ordering::Relaxed) - before)
}

fn fixture(name: &str) -> Vec<u8> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let path = if name.ends_with(".bin") {
        dir.join("tests/fuzz-found").join(name)
    } else {
        dir.join("../../tests/fixtures/io").join(name)
    };
    std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn chunk(fourcc: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut out = fourcc.to_vec();
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(data);
    if data.len() % 2 == 1 {
        out.push(0);
    }
    out
}

fn riff(chunks: &[Vec<u8>]) -> Vec<u8> {
    let body: Vec<u8> = chunks.concat();
    let mut out = b"RIFF".to_vec();
    out.extend_from_slice(&(body.len() as u32 + 4).to_le_bytes());
    out.extend_from_slice(b"WEBP");
    out.extend_from_slice(&body);
    out
}

fn u24(v: u32) -> [u8; 3] {
    [v as u8, (v >> 8) as u8, (v >> 16) as u8]
}

fn vp8x(flags: u8, w: u32, h: u32) -> Vec<u8> {
    let mut d = vec![flags, 0, 0, 0];
    d.extend_from_slice(&u24(w - 1));
    d.extend_from_slice(&u24(h - 1));
    chunk(b"VP8X", &d)
}

/// The 16 x 16 lossy fixture's VP8 data with its frame header set to w x h.
fn vp8_data(w: u16, h: u16) -> Vec<u8> {
    let file = fixture("webp_lossy.webp");
    let mut d = file[20..20 + 134].to_vec();
    d[6..8].copy_from_slice(&w.to_le_bytes());
    d[8..10].copy_from_slice(&h.to_le_bytes());
    d
}

/// The 16 x 16 lossless fixture's VP8L data with its header set to w x h.
fn vp8l_data(w: u32, h: u32, alpha: bool) -> Vec<u8> {
    let file = fixture("webp_lossless.webp");
    let at = file.windows(4).position(|c| c == b"VP8L").unwrap();
    let size = u32::from_le_bytes(file[at + 4..at + 8].try_into().unwrap()) as usize;
    let mut d = file[at + 8..at + 8 + size].to_vec();
    let old = u32::from_le_bytes(d[1..5].try_into().unwrap());
    let v = (old & 0xE000_0000) | ((alpha as u32) << 28) | ((h - 1) << 14) | (w - 1);
    d[1..5].copy_from_slice(&v.to_le_bytes());
    d
}

/// An animated WebP: canvas w x h, one frame of fw x fh holding `image`.
fn animated(w: u32, h: u32, fw: u32, fh: u32, image: Vec<u8>) -> Vec<u8> {
    let mut frame = Vec::new();
    frame.extend_from_slice(&u24(0));
    frame.extend_from_slice(&u24(0));
    frame.extend_from_slice(&u24(fw - 1));
    frame.extend_from_slice(&u24(fh - 1));
    frame.extend_from_slice(&u24(100));
    frame.push(0);
    frame.extend_from_slice(&image);
    riff(&[
        vp8x(0x12, w, h),
        chunk(b"ANIM", &[0, 0, 0, 0, 0, 0]),
        chunk(b"ANMF", &frame),
    ])
}

#[test]
fn webp_files_that_would_pass_the_memory_budget_are_refused_before_decoding() {
    let budget_error = "WebP needs more than 1 GiB to decode";
    let size_error = "WebP image data does not match its declared size";
    let chunk_error = "WebP chunk is larger than the file";
    let lossy = |w, h| riff(&[chunk(b"VP8 ", &vp8_data(w, h))]);
    let mut past_riff = riff(&[vp8x(0x20, 16, 16), chunk(b"VP8 ", &vp8_data(16, 16))]);
    past_riff.extend_from_slice(b"ICCP");
    past_riff.extend_from_slice(&(3_u32 << 30).to_le_bytes());
    let cases: Vec<(&str, Vec<u8>, &str)> = vec![
        // Fuzz finds: a 190-byte lossy file claiming 16383 x 16383, and one
        // claiming 15632 x 12801 whose RIFF size is 1 (so the M57 chunk
        // walk saw no chunks).
        (
            "webp-lossy-huge-canvas.bin",
            fixture("webp-lossy-huge-canvas.bin"),
            budget_error,
        ),
        (
            "webp-lossy-huge-canvas-short-riff.bin",
            fixture("webp-lossy-huge-canvas-short-riff.bin"),
            budget_error,
        ),
        // The same cause, built here: the fixture's 134 bytes of VP8 data
        // with the frame header set to the largest WebP size.
        ("lossy 16383 x 16383", lossy(16383, 16383), budget_error),
        // Lossless without alpha: image-webp decodes to RGBA, then copies
        // to RGB (4 + 3 bytes a pixel before ours).
        (
            "lossless 16383 x 16383",
            riff(&[chunk(b"VP8L", &vp8l_data(16383, 16383, false))]),
            budget_error,
        ),
        // VP8X says 16 x 16, but the VP8 frame header inside claims
        // 16383 x 16383: the codec sizes its YUV planes (400 MB) from the
        // frame header before it compares the two.
        (
            "VP8X 16 x 16 holding a 16383 x 16383 VP8",
            riff(&[vp8x(0, 16, 16), chunk(b"VP8 ", &vp8_data(16383, 16383))]),
            size_error,
        ),
        (
            "VP8X 16 x 16 holding a 16383 x 16383 VP8L",
            riff(&[
                vp8x(0x10, 16, 16),
                chunk(b"VP8L", &vp8l_data(16383, 16383, true)),
            ]),
            size_error,
        ),
        // Animation: the codec keeps a canvas (4 bytes a pixel) and the frame
        // beside the output, so a 12000 x 12000 canvas passes 1 GiB.
        (
            "animated 12000 x 12000",
            animated(12000, 12000, 16, 16, chunk(b"VP8 ", &vp8_data(16, 16))),
            budget_error,
        ),
        // A frame whose VP8 header disagrees with the frame size.
        (
            "animated frame 16 x 16 holding a 16383 x 16383 VP8",
            animated(16, 16, 16, 16, chunk(b"VP8 ", &vp8_data(16383, 16383))),
            size_error,
        ),
        // image-webp walks a VP8X file up to 10 bytes past the RIFF end, so
        // it finds this ICCP chunk header after the RIFF data and allocates
        // the 3 GiB it claims; the M57 walk stopped at the RIFF end.
        (
            "ICCP claiming 3 GiB just past the RIFF end",
            past_riff,
            chunk_error,
        ),
    ];
    let mut failures = Vec::new();
    for (name, bytes, want) in &cases {
        let (result, peak) = decode_counted(bytes);
        let ok = matches!(&result, Err(e) if e.contains(want)) && peak <= REFUSAL_PEAK;
        eprintln!("{name}: {result:?}, peak {} MiB", peak >> 20);
        if !ok {
            failures.push(format!(
                "{name}: wanted Err(\"{want}\") under {} MiB, got {result:?} with a {} MiB peak",
                REFUSAL_PEAK >> 20,
                peak >> 20
            ));
        }
    }
    // Sizes under the budget still decode, and the decode stays under it.
    for (name, bytes, size) in [
        ("webp_lossy.webp", fixture("webp_lossy.webp"), (16, 16)),
        (
            "webp_lossless.webp",
            fixture("webp_lossless.webp"),
            (16, 16),
        ),
    ] {
        let (result, peak) = decode_counted(&bytes);
        eprintln!("{name}: {result:?}, peak {} KiB", peak >> 10);
        if result != Ok(size) || peak > WEBP_DECODE_BUDGET {
            failures.push(format!("{name}: {result:?}, peak {} MiB", peak >> 20));
        }
    }
    // A 64 MP lossy header passes the budget: the codec then reads the
    // fixture's 16 x 16 data, finds it corrupt and says so, under 1 GiB.
    let (result, peak) = decode_counted(&lossy(8000, 8000));
    eprintln!("lossy 8000 x 8000: {result:?}, peak {} MiB", peak >> 20);
    if !matches!(&result, Err(e) if e.starts_with("codec error")) || peak > WEBP_DECODE_BUDGET {
        failures.push(format!(
            "lossy 8000 x 8000: wanted a codec error under 1 GiB, got {result:?} with a {} MiB peak",
            peak >> 20
        ));
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
