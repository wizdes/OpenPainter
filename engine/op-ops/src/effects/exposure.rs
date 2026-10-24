// Provisional own target for paint.net 5.1. See https://paint.net/doc/latest/AdjustmentsMenu.html
use crate::effect::*;
use serde_json::{Value, json};

pub fn spec() -> EffectSpec {
    EffectSpec {
        id: "effect.adjust.exposure",
        category: "Adjustments",
        label_key: "effect.exposure",
        schema: ParamSchema {
            fields: vec![Param {
                key: "Exposure",
                kind: ParamKind::Float,
                min: Some(-5.0),
                max: Some(5.0),
                default: Some(json!(0.0)),
            }],
        },
        contract: ArithmeticContract {
            alpha_form: AlphaForm::Straight,
            working_space: WorkingSpace::Encoded,
            quantization_points: &["f64 exposure power", "nearest byte"],
            rounding: RoundingRule::NearestHalfUp,
            border: BorderRule::Clamp,
            target: "own",
        },
        prepare,
        required_input: pointwise_input,
        passes: &[PassKind::Pointwise],
        render_cpu,
        gpu: Some(GpuKernel::Exposure),
    }
}
fn prepare(
    _: &op_core::doc::DocState,
    _: &op_core::doc::SelectionState,
    params: &Value,
) -> Result<Prepared> {
    let factor = 2.0f64.powf(params["Exposure"].as_f64().unwrap());
    let mut table = Box::new([[0u8; 256]; 3]);
    for value in 0..=255 {
        let output = ((value as f64 * factor).round().clamp(0.0, 255.0)) as u8;
        for channel in &mut *table {
            channel[value] = output;
        }
    }
    Ok(Prepared {
        channel_lut: Some(table),
    })
}
fn render_cpu(sample: &PixelReader<'_>, x: i32, y: i32, prepared: &Prepared, _: &Value) -> [u8; 4] {
    let mut pixel = sample(x, y);
    let Some(table) = prepared.channel_lut.as_ref() else {
        return pixel;
    };
    for channel in 0..3 {
        pixel[channel] = table[channel][pixel[channel] as usize];
    }
    pixel
}
