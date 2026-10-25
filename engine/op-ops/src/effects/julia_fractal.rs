//! Port of OpenPDN JuliaFractalEffect.cs at commit cca476b, MIT.
//! Copyright dotPDN LLC and contributors.
use crate::effect::*;
use serde_json::{Value, json};

fn render(sample: &PixelReader<'_>, x: i32, y: i32, prep: &Prepared, p: &Value) -> [u8; 4] {
    let options = super::fractal_common::FractalOptions {
        factor: p["factor"].as_f64().unwrap(),
        zoom: p["zoom"].as_f64().unwrap(),
        angle: p["angle"].as_f64().unwrap(),
        quality: p["quality"].as_i64().unwrap() as i32,
        mandelbrot: false,
        invert: false,
    };
    let fractal = super::fractal_common::pixel(x, y, prep.width, prep.height, &options);
    super::fractal_common::provisional_blend(
        sample(x, y),
        fractal,
        p["blend_mode"].as_str().unwrap(),
    )
}
pub fn spec() -> EffectSpec {
    EffectSpec {
        id: "effect.render.julia_fractal",
        category: "Render",
        label_key: "effect.julia_fractal",
        schema: ParamSchema {
            fields: vec![
                Param {
                    key: "factor",
                    kind: ParamKind::Float,
                    min: Some(1.0),
                    max: Some(10.0),
                    default: Some(json!(4.0)),
                },
                Param {
                    key: "zoom",
                    kind: ParamKind::Float,
                    min: Some(0.1),
                    max: Some(50.0),
                    default: Some(json!(1.0)),
                },
                Param {
                    key: "angle",
                    kind: ParamKind::Float,
                    min: Some(-180.0),
                    max: Some(180.0),
                    default: Some(json!(0.0)),
                },
                Param {
                    key: "quality",
                    kind: ParamKind::Int,
                    min: Some(1.0),
                    max: Some(5.0),
                    default: Some(json!(2)),
                },
                Param {
                    key: "blend_mode",
                    kind: ParamKind::Choice(super::fractal_common::BLEND_MODES),
                    min: None,
                    max: None,
                    default: Some(json!("normal")),
                },
            ],
        },
        contract: ArithmeticContract {
            alpha_form: AlphaForm::Straight,
            working_space: WorkingSpace::Encoded,
            quantization_points: &["each subsample clamp to byte", "integer sample average"],
            rounding: RoundingRule::NearestHalfUp,
            border: BorderRule::Clamp,
            target: "legacy-3.36",
        },
        prepare: prepare_noop,
        required_input: pointwise_input,
        passes: &[PassKind::Iterative],
        render_cpu: render,
        gpu: None,
    }
}
