//! Port of OpenPDN MotionBlurEffect.cs and Surface.GetBilinearSample, commit cca476b, MIT.
use crate::effect::*;
use op_core::doc::{DocState, SelectionState};
use serde_json::{Value, json};
fn prepare(doc: &DocState, _: &SelectionState, p: &Value) -> Result<Prepared> {
    Ok(Prepared {
        width: doc.width as i32,
        height: doc.height as i32,
        halo: p["distance"].as_i64().unwrap_or(10) as i32 + 1,
        gaussian: None,
    })
}
fn bilinear(sample: &PixelReader<'_>, x: f32, y: f32, prep: &Prepared) -> [u8; 4] {
    let ix = x.floor() as i32;
    let iy = y.floor() as i32;
    let fx = (256.0 * (x - ix as f32)) as u64;
    let fy = (256.0 * (y - iy as f32)) as u64;
    let xx = (ix + 1).min(prep.width - 1);
    let yy = (iy + 1).min(prep.height - 1);
    let colors = [
        sample(ix, iy),
        sample(xx, iy),
        sample(ix, yy),
        sample(xx, yy),
    ];
    let weights = [
        (256 - fx) * (256 - fy),
        fx * (256 - fy),
        (256 - fx) * fy,
        fx * fy,
    ];
    let alpha: u64 = (0..4).map(|i| u64::from(colors[i][3]) * weights[i]).sum();
    if alpha == 0 {
        return [0; 4];
    }
    let mut out = [0; 4];
    out[3] = ((alpha + 32768) >> 16) as u8;
    for channel in 0..3 {
        let sum: u64 = (0..4)
            .map(|i| u64::from(colors[i][channel]) * u64::from(colors[i][3]) * weights[i])
            .sum();
        out[channel] = (sum / alpha) as u8;
    }
    out
}
fn render(sample: &PixelReader<'_>, x: i32, y: i32, prep: &Prepared, p: &Value) -> [u8; 4] {
    let angle = p["angle"].as_f64().unwrap_or(25.0);
    let distance = p["distance"].as_i64().unwrap_or(10) as usize;
    let centered = p["centered"].as_bool().unwrap_or(true);
    let theta = (angle + 180.0).to_radians();
    let mut ex = (distance as f64 * theta.cos()) as f32;
    let mut ey = (-(distance as f64) * theta.sin()) as f32;
    let (sx, sy) = if centered {
        ex *= 0.5;
        ey *= 0.5;
        (-ex, -ey)
    } else {
        (0.0, 0.0)
    };
    let count = (1 + distance) * 3 / 2;
    let mut colors = Vec::with_capacity(count);
    for i in 0..count {
        let t = if count == 1 {
            0.0
        } else {
            i as f32 / (count - 1) as f32
        };
        let xx = x as f32 + sx + (ex - sx) * t;
        let yy = y as f32 + sy + (ey - sy) * t;
        if xx >= 0.0 && yy >= 0.0 && xx <= (prep.width - 1) as f32 && yy <= (prep.height - 1) as f32
        {
            colors.push(bilinear(sample, xx, yy, prep));
        }
    }
    blend_samples(&colors)
}
pub fn spec() -> EffectSpec {
    EffectSpec {
        id: "effect.blur.motion",
        category: "Blurs",
        label_key: "effect.motion_blur",
        schema: ParamSchema {
            fields: vec![
                Param {
                    key: "angle",
                    kind: ParamKind::Float,
                    min: Some(-180.0),
                    max: Some(180.0),
                    default: Some(json!(25.0)),
                },
                Param {
                    key: "centered",
                    kind: ParamKind::Bool,
                    min: None,
                    max: None,
                    default: Some(json!(true)),
                },
                Param {
                    key: "distance",
                    kind: ParamKind::Int,
                    min: Some(1.0),
                    max: Some(200.0),
                    default: Some(json!(10)),
                },
            ],
        },
        contract: ArithmeticContract {
            alpha_form: AlphaForm::Straight,
            working_space: WorkingSpace::Encoded,
            quantization_points: &["bilinear 8-bit fraction", "alpha-weighted blend division"],
            rounding: RoundingRule::NearestHalfUp,
            border: BorderRule::Transparent,
            target: "legacy-3.36",
        },
        prepare,
        required_input: neighborhood_input,
        passes: &[PassKind::Neighborhood],
        render_cpu: render,
        gpu: Some(GpuKernel::Motion),
    }
}
