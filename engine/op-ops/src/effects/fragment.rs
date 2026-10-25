//! Port of OpenPDN FragmentEffect.cs, commit cca476b, MIT.
use crate::effect::*;
use op_core::doc::{DocState, SelectionState};
use serde_json::{Value, json};
fn prepare(doc: &DocState, _: &SelectionState, p: &Value) -> Result<Prepared> {
    Ok(Prepared {
        width: doc.width as i32,
        height: doc.height as i32,
        halo: p["distance"].as_i64().unwrap_or(8) as i32,
        gaussian: None,
    })
}
fn render(sample: &PixelReader<'_>, x: i32, y: i32, prep: &Prepared, p: &Value) -> [u8; 4] {
    let count = p["fragments"].as_i64().unwrap_or(4) as usize;
    let distance = p["distance"].as_i64().unwrap_or(8) as f64;
    let rotation = p["rotation"].as_f64().unwrap_or(0.0);
    let mut samples = Vec::with_capacity(count);
    for i in 0..count {
        let theta =
            (rotation - 90.0).to_radians() + std::f64::consts::TAU * i as f64 / count as f64;
        let dx = (-distance * theta.sin()).round() as i32;
        let dy = (-distance * theta.cos()).round() as i32;
        let sx = x - dx;
        let sy = y - dy;
        if sx >= 0 && sy >= 0 && sx < prep.width && sy < prep.height {
            samples.push(sample(sx, sy));
        }
    }
    blend_samples(&samples)
}
pub fn spec() -> EffectSpec {
    EffectSpec {
        id: "effect.blur.fragment",
        category: "Blurs",
        label_key: "effect.fragment_blur",
        schema: ParamSchema {
            fields: vec![
                Param {
                    key: "fragments",
                    kind: ParamKind::Int,
                    min: Some(2.0),
                    max: Some(50.0),
                    default: Some(json!(4)),
                },
                Param {
                    key: "distance",
                    kind: ParamKind::Int,
                    min: Some(0.0),
                    max: Some(100.0),
                    default: Some(json!(8)),
                },
                Param {
                    key: "rotation",
                    kind: ParamKind::Float,
                    min: Some(0.0),
                    max: Some(360.0),
                    default: Some(json!(0.0)),
                },
            ],
        },
        contract: ArithmeticContract {
            alpha_form: AlphaForm::Straight,
            working_space: WorkingSpace::Encoded,
            quantization_points: &["offset rounding", "alpha-weighted blend division"],
            rounding: RoundingRule::NearestHalfUp,
            border: BorderRule::Transparent,
            target: "legacy-3.36",
        },
        prepare,
        required_input: neighborhood_input,
        passes: &[PassKind::Neighborhood],
        render_cpu: render,
        gpu: Some(GpuKernel::Fragment),
    }
}
