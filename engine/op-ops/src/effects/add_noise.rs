//! Port of OpenPDN AddNoiseEffect.cs at commit cca476b, MIT.
//! Copyright dotPDN LLC and contributors. The default seed matches the oracle shim.
use crate::effect::*;
use op_core::doc::{DocState, SelectionState};
use serde_json::{Value, json};
use std::sync::{Arc, OnceLock};

const TABLE_SIZE: usize = 16384;
const DEFAULT_SEED: i32 = 0x51ed270b;

struct LegacyRandom {
    seed: [i32; 56],
    inext: usize,
    inextp: usize,
}
impl LegacyRandom {
    fn new(seed: i32) -> Self {
        const BIG: i32 = i32::MAX;
        let mut values = [0; 56];
        let subtraction = if seed == i32::MIN { BIG } else { seed.abs() };
        let mut mj = 161803398 - subtraction;
        values[55] = mj;
        let mut mk = 1;
        for i in 1..55 {
            let ii = 21 * i % 55;
            values[ii] = mk;
            mk = mj.wrapping_sub(mk);
            if mk < 0 {
                mk += BIG;
            }
            mj = values[ii];
        }
        for _ in 0..4 {
            for i in 1..56 {
                values[i] = values[i].wrapping_sub(values[1 + (i + 30) % 55]);
                if values[i] < 0 {
                    values[i] += BIG;
                }
            }
        }
        Self {
            seed: values,
            inext: 0,
            inextp: 21,
        }
    }
    fn next(&mut self) -> i32 {
        self.inext += 1;
        self.inextp += 1;
        if self.inext >= 56 {
            self.inext = 1;
        }
        if self.inextp >= 56 {
            self.inextp = 1;
        }
        let mut value = self.seed[self.inext].wrapping_sub(self.seed[self.inextp]);
        if value == i32::MAX {
            value -= 1;
        }
        if value < 0 {
            value += i32::MAX;
        }
        self.seed[self.inext] = value;
        value
    }
    fn next_double(&mut self) -> f64 {
        f64::from(self.next()) / f64::from(i32::MAX)
    }
    fn next_range(&mut self, max: usize) -> usize {
        (self.next_double() * max as f64) as usize
    }
}

fn lookup() -> &'static [i32; TABLE_SIZE] {
    static LOOKUP: OnceLock<[i32; TABLE_SIZE]> = OnceLock::new();
    LOOKUP.get_or_init(|| {
        let normal = |x: f64, scale: f64| scale * (-x * x / 2.0).exp();
        let mut l = 5.0;
        let mut r = 10.0;
        let mut scale = 50.0;
        while r - l > 0.0000001 {
            let mut sum = 0.0;
            scale = (l + r) * 0.5;
            for i in 0..TABLE_SIZE {
                sum += normal(
                    16.0 * (i as f64 - (TABLE_SIZE / 2) as f64) / TABLE_SIZE as f64,
                    scale,
                );
                if sum > 1_000_000.0 {
                    break;
                }
            }
            if sum > TABLE_SIZE as f64 {
                r = scale;
            } else if sum < TABLE_SIZE as f64 {
                l = scale;
            } else {
                break;
            }
        }
        let mut table = [0; TABLE_SIZE];
        let mut sum = 0.0;
        let mut rounded = 0usize;
        for i in 0..TABLE_SIZE {
            sum += normal(
                16.0 * (i as f64 - (TABLE_SIZE / 2) as f64) / TABLE_SIZE as f64,
                scale,
            );
            let last = rounded;
            rounded = (sum as usize).min(TABLE_SIZE);
            for entry in table.iter_mut().take(rounded).skip(last) {
                *entry = (i as i32 - (TABLE_SIZE / 2) as i32) * 65536 / TABLE_SIZE as i32;
            }
        }
        table
    })
}

