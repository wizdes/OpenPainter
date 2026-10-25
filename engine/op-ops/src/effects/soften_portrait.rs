//! Port of OpenPDN SoftenPortraitEffect.cs composition, commit cca476b, MIT.
//! Copyright dotPDN LLC and contributors.
use crate::effect::*;
use crate::effects::intensity;
use op_core::doc::{DocState, SelectionState};
use serde_json::{Value, json};

fn prepare(doc: &DocState, selection: &SelectionState, p: &Value) -> Result<Prepared> {
    let mut blur = crate::effects::gaussian_blur::spec().default_params();
    blur["radius"] = json!(p["softness"].as_i64().unwrap() * 3);
    (crate::effects::gaussian_blur::spec().prepare)(doc, selection, &blur)
}
fn render(sample: &PixelReader<'_>, x: i32, y: i32, prep: &Prepared, p: &Value) -> [u8; 4] {
    let mut blur_params = crate::effects::gaussian_blur::spec().default_params();
    blur_params["radius"] = json!(p["softness"].as_i64().unwrap() * 3);
    let blur = (crate::effects::gaussian_blur::spec().render_cpu)(sample, x, y, prep, &blur_params);
    let lighting = p["lighting"].as_i64().unwrap();
    let adjusted = (crate::effects::brightness_contrast::spec().render_cpu)(
        &|_, _| blur,
        x,
        y,
        prep,
        &json!({"Brightness":lighting,"Contrast":-lighting / 2}),
    );
    let src = sample(x, y);
    let gray = intensity(src);
    let warmth = p["warmth"].as_i64().unwrap() as f32 / 100.0;
    let lit = [
        (f32::from(gray) * (1.0 + warmth)).clamp(0.0, 255.0) as u8,
        gray,
        (f32::from(gray) * (1.0 - warmth)).clamp(0.0, 255.0) as u8,
        src[3],
    ];
    crate::effects::glow::legacy_blend(lit, adjusted, true)
}
pub fn reference_rgba8(
    width: u32,
    height: u32,
    bytes: &[u8],
    softness: u32,
    lighting: i32,
    warmth: i32,
) -> Vec<u8> {
    let blur = crate::effects::gaussian_blur::reference_rgba8_with_radius(
        width,
        height,
        bytes,
        softness * 3,
    );
    let bc = crate::effects::brightness_contrast::spec();
    let params = json!({"Brightness":lighting,"Contrast":-lighting / 2});
    let mut output = Vec::with_capacity(bytes.len());
    for (src, blurred) in bytes.chunks_exact(4).zip(blur.chunks_exact(4)) {
        let src: [u8; 4] = src.try_into().unwrap();
        let blurred: [u8; 4] = blurred.try_into().unwrap();
        let adjusted = (bc.render_cpu)(&|_, _| blurred, 0, 0, &Prepared::default(), &params);
        let gray = intensity(src);
        let warm = warmth as f32 / 100.0;
        let lit = [
            (f32::from(gray) * (1.0 + warm)).clamp(0.0, 255.0) as u8,
            gray,
            (f32::from(gray) * (1.0 - warm)).clamp(0.0, 255.0) as u8,
            src[3],
        ];
        output.extend_from_slice(&crate::effects::glow::legacy_blend(lit, adjusted, true));
    }
    output
}
pub fn spec() -> EffectSpec {
    EffectSpec {
        id: "effect.photo.soften_portrait",
        category: "Photo",
        label_key: "effect.soften_portrait",
        schema: ParamSchema {
            fields: vec![
                Param {
                    key: "softness",
                    kind: ParamKind::Int,
                    min: Some(0.0),
                    max: Some(10.0),
                    default: Some(json!(5)),
                },
                Param {
                    key: "lighting",
                    kind: ParamKind::Int,
                    min: Some(-20.0),
                    max: Some(20.0),
                    default: Some(json!(0)),
                },
                Param {
                    key: "warmth",
                    kind: ParamKind::Int,
                    min: Some(0.0),
                    max: Some(20.0),
                    default: Some(json!(10)),
                },
            ],
        },
        contract: ArithmeticContract {
            alpha_form: AlphaForm::Straight,
            working_space: WorkingSpace::Encoded,
            quantization_points: &["Gaussian blur", "brightness contrast", "overlay blend"],
            rounding: RoundingRule::NearestHalfUp,
            border: BorderRule::Transparent,
            target: "legacy-3.36",
        },
        prepare,
        required_input: neighborhood_input,
        passes: &[PassKind::Neighborhood, PassKind::Pointwise],
        render_cpu: render,
        gpu: Some(GpuKernel::SoftenPortrait),
    }
}
