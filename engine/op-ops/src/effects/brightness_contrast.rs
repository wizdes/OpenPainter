// Ported from OpenPDN 3.36 at cca476b (MIT),
// BrightnessAndContrastAdjustment. Copyright dotPDN LLC and contributors.
use crate::effect::*;
use crate::effects::intensity;
use serde_json::json;

pub fn spec() -> EffectSpec {
    EffectSpec {
        id: "effect.adjust.brightness_contrast",
        category: "Adjustments",
        label_key: "effect.brightness_contrast",
        schema: ParamSchema {
            fields: vec![
                Param {
                    key: "Brightness",
                    kind: ParamKind::Int,
                    min: Some(-100.0),
                    max: Some(100.0),
                    default: Some(json!(0)),
                },
                Param {
                    key: "Contrast",
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
            quantization_points: &["intensity truncation", "integer division", "output clamp"],
            rounding: RoundingRule::NearestHalfUp,
            border: BorderRule::Clamp,
            target: "legacy-3.36",
        },
        prepare: prepare_noop,
        required_input: pointwise_input,
        passes: &[PassKind::Pointwise],
        render_cpu: render_pixel,
        gpu: Some(GpuKernel::BrightnessContrast),
    }
}

fn render_pixel(
    sample: &PixelReader<'_>,
    x: i32,
    y: i32,
    _: &Prepared,
    params: &serde_json::Value,
) -> [u8; 4] {
    let mut pixel = sample(x, y);
    let brightness = params["Brightness"].as_i64().unwrap() as i32;
    let contrast = params["Contrast"].as_i64().unwrap() as i32;
    let i = i32::from(intensity(pixel));
    let (multiply, divide) = if contrast < 0 {
        (contrast + 100, 100)
    } else if contrast > 0 {
        (100, 100 - contrast)
    } else {
        (1, 1)
    };
    if divide == 0 {
        let value = if i + brightness < 128 { 0 } else { 255 };
        pixel[..3].fill(value);
    } else {
        let shift = if divide == 100 {
            (i - 127) * multiply / divide + 127 - i + brightness
        } else {
            (i - 127 + brightness) * multiply / divide + 127 - i
        };
        for channel in &mut pixel[..3] {
            *channel = (i32::from(*channel) + shift).clamp(0, 255) as u8;
        }
    }
    pixel
}
