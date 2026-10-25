//! Port of OpenPDN ReduceNoiseEffect.cs and LocalHistogramEffect.cs,
//! commit cca476b, MIT. Copyright dotPDN LLC and contributors.
use crate::effect::*;
use op_core::doc::{DocState, SelectionState};
use serde_json::{Value, json};

fn prepare(doc: &DocState, _: &SelectionState, p: &Value) -> Result<Prepared> {
    Ok(Prepared {
        width: doc.width as i32,
        height: doc.height as i32,
        halo: p["radius"].as_i64().unwrap() as i32,
        ..Prepared::default()
    })
}

fn render(sample: &PixelReader<'_>, x: i32, y: i32, prep: &Prepared, p: &Value) -> [u8; 4] {
    let color = sample(x, y);
    let radius = p["radius"].as_i64().unwrap() as i32;
    let cutoff = ((2 * radius + 1).pow(2) + 2) / 4;
    let mut counts = [0u32; 3];
    let mut area = 0u32;
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
            let neighbor = sample(sx, sy);
            area += 1;
            for channel in 0..3 {
                if neighbor[channel] < color[channel] {
                    counts[channel] += 1;
                }
            }
        }
    }
    if area == 0 {
        return color;
    }
    let strength = p["strength"].as_f64().unwrap();
    let intensity =
        (0.299 * f64::from(color[0]) + 0.587 * f64::from(color[1]) + 0.114 * f64::from(color[2]))
            / 255.0;
    let lerp = -0.2 * strength * (1.0 - 0.75 * intensity);
    let mut out = color;
    for channel in 0..3 {
        let normalized = (counts[channel] * 255 / area) as f64;
        out[channel] = (f64::from(color[channel]) + (normalized - f64::from(color[channel])) * lerp)
            .clamp(0.0, 255.0) as u8;
    }
    // OpenPDN's normalized color is opaque, so alpha is also interpolated.
    out[3] = (f64::from(color[3]) + (255.0 - f64::from(color[3])) * lerp).clamp(0.0, 255.0) as u8;
    out
}

pub fn spec() -> EffectSpec {
    EffectSpec {
        id: "effect.noise.reduce",
        category: "Noise",
        label_key: "effect.reduce_noise",
        schema: ParamSchema {
            fields: vec![
                Param {
                    key: "radius",
                    kind: ParamKind::Int,
                    min: Some(0.0),
                    max: Some(200.0),
                    default: Some(json!(10)),
                },
                Param {
                    key: "strength",
                    kind: ParamKind::Float,
                    min: Some(0.0),
                    max: Some(1.0),
                    default: Some(json!(0.4)),
                },
            ],
        },
        contract: ArithmeticContract {
            alpha_form: AlphaForm::Straight,
            working_space: WorkingSpace::Encoded,
            quantization_points: &["histogram percentile", "output clamp"],
            rounding: RoundingRule::NearestHalfUp,
            border: BorderRule::Transparent,
            target: "legacy-3.36",
        },
        prepare,
        required_input: neighborhood_input,
        passes: &[PassKind::Neighborhood],
        render_cpu: render,
        gpu: Some(GpuKernel::ReduceNoise),
    }
}
