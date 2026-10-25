//! Provisional 5.1 Crystalize from https://paint.net/doc/latest/EffectsDistortMenu.html.
use crate::effect::*;
use crate::effects::distort_common as d;
use serde_json::{Value, json};

fn hash(x: i32, y: i32, seed: u32) -> u32 {
    let mut h = (x as u32).wrapping_mul(0x9e37_79b9) ^ (y as u32).wrapping_mul(0x85eb_ca6b) ^ seed;
    h ^= h >> 16;
    h = h.wrapping_mul(0x7feb_352d);
    h ^ (h >> 15)
}
fn point(s: &PixelReader<'_>, x: f32, y: f32, p: &Prepared, v: &Value) -> [u8; 4] {
    let size = v["cell_size"].as_i64().unwrap() as f32;
    let seed = v["randomize"].as_u64().unwrap() as u32;
    let cx = (x / size).floor() as i32;
    let cy = (y / size).floor() as i32;
    let mut nearest = (f32::INFINITY, 0, 0);
    for dy in -1..=1 {
        for dx in -1..=1 {
            let gx = cx + dx;
            let gy = cy + dy;
            let h = hash(gx, gy, seed);
            let sx = (gx as f32 + 0.2 + (h & 0xff) as f32 / 425.0) * size;
            let sy = (gy as f32 + 0.2 + ((h >> 8) & 0xff) as f32 / 425.0) * size;
            let distance = (x - sx).powi(2) + (y - sy).powi(2);
            if distance < nearest.0 {
                nearest = (distance, sx.round() as i32, sy.round() as i32);
            }
        }
    }
    let sx = nearest.1.clamp(0, p.width - 1);
    let sy = nearest.2.clamp(0, p.height - 1);
    s(sx, sy)
}
fn render(s: &PixelReader<'_>, x: i32, y: i32, p: &Prepared, v: &Value) -> [u8; 4] {
    if p.width <= 0 || p.height <= 0 {
        return s(x, y);
    }
    let q = v["quality"].as_u64().unwrap() as usize;
    if v["cell_size"].as_i64().unwrap() == 1 {
        return s(x, y);
    }
    let samples: Vec<_> = d::rgss(q)
        .into_iter()
        .map(|(ox, oy)| point(s, x as f32 + ox, y as f32 + oy, p, v))
        .collect();
    d::blend(&samples)
}
pub fn spec() -> EffectSpec {
    EffectSpec {
        id: "effect.distort.crystalize",
        category: "Distort",
        label_key: "effect.crystalize",
        schema: ParamSchema {
            fields: vec![
                d::field("cell_size", ParamKind::Int, 1.0, 100.0, json!(10)),
                d::field("quality", ParamKind::Int, 1.0, 5.0, json!(2)),
                d::field("randomize", ParamKind::Int, 0.0, 4294967295.0, json!(0)),
            ],
        },
        contract: d::contract(BorderRule::Clamp, "own"),
        prepare: prepare_noop,
        required_input: d::input_whole,
        passes: &[PassKind::Warp],
        render_cpu: render,
        gpu: None,
    }
}
