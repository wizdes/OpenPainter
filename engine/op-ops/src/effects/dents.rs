//! Port of OpenPDN DentsEffect.cs and PerlinNoise2D.cs at cca476b (MIT, dotPDN LLC).
use crate::effect::*;
use crate::effects::distort_common as d;
use serde_json::{Value, json};
const PERM: [usize; 256] = [
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
    PERM[i & 255]
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
fn noise_once(mut x: f64, mut y: f64, seed: usize) -> f64 {
    let xf = x.floor();
    let yf = y.floor();
    let ix = xf as i32 as usize & 255;
    let iy = yf as i32 as usize & 255;
    x -= xf;
    y -= yf;
    let u = fade(x);
    let v = fade(y);
    let a = perm(ix + seed) + iy;
    let b = perm(ix + 1 + seed) + iy;
    let left = grad(perm(perm(a)), x, y);
    let right = grad(perm(perm(b)), x - 1.0, y);
    let top = left + u * (right - left);
    let left = grad(perm(perm(a + 1)), x, y - 1.0);
    let right = grad(perm(perm(b + 1)), x - 1.0, y - 1.0);
    top + v * (left + u * (right - left) - top)
}
fn noise(mut x: f64, mut y: f64, detail: f64, roughness: f64, seed: usize) -> f64 {
    let angle = 137.2_f64.to_radians();
    let (sn, cs) = angle.sin_cos();
    let mut total = 0.0;
    let mut frequency = 1.0;
    let mut amplitude = 1.0;
    let mut partial = detail;
    for _ in 0..(detail.ceil() as usize) {
        let xr = x * cs - y * sn;
        let yr = x * sn + y * cs;
        let mut n = noise_once(xr * frequency, yr * frequency, seed) * amplitude;
        if partial < 1.0 {
            n *= partial;
        }
        total += n;
        amplitude *= roughness;
        if amplitude < 0.001 {
            break;
        }
        frequency *= 2.0;
        partial -= 1.0;
        x = xr + 499.0;
        y = yr + 506.0;
    }
    total
}
fn render(s: &PixelReader<'_>, x: i32, y: i32, p: &Prepared, v: &Value) -> [u8; 4] {
    if p.width <= 0 || p.height <= 0 {
        return s(x, y);
    }
    let scale = v["scale"].as_f64().unwrap();
    let refraction = v["refraction"].as_f64().unwrap();
    let radius = f64::from(p.width.min(p.height)) * 0.5;
    let scale_r = (400.0 / radius) / scale;
    let refraction_scale = (refraction / 100.0) / scale_r;
    let detail_raw = 1.0 + v["roughness"].as_f64().unwrap() / 10.0;
    let max_detail = (scale_r.ln() / 0.5_f64.ln()).floor();
    let detail = if detail_raw > max_detail && max_detail >= 1.0 {
        max_detail
    } else {
        detail_raw
    };
    let roughness = v["roughness"].as_f64().unwrap() / 100.0;
    let theta = std::f64::consts::TAU * v["tension"].as_f64().unwrap() / 10.0;
    let seed = v["seed"].as_u64().unwrap() as usize;
    let quality = v["quality"].as_u64().unwrap() as usize;
    let samples: Vec<_> = d::rgss(quality)
        .into_iter()
        .map(|(ox, oy)| {
            let px = f64::from(x) - f64::from(p.width) / 2.0 + f64::from(ox);
            let py = f64::from(y) - f64::from(p.height) / 2.0 - f64::from(oy);
            let angle = theta * noise(px * scale_r, py * scale_r, detail, roughness, seed);
            let sx = (px + refraction_scale * (-angle).sin() + f64::from(p.width) / 2.0) as f32;
            let sy = (py + refraction_scale * angle.cos() + f64::from(p.height) / 2.0) as f32;
            // WarpEffectBase.ReflectCoord reflects around the image width or height,
            // then Surface.GetBilinearSampleClamped handles the last pixel.
            let reflect = |value: f32, len: i32| {
                let period = 2.0 * len as f32;
                let t = value.rem_euclid(period);
                if t > len as f32 { period - t } else { t }
            };
            d::bilinear(s, reflect(sx, p.width), reflect(sy, p.height), p, "clamp")
        })
        .collect();
    if quality == 1 {
        samples[0]
    } else {
        d::blend(&samples)
    }
}
pub fn spec() -> EffectSpec {
    EffectSpec {
        id: "effect.distort.dents",
        category: "Distort",
        label_key: "effect.dents",
        schema: ParamSchema {
            fields: vec![
                d::field("scale", ParamKind::Float, 1.0, 200.0, json!(25.0)),
                d::field("refraction", ParamKind::Float, 0.0, 200.0, json!(50.0)),
                d::field("roughness", ParamKind::Float, 0.0, 100.0, json!(10.0)),
                d::field("tension", ParamKind::Float, 0.0, 100.0, json!(10.0)),
                d::field("quality", ParamKind::Int, 1.0, 5.0, json!(2)),
                d::field("seed", ParamKind::Int, 0.0, 255.0, json!(0)),
            ],
        },
        contract: d::contract(BorderRule::Mirror, "legacy-3.36"),
        prepare: prepare_noop,
        required_input: d::input_whole,
        passes: &[PassKind::Warp],
        render_cpu: render,
        gpu: None,
    }
}
