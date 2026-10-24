// Provisional own target for paint.net 5.1. See https://paint.net/doc/latest/AdjustmentsMenu.html
use crate::effect::*;
use crate::effects::intensity;
use serde_json::{Value, json};

pub fn spec() -> EffectSpec {
    EffectSpec {
        id: "effect.adjust.highlights_shadows",
        category: "Adjustments",
        label_key: "effect.highlights_shadows",
        schema: ParamSchema {
            fields: vec![
                Param {
                    key: "Highlights",
                    kind: ParamKind::Int,
                    min: Some(-100.0),
                    max: Some(100.0),
                    default: Some(json!(0)),
                },
                Param {
                    key: "Shadows",
                    kind: ParamKind::Int,
                    min: Some(-100.0),
                    max: Some(100.0),
                    default: Some(json!(0)),
                },
            ],
        },
        contract: ArithmeticContract {
            alpha_form: AlphaForm::Straight,
            working_space: WorkingSpace::Encoded,
            quantization_points: &["integer intensity", "integer division", "output clamp"],
            rounding: RoundingRule::NearestHalfUp,
            border: BorderRule::Clamp,
            target: "own",
        },
        prepare: prepare_noop,
        required_input: pointwise_input,
        passes: &[PassKind::Pointwise],
        render_cpu,
        gpu: Some(GpuKernel::HighlightsShadows),
    }
}
fn render_cpu(sample: &PixelReader<'_>, x: i32, y: i32, _: &Prepared, params: &Value) -> [u8; 4] {
    let mut pixel = sample(x, y);
    let l = i32::from(intensity(pixel));
    let shadows = params["Shadows"].as_i64().unwrap() as i32;
    let highlights = params["Highlights"].as_i64().unwrap() as i32;
    let shift = (shadows * (255 - l) * (255 - l) + highlights * l * l) / (100 * 255);
    for channel in &mut pixel[..3] {
        *channel = (i32::from(*channel) + shift).clamp(0, 255) as u8;
    }
    pixel
}
