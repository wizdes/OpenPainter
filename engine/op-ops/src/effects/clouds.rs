//! Port of OpenPDN CloudsEffect.cs at commit cca476b, MIT.
//! Copyright dotPDN LLC and contributors. The legacy oracle fixes instance seed to 0x5a.
use crate::effect::*;
use op_core::doc::LayerBlendMode;
use serde_json::{Value, json};

const PERM: [u8; 256] = [
    151, 160, 137, 91, 90, 15, 131, 13, 201, 95, 96, 53, 194, 233, 7, 225, 140, 36, 103, 30, 69,
    142, 8, 99, 37, 240, 21, 10, 23, 190, 6, 148, 247, 120, 234, 75, 0, 26, 197, 62, 94, 252, 219,
    203, 117, 35, 11, 32, 57, 177, 33, 88, 237, 149, 56, 87, 174, 20, 125, 136, 171, 168, 68, 175,
    74, 165, 71, 134, 139, 48, 27, 166, 77, 146, 158, 231, 83, 111, 229, 122, 60, 211, 133, 230,
    220, 105, 92, 41, 55, 46, 245, 40, 244, 102, 143, 54, 65, 25, 63, 161, 1, 216, 80, 73, 209, 76,
    132, 187, 208, 89, 18, 169, 200, 196, 135, 130, 116, 188, 159, 86, 164, 100, 109, 198, 173,
    186, 3, 64, 52, 217, 226, 250, 124, 123, 5, 202, 38, 147, 118, 126, 255, 82, 85, 212, 207, 206,
    59, 227, 47, 16, 58, 17, 182, 189, 28, 42, 223, 183, 170, 213, 119, 248, 152, 2, 44, 154, 163,
    70, 221, 153, 101, 155, 167, 43, 172, 9, 129, 22, 39, 253, 19, 98, 108, 110, 79, 113, 224, 232,
    178, 185, 112, 104, 218, 246, 97, 228, 251, 34, 242, 193, 238, 210, 144, 12, 191, 179, 162,
    241, 81, 51, 145, 235, 249, 14, 239, 107, 49, 192, 214, 31, 181, 199, 106, 157, 184, 84, 204,
    176, 115, 121, 50, 45, 127, 4, 150, 254, 138, 236, 205, 93, 222, 114, 67, 29, 24, 72, 243, 141,
    128, 195, 78, 66, 215, 61, 156, 180,
];
fn perm(i: usize) -> usize {
    PERM[i & 255] as usize
}
fn fade(t: f64) -> f64 {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}
fn grad(hash: usize, x: f64, y: f64) -> f64 {
    let h = hash & 15;
    let u = if h < 8 { x } else { y };
    let v = if h < 4 {
        y
    } else if h == 12 || h == 14 {
        x
    } else {
        0.0
    };
    (if h & 1 == 0 { u } else { -u }) + (if h & 2 == 0 { v } else { -v })
}
fn noise(ix: u8, iy: u8, x: f64, y: f64, seed: u8) -> f64 {
    let u = fade(x);
    let v = fade(y);
    let a = perm(ix as usize + seed as usize) + iy as usize;
    let b = perm(ix as usize + 1 + seed as usize) + iy as usize;
    let aa = perm(a);
    let ab = perm(a + 1);
    let ba = perm(b);
    let bb = perm(b + 1);
    let edge1 = grad(perm(aa), x, y) * (1.0 - u) + grad(perm(ba), x - 1.0, y) * u;
    let edge2 = grad(perm(ab), x, y - 1.0) * (1.0 - u) + grad(perm(bb), x - 1.0, y - 1.0) * u;
    edge1 * (1.0 - v) + edge2 * v
}
fn color(p: &Value, key: &str) -> [u8; 4] {
    let a = p[key].as_array().unwrap();
    let alpha_key = if key == "color_1" {
        "color_1_alpha"
    } else {
        "color_2_alpha"
    };
    [
        a[0].as_u64().unwrap() as u8,
        a[1].as_u64().unwrap() as u8,
        a[2].as_u64().unwrap() as u8,
        a.get(3)
            .and_then(Value::as_u64)
            .unwrap_or_else(|| p[alpha_key].as_u64().unwrap()) as u8,
    ]
}
fn render(sample: &PixelReader<'_>, x: i32, y: i32, prep: &Prepared, p: &Value) -> [u8; 4] {
    let mut val = 0.0;
    let mut mult = 1.0;
    let mut div = p["scale"].as_i64().unwrap() as i32;
    let power = p["roughness"].as_f64().unwrap();
    let seed = (p["seed"].as_u64().unwrap() as u8) ^ 0x5a;
    for i in 0..12 {
        if mult <= 0.03 || div <= 0 {
            break;
        }
        let mut dxr = 65536.0 + f64::from(2 * x - prep.width) / f64::from(div);
        let mut dyr = 65536.0 + f64::from(2 * y - prep.height) / f64::from(div);
        let dxd = dxr as i32;
        let dyd = dyr as i32;
        dxr -= f64::from(dxd);
        dyr -= f64::from(dyd);
        val += noise(dxd as u8, dyd as u8, dxr, dyr, seed ^ (i as u8)) * mult;
        div /= 2;
        mult *= power;
    }
    let fraction = (val + 1.0) / 2.0;
    let from = color(p, "color_1");
    let to = color(p, "color_2");
    let mut out = [0; 4];
    for channel in 0..4 {
        out[channel] = (f64::from(from[channel])
            + (f64::from(to[channel]) - f64::from(from[channel])) * fraction)
            .clamp(0.0, 255.0) as u8;
    }
    let mode = match p["blend_mode"].as_str().unwrap() {
        "normal" => LayerBlendMode::Normal,
        "multiply" => LayerBlendMode::Multiply,
        "additive" => LayerBlendMode::Additive,
        "color_burn" => LayerBlendMode::ColorBurn,
        "color_dodge" => LayerBlendMode::ColorDodge,
        "reflect" => LayerBlendMode::Reflect,
        "glow" => LayerBlendMode::Glow,
        "overlay" => LayerBlendMode::Overlay,
        "difference" => LayerBlendMode::Difference,
        "negation" => LayerBlendMode::Negation,
        "lighten" => LayerBlendMode::Lighten,
        "darken" => LayerBlendMode::Darken,
        "screen" => LayerBlendMode::Screen,
        "xor" => LayerBlendMode::Xor,
        _ => unreachable!(),
    };
    if mode == LayerBlendMode::Normal && out[3] == 255 {
        return out;
    }
    let combined = crate::composite::composite_pixel(sample(x, y).into(), out.into(), mode, 255);
    [combined.r, combined.g, combined.b, combined.a]
}
/// Default black-to-white legacy field for seeded GPU colorization.
pub fn reference_rgba8(width: u32, height: u32, seed: u8) -> Vec<u8> {
    let mut params = spec().default_params();
    params["seed"] = json!(seed);
    let prep = Prepared {
        width: width as i32,
        height: height as i32,
        ..Prepared::default()
    };
    let sample = |_: i32, _: i32| [0; 4];
    let mut out = vec![0; width as usize * height as usize * 4];
    for y in 0..height as i32 {
        for x in 0..width as i32 {
            let index = (y as usize * width as usize + x as usize) * 4;
            out[index..index + 4].copy_from_slice(&render(&sample, x, y, &prep, &params));
        }
    }
    out
}
pub fn spec() -> EffectSpec {
    EffectSpec {
        id: "effect.render.clouds",
        category: "Render",
        label_key: "effect.clouds",
        schema: ParamSchema {
            fields: vec![
                Param {
                    key: "scale",
                    kind: ParamKind::Int,
                    min: Some(2.0),
                    max: Some(1000.0),
                    default: Some(json!(250)),
                },
                Param {
                    key: "roughness",
                    kind: ParamKind::Float,
                    min: Some(0.0),
                    max: Some(1.0),
                    default: Some(json!(0.5)),
                },
                Param {
                    key: "blend_mode",
                    kind: ParamKind::Choice(&[
                        "normal",
                        "multiply",
                        "additive",
                        "color_burn",
                        "color_dodge",
                        "reflect",
                        "glow",
                        "overlay",
                        "difference",
                        "negation",
                        "lighten",
                        "darken",
                        "screen",
                        "xor",
                    ]),
                    min: None,
                    max: None,
                    default: Some(json!("normal")),
                },
                Param {
                    key: "seed",
                    kind: ParamKind::Int,
                    min: Some(0.0),
                    max: Some(255.0),
                    default: Some(json!(0)),
                },
                Param {
                    key: "color_1",
                    kind: ParamKind::RgbBytes,
                    min: None,
                    max: None,
                    default: Some(json!([0, 0, 0])),
                },
                Param {
                    key: "color_1_alpha",
                    kind: ParamKind::Int,
                    min: Some(0.0),
                    max: Some(255.0),
                    default: Some(json!(255)),
                },
                Param {
                    key: "color_2",
                    kind: ParamKind::RgbBytes,
                    min: None,
                    max: None,
                    default: Some(json!([255, 255, 255])),
                },
                Param {
                    key: "color_2_alpha",
                    kind: ParamKind::Int,
                    min: Some(0.0),
                    max: Some(255.0),
                    default: Some(json!(255)),
                },
            ],
        },
        contract: ArithmeticContract {
            alpha_form: AlphaForm::Straight,
            working_space: WorkingSpace::Encoded,
            quantization_points: &["perlin noise", "channel clamp and truncation"],
            rounding: RoundingRule::NearestHalfUp,
            border: BorderRule::Clamp,
            target: "legacy-3.36",
        },
        prepare: prepare_noop,
        required_input: pointwise_input,
        passes: &[PassKind::Pointwise],
        render_cpu: render,
        gpu: Some(GpuKernel::Clouds),
    }
}
