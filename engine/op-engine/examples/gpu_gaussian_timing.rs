//! Release timing: Gaussian Blur r=50 on a 6000x4000 image through the engine
//! command (the full operation: tile reads, GPU chunks, coverage, tile writes,
//! history commit), with the GPU on and with it forced off.
//!
//! cargo run --release -p op-engine --example gpu_gaussian_timing [-- --skip-cpu]
use op_engine::{Engine, EngineOptions};
use serde_json::{Value, json};
use std::time::Instant;

fn info(e: &Engine, id: u64) -> Value {
    serde_json::from_slice(&e.query(id, "doc.info", json!({})).unwrap()).unwrap()
}
fn run(force_cpu: bool, pixels: &[u8], width: u32, height: u32) -> f64 {
    let e = Engine::with_options(
        None,
        EngineOptions {
            force_cpu,
            ..EngineOptions::default()
        },
    )
    .unwrap();
    // Start the GPU on a small document so the timed run excludes device creation.
    let warm = e.import_pixels(4, 4, &[128; 64], None).unwrap();
    let start = Instant::now();
    e.dispatch(warm, "effect.blur.gaussian", json!({"radius":2}))
        .unwrap();
    let warm_ms = start.elapsed().as_secs_f64() * 1000.0;
    let doc = e.import_pixels(width, height, pixels, None).unwrap();
    let start = Instant::now();
    e.dispatch(doc, "effect.blur.gaussian", json!({"radius":50}))
        .unwrap();
    let ms = start.elapsed().as_secs_f64() * 1000.0;
    let i = info(&e, doc);
    let label = if force_cpu { "cpu" } else { "gpu" };
    println!("{label}: first effect (device start + tiny blur) {warm_ms:.0} ms");
    println!(
        "{label}: gaussian r=50 {width}x{height} full operation {ms:.0} ms ({:.2} s)",
        ms / 1000.0
    );
    println!("{label}: gpu={} reason={}", i["gpu"], i["gpu_reason"]);
    println!("{label}: device {}", i["gpu_device"]);
    println!("{label}: effects {}", i["gpu_effects"]);
    assert_eq!(i["history_labels"].as_array().unwrap().len(), 2);
    ms
}

fn main() {
    let (width, height) = (6000u32, 4000u32);
    let mut seed = 0x1234_5678u32;
    let mut pixels = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            pixels.extend_from_slice(&[(x % 256) as u8, (y % 256) as u8, (seed >> 24) as u8, 255]);
        }
    }
    let gpu = run(false, &pixels, width, height);
    if std::env::args().any(|a| a == "--skip-cpu") {
        return;
    }
    let cpu = run(true, &pixels, width, height);
    println!(
        "speedup: {:.0}x (gpu {:.2} s, cpu {:.2} s)",
        cpu / gpu,
        gpu / 1000.0,
        cpu / 1000.0
    );
}
