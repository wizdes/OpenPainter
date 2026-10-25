// Ported from OpenPDN 3.36 at cca476b (MIT), UnaryPixelOps.Level and
// HistogramRgb.MakeLevelsAuto. Copyright dotPDN LLC and contributors.
use crate::effect::*;
use serde_json::{Value, json};

pub fn spec() -> EffectSpec {
    EffectSpec {
        id: "effect.adjust.levels",
        category: "Adjustments",
        label_key: "effect.levels",
        schema: ParamSchema {
            fields: vec![
                Param {
                    key: "InputBlack",
                    kind: ParamKind::RgbBytes,
                    min: None,
                    max: None,
                    default: Some(json!([0, 0, 0])),
                },
                Param {
                    key: "InputWhite",
                    kind: ParamKind::RgbBytes,
                    min: None,
                    max: None,
                    default: Some(json!([255, 255, 255])),
                },
                Param {
                    key: "Gamma",
                    kind: ParamKind::RgbGamma,
                    min: None,
                    max: None,
                    default: Some(json!([1.0, 1.0, 1.0])),
                },
                Param {
                    key: "OutputBlack",
                    kind: ParamKind::RgbBytes,
                    min: None,
                    max: None,
                    default: Some(json!([0, 0, 0])),
                },
                Param {
                    key: "OutputWhite",
                    kind: ParamKind::RgbBytes,
                    min: None,
                    max: None,
                    default: Some(json!([255, 255, 255])),
                },
                Param {
                    key: "Auto",
                    kind: ParamKind::Bool,
                    min: None,
                    max: None,
                    default: Some(json!(false)),
                },
            ],
        },
        contract: ArithmeticContract {
            alpha_form: AlphaForm::Straight,
            working_space: WorkingSpace::Encoded,
            quantization_points: &["gamma float32", "f64 power", "output byte truncation"],
            rounding: RoundingRule::NearestHalfUp,
            border: BorderRule::Clamp,
            target: "legacy-3.36",
        },
        prepare,
        required_input: pointwise_input,
        passes: &[PassKind::Reduction, PassKind::Pointwise],
        render_cpu,
        gpu: Some(GpuKernel::Levels),
    }
}

fn prepare(
    doc: &op_core::doc::DocState,
    selection: &op_core::doc::SelectionState,
    params: &Value,
) -> Result<Prepared> {
    if params["Auto"] == true {
        return crate::effects::auto_level::prepare(doc, selection, params);
    }
    let mut table = Box::new([[0u8; 256]; 3]);
    for channel in 0..3 {
        let low = params["InputBlack"][channel].as_u64().unwrap() as u8;
        let high = params["InputWhite"][channel].as_u64().unwrap() as u8;
        let out_low = params["OutputBlack"][channel].as_u64().unwrap() as u8;
        let out_high = params["OutputWhite"][channel].as_u64().unwrap() as u8;
        if high <= low || out_high < out_low {
            return Err(EffectError::Parameters(
                "Levels white point must exceed black point".into(),
            ));
        }
        let gamma = params["Gamma"][channel].as_f64().unwrap() as f32;
        for (value, out) in table[channel].iter_mut().enumerate() {
            *out = if value <= low as usize {
                out_low
            } else if value >= high as usize {
                out_high
            } else {
                (f64::from(out_low)
                    + f64::from(out_high - out_low)
                        * ((value as f64 - f64::from(low)) / f64::from(high - low))
                            .powf(f64::from(gamma))) as u8
            };
        }
    }
    Ok(Prepared {
        channel_lut: Some(table),
        ..Prepared::default()
    })
}

fn render_cpu(sample: &PixelReader<'_>, x: i32, y: i32, prepared: &Prepared, _: &Value) -> [u8; 4] {
    let mut pixel = sample(x, y);
    if let Some(table) = &prepared.channel_lut {
        for channel in 0..3 {
            pixel[channel] = table[channel][pixel[channel] as usize];
        }
    }
    pixel
}
