//! Port of OpenPDN BulgeEffect.cs at cca476b (MIT, dotPDN LLC).
use crate::effect::*;
use crate::effects::distort_common as d;
use serde_json::{Value, json};
fn render(s: &PixelReader<'_>, x: i32, y: i32, p: &Prepared, v: &Value) -> [u8; 4] {
    let hw = p.width as f32 / 2.0;
    let hh = p.height as f32 / 2.0;
    let radius = hw.min(hh);
    if radius == 0.0 {
        return s(x, y);
    }
    let cx = hw * (1.0 + v["center_x"].as_f64().unwrap() as f32);
    let cy = hh * (1.0 + v["center_y"].as_f64().unwrap() as f32);
    let quality = v["quality"].as_i64().unwrap() as usize;
    let mut samples = Vec::with_capacity(quality * quality);
    for (ox, oy) in d::rgss(quality) {
        let u = x as f32 - cx + ox;
        let w = y as f32 - cy - oy;
        let t = 1.0 - (u * u + w * w).sqrt() / radius;
        if t <= 0.0 {
            samples.push(s(x, y));
            continue;
        }
        let scale = 1.0 - (v["amount"].as_i64().unwrap() as f32 / 100.0) * t * t;
        samples.push(d::bilinear(
            s,
            u * scale + cx,
            w * scale + cy,
            p,
            v["edge_behavior"].as_str().unwrap(),
        ));
    }
    if quality == 1 {
        samples[0]
    } else {
        d::blend(&samples)
    }
}
pub fn spec() -> EffectSpec {
    EffectSpec {
        id: "effect.distort.bulge",
        category: "Distort",
        label_key: "effect.bulge",
        schema: ParamSchema {
            fields: vec![
                d::field("amount", ParamKind::Int, -200.0, 100.0, json!(45)),
                d::field("center_x", ParamKind::Float, -1.0, 1.0, json!(0.0)),
                d::field("center_y", ParamKind::Float, -1.0, 1.0, json!(0.0)),
                d::choice(
                    "edge_behavior",
                    &["clamp", "wrap", "mirror", "transparent"],
                    "clamp",
                ),
                d::field("quality", ParamKind::Int, 1.0, 5.0, json!(1)),
            ],
        },
        contract: d::contract(BorderRule::Clamp, "legacy-3.36"),
        prepare: prepare_noop,
        required_input: d::input_whole,
        passes: &[PassKind::Warp],
        render_cpu: render,
        gpu: Some(GpuKernel::Bulge),
    }
}
