//! Port of OpenPDN ZoomBlurEffect.cs, commit cca476b, MIT.
use crate::effect::*;
use op_core::doc::{DocState, SelectionState};
use serde_json::{Value, json};
fn prepare(doc: &DocState, _: &SelectionState, _: &Value) -> Result<Prepared> {
    Ok(Prepared {
        width: doc.width as i32,
        height: doc.height as i32,
        halo: 0,
        gaussian: None,
    })
}
fn render(sample: &PixelReader<'_>, x: i32, y: i32, prep: &Prepared, p: &Value) -> [u8; 4] {
    let amount = p["amount"].as_i64().unwrap_or(10);
    let ox = p["offset_x"].as_f64().unwrap_or(0.0);
    let oy = p["offset_y"].as_f64().unwrap_or(0.0);
    let cx = (prep.width as f64 * ox * 32768.0) as i64 + ((prep.width as i64) << 15);
    let cy = (prep.height as f64 * oy * 32768.0) as i64 + ((prep.height as i64) << 15);
    let mut fx = ((x as i64) << 16) - cx;
    let mut fy = ((y as i64) << 16) - cy;
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
    for _ in 0..64 {
        fx -= ((fx >> 4) * amount) >> 10;
        fy -= ((fy >> 4) * amount) >> 10;
        let u = ((fx + cx + 32768) >> 16) as i32;
        let v = ((fy + cy + 32768) >> 16) as i32;
        if u >= 0 && v >= 0 && u < prep.width && v < prep.height {
            add(u, v);
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
        id: "effect.blur.zoom",
        category: "Blurs",
        label_key: "effect.zoom_blur",
        schema: ParamSchema {
            fields: vec![
                Param {
                    key: "amount",
                    kind: ParamKind::Int,
                    min: Some(0.0),
                    max: Some(100.0),
                    default: Some(json!(10)),
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
            ],
        },
        contract: ArithmeticContract {
            alpha_form: AlphaForm::Straight,
            working_space: WorkingSpace::Encoded,
            quantization_points: &["fixed-point zoom", "alpha-weighted blend division"],
            rounding: RoundingRule::NearestHalfUp,
            border: BorderRule::Transparent,
            target: "legacy-3.36",
        },
        prepare,
        required_input: whole_image_input,
        passes: &[PassKind::Warp],
        render_cpu: render,
        gpu: Some(GpuKernel::Zoom),
    }
}
