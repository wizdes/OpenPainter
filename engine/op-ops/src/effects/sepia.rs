// Ported from OpenPDN 3.36 at cca476b (MIT), SepiaEffect and
// UnaryPixelOps.Level. Copyright dotPDN LLC and contributors.
use crate::effect::*;
use crate::effects::intensity;
use serde_json::json;

pub fn spec() -> EffectSpec {
    EffectSpec {
        id: "effect.adjust.sepia",
        category: "Adjustments",
        label_key: "effect.sepia",
        // paint.net 5.x adds Intensity: 0 gray, 50 legacy, 100 stronger.
        // https://www.getpaint.net/doc/latest/AdjustmentsMenu.html
        schema: ParamSchema {
            fields: vec![Param {
                key: "Intensity",
                kind: ParamKind::Int,
                min: Some(0.0),
                max: Some(100.0),
                default: Some(json!(50)),
            }],
        },
        contract: ArithmeticContract {
            alpha_form: AlphaForm::Straight,
            working_space: WorkingSpace::Encoded,
            quantization_points: &["intensity truncation", "levels output truncation"],
            rounding: RoundingRule::NearestHalfUp,
            border: BorderRule::Clamp,
            target: "legacy-3.36",
        },
        prepare: prepare_noop,
        required_input: pointwise_input,
        passes: &[PassKind::Pointwise],
        render_cpu: |sample, x, y, _, params| {
            let pixel = sample(x, y);
            let value = intensity(pixel);
            let strength = params["Intensity"].as_i64().unwrap() as f64 / 100.0;
            let level = |gamma: f64| -> u8 {
                if value == 0 || value == 255 {
                    value
                } else {
                    (255.0 * (f64::from(value) / 255.0).powf(gamma)) as u8
                }
            };
            [
                level(1.0 - 0.4 * strength),
                level(1.0),
                level(1.0 + 0.4 * strength),
                pixel[3],
            ]
        },
        gpu: Some(GpuKernel::Sepia),
    }
}
