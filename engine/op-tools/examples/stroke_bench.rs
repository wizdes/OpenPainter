use op_core::doc::{DocState, ToolCompositeMode};
use op_core::tiles::TileStore;
use op_tools::{BrushSettings, PressureMode, StrokeSample, StrokeSession, Tool};
use std::sync::Arc;
use std::time::Instant;
fn main() {
    let doc = DocState::new(
        Arc::new(TileStore::new(std::env::temp_dir(), 256 * 1024 * 1024).unwrap()),
        6000,
        4000,
        96.0,
    )
    .unwrap();
    let settings = BrushSettings {
        tool: Tool::Paintbrush,
        width: 60.0,
        color: [180, 30, 40, 255],
        antialias: true,
        spacing: 0.1,
        pressure: PressureMode::None,
        blend: ToolCompositeMode::Normal,
    };
    let mut stroke = StrokeSession::new(doc, 0, settings, 123).unwrap();
    let samples: Vec<_> = (0..2000)
        .map(|i| {
            StrokeSample::mouse(
                100.0 + i as f64 * 2.8,
                2000.0 + (i as f64 * 0.01).sin() * 100.0,
                i,
            )
        })
        .collect();
    let mut times = Vec::new();
    for batch in samples.chunks(16) {
        let now = Instant::now();
        stroke.append(batch).unwrap();
        times.push(now.elapsed().as_secs_f64() * 1000.0);
    }
    times.sort_by(|a, b| a.total_cmp(b));
    println!(
        "2000 samples, 60 px, 6000x4000: {} batches; median {:.2} ms, p95 {:.2} ms, max {:.2} ms per 16 samples; {} touched tiles",
        times.len(),
        times[times.len() / 2],
        times[(times.len() * 95 / 100).min(times.len() - 1)],
        times[times.len() - 1],
        stroke.touched_tiles()
    );
}
