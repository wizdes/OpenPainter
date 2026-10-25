//! Provisional paint.net 5.1 Turbulence interpretation.
//! Controls follow https://paint.net/doc/latest/EffectsRenderMenu.html.
//! Fixed point value noise keeps CPU and GPU bytes identical for a given seed.
use crate::effect::*;
use serde_json::{Value, json};

fn hash(x: i32, y: i32, channel: u32, seed: u32) -> i32 {
    let mut n = (x as u32).wrapping_mul(374_761_393)
        ^ (y as u32).wrapping_mul(668_265_263)
        ^ channel.wrapping_mul(1_442_695_041)
        ^ seed;
    n = (n ^ (n >> 13)).wrapping_mul(1_274_126_177);
    (((n ^ (n >> 16)) >> 17) as i32) - 16_384
}
fn smooth_step(fraction: i32) -> i32 {
    let fraction = fraction as u32;
    let square = (fraction * fraction + 32_768) >> 16;
    let cube = (square * fraction + 32_768) >> 16;
    (3 * square - 2 * cube) as i32
}
fn lerp(a: i32, b: i32, fraction: i32) -> i32 {
    a + ((b - a) * fraction + 32_768) / 65_536
}
fn noise(x: i32, y: i32, frequency: i32, period: i32, channel: u32, seed: u32) -> i32 {
    let scaled_x = x * frequency;
    let scaled_y = y * frequency;
    let gx = scaled_x / period;
    let gy = scaled_y / period;
    let fx = smooth_step((scaled_x % period) * 65_536 / period);
    let fy = smooth_step((scaled_y % period) * 65_536 / period);
    let upper = lerp(
        hash(gx, gy, channel, seed),
        hash(gx + 1, gy, channel, seed),
        fx,
    );
    let lower = lerp(
        hash(gx, gy + 1, channel, seed),
        hash(gx + 1, gy + 1, channel, seed),
        fx,
    );
    lerp(upper, lower, fy)
}
#[derive(Clone, Copy)]
struct TurbulenceOptions {
    octaves: i32,
    period: i32,
    size: i32,
    seed: u32,
    turbulence: bool,
    blend: bool,
}
fn pixel(sample: &PixelReader<'_>, x: i32, y: i32, options: &TurbulenceOptions) -> [u8; 4] {
    let TurbulenceOptions {
        octaves,
        period,
        size,
        seed,
        turbulence,
        blend,
    } = *options;
    let mut out = [0; 4];
    for (channel, output) in out.iter_mut().enumerate().take(3) {
        let mut sum = 0;
        let mut weight = 0;
        let mut amplitude = 256;
        let mut frequency = 1;
        for _ in 0..octaves {
            let value = noise(x, y, frequency, period, channel as u32, seed);
            sum += (if turbulence { value.abs() } else { value }) * amplitude;
            weight += amplitude;
            frequency *= 2;
            amplitude = amplitude * size / 100;
        }
        let avg = sum / weight;
        *output = if turbulence {
            ((avg * 255 + 8_192) / 16_384).clamp(0, 255) as u8
        } else {
            (((avg + 16_384) * 255 + 16_384) / 32_768).clamp(0, 255) as u8
        };
    }
    out[3] = 255;
    if blend {
        let original = sample(x, y);
        for channel in 0..3 {
            out[channel] = ((u16::from(out[channel]) + u16::from(original[channel])) / 2) as u8;
        }
        out[3] = original[3];
    }
    out
}
fn render(sample: &PixelReader<'_>, x: i32, y: i32, _: &Prepared, p: &Value) -> [u8; 4] {
    let options = TurbulenceOptions {
        octaves: p["octaves"].as_i64().unwrap() as i32,
        period: p["period"].as_i64().unwrap() as i32,
        size: p["size"].as_i64().unwrap() as i32,
        seed: p["seed"].as_u64().unwrap() as u32,
        turbulence: p["noise"].as_str().unwrap() == "turbulence",
        blend: p["blend"].as_bool().unwrap(),
    };
    pixel(sample, x, y, &options)
}
pub fn reference_rgba8(width: u32, height: u32, seed: u32) -> Vec<u8> {
    let mut out = vec![0; width as usize * height as usize * 4];
    let sample = |_: i32, _: i32| [0; 4];
    let options = TurbulenceOptions {
        octaves: 4,
        period: 128,
        size: 50,
        seed,
        turbulence: true,
        blend: false,
    };
    for y in 0..height as i32 {
        for x in 0..width as i32 {
            let index = (y as usize * width as usize + x as usize) * 4;
            out[index..index + 4].copy_from_slice(&pixel(&sample, x, y, &options));
        }
    }
    out
}
pub fn spec() -> EffectSpec {
    EffectSpec {
        id: "effect.render.turbulence",
        category: "Render",
        label_key: "effect.turbulence",
        schema: ParamSchema {
            fields: vec![
                Param {
                    key: "octaves",
                    kind: ParamKind::Int,
                    min: Some(1.0),
                    max: Some(10.0),
                    default: Some(json!(4)),
                },
                Param {
                    key: "period",
                    kind: ParamKind::Int,
                    min: Some(1.0),
                    max: Some(1000.0),
                    default: Some(json!(128)),
                },
                Param {
                    key: "size",
                    kind: ParamKind::Int,
                    min: Some(0.0),
                    max: Some(100.0),
                    default: Some(json!(50)),
                },
                Param {
                    key: "noise",
                    kind: ParamKind::Choice(&["turbulence", "fractal_sum"]),
                    min: None,
                    max: None,
                    default: Some(json!("turbulence")),
                },
                Param {
                    key: "seed",
                    kind: ParamKind::Int,
                    min: Some(0.0),
                    max: Some(i32::MAX as f64),
                    default: Some(json!(0)),
                },
                Param {
                    key: "blend",
                    kind: ParamKind::Bool,
                    min: None,
                    max: None,
                    default: Some(json!(false)),
                },
            ],
        },
        contract: ArithmeticContract {
            alpha_form: AlphaForm::Straight,
            working_space: WorkingSpace::Encoded,
            quantization_points: &[
                "Q16 interpolation",
                "Q8 octave weights",
                "integer output clamp",
            ],
            rounding: RoundingRule::NearestHalfUp,
            border: BorderRule::Clamp,
            target: "own",
        },
        prepare: prepare_noop,
        required_input: pointwise_input,
        passes: &[PassKind::Pointwise],
        render_cpu: render,
        gpu: Some(GpuKernel::Turbulence),
    }
}
