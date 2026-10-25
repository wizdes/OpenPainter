//! Provisional Square Blur from https://paint.net/doc/latest/EffectsBlursMenu.html.
use crate::effect::*;
use crate::effects::blur_common;
use serde_json::{Value, json};
fn render(s: &PixelReader<'_>, x: i32, y: i32, prep: &Prepared, p: &Value) -> [u8; 4] {
    blur_common::average(s, x, y, prep, p, false)
}
pub fn spec() -> EffectSpec {
    EffectSpec {
        id: "effect.blur.square",
        category: "Blurs",
        label_key: "effect.square_blur",
        schema: ParamSchema {
            fields: vec![
                Param {
                    key: "radius",
                    kind: ParamKind::Int,
                    min: Some(0.0),
                    max: Some(200.0),
                    default: Some(json!(4)),
                },
                Param {
                    key: "gamma_boost",
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
            quantization_points: &["gamma weighted average", "nearest channel"],
            rounding: RoundingRule::NearestHalfUp,
            border: BorderRule::Transparent,
            target: "own",
        },
        prepare: blur_common::prepare,
        required_input: neighborhood_input,
        passes: &[PassKind::Neighborhood],
        render_cpu: render,
        gpu: Some(GpuKernel::Square),
    }
}
