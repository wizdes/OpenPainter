//! Port of OpenPDN InkSketchEffect.cs and GlowEffect.cs, commit cca476b, MIT.
//! InkSketchEffect copyright David Issel (BoltBait), MIT.
//! https://paint.net/doc/latest/EffectsArtisticMenu.html
use crate::effect::*;
use op_core::doc::LayerBlendMode;
use serde_json::{Value, json};

const CONV: [[i32; 5]; 5] = [
    [-1, -1, -1, -1, -1],
    [-1, -1, -1, -1, -1],
    [-1, -1, 30, -1, -1],
    [-1, -1, -1, -1, -1],
    [-1, -1, -5, -1, -1],
];
fn prepare(
    doc: &op_core::doc::DocState,
    _: &op_core::doc::SelectionState,
    _: &Value,
) -> Result<Prepared> {
    Ok(Prepared {
        width: doc.width as i32,
        height: doc.height as i32,
        halo: 6,
        ..Prepared::default()
    })
}
fn render(sample: &PixelReader<'_>, x: i32, y: i32, prep: &Prepared, p: &Value) -> [u8; 4] {
    let original = sample(x, y);
    let coloring = p["coloring"].as_i64().unwrap_or(50);
    let threshold = p["ink_outline"].as_i64().unwrap_or(50) as i32 * 255 / 100;
    let blurred = super::artistic_common::blur(sample, x, y, prep, 6);
    let adjusted = super::artistic_common::brightness_contrast(
        blurred,
        -(coloring - 50) * 2,
        -(coloring - 50) * 2,
    );
    let background = super::artistic_common::blend(original, adjusted, LayerBlendMode::Screen);
    let mut sums = [0i32; 3];
    for (j, row) in CONV.iter().enumerate() {
        let sy = y + j as i32 - 2;
        if sy < 0 || sy >= prep.height {
            continue;
        }
        for (i, &weight) in row.iter().enumerate() {
            let sx = x + i as i32 - 2;
            if sx < 0 || sx >= prep.width {
                continue;
            }
            let color = sample(sx, sy);
            for c in 0..3 {
                sums[c] += i32::from(color[c]) * weight;
            }
        }
    }
    let rgb = sums.map(|sum| sum.clamp(0, 255) as u8);
    let line = if i32::from(super::intensity([rgb[0], rgb[1], rgb[2], 255])) > threshold {
        255
    } else {
        0
    };
    super::artistic_common::blend([line, line, line, 255], background, LayerBlendMode::Darken)
}
pub fn spec() -> EffectSpec {
    EffectSpec {
        id: "effect.artistic.ink_sketch",
        category: "Artistic",
        label_key: "effect.ink_sketch",
        schema: ParamSchema {
            fields: vec![
                Param {
                    key: "ink_outline",
                    kind: ParamKind::Int,
                    min: Some(0.0),
                    max: Some(99.0),
                    default: Some(json!(50)),
                },
                Param {
                    key: "coloring",
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
            quantization_points: &[
                "glow blur",
                "brightness and contrast",
                "5x5 convolution",
                "desaturate",
                "darken",
            ],
            rounding: RoundingRule::NearestHalfUp,
            border: BorderRule::Transparent,
            target: "legacy-3.36",
        },
        prepare,
        required_input: neighborhood_input,
        passes: &[PassKind::Neighborhood],
        render_cpu: render,
        gpu: Some(GpuKernel::InkSketch),
    }
}
