//! Port of OpenPDN GlowEffect.cs composition, commit cca476b, MIT.
//! Copyright dotPDN LLC and contributors.
use crate::effect::*;
use op_core::doc::{DocState, SelectionState};
use serde_json::{Value, json};

fn prepare(doc: &DocState, selection: &SelectionState, p: &Value) -> Result<Prepared> {
    let mut blur = crate::effects::gaussian_blur::spec().default_params();
    blur["radius"] = p["radius"].clone();
    (crate::effects::gaussian_blur::spec().prepare)(doc, selection, &blur)
}
fn render(sample: &PixelReader<'_>, x: i32, y: i32, prep: &Prepared, p: &Value) -> [u8; 4] {
    let mut blur_params = crate::effects::gaussian_blur::spec().default_params();
    blur_params["radius"] = p["radius"].clone();
    let blur = (crate::effects::gaussian_blur::spec().render_cpu)(sample, x, y, prep, &blur_params);
    let adjusted = (crate::effects::brightness_contrast::spec().render_cpu)(
        &|_, _| blur,
        x,
        y,
        prep,
        &json!({"Brightness":p["brightness"],"Contrast":p["contrast"]}),
    );
    let src = sample(x, y);
    legacy_blend(adjusted, src, false)
}
fn scale(a: i32, b: i32) -> i32 {
    let value = a * b + 128;
    (value + (value >> 8)) >> 8
}
pub(crate) fn legacy_blend(lhs: [u8; 4], rhs: [u8; 4], overlay: bool) -> [u8; 4] {
    let lhs_a = i32::from(lhs[3]);
    let rhs_a = i32::from(rhs[3]);
    let y = scale(lhs_a, 255 - rhs_a);
    let total_a = y + rhs_a;
    if total_a == 0 {
        return [0; 4];
    }
    let x = scale(lhs_a, rhs_a);
    let z = rhs_a - x;
    let mut out = [0; 4];
    for channel in 0..3 {
        let a = i32::from(lhs[channel]);
        let b = i32::from(rhs[channel]);
        let blended = if overlay {
            if a < 128 {
                scale(2 * a, b)
            } else {
                255 - scale(2 * (255 - a), 255 - b)
            }
        } else {
            a + b - scale(a, b)
        };
        out[channel] = ((a * y + b * z + blended * x) / total_a) as u8;
    }
    out[3] = total_a as u8;
    out
}
pub fn reference_rgba8(
    width: u32,
    height: u32,
    bytes: &[u8],
    radius: u32,
    brightness: i32,
    contrast: i32,
) -> Vec<u8> {
    let blur =
        crate::effects::gaussian_blur::reference_rgba8_with_radius(width, height, bytes, radius);
    let bc = crate::effects::brightness_contrast::spec();
    let params = json!({"Brightness":brightness,"Contrast":contrast});
    let mut output = Vec::with_capacity(bytes.len());
    for (src, blurred) in bytes.chunks_exact(4).zip(blur.chunks_exact(4)) {
        let blurred: [u8; 4] = blurred.try_into().unwrap();
        let adjusted = (bc.render_cpu)(&|_, _| blurred, 0, 0, &Prepared::default(), &params);
        output.extend_from_slice(&legacy_blend(adjusted, src.try_into().unwrap(), false));
    }
    output
}
pub fn spec() -> EffectSpec {
    EffectSpec {
        id: "effect.photo.glow",
        category: "Photo",
        label_key: "effect.glow",
        schema: ParamSchema {
            fields: vec![
                Param {
                    key: "radius",
                    kind: ParamKind::Int,
                    min: Some(1.0),
                    max: Some(20.0),
                    default: Some(json!(6)),
                },
                Param {
                    key: "brightness",
                    kind: ParamKind::Int,
                    min: Some(-100.0),
                    max: Some(100.0),
                    default: Some(json!(10)),
                },
                Param {
                    key: "contrast",
                    kind: ParamKind::Int,
                    min: Some(-100.0),
                    max: Some(100.0),
                    default: Some(json!(10)),
                },
            ],
        },
        contract: ArithmeticContract {
            alpha_form: AlphaForm::Straight,
            working_space: WorkingSpace::Encoded,
            quantization_points: &["Gaussian blur", "brightness contrast", "screen blend"],
            rounding: RoundingRule::NearestHalfUp,
            border: BorderRule::Transparent,
            target: "legacy-3.36",
        },
        prepare,
        required_input: neighborhood_input,
        passes: &[PassKind::Neighborhood, PassKind::Pointwise],
        render_cpu: render,
        gpu: Some(GpuKernel::Glow),
    }
}
