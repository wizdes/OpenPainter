//! Port of OpenPDN SurfaceBlurEffect.cs and LocalHistogramEffect.cs, commit cca476b, MIT.
//! The original source file is excluded from the OpenPDN 3.36 project.
use crate::effect::*;
use op_core::doc::{DocState, SelectionState};
use serde_json::{Value, json};

fn prepare(doc: &DocState, _: &SelectionState, p: &Value) -> Result<Prepared> {
    Ok(Prepared {
        width: doc.width as i32,
        height: doc.height as i32,
        halo: p["radius"].as_i64().unwrap_or(6) as i32,
        ..Prepared::default()
    })
}

fn render(sample: &PixelReader<'_>, x: i32, y: i32, prep: &Prepared, p: &Value) -> [u8; 4] {
    let radius = p["radius"].as_i64().unwrap_or(6) as i32;
    let threshold = p["threshold"].as_i64().unwrap_or(15) as f64;
    let cutoff = ((2 * radius + 1).pow(2) + 2) / 4;
    let center = sample(x, y);
    let mut hist = [[0u32; 256]; 3];
    for dy in -radius..=radius {
        for dx in -radius..=radius {
            let sx = x + dx;
            let sy = y + dy;
            if sx < 0
                || sy < 0
                || sx >= prep.width
                || sy >= prep.height
                || dx * dx + dy * dy > cutoff
            {
                continue;
            }
            let c = sample(sx, sy);
            for channel in 0..3 {
                hist[channel][c[channel] as usize] += 1;
            }
        }
    }
    let mut out = center;
    for channel in 0..3 {
        let mut sum = 0u64;
        let mut divisor = 0u64;
        for (bin, &count) in hist[channel].iter().enumerate() {
            let diff = bin.abs_diff(center[channel] as usize);
            let intensity = (255.0 - diff as f64 * 96.0 / threshold).round().max(0.0) as u64;
            let weight = u64::from(count) * intensity;
            sum += bin as u64 * weight;
            divisor += weight;
        }
        if let Some(value) = (sum + divisor / 2).checked_div(divisor) {
            out[channel] = value as u8;
        }
    }
    out
}

pub fn spec() -> EffectSpec {
    EffectSpec {
        id: "effect.blur.surface",
        category: "Blurs",
        label_key: "effect.surface_blur",
        schema: ParamSchema {
            fields: vec![
                Param {
                    key: "radius",
                    kind: ParamKind::Int,
                    min: Some(1.0),
                    max: Some(100.0),
                    default: Some(json!(6)),
                },
                Param {
                    key: "threshold",
                    kind: ParamKind::Int,
                    min: Some(1.0),
                    max: Some(100.0),
                    default: Some(json!(15)),
                },
            ],
        },
        contract: ArithmeticContract {
            alpha_form: AlphaForm::Straight,
            working_space: WorkingSpace::Encoded,
            quantization_points: &[
                "intensity function midpoint away",
                "weighted channel division",
            ],
            rounding: RoundingRule::NearestHalfUp,
            border: BorderRule::Transparent,
            target: "legacy-3.36",
        },
        prepare,
        required_input: neighborhood_input,
        passes: &[PassKind::Neighborhood],
        render_cpu: render,
        gpu: Some(GpuKernel::Surface),
    }
}
