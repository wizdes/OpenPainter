// Ported from OpenPDN 3.36 at cca476b (MIT), HueAndSaturationAdjustment,
// UnaryPixelOps.HueSaturationLightness, RgbColor, and HsvColor.
// Copyright dotPDN LLC and contributors.
use crate::effect::*;
use crate::effects::intensity;
use serde_json::json;

pub fn spec() -> EffectSpec {
    EffectSpec {
        id: "effect.adjust.hue_saturation",
        category: "Adjustments",
        label_key: "effect.hue_saturation",
        schema: ParamSchema {
            fields: vec![
                Param {
                    key: "Hue",
                    kind: ParamKind::Int,
                    min: Some(-180.0),
                    max: Some(180.0),
                    default: Some(json!(0)),
                },
                Param {
                    key: "Saturation",
                    kind: ParamKind::Int,
                    min: Some(0.0),
                    max: Some(200.0),
                    default: Some(json!(100)),
                },
                Param {
                    key: "Lightness",
                    kind: ParamKind::Int,
                    min: Some(-100.0),
                    max: Some(100.0),
                    default: Some(json!(0)),
                },
            ],
        },
        contract: ArithmeticContract {
            alpha_form: AlphaForm::Straight,
            working_space: WorkingSpace::Encoded,
            quantization_points: &[
                "saturation integer shift",
                "HSV integer conversion",
                "lightness integer blend",
            ],
            rounding: RoundingRule::NearestHalfUp,
            border: BorderRule::Clamp,
            target: "legacy-3.36",
        },
        prepare: prepare_noop,
        required_input: pointwise_input,
        passes: &[PassKind::Pointwise],
        render_cpu: render_pixel,
        gpu: None,
    }
}

fn render_pixel(
    sample: &PixelReader<'_>,
    x: i32,
    y: i32,
    _: &Prepared,
    params: &serde_json::Value,
) -> [u8; 4] {
    let mut pixel = sample(x, y);
    let hue_delta = params["Hue"].as_i64().unwrap() as i32;
    let mut saturation = params["Saturation"].as_i64().unwrap() as i32;
    let lightness = params["Lightness"].as_i64().unwrap() as i32;
    if hue_delta == 0 && saturation == 100 && lightness == 0 {
        return pixel;
    }
    if saturation > 100 {
        saturation = (saturation - 100) * 3 + 100;
    }
    let factor = saturation * 1024 / 100;
    let value = i32::from(intensity(pixel));
    for channel in &mut pixel[..3] {
        *channel =
            ((value * 1024 + (i32::from(*channel) - value) * factor) >> 10).clamp(0, 255) as u8;
    }
    let r = f64::from(pixel[0]) / 255.0;
    let g = f64::from(pixel[1]) / 255.0;
    let b = f64::from(pixel[2]) / 255.0;
    let min = r.min(g.min(b));
    let max = r.max(g.max(b));
    let delta = max - min;
    let (h, s) = if max == 0.0 || delta == 0.0 {
        (0.0, 0.0)
    } else {
        let h = if r == max {
            (g - b) / delta
        } else if g == max {
            2.0 + (b - r) / delta
        } else {
            4.0 + (r - g) / delta
        };
        let h = h * 60.0;
        (if h < 0.0 { h + 360.0 } else { h }, delta / max)
    };
    let hue = ((h as i32 + hue_delta).rem_euclid(360)) as f64;
    let s = (s * 100.0) as i32 as f64 / 100.0;
    let v = (max * 100.0) as i32 as f64 / 100.0;
    let sector_pos = hue / 60.0;
    let sector = sector_pos.floor() as i32;
    let fraction = sector_pos - f64::from(sector);
    let p = v * (1.0 - s);
    let q = v * (1.0 - s * fraction);
    let t = v * (1.0 - s * (1.0 - fraction));
    let rgb = match sector {
        0 => [v, t, p],
        1 => [q, v, p],
        2 => [p, v, t],
        3 => [p, q, v],
        4 => [t, p, v],
        _ => [v, p, q],
    };
    for (channel, value) in pixel[..3].iter_mut().zip(rgb) {
        *channel = (value * 255.0) as u8;
    }
    if lightness != 0 {
        let alpha = lightness.abs() * 255 / 100;
        let target = if lightness > 0 { 255 } else { 0 };
        for channel in &mut pixel[..3] {
            *channel = ((i32::from(*channel) * (255 - alpha) + target * alpha) / 256) as u8;
        }
    }
    pixel
}
