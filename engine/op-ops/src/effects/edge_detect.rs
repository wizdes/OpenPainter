//! Port of OpenPDN EdgeDetectEffect.cs and ColorDifferenceEffect.cs,
//! commit cca476b, MIT. paint.net 5.1 docs: https://paint.net/doc/latest/EffectsStylizeMenu.html
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
        halo: p["blurring"].as_i64().unwrap_or(0) as i32 + 1,
        ..Prepared::default()
    })
}

fn render(sample: &PixelReader<'_>, x: i32, y: i32, prep: &Prepared, p: &Value) -> [u8; 4] {
    let radius = p["blurring"].as_i64().unwrap_or(0) as i32;
    let blurred = |sx: i32, sy: i32| {
        if radius == 0 {
            return sample(sx, sy);
        }
        let mut sums = [0u32; 4];
        let mut count = 0u32;
        for dy in -radius..=radius {
            for dx in -radius..=radius {
                let xx = sx + dx;
                let yy = sy + dy;
                if xx < 0 || yy < 0 || xx >= prep.width || yy >= prep.height {
                    continue;
                }
                let color = sample(xx, yy);
                for c in 0..4 {
                    sums[c] += u32::from(color[c]);
                }
                count += 1;
            }
        }
        if count == 0 {
            [0; 4]
        } else {
            sums.map(|sum| (sum / count) as u8)
        }
    };
    let algorithm = p["algorithm"].as_str().unwrap_or("legacy");
    let mut edge = if algorithm == "legacy" {
        super::stylize_common::render(
            &blurred,
            x,
            y,
            prep,
            p["angle"].as_f64().unwrap_or(45.0),
            false,
            false,
        )
    } else {
        let middle = if algorithm == "sobel" { 2 } else { 1 };
        let mut dx = [0i32; 3];
        let mut dy = [0i32; 3];
        for row in -1..=1 {
            for col in -1..=1 {
                let sx = x + col;
                let sy = y + row;
                if sx < 0 || sy < 0 || sx >= prep.width || sy >= prep.height {
                    continue;
                }
                let color = blurred(sx, sy);
                let wx = col * if row == 0 { middle } else { 1 };
                let wy = row * if col == 0 { middle } else { 1 };
                for c in 0..3 {
                    dx[c] += wx * i32::from(color[c]);
                    dy[c] += wy * i32::from(color[c]);
                }
            }
        }
        let rgb = [0, 1, 2].map(|c| {
            (((dx[c] * dx[c] + dy[c] * dy[c]) as f64).sqrt() / 4.0)
                .round()
                .clamp(0.0, 255.0) as u8
        });
        [rgb[0], rgb[1], rgb[2], 255]
    };
    let strength = p["strength"].as_u64().unwrap_or(100) as u32;
    for (c, channel) in edge[..3].iter_mut().enumerate() {
        *channel = ((u32::from(*channel) * strength + 50) / 100) as u8;
        if p["overlay_edges"].as_bool().unwrap_or(false) {
            *channel = (*channel).max(sample(x, y)[c]);
        }
    }
    edge
}

pub fn spec() -> EffectSpec {
    EffectSpec {
        id: "effect.stylize.edge_detect",
        category: "Stylize",
        label_key: "effect.edge_detect",
        schema: ParamSchema {
            fields: vec![
                Param {
                    key: "angle",
                    kind: ParamKind::Float,
                    min: Some(-180.0),
                    max: Some(180.0),
                    default: Some(json!(45.0)),
                },
                Param {
                    key: "strength",
                    kind: ParamKind::Int,
                    min: Some(0.0),
                    max: Some(100.0),
                    default: Some(json!(100)),
                },
                Param {
                    key: "blurring",
                    kind: ParamKind::Int,
                    min: Some(0.0),
                    max: Some(20.0),
                    default: Some(json!(0)),
                },
                Param {
                    key: "algorithm",
                    kind: ParamKind::Choice(&["legacy", "prewitt", "sobel"]),
                    min: None,
                    max: None,
                    default: Some(json!("legacy")),
                },
                Param {
                    key: "overlay_edges",
                    kind: ParamKind::Bool,
                    min: None,
                    max: None,
                    default: Some(json!(false)),
                },
            ],
        },
        contract: ArithmeticContract {
            alpha_form: AlphaForm::Straight,
            working_space: WorkingSpace::Encoded,
            quantization_points: &["f64 convolution sum truncates to integer", "clamp to byte"],
            rounding: RoundingRule::NearestHalfUp,
            border: BorderRule::Transparent,
            target: "legacy-3.36",
        },
        prepare,
        required_input: neighborhood_input,
        passes: &[PassKind::Neighborhood],
        render_cpu: render,
        gpu: Some(GpuKernel::EdgeDetect),
    }
}
