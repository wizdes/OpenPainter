//! Shared fixed-point RGBA sampling for OpenPDN distortion ports (cca476b, MIT).
use crate::effect::{PixelReader, Prepared, Rect};

pub fn input_whole(_: Rect, p: &Prepared) -> Rect {
    Rect::new(0, 0, p.width.max(0) as u32, p.height.max(0) as u32)
}
pub fn input_radius(rect: Rect, p: &Prepared) -> Rect {
    crate::effect::neighborhood_input(rect, p)
}
pub fn blend(samples: &[[u8; 4]]) -> [u8; 4] {
    crate::effect::blend_samples(samples)
}
pub fn bilinear(sample: &PixelReader<'_>, x: f32, y: f32, p: &Prepared, edge: &str) -> [u8; 4] {
    if !x.is_finite() || !y.is_finite() || p.width <= 0 || p.height <= 0 {
        return [0; 4];
    }
    let map = |v: f32, len: i32| -> f32 {
        match edge {
            "wrap" => v.rem_euclid(len as f32),
            "mirror" | "reflect" => {
                if len <= 1 {
                    0.0
                } else {
                    let period = (2 * (len - 1)) as f32;
                    let w = v.rem_euclid(period);
                    if w < len as f32 { w } else { period - w }
                }
            }
            _ => v.clamp(0.0, (len - 1) as f32),
        }
    };
    if edge == "transparent" && (x < 0.0 || y < 0.0 || x >= p.width as f32 || y >= p.height as f32)
    {
        return [0; 4];
    }
    let x = map(x, p.width);
    let y = map(y, p.height);
    let ix = x.floor() as i32;
    let iy = y.floor() as i32;
    let fx = (256.0 * (x - ix as f32)) as u64;
    let fy = (256.0 * (y - iy as f32)) as u64;
    let x1 = if edge == "wrap" {
        (ix + 1) % p.width
    } else {
        (ix + 1).min(p.width - 1)
    };
    let y1 = if edge == "wrap" {
        (iy + 1) % p.height
    } else {
        (iy + 1).min(p.height - 1)
    };
    let pixels = [
        sample(ix, iy),
        sample(x1, iy),
        sample(ix, y1),
        sample(x1, y1),
    ];
    let weights = [
        (256 - fx) * (256 - fy),
        fx * (256 - fy),
        (256 - fx) * fy,
        fx * fy,
    ];
    let af: u64 = pixels
        .iter()
        .zip(weights)
        .map(|(c, w)| u64::from(c[3]) * w)
        .sum();
    let a = (af + 32768) >> 16;
    if a == 0 {
        return [0; 4];
    }
    let mut out = [0; 4];
    for k in 0..3 {
        let sum: u64 = pixels
            .iter()
            .zip(weights)
            .map(|(c, w)| u64::from(c[3]) * u64::from(c[k]) * w)
            .sum();
        out[k] = (sum / af) as u8;
    }
    out[3] = a as u8;
    out
}
pub fn field(
    key: &'static str,
    kind: crate::effect::ParamKind,
    min: f64,
    max: f64,
    default: serde_json::Value,
) -> crate::effect::Param {
    crate::effect::Param {
        key,
        kind,
        min: Some(min),
        max: Some(max),
        default: Some(default),
    }
}
pub fn choice(
    key: &'static str,
    options: &'static [&'static str],
    default: &'static str,
) -> crate::effect::Param {
    crate::effect::Param {
        key,
        kind: crate::effect::ParamKind::Choice(options),
        min: None,
        max: None,
        default: Some(serde_json::json!(default)),
    }
}
pub fn contract(
    border: crate::effect::BorderRule,
    target: &'static str,
) -> crate::effect::ArithmeticContract {
    use crate::effect::*;
    ArithmeticContract {
        alpha_form: AlphaForm::Straight,
        working_space: WorkingSpace::Encoded,
        quantization_points: &["RGBA8 weighted samples"],
        rounding: RoundingRule::NearestHalfUp,
        border,
        target,
    }
}
pub fn rgss(quality: usize) -> Vec<(f32, f32)> {
    let count = quality * quality;
    if count == 1 {
        return vec![(0.0, 0.0)];
    }
    (0..count)
        .map(|i| {
            let y = (i + 1) as f64 / (count + 1) as f64;
            let x = y * quality as f64;
            ((x - x.trunc() - 0.5) as f32, (y - 0.5) as f32)
        })
        .collect()
}
