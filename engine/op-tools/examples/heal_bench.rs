use op_core::doc::{DocState, ToolCompositeMode};
use op_core::tiles::{TileCoord, TileData, TileStore};
use op_tools::{BrushSettings, CloneSource, PressureMode, StrokeSample, StrokeSession, Tool};
use std::sync::Arc;
use std::time::Instant;
fn main() {
    let mut d = DocState::new(
        Arc::new(TileStore::new(std::env::temp_dir(), 1 << 28).unwrap()),
        512,
        256,
        96.0,
    )
    .unwrap();
    for tx in 0..2 {
        let coord = TileCoord { x: tx, y: 0 };
        let mut tile = d.read_tile(0, coord).unwrap();
        for y in 0..256usize {
            for x in 0..256usize {
                let v = ((x * 37 + y * 17) % 51 + 80) as u8;
                tile[(y * 256 + x) * 4..(y * 256 + x) * 4 + 4].copy_from_slice(&[v, v, v, 255]);
            }
        }
        d = d
            .write_tile(0, coord, TileData::try_from(tile).unwrap())
            .unwrap();
    }
    let settings = BrushSettings {
        tool: Tool::HealingBrush,
        width: 200.0,
        color: [255; 4],
        antialias: false,
        spacing: 1.0,
        pressure: PressureMode::None,
        blend: ToolCompositeMode::Normal,
    };
    let mut times = Vec::new();
    for _ in 0..5 {
        let mut session = StrokeSession::new_heal(
            d.clone(),
            0,
            settings.clone(),
            1,
            CloneSource::Layer(0),
            (128, 128),
            None,
        )
        .unwrap();
        let start = Instant::now();
        session
            .append(&[StrokeSample::mouse(384.5, 128.5, 0)])
            .unwrap();
        times.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    times.sort_by(f64::total_cmp);
    println!(
        "200 px heal dab: median {:.2} ms; samples {:?}",
        times[2], times
    );
}
