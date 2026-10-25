//! Port of OpenPDN OutlineEffect.cs and LocalHistogramEffect.cs, commit cca476b, MIT.
//! https://paint.net/doc/latest/EffectsStylizeMenu.html
use crate::effect::*;
use serde_json::{Value, json};

fn prepare(
    doc: &op_core::doc::DocState,
    _: &op_core::doc::SelectionState,
    p: &Value,
) -> Result<Prepared> {
    Ok(Prepared {
        width: doc.width as i32,
        height: doc.height as i32,
        halo: p["thickness"].as_i64().unwrap_or(3) as i32
            + i32::from(p["quality"].as_str() == Some("high")),
        ..Prepared::default()
    })
}
fn spread(hist: &[u32; 256], area: i32, intensity: i32) -> u8 {
    let low = area * (100 - intensity) / 200;
    let high = area * (100 + intensity) / 200;
    let mut count = 0;
    let mut first = 0;
    while first < 255 && hist[first] == 0 {
        first += 1;
    }
    while first < 255 && count < low {
        count += hist[first] as i32;
        first += 1;
    }
    let mut last = first;
    while last < 255 && count < high {
        count += hist[last] as i32;
        last += 1;
    }
    (255 - (last - first)) as u8
}
fn legacy_alpha(blue: &[u32; 256], alpha: &[u32; 256], area: i32, intensity: i32) -> u8 {
    let low = area * (100 - intensity) / 200;
    let high = area * (100 + intensity) / 200;
    let mut first = 0;
    while first < 255 && blue[first] == 0 {
        first += 1;
    }
    let mut count = 0;
    while first < 255 && count < low {
        count += alpha[first] as i32;
        first += 1;
    }
    let mut last = first;
    while last < 255 && count < high {
        count += alpha[last] as i32;
        last += 1;
    }
    last as u8
}
fn render(sample: &PixelReader<'_>, x: i32, y: i32, prep: &Prepared, p: &Value) -> [u8; 4] {
    let radius = p["thickness"].as_i64().unwrap_or(3) as i32
        + i32::from(p["quality"].as_str() == Some("high"));
    let intensity = p["intensity"].as_i64().unwrap_or(50) as i32;
    let cutoff = ((radius * 2 + 1).pow(2) + 2) / 4;
    let mut hist = [[0u32; 256]; 4];
    let mut area = 0;
    for dy in -radius..=radius {
        let sy = y + dy;
        if sy < 0 || sy >= prep.height {
            continue;
        }
        for dx in -radius..=radius {
            let sx = x + dx;
            if sx < 0 || sx >= prep.width || dx * dx + dy * dy > cutoff {
                continue;
            }
            if p["quality"].as_str() == Some("fast") && (dx & 1 != 0 || dy & 1 != 0) {
                continue;
            }
            let color = sample(sx, sy);
            for c in 0..4 {
                hist[c][color[c] as usize] += 1;
            }
            area += 1;
        }
    }
    let a = legacy_alpha(&hist[2], &hist[3], area, intensity);
    [
        spread(&hist[0], area, intensity),
        spread(&hist[1], area, intensity),
        spread(&hist[2], area, intensity),
        a,
    ]
}
pub fn spec() -> EffectSpec {
    EffectSpec {
        id: "effect.stylize.outline",
        category: "Stylize",
        label_key: "effect.outline",
        schema: ParamSchema {
            fields: vec![
                Param {
                    key: "thickness",
                    kind: ParamKind::Int,
                    min: Some(1.0),
                    max: Some(200.0),
                    default: Some(json!(3)),
                },
                Param {
                    key: "intensity",
                    kind: ParamKind::Int,
                    min: Some(0.0),
                    max: Some(100.0),
                    default: Some(json!(50)),
                },
                Param {
                    key: "quality",
                    kind: ParamKind::Choice(&["fast", "standard", "high"]),
                    min: None,
                    max: None,
                    default: Some(json!("standard")),
                },
            ],
        },
        contract: ArithmeticContract {
            alpha_form: AlphaForm::Straight,
            working_space: WorkingSpace::Encoded,
            quantization_points: &["integer histogram thresholds"],
            rounding: RoundingRule::NearestHalfUp,
            border: BorderRule::Transparent,
            target: "legacy-3.36",
        },
        prepare,
        required_input: neighborhood_input,
        passes: &[PassKind::Neighborhood],
        render_cpu: render,
        gpu: Some(GpuKernel::Outline),
    }
}
