//! Shared OpenPDN Glow/Pencil operations, commit cca476b, MIT.
use crate::effect::{PixelReader, Prepared};
use op_core::doc::LayerBlendMode;

pub(crate) fn blur(
    sample: &PixelReader<'_>,
    x: i32,
    y: i32,
    prep: &Prepared,
    radius: i64,
) -> [u8; 4] {
    let spec = super::gaussian_blur::spec();
    let mut params = spec.default_params();
    params["radius"] = serde_json::json!(radius);
    (spec.render_cpu)(sample, x, y, prep, &params)
}
pub(crate) fn brightness_contrast(pixel: [u8; 4], brightness: i64, contrast: i64) -> [u8; 4] {
    let spec = super::brightness_contrast::spec();
    (spec.render_cpu)(
        &|_, _| pixel,
        0,
        0,
        &Prepared::default(),
        &serde_json::json!({"Brightness": brightness, "Contrast": contrast}),
    )
}
pub(crate) fn blend(bottom: [u8; 4], top: [u8; 4], mode: LayerBlendMode) -> [u8; 4] {
    // OpenPDN UserBlendOps.Generated.cs uses FastScaleByteByByte and integer
    // division in the generalized blend formula. The order is lhs, rhs.
    fn scale(a: u32, b: u32) -> u32 {
        let n = a * b + 0x80;
        ((n >> 8) + n) >> 8
    }
    let lhs_a = u32::from(bottom[3]);
    let rhs_a = u32::from(top[3]);
    let y = scale(lhs_a, 255 - rhs_a);
    let total = y + rhs_a;
    if total == 0 {
        return [0; 4];
    }
    let x = scale(lhs_a, rhs_a);
    let z = rhs_a - x;
    let mut out = [0; 4];
    for c in 0..3 {
        let lhs = u32::from(bottom[c]);
        let rhs = u32::from(top[c]);
        let blended = match mode {
            LayerBlendMode::Screen => lhs + rhs - scale(lhs, rhs),
            LayerBlendMode::Darken => lhs.min(rhs),
            LayerBlendMode::ColorDodge => {
                if rhs == 255 {
                    255
                } else {
                    (lhs * 255 / (255 - rhs)).min(255)
                }
            }
            _ => unreachable!(),
        };
        out[c] = ((lhs * y + rhs * z + blended * x) / total) as u8;
    }
    out[3] = (y + rhs_a) as u8;
    out
}
