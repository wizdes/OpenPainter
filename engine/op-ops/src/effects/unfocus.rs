//! Port of OpenPDN UnfocusEffect.cs and LocalHistogramEffect.cs, commit cca476b, MIT.
use crate::effect::*;
use op_core::doc::{DocState, SelectionState};
use serde_json::{Value, json};

fn prepare(doc: &DocState, _: &SelectionState, p: &Value) -> Result<Prepared> {
    Ok(Prepared {
        width: doc.width as i32,
        height: doc.height as i32,
        halo: p["radius"].as_i64().unwrap_or(4) as i32,
        ..Prepared::default()
    })
}

fn render(sample: &PixelReader<'_>, x: i32, y: i32, prep: &Prepared, p: &Value) -> [u8; 4] {
    let radius = p["radius"].as_i64().unwrap_or(4) as i32;
    let cutoff = ((2 * radius + 1).pow(2) + 2) / 4;
    let mut area = 0u64;
    let mut alpha = 0u64;
    let mut rgb = [0u64; 3];
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
            alpha += u64::from(c[3]);
            for channel in 0..3 {
                rgb[channel] += u64::from(c[channel]) * u64::from(c[3]);
            }
        }
    }
    if area == 0 {
        return [0; 4];
    }
    [
        (rgb[0] / (area * 255)) as u8,
        (rgb[1] / (area * 255)) as u8,
        (rgb[2] / (area * 255)) as u8,
        (alpha / area) as u8,
    ]
}

pub fn spec() -> EffectSpec {
    EffectSpec {
        id: "effect.blur.unfocus",
        category: "Blurs",
        label_key: "effect.unfocus",
        schema: ParamSchema {
            fields: vec![Param {
                key: "radius",
                kind: ParamKind::Int,
                min: Some(1.0),
                max: Some(200.0),
                default: Some(json!(4)),
            }],
        },
        contract: ArithmeticContract {
            alpha_form: AlphaForm::Straight,
            working_space: WorkingSpace::Encoded,
            quantization_points: &["alpha-weighted channel division", "alpha area division"],
            rounding: RoundingRule::NearestHalfUp,
            border: BorderRule::Transparent,
            target: "legacy-3.36",
        },
        prepare,
        required_input: neighborhood_input,
        passes: &[PassKind::Neighborhood],
        render_cpu: render,
        gpu: Some(GpuKernel::Unfocus),
    }
}
