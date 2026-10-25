//! Provisional paint.net 5.1 Vignette from the Photo effect description.
//! https://paint.net/doc/latest/EffectsPhotoMenu.html
use crate::effect::*;
use serde_json::{Value, json};

fn render(sample: &PixelReader<'_>, x: i32, y: i32, prep: &Prepared, p: &Value) -> [u8; 4] {
    let mut pixel = sample(x, y);
    let strength = p["strength"].as_f64().unwrap() / 100.0;
    if strength == 0.0 || prep.width == 0 || prep.height == 0 {
        return pixel;
    }
    let cx = (p["center_x"].as_f64().unwrap() + 1.0) * 0.5;
    let cy = (p["center_y"].as_f64().unwrap() + 1.0) * 0.5;
    let nx = ((f64::from(x) + 0.5) / f64::from(prep.width) - cx) * 2.0;
    let ny = ((f64::from(y) + 0.5) / f64::from(prep.height) - cy) * 2.0;
    let distance = (nx * nx + ny * ny).sqrt();
    let radius = p["radius"].as_f64().unwrap() / 100.0;
    let edge = ((distance - radius) / (1.0 - radius).max(0.001)).clamp(0.0, 1.0);
    let smooth = edge * edge * (3.0 - 2.0 * edge);
    let factor = 1.0 - smooth * strength;
    for byte in &mut pixel[..3] {
        *byte = (f64::from(*byte) * factor).round() as u8;
    }
    pixel
}

pub fn spec() -> EffectSpec {
    EffectSpec {
        id: "effect.photo.vignette",
        category: "Photo",
        label_key: "effect.vignette",
        schema: ParamSchema {
            fields: vec![
                Param {
                    key: "center_x",
                    kind: ParamKind::Float,
                    min: Some(-1.0),
                    max: Some(1.0),
                    default: Some(json!(0.0)),
                },
                Param {
                    key: "center_y",
                    kind: ParamKind::Float,
                    min: Some(-1.0),
                    max: Some(1.0),
                    default: Some(json!(0.0)),
                },
                Param {
                    key: "radius",
                    kind: ParamKind::Int,
                    min: Some(0.0),
                    max: Some(100.0),
                    default: Some(json!(50)),
                },
                Param {
                    key: "strength",
                    kind: ParamKind::Int,
                    min: Some(0.0),
                    max: Some(100.0),
                    default: Some(json!(50)),
                },
            ],
        },
        contract: ArithmeticContract {
            alpha_form: AlphaForm::Straight,
            working_space: WorkingSpace::Encoded,
            quantization_points: &["output byte"],
            rounding: RoundingRule::NearestHalfUp,
            border: BorderRule::Clamp,
            target: "own",
        },
        prepare: prepare_noop,
        required_input: pointwise_input,
        passes: &[PassKind::Pointwise],
        render_cpu: render,
        gpu: Some(GpuKernel::Vignette),
    }
}
