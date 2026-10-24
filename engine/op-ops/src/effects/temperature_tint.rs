// Provisional OpenPainter arithmetic, based on paint.net's description of
// warmth/coolness and tint in https://www.getpaint.net/doc/latest/AdjustmentsMenu.html.
// Replace the provisional range and arithmetic after a paint.net 5.1 capture.
use crate::effect::*;
use serde_json::json;

pub fn spec() -> EffectSpec {
    EffectSpec {
        id: "effect.adjust.temperature_tint",
        category: "Adjustments",
        label_key: "effect.temperature_tint",
        schema: ParamSchema {
            fields: vec![
                Param {
                    key: "Temperature",
                    kind: ParamKind::Int,
                    min: Some(-100.0),
                    max: Some(100.0),
                    default: Some(json!(0)),
                },
                Param {
                    key: "Tint",
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
            quantization_points: &["integer channel shift", "output clamp"],
            rounding: RoundingRule::NearestHalfUp,
            border: BorderRule::Clamp,
            target: "own",
        },
        prepare: prepare_noop,
        required_input: pointwise_input,
        passes: &[PassKind::Pointwise],
        render_cpu: |sample, x, y, _, params| {
            let mut pixel = sample(x, y);
            let temperature = params["Temperature"].as_i64().unwrap() as i32;
            let tint = params["Tint"].as_i64().unwrap() as i32;
            let shifts = [
                temperature / 2 + tint / 2,
                temperature / 2 - tint,
                -temperature + tint / 2,
            ];
            for (channel, shift) in pixel[..3].iter_mut().zip(shifts) {
                *channel = (i32::from(*channel) + shift).clamp(0, 255) as u8;
            }
            pixel
        },
        gpu: Some(GpuKernel::TemperatureTint),
    }
}
