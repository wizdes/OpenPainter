//! Port of OpenPDN RadialBlurEffect.cs, commit cca476b, MIT.
use crate::effect::*;
use op_core::doc::{DocState, SelectionState};
use serde_json::{Value, json};
fn prepare(doc: &DocState, _: &SelectionState, _: &Value) -> Result<Prepared> {
    Ok(Prepared {
        width: doc.width as i32,
        height: doc.height as i32,
        halo: 0,
        gaussian: None,
        noise: None,
        channel_lut: None,
    })
}
fn rotate(fx: &mut i32, fy: &mut i32, fr: i32) {
    let cx = *fx;
    let cy = *fy;
    let quadratic = fr.wrapping_mul(fr) >> 11;
    *fx = cx
        .wrapping_sub((cy >> 8).wrapping_mul(fr) >> 8)
        .wrapping_sub((cx >> 14).wrapping_mul(quadratic) >> 8);
    *fy = cy
        .wrapping_add((cx >> 8).wrapping_mul(fr) >> 8)
        .wrapping_sub((cy >> 14).wrapping_mul(quadratic) >> 8);
}
fn render(sample: &PixelReader<'_>, x: i32, y: i32, prep: &Prepared, p: &Value) -> [u8; 4] {
    let quality = p["quality"].as_i64().unwrap_or(2) as i32;
    let n = quality * quality * (30 + quality * quality);
    let angle = p["angle"].as_f64().unwrap_or(2.0);
    let ox = p["offset_x"].as_f64().unwrap_or(0.0);
    let oy = p["offset_y"].as_f64().unwrap_or(0.0);
    let cx = (prep.width << 15) + (ox * (prep.width << 15) as f64) as i32;
    let cy = (prep.height << 15) + (oy * (prep.height << 15) as f64) as i32;
    let fx = (x << 16) - cx;
    let fy = (y << 16) - cy;
    let step = (angle * std::f64::consts::PI * 65536.0 / 181.0) as i32 / n;
    let mut x1 = fx;
    let mut y1 = fy;
    let mut x2 = fx;
    let mut y2 = fy;
    let mut count = 0u64;
    let mut alpha = 0u64;
    let mut rgb = [0u64; 3];
    let mut add = |u: i32, v: i32| {
        let c = sample(u, v);
        count += 1;
        alpha += u64::from(c[3]);
        for k in 0..3 {
            rgb[k] += u64::from(c[k]) * u64::from(c[3]);
        }
    };
    add(x, y);
    for _ in 0..n {
        rotate(&mut x1, &mut y1, step);
        rotate(&mut x2, &mut y2, -step);
        for (xx, yy) in [(x1, y1), (x2, y2)] {
            let u = xx.wrapping_add(cx).wrapping_add(32768) >> 16;
            let v = yy.wrapping_add(cy).wrapping_add(32768) >> 16;
            if u > 0 && v > 0 && u < prep.width && v < prep.height {
                add(u, v);
            }
        }
    }
    if alpha == 0 {
        return [0; 4];
    }
    [
        (rgb[0] / alpha) as u8,
        (rgb[1] / alpha) as u8,
        (rgb[2] / alpha) as u8,
        (alpha / count) as u8,
    ]
}
pub fn spec() -> EffectSpec {
    EffectSpec {
        id: "effect.blur.radial",
        category: "Blurs",
        label_key: "effect.radial_blur",
        schema: ParamSchema {
            fields: vec![
                Param {
                    key: "angle",
                    kind: ParamKind::Float,
                    min: Some(0.0),
                    max: Some(360.0),
                    default: Some(json!(2.0)),
                },
                Param {
                    key: "offset_x",
                    kind: ParamKind::Float,
                    min: Some(-2.0),
                    max: Some(2.0),
                    default: Some(json!(0.0)),
                },
                Param {
                    key: "offset_y",
                    kind: ParamKind::Float,
                    min: Some(-2.0),
                    max: Some(2.0),
                    default: Some(json!(0.0)),
                },
                Param {
                    key: "quality",
                    kind: ParamKind::Int,
                    min: Some(1.0),
                    max: Some(5.0),
                    default: Some(json!(2)),
                },
            ],
        },
        contract: ArithmeticContract {
            alpha_form: AlphaForm::Straight,
            working_space: WorkingSpace::Encoded,
            quantization_points: &["fixed-point rotation", "alpha-weighted blend division"],
            rounding: RoundingRule::NearestHalfUp,
            border: BorderRule::Transparent,
            target: "legacy-3.36",
        },
        prepare,
        required_input: whole_image_input,
        passes: &[PassKind::Warp],
        render_cpu: render,
        gpu: Some(GpuKernel::Radial),
    }
}