pub fn delta_map(
    width: u32,
    height: u32,
    intensity: i32,
    saturation: i32,
    coverage: f64,
    seed: i32,
) -> Vec<[i32; 3]> {
    let width = width as usize;
    let height = height as usize;
    let mut rng = LegacyRandom::new(seed);
    let mut deltas = vec![[0; 3]; width * height];
    let dev = intensity.pow(2) / 4;
    let sat = saturation * 4096 / 100;
    let coverage = coverage / 100.0;
    let table = lookup();
    for delta in &mut deltas {
        if rng.next_double() > coverage {
            continue;
        }
        let r = table[rng.next_range(TABLE_SIZE)];
        let g = table[rng.next_range(TABLE_SIZE)];
        let b = table[rng.next_range(TABLE_SIZE)];
        let i = (4899 * r + 9618 * g + 1867 * b) >> 14;
        let values = [r, g, b];
        for channel in 0..3 {
            let value = i + (((values[channel] - i) * sat) >> 12);
            delta[channel] = (value * dev + 32768) >> 16;
        }
    }
    deltas
}
fn prepare(doc: &DocState, _: &SelectionState, p: &Value) -> Result<Prepared> {
    let deltas = delta_map(
        doc.width,
        doc.height,
        p["intensity"].as_i64().unwrap() as i32,
        p["saturation"].as_i64().unwrap() as i32,
        p["coverage"].as_f64().unwrap(),
        p["seed"].as_i64().unwrap() as i32,
    );
    Ok(Prepared {
        width: doc.width as i32,
        height: doc.height as i32,
        noise: Some(Arc::new(deltas)),
        ..Prepared::default()
    })
}
pub fn reference_rgba8(width: u32, height: u32, bytes: &[u8]) -> Vec<u8> {
    let deltas = delta_map(width, height, 64, 100, 100.0, DEFAULT_SEED);
    let mut output = bytes.to_vec();
    for (pixel, delta) in output.chunks_exact_mut(4).zip(deltas) {
        for channel in 0..3 {
            pixel[channel] = (i32::from(pixel[channel]) + delta[channel]).clamp(0, 255) as u8;
        }
    }
    output
}
fn render(sample: &PixelReader<'_>, x: i32, y: i32, prep: &Prepared, _: &Value) -> [u8; 4] {
    let mut pixel = sample(x, y);
    if let Some(noise) = &prep.noise {
        let delta = noise[y as usize * prep.width as usize + x as usize];
        for channel in 0..3 {
            pixel[channel] = (i32::from(pixel[channel]) + delta[channel]).clamp(0, 255) as u8;
        }
    }
    pixel
}
pub fn spec() -> EffectSpec {
    EffectSpec {
        id: "effect.noise.add",
        category: "Noise",
        label_key: "effect.add_noise",
        schema: ParamSchema {
            fields: vec![
                Param {
                    key: "intensity",
                    kind: ParamKind::Int,
                    min: Some(0.0),
                    max: Some(100.0),
                    default: Some(json!(64)),
                },
                Param {
                    key: "saturation",
                    kind: ParamKind::Int,
                    min: Some(0.0),
                    max: Some(400.0),
                    default: Some(json!(100)),
                },
                Param {
                    key: "coverage",
                    kind: ParamKind::Float,
                    min: Some(0.0),
                    max: Some(100.0),
                    default: Some(json!(100.0)),
                },
                Param {
                    key: "seed",
                    kind: ParamKind::Int,
                    min: Some(0.0),
                    max: Some(i32::MAX as f64),
                    default: Some(json!(DEFAULT_SEED)),
                },
            ],
        },
        contract: ArithmeticContract {
            alpha_form: AlphaForm::Straight,
            working_space: WorkingSpace::Encoded,
            quantization_points: &["Gaussian lookup", "integer shift", "output clamp"],
            rounding: RoundingRule::NearestHalfUp,
            border: BorderRule::Clamp,
            target: "legacy-3.36",
        },
        prepare,
        required_input: pointwise_input,
        passes: &[PassKind::Pointwise],
        render_cpu: render,
        gpu: Some(GpuKernel::AddNoise),
    }
}
