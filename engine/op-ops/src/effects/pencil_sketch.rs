//! Port of OpenPDN PencilSketchEffect.cs, commit cca476b, MIT.
//! https://paint.net/doc/latest/EffectsArtisticMenu.html
use crate::effect::*;
use op_core::doc::LayerBlendMode;
use serde_json::{Value, json};

fn prepare(
    doc: &op_core::doc::DocState,
    _: &op_core::doc::SelectionState,
    p: &Value,
) -> Result<Prepared> {
    Ok(Prepared {
        width: doc.width as i32,
        height: doc.height as i32,
        halo: p["pencil_tip_size"].as_i64().unwrap_or(2) as i32,
        ..Prepared::default()
    })
}
fn render(sample: &PixelReader<'_>, x: i32, y: i32, prep: &Prepared, p: &Value) -> [u8; 4] {
    let original = sample(x, y);
    let radius = p["pencil_tip_size"].as_i64().unwrap_or(2);
    let range = p["range"].as_i64().unwrap_or(0);
    let blurred = super::artistic_common::blur(sample, x, y, prep, radius);
    let adjusted = super::artistic_common::brightness_contrast(blurred, range, -range);
    let gray = super::intensity([
        255 - adjusted[0],
        255 - adjusted[1],
        255 - adjusted[2],
        adjusted[3],
    ]);
    let source_gray = super::intensity(original);
    super::artistic_common::blend(
        [source_gray, source_gray, source_gray, original[3]],
        [gray, gray, gray, adjusted[3]],
        LayerBlendMode::ColorDodge,
    )
}
pub fn spec() -> EffectSpec {
    EffectSpec {
        id: "effect.artistic.pencil_sketch",
        category: "Artistic",
        label_key: "effect.pencil_sketch",
        schema: ParamSchema {
            fields: vec![
                Param {
                    key: "pencil_tip_size",
                    kind: ParamKind::Int,
                    min: Some(1.0),
                    max: Some(20.0),
                    default: Some(json!(2)),
                },
                Param {
                    key: "range",
                    kind: ParamKind::Int,
                    min: Some(-20.0),
                    max: Some(20.0),
                    default: Some(json!(0)),
                },
            ],
        },
        contract: ArithmeticContract {
            alpha_form: AlphaForm::Straight,
            working_space: WorkingSpace::Encoded,
            quantization_points: &[
                "blur",
                "brightness and contrast",
                "desaturate",
                "color dodge",
            ],
            rounding: RoundingRule::NearestHalfUp,
            border: BorderRule::Transparent,
            target: "legacy-3.36",
        },
        prepare,
        required_input: neighborhood_input,
        passes: &[PassKind::Neighborhood],
        render_cpu: render,
        gpu: Some(GpuKernel::PencilSketch),
    }
}
