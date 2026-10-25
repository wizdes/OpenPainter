//! Provisional 5.1 Rotate / Zoom controls: https://paint.net/doc/latest/RotateZoom.html.
//! Geometry follows OpenPDN RotateZoomEffect.cs at cca476b (MIT, dotPDN LLC).
use crate::effect::*;
use crate::effects::distort_common as d;
use serde_json::{Value, json};
fn render(s: &PixelReader<'_>, x: i32, y: i32, p: &Prepared, v: &Value) -> [u8; 4] {
    if p.width <= 0 || p.height <= 0 {
        return s(x, y);
    }
    let angle = (v["angle"].as_f64().unwrap() as f32).to_radians();
    let axis = (v["axis"].as_f64().unwrap() as f32).to_radians();
    let tilt = (v["tilt"].as_f64().unwrap() as f32).to_radians();
    let zoom = v["zoom"].as_f64().unwrap() as f32;
    let pan_x = v["pan_x"].as_f64().unwrap() as f32 * p.width as f32 / 2.0;
    let pan_y = v["pan_y"].as_f64().unwrap() as f32 * p.height as f32 / 2.0;
    let center_x = p.width as f32 / 2.0;
    let center_y = p.height as f32 / 2.0;
    let div_z = (p.width as f32).hypot(p.height as f32) / 2.0;
    let edge = v["tiling"].as_str().unwrap();
    let sampling = v["sampling"].as_str().unwrap();
    let q = v["quality"].as_u64().unwrap() as usize;
    let samples: Vec<_> = d::rgss(q)
        .into_iter()
        .map(|(ox, oy)| {
            let dx = x as f32 + ox - center_x - pan_x;
            let dy = y as f32 + oy - center_y - pan_y;
            let (sa, ca) = angle.sin_cos();
            let (sb, cb) = axis.sin_cos();
            let (st, ct) = tilt.sin_cos();
            let ux = dx * cb + dy * sb;
            let uy = -dx * sb + dy * cb;
            let tx = ux / ct.max(0.001);
            let z = tx * st;
            if z <= -div_z {
                return [0; 4];
            }
            let rx = tx / ct.max(0.001) * ca + uy * sa;
            let ry = -tx / ct.max(0.001) * sa + uy * ca;
            let factor = div_z / (zoom * (div_z + z));
            let sx = rx * factor + center_x;
            let sy = ry * factor + center_y;
            if sampling == "nearest" {
                let mut ix = sx.round() as i32;
                let mut iy = sy.round() as i32;
                if edge == "transparent" && (ix < 0 || iy < 0 || ix >= p.width || iy >= p.height) {
                    return [0; 4];
                }
                if edge == "repeat" {
                    ix = ix.rem_euclid(p.width);
                    iy = iy.rem_euclid(p.height);
                } else {
                    ix = ix.clamp(0, p.width - 1);
                    iy = iy.clamp(0, p.height - 1);
                }
                s(ix, iy)
            } else {
                d::bilinear(
                    s,
                    sx,
                    sy,
                    p,
                    if edge == "repeat" {
                        "wrap"
                    } else if edge == "mirror" {
                        "mirror"
                    } else {
                        "transparent"
                    },
                )
            }
        })
        .collect();
    if q == 1 {
        samples[0]
    } else {
        d::blend(&samples)
    }
}
pub fn spec() -> EffectSpec {
    EffectSpec {
        id: "effect.layers.rotate_zoom",
        category: "Layers",
        label_key: "effect.rotate_zoom",
        schema: ParamSchema {
            fields: vec![
                d::field("angle", ParamKind::Float, -180.0, 180.0, json!(0.0)),
                d::field("axis", ParamKind::Float, -180.0, 180.0, json!(0.0)),
                d::field("tilt", ParamKind::Float, -89.0, 89.0, json!(0.0)),
                d::field("pan_x", ParamKind::Float, -2.0, 2.0, json!(0.0)),
                d::field("pan_y", ParamKind::Float, -2.0, 2.0, json!(0.0)),
                d::field("zoom", ParamKind::Float, 0.01, 16.0, json!(1.0)),
                d::field("quality", ParamKind::Int, 1.0, 5.0, json!(1)),
                d::choice(
                    "tiling",
                    &["transparent", "repeat", "mirror"],
                    "transparent",
                ),
                d::choice("sampling", &["bilinear", "nearest"], "bilinear"),
            ],
        },
        contract: d::contract(BorderRule::Transparent, "legacy-3.36"),
        prepare: prepare_noop,
        required_input: d::input_whole,
        passes: &[PassKind::Warp],
        render_cpu: render,
        gpu: None,
    }
}
