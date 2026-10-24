use op_core::doc::{DocState, LayerBlendMode};
use op_core::tiles::{TILE_BYTES, TileCoord, TileData, TileStore};
use op_ops::composite::CompositeCache;
use std::sync::Arc;
use std::time::Instant;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dir = std::env::temp_dir().join(format!("op-composite-bench-{}", std::process::id()));
    std::fs::create_dir_all(&dir)?;
    let store = Arc::new(TileStore::new(&dir, 2 * 1024 * 1024 * 1024)?);
    let mut doc = DocState::new(store, 6000, 4000, 96.0)?;
    let modes = [
        LayerBlendMode::Normal,
        LayerBlendMode::Multiply,
        LayerBlendMode::Overlay,
        LayerBlendMode::Screen,
        LayerBlendMode::ColorDodge,
    ];
    for (layer, mode) in modes.into_iter().enumerate() {
        if layer > 0 {
            doc = doc.add_layer(layer, format!("layer {layer}"))?;
        }
        doc = doc.set_layer_blend(layer, mode)?;
        doc = doc.set_layer_opacity(layer, (255 - layer * 30) as u8)?;
        let data =
            TileData::try_from([50 + layer as u8 * 30, 130, 220, 180].repeat(TILE_BYTES / 4))?;
        for y in 0..16 {
            for x in 0..24 {
                doc = doc.write_tile(layer, TileCoord { x, y }, data.clone())?;
            }
        }
    }
    let mut cache = CompositeCache::new();
    let start = Instant::now();
    for y in 0..16 {
        for x in 0..24 {
            cache.get(&doc, TileCoord { x, y })?;
        }
    }
    println!(
        "6000x4000, 5 layers, full: {:.1} ms",
        start.elapsed().as_secs_f64() * 1000.0
    );
    let dirty = TileCoord { x: 12, y: 8 };
    let data = TileData::try_from([200, 80, 30, 230].repeat(TILE_BYTES / 4))?;
    doc = doc.write_tile(4, dirty, data)?;
    cache.invalidate_tiles([dirty]);
    let start = Instant::now();
    cache.get(&doc, dirty)?;
    println!(
        "one dirty tile: {:.1} ms",
        start.elapsed().as_secs_f64() * 1000.0
    );
    Ok(())
}
