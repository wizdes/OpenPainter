// Ported from OpenPDN 3.36 at cca476b (MIT), PosterizeAdjustment.
// Copyright dotPDN LLC and contributors.
use crate::effect::*;
use serde_json::json;

pub fn spec() -> EffectSpec {
    EffectSpec {
        id: "effect.adjust.posterize",
        category: "Adjustments",
        label_key: "effect.posterize",
        schema: ParamSchema {
            fields: vec![
                Param {
                    key: "RedLevels",
                    kind: ParamKind::Int,
                    min: Some(2.0),
                    max: Some(64.0),
                    default: Some(json!(16)),
                },
                Param {
                    key: "GreenLevels",
                    kind: ParamKind::Int,
                    min: Some(2.0),
                    max: Some(64.0),
                    default: Some(json!(16)),
                },
                Param {
                    key: "BlueLevels",
                    kind: ParamKind::Int,
                    min: Some(2.0),
                    max: Some(64.0),
                    default: Some(json!(16)),
                },
                Param {
                    key: "LinkLevels",
                    kind: ParamKind::Bool,
                    min: None,
                    max: None,
                    default: Some(json!(true)),
                },
            ],
        },
        contract: ArithmeticContract {
            alpha_form: AlphaForm::Straight,
            working_space: WorkingSpace::Encoded,
            quantization_points: &["integer levels"],
            rounding: RoundingRule::NearestHalfUp,
            border: BorderRule::Clamp,
            target: "legacy-3.36",
        },
        prepare: prepare_noop,
        required_input: pointwise_input,
        passes: &[PassKind::Pointwise],
        render_cpu: render_pixel,
        gpu: Some(GpuKernel::Posterize),
    }
}

fn level(value: u8, count: usize) -> u8 {
    // Closed form of OpenPDN's running counter. It gives the same byte for
    // every 8-bit input and every allowed level count (2 through 64).
    let j = if value == 0 {
        0
    } else {
        (usize::from(value) * count - 1) / 255
    };
    (255 * j / (count - 1)) as u8
}

fn render_pixel(
    sample: &PixelReader<'_>,
    x: i32,
    y: i32,
    _: &Prepared,
    params: &serde_json::Value,
) -> [u8; 4] {
    let mut pixel = sample(x, y);
    let red = params["RedLevels"].as_u64().unwrap() as usize;
    let linked = params["LinkLevels"].as_bool().unwrap();
    let counts = [
        red,
        if linked {
            red
        } else {
            params["GreenLevels"].as_u64().unwrap() as usize
        },
        if linked {
            red
        } else {
            params["BlueLevels"].as_u64().unwrap() as usize
        },
    ];
    for (channel, count) in pixel[..3].iter_mut().zip(counts) {
        *channel = level(*channel, count);
    }
    pixel
}
