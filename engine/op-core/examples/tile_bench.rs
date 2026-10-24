use op_core::tiles::{DEFAULT_BUDGET, TILE_BYTES, TileData, TileStore};
use std::time::Instant;

fn main() {
    let directory = tempfile::tempdir().unwrap();
    let store = TileStore::new(directory.path(), 100 * 1024 * 1024).unwrap();
    let mut ids = Vec::with_capacity(400);
    let mut seed = 0x4829_8103_7a12_5a31u64;
    let start = Instant::now();
    for _ in 0..400 {
        let mut bytes = vec![0; TILE_BYTES];
        for byte in &mut bytes {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            *byte = seed as u8;
        }
        ids.push(store.insert(TileData::try_from(bytes).unwrap()).unwrap());
    }
    let insert = start.elapsed().as_secs_f64();
    let start = Instant::now();
    store.set_budget(32 * 1024 * 1024);
    let evict = start.elapsed().as_secs_f64();
    let start = Instant::now();
    for id in ids {
        std::hint::black_box(store.get(id).unwrap()[0]);
    }
    let reload = start.elapsed().as_secs_f64();
    drop(store);
    let store = TileStore::new(directory.path(), DEFAULT_BUDGET).unwrap();
    let ids: Vec<_> = (0..4000u32)
        .map(|index| {
            let bytes = vec![index as u8; TILE_BYTES];
            store.insert(TileData::try_from(bytes).unwrap()).unwrap()
        })
        .collect();
    let gets = 100_000;
    let start = Instant::now();
    for index in 0..gets {
        let id = ids[(index * 7919) % ids.len()];
        std::hint::black_box(store.get(id).unwrap()[0]);
    }
    let resident_gets = start.elapsed().as_secs_f64();
    let mib = 100.0;
    println!("insert MB/s: {:.2}", mib / insert);
    println!("evict MB/s: {:.2}", 68.0 / evict);
    println!("reload MB/s: {:.2}", mib / reload);
    println!("resident gets/s: {:.0}", gets as f64 / resident_gets);
}
