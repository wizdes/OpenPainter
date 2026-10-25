//! Coordinate-seeded port of OpenPDN FrostedGlassEffect.cs at cca476b (MIT, dotPDN LLC).
//! The original uses a thread RNG; coordinate seeding makes tiles independent.
use crate::effect::*;
use crate::effects::distort_common as d;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

// System.Random(int) uses the legacy subtractive generator in .NET 8.
struct LegacyRandom {
    values: [i32; 56],
    next: usize,
    nextp: usize,
}
impl LegacyRandom {
    fn new(seed: i32) -> Self {
        const BIG: i32 = i32::MAX;
        let mut values = [0; 56];
        let mut mj = 161_803_398 - if seed == i32::MIN { BIG } else { seed.abs() };
        values[55] = mj;
        let mut mk = 1;
        for i in 1..55 {
            let ii = (21 * i) % 55;
            values[ii] = mk;
            mk = mj.wrapping_sub(mk);
            if mk < 0 {
                mk = mk.wrapping_add(BIG);
            }
            mj = values[ii];
        }
        for _ in 0..4 {
            for i in 1..56 {
                values[i] = values[i].wrapping_sub(values[1 + (i + 30) % 55]);
                if values[i] < 0 {
                    values[i] = values[i].wrapping_add(BIG);
                }
            }
        }
        Self {
            values,
            next: 0,
            nextp: 21,
        }
    }
    fn next_double(&mut self) -> f64 {
        self.next += 1;
        if self.next >= 56 {
            self.next = 1;
        }
        self.nextp += 1;
        if self.nextp >= 56 {
            self.nextp = 1;
        }
        let mut value = self.values[self.next].wrapping_sub(self.values[self.nextp]);
        if value == i32::MAX {
            value -= 1;
        }
        if value < 0 {
            value = value.wrapping_add(i32::MAX);
        }
        self.values[self.next] = value;
        value as f64 / i32::MAX as f64
    }
}
type Coordinates = Arc<Vec<(f32, f32)>>;
type Cache = HashMap<(i32, i32, u64, u64, usize, i32, u64), Coordinates>;
static COORDS: OnceLock<Mutex<Cache>> = OnceLock::new();
fn coordinates(p: &Prepared, v: &Value) -> Coordinates {
    let min = v["minimum_scatter_radius"]
        .as_f64()
        .unwrap()
        .min((p.width.min(p.height) / 2) as f64);
    let max = v["maximum_scatter_radius"].as_f64().unwrap();
    let count = v["smoothness"].as_i64().unwrap() as usize;
    let seed = v["seed"].as_i64().unwrap() as i32;
    let diffusion = v["diffusion"].as_f64().unwrap();
    let key = (
        p.width,
        p.height,
        min.to_bits(),
        max.to_bits(),
        count,
        seed,
        diffusion.to_bits(),
    );
    let cache = COORDS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut guard = cache.lock().unwrap();
    if let Some(found) = guard.get(&key) {
        return found.clone();
    }
    let mut rand = LegacyRandom::new(seed);
    let mut coords = Vec::with_capacity(p.width.max(0) as usize * p.height.max(0) as usize * count);
    for y in 0..p.height {
        for x in 0..p.width {
            for _ in 0..count {
                loop {
                    let angle = rand.next_double() * std::f64::consts::TAU;
                    let distance = min + (max - min) * rand.next_double().powf(1.0 / diffusion);
                    let sx = x as f64 + angle.cos() * distance;
                    let sy = y as f64 + angle.sin() * distance;
                    if sx >= 0.0 && sy >= 0.0 && sx <= p.width as f64 && sy <= p.height as f64 {
                        coords.push((sx as f32, sy as f32));
                        break;
                    }
                }
            }
        }
    }
    let coords = Arc::new(coords);
    guard.insert(key, coords.clone());
    coords
}
fn render(s: &PixelReader<'_>, x: i32, y: i32, p: &Prepared, v: &Value) -> [u8; 4] {
    let coords = coordinates(p, v);
    let count = v["smoothness"].as_i64().unwrap() as usize;
    let start = (y as usize * p.width as usize + x as usize) * count;
    let samples: Vec<_> = coords[start..start + count]
        .iter()
        .map(|&(sx, sy)| d::bilinear(s, sx, sy, p, "transparent"))
        .collect();
    d::blend(&samples)
}

pub fn spec() -> EffectSpec {
    EffectSpec {
        id: "effect.distort.frosted_glass",
        category: "Distort",
        label_key: "effect.frosted_glass",
        schema: ParamSchema {
            fields: vec![
                d::field(
                    "maximum_scatter_radius",
                    ParamKind::Float,
                    0.0,
                    200.0,
                    json!(3.0),
                ),
                d::field(
                    "minimum_scatter_radius",
                    ParamKind::Float,
                    0.0,
                    200.0,
                    json!(0.0),
                ),
                d::field("smoothness", ParamKind::Int, 1.0, 8.0, json!(2)),
                d::field("diffusion", ParamKind::Float, 0.1, 10.0, json!(1.0)),
                d::field("seed", ParamKind::Int, 0.0, 2147483647.0, json!(0x51ed270b)),
            ],
        },
        contract: d::contract(BorderRule::Clamp, "legacy-3.36"),
        prepare: prepare_noop,
        required_input: d::input_whole,
        passes: &[PassKind::Neighborhood],
        render_cpu: render,
        // CPU-only: the legacy random stream and edge rejection are sequential.
        gpu: None,
    }
}
