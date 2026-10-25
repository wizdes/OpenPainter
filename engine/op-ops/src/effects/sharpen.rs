//! Port of OpenPDN SharpenEffect.cs and LocalHistogramEffect.cs,
//! commit cca476b, MIT. Copyright dotPDN LLC and contributors.
use crate::effect::*;
use op_core::doc::{DocState, SelectionState};
use serde_json::{Value, json};

fn prepare(doc: &DocState, _: &SelectionState, p: &Value) -> Result<Prepared> {
    Ok(Prepared {
        width: doc.width as i32,
        height: doc.height as i32,
        halo: p["amount"].as_i64().unwrap() as i32,
        ..Prepared::default()
    })
}
fn render(sample: &PixelReader<'_>, x: i32, y: i32, prep: &Prepared, p: &Value) -> [u8; 4] {
    let src = sample(x, y);
    let median = (crate::effects::median_blur::spec().render_cpu)(
        sample,
        x,
        y,
        prep,
        &json!({"radius":p["amount"],"percentile":50}),
    );
    std::array::from_fn(|channel| {
        (1.5 * f32::from(src[channel]) - 0.5 * f32::from(median[channel])).clamp(0.0, 255.0) as u8
    })
}
pub fn spec() -> EffectSpec {
    EffectSpec {
        id: "effect.photo.sharpen",
        category: "Photo",
        label_key: "effect.sharpen",
        schema: ParamSchema {
            fields: vec![Param {
                key: "amount",
                kind: ParamKind::Int,
                min: Some(1.0),
                max: Some(20.0),
                default: Some(json!(2)),
            }],
        },
        contract: ArithmeticContract {
            alpha_form: AlphaForm::Straight,
            working_space: WorkingSpace::Encoded,
            quantization_points: &["rank division", "output clamp"],
            rounding: RoundingRule::NearestHalfUp,
            border: BorderRule::Transparent,
            target: "legacy-3.36",
        },
        prepare,
        required_input: neighborhood_input,
        passes: &[PassKind::Neighborhood],
        render_cpu: render,
        gpu: Some(GpuKernel::Sharpen),
    }
}
