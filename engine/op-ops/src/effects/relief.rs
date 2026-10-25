//! Port of OpenPDN ReliefEffect.cs and ColorDifferenceEffect.cs,
//! commit cca476b, MIT. paint.net 5.1 docs: https://paint.net/doc/latest/EffectsStylizeMenu.html
use crate::effect::*;
use serde_json::{Value, json};

fn render(sample: &PixelReader<'_>, x: i32, y: i32, prep: &Prepared, p: &Value) -> [u8; 4] {
    super::stylize_common::render(
        sample,
        x,
        y,
        prep,
        p["angle"].as_f64().unwrap_or(45.0),
        false,
        true,
    )
}

pub fn spec() -> EffectSpec {
    EffectSpec {
        id: "effect.stylize.relief",
        category: "Stylize",
        label_key: "effect.relief",
        schema: ParamSchema {
            fields: vec![Param {
                key: "angle",
                kind: ParamKind::Float,
                min: Some(-180.0),
                max: Some(180.0),
                default: Some(json!(45.0)),
            }],
        },
        contract: ArithmeticContract {
            alpha_form: AlphaForm::Straight,
            working_space: WorkingSpace::Encoded,
            quantization_points: &["f64 convolution sum truncates to integer", "clamp to byte"],
            rounding: RoundingRule::NearestHalfUp,
            border: BorderRule::Transparent,
            target: "legacy-3.36",
        },
        prepare: super::stylize_common::prepare,
        required_input: neighborhood_input,
        passes: &[PassKind::Neighborhood],
        render_cpu: render,
        gpu: Some(GpuKernel::Relief),
    }
}
