//! Port of OpenPDN OilPaintingEffect.cs, commit cca476b, MIT.
//! https://paint.net/doc/latest/EffectsArtisticMenu.html
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
        halo: p["brush_size"].as_i64().unwrap_or(3) as i32,
        ..Prepared::default()
    })
}
fn render(sample: &PixelReader<'_>, x: i32, y: i32, prep: &Prepared, p: &Value) -> [u8; 4] {
    let radius = p["brush_size"].as_i64().unwrap_or(3) as i32;
    let coarse = p["coarseness"].as_u64().unwrap_or(50) as usize;
    let mut counts = [0u32; 256];
    let mut sums = [[0u32; 4]; 256];
    for sy in (y - radius).max(0)..=(y + radius).min(prep.height - 1) {
        for sx in (x - radius).max(0)..=(x + radius).min(prep.width - 1) {
            let color = sample(sx, sy);
            let n = u32::from(super::intensity(color)) * coarse as u32 + 0x80;
            let bin = (((n >> 8) + n) >> 8) as usize;
            counts[bin] += 1;
            for c in 0..4 {
                sums[bin][c] += u32::from(color[c]);
            }
        }
    }
    let chosen = (0..=coarse)
        .max_by_key(|&bin| (counts[bin], std::cmp::Reverse(bin)))
        .unwrap();
    let count = counts[chosen].max(1);
    sums[chosen].map(|sum| (sum / count) as u8)
}
pub fn spec() -> EffectSpec {
    EffectSpec {
        id: "effect.artistic.oil_painting",
        category: "Artistic",
        label_key: "effect.oil_painting",
        schema: ParamSchema {
            fields: vec![
                Param {
                    key: "brush_size",
                    kind: ParamKind::Int,
                    min: Some(1.0),
                    max: Some(8.0),
                    default: Some(json!(3)),
                },
                Param {
                    key: "coarseness",
                    kind: ParamKind::Int,
                    min: Some(3.0),
                    max: Some(255.0),
                    default: Some(json!(50)),
                },
            ],
        },
        contract: ArithmeticContract {
            alpha_form: AlphaForm::Straight,
            working_space: WorkingSpace::Encoded,
            quantization_points: &[
                "intensity truncation",
                "FastScaleByteByByte",
                "mean truncation",
            ],
            rounding: RoundingRule::NearestHalfUp,
            border: BorderRule::Transparent,
            target: "legacy-3.36",
        },
        prepare,
        required_input: neighborhood_input,
        passes: &[PassKind::Neighborhood],
        render_cpu: render,
        gpu: Some(GpuKernel::OilPainting),
    }
}
