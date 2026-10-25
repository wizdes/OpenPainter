//! Port of OpenPDN RedEyeRemoveEffect.cs and UnaryPixelOps.RedEyeRemove,
//! commit cca476b, MIT. Copyright dotPDN LLC and contributors.
use crate::effect::*;
use serde_json::{Value, json};

fn render(sample: &PixelReader<'_>, x: i32, y: i32, _: &Prepared, p: &Value) -> [u8; 4] {
    let mut color = sample(x, y);
    let max = *color[..3].iter().max().unwrap();
    let min = *color[..3].iter().min().unwrap();
    let saturation = if max == 0 {
        0
    } else {
        (u32::from(max - min) * 255 / u32::from(max)) as i32
    };
    let difference = i32::from(color[0]) - i32::from(color[1].max(color[2]));
    if difference > p["tolerance"].as_i64().unwrap() as i32 && saturation > 100 {
        let intensity =
            0.299 * f64::from(color[0]) + 0.587 * f64::from(color[1]) + 0.114 * f64::from(color[2]);
        color[0] = (intensity * p["saturation"].as_f64().unwrap() / 100.0) as u8;
    }
    color
}

pub fn spec() -> EffectSpec {
    EffectSpec {
        id: "effect.photo.red_eye_removal",
        category: "Photo",
        label_key: "effect.red_eye_removal",
        schema: ParamSchema {
            fields: vec![
                Param {
                    key: "tolerance",
                    kind: ParamKind::Int,
                    min: Some(0.0),
                    max: Some(100.0),
                    default: Some(json!(70)),
                },
                Param {
                    key: "saturation",
                    kind: ParamKind::Int,
                    min: Some(0.0),
                    max: Some(100.0),
                    default: Some(json!(90)),
                },
            ],
        },
        contract: ArithmeticContract {
            alpha_form: AlphaForm::Straight,
            working_space: WorkingSpace::Encoded,
            quantization_points: &["saturation truncation", "intensity truncation"],
            rounding: RoundingRule::NearestHalfUp,
            border: BorderRule::Clamp,
            target: "legacy-3.36",
        },
        prepare: prepare_noop,
        required_input: pointwise_input,
        passes: &[PassKind::Pointwise],
        render_cpu: render,
        gpu: Some(GpuKernel::RedEyeRemoval),
    }
}
