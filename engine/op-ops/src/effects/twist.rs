//! Port of OpenPDN TwistEffect.cs at cca476b (MIT, dotPDN LLC).
use crate::effect::*;
use crate::effects::distort_common as d;
use serde_json::{Value, json};
fn render(s: &PixelReader<'_>, x: i32, y: i32, p: &Prepared, v: &Value) -> [u8; 4] {
    let cx = p.width as f64 * 0.5 * (1.0 + v["center_x"].as_f64().unwrap());
    let cy = p.height as f64 * 0.5 * (1.0 + v["center_y"].as_f64().unwrap());
    let amount = -v["amount"].as_f64().unwrap();
    let size = 1.0 / v["size"].as_f64().unwrap();
    let quality = v["quality"].as_i64().unwrap() as usize;
    let mut samples = Vec::with_capacity(quality * quality);
    for (ox, oy) in d::rgss(quality) {
        let u = (x as f32 - cx as f32 + ox) as f64;
        let w = (y as f32 - cy as f32 + oy) as f64;
        let r = u.hypot(w);
        let mut theta = w.atan2(u);
        let t = (1.0 - r * size / (p.width.min(p.height) as f64 * 0.5))
            .max(0.0)
            .powi(3);
        theta += t * amount * amount * amount.signum() / 100.0;
        samples.push(d::bilinear(
            s,
            (cx + r * theta.cos()) as f32,
            (cy + r * theta.sin()) as f32,
            p,
            "clamp",
        ));
    }
    d::blend(&samples)
}
pub fn spec() -> EffectSpec {
    EffectSpec {
        id: "effect.distort.twist",
        category: "Distort",
        label_key: "effect.twist",
        schema: ParamSchema {
            fields: vec![
                d::field("amount", ParamKind::Float, -200.0, 200.0, json!(30.0)),
                d::field("size", ParamKind::Float, 0.01, 2.0, json!(1.0)),
                d::field("center_x", ParamKind::Float, -1.0, 1.0, json!(0.0)),
                d::field("center_y", ParamKind::Float, -1.0, 1.0, json!(0.0)),
                d::field("quality", ParamKind::Int, 1.0, 5.0, json!(2)),
            ],
        },
        contract: d::contract(BorderRule::Clamp, "legacy-3.36"),
        prepare: prepare_noop,
        required_input: d::input_whole,
        passes: &[PassKind::Warp],
        render_cpu: render,
        gpu: Some(GpuKernel::Twist),
    }
}
