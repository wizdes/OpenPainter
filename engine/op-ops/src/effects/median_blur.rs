//! Port of OpenPDN MedianEffect.cs and LocalHistogramEffect.cs, commit cca476b, MIT.
use crate::effect::*;
use op_core::doc::{DocState, SelectionState};
use serde_json::{Value, json};
fn prepare(doc: &DocState, _: &SelectionState, p: &Value) -> Result<Prepared> {
    Ok(Prepared {
        width: doc.width as i32,
        height: doc.height as i32,
        halo: p["radius"].as_i64().unwrap_or(10) as i32,
        gaussian: None,
    })
}
fn percentile(hist: &[u32; 256], area: u32, rank: u32) -> u8 {
    let minimum = area * rank / 100;
    let mut value = 0usize;
    let mut count = 0u32;
    while value < 255 && hist[value] == 0 {
        value += 1;
    }
    while value < 255 && count < minimum {
        count += hist[value];
        value += 1;
    }
    value as u8
}
fn render(sample: &PixelReader<'_>, x: i32, y: i32, prep: &Prepared, p: &Value) -> [u8; 4] {
    let radius = p["radius"].as_i64().unwrap_or(10) as i32;
    let rank = p["percentile"].as_u64().unwrap_or(50) as u32;
    let cutoff = ((2 * radius + 1).pow(2) + 2) / 4;
    let mut hist = [[0u32; 256]; 4];
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
            let c = sample(sx, sy);
            area += 1;
            for channel in 0..4 {
                hist[channel][c[channel] as usize] += 1;
            }
        }
    }
    if area == 0 {
        return [0; 4];
    }
    std::array::from_fn(|channel| percentile(&hist[channel], area, rank))
}
pub fn spec() -> EffectSpec {
    EffectSpec {
        id: "effect.blur.median",
        category: "Blurs",
        label_key: "effect.median_blur",
        schema: ParamSchema {
            fields: vec![
                Param {
                    key: "radius",
                    kind: ParamKind::Int,
                    min: Some(1.0),
                    max: Some(200.0),
                    default: Some(json!(10)),
                },
                Param {
                    key: "percentile",
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
            quantization_points: &["rank division"],
            rounding: RoundingRule::NearestHalfUp,
            border: BorderRule::Transparent,
            target: "legacy-3.36",
        },
        prepare,
        required_input: neighborhood_input,
        passes: &[PassKind::Neighborhood],
        render_cpu: render,
        gpu: Some(GpuKernel::Median),
    }
}
