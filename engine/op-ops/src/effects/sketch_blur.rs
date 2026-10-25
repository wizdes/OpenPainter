//! Provisional Sketch Blur from https://paint.net/doc/latest/EffectsBlursMenu.html.
//! The documented P² quantile estimator is approximated with a sampled luminance rank.
use crate::effect::*;
use crate::effects::blur_common;
use serde_json::{Value, json};
fn render(sample: &PixelReader<'_>, x: i32, y: i32, prep: &Prepared, p: &Value) -> [u8; 4] {
    let radius = p["radius"].as_i64().unwrap_or(4) as i32;
    if radius == 0 {
        return sample(x, y);
    }
    let percentile = p["percentile"].as_u64().unwrap_or(50) as usize;
    let smoothness = p["smoothness"].as_i64().unwrap_or(5) as i32;
    let stride = ((11 - smoothness) / 3).clamp(1, 3);
    let mut neighbors = Vec::new();
    for dy in -radius..=radius {
        for dx in -radius..=radius {
            let sx = x + dx;
            let sy = y + dy;
            if sx < 0
                || sy < 0
                || sx >= prep.width
                || sy >= prep.height
                || dx * dx + dy * dy > radius * radius
                || dx.rem_euclid(stride) != 0
                || dy.rem_euclid(stride) != 0
            {
                continue;
            }
            neighbors.push(sample(sx, sy));
        }
    }
    if neighbors.is_empty() {
        return sample(x, y);
    }
    neighbors.sort_by_key(|c| {
        (u32::from(c[0]) * 19595 + u32::from(c[1]) * 38470 + u32::from(c[2]) * 7471) >> 16
    });
    neighbors[(neighbors.len() - 1) * percentile / 100]
}
pub fn spec() -> EffectSpec {
    EffectSpec {
        id: "effect.blur.sketch",
        category: "Blurs",
        label_key: "effect.sketch_blur",
        schema: ParamSchema {
            fields: vec![
                Param {
                    key: "radius",
                    kind: ParamKind::Int,
                    min: Some(0.0),
                    max: Some(200.0),
                    default: Some(json!(4)),
                },
                Param {
                    key: "percentile",
                    kind: ParamKind::Int,
                    min: Some(0.0),
                    max: Some(100.0),
                    default: Some(json!(50)),
                },
                Param {
                    key: "smoothness",
                    kind: ParamKind::Int,
                    min: Some(1.0),
                    max: Some(10.0),
                    default: Some(json!(5)),
                },
            ],
        },
        contract: ArithmeticContract {
            alpha_form: AlphaForm::Straight,
            working_space: WorkingSpace::Encoded,
            quantization_points: &["luminance rank"],
            rounding: RoundingRule::NearestHalfUp,
            border: BorderRule::Transparent,
            target: "own",
        },
        prepare: blur_common::prepare,
        required_input: neighborhood_input,
        passes: &[PassKind::Neighborhood],
        render_cpu: render,
        gpu: Some(GpuKernel::Sketch),
    }
}
