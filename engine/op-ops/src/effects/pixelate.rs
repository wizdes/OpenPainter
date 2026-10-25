//! Port of OpenPDN PixelateEffect.cs at cca476b (MIT, dotPDN LLC).
use crate::effect::*;
use crate::effects::distort_common as d;
use serde_json::{Value, json};
fn cell(s: &PixelReader<'_>, left: i32, top: i32, p: &Prepared, v: &Value) -> [u8; 4] {
    let size = v["cell_size"].as_i64().unwrap() as i32;
    let left = left.clamp(0, p.width - 1);
    let top = top.clamp(0, p.height - 1);
    let right = (left + size - 1).min(p.width - 1);
    let bottom = (top + size - 1).min(p.height - 1);
    match v["scale_down"].as_str().unwrap() {
        "nearest" => s(left, top),
        "bilinear" => d::bilinear(
            s,
            (left + right) as f32 * 0.5,
            (top + bottom) as f32 * 0.5,
            p,
            "clamp",
        ),
        "bicubic" => {
            let mid = d::bilinear(
                s,
                (left + right) as f32 * 0.5,
                (top + bottom) as f32 * 0.5,
                p,
                "clamp",
            );
            d::blend(&[mid, mid, s(left, top), s(right, bottom)])
        }
        _ => d::blend(&[
            s(left, top),
            s(right, top),
            s(left, bottom),
            s(right, bottom),
        ]),
    }
}
fn render(s: &PixelReader<'_>, x: i32, y: i32, p: &Prepared, v: &Value) -> [u8; 4] {
    let size = v["cell_size"].as_i64().unwrap() as i32;
    let left = x.div_euclid(size) * size;
    let top = y.div_euclid(size) * size;
    let first = cell(s, left, top, p, v);
    if v["scale_up"] == "nearest" {
        return first;
    }
    let mut tx = (x - left) as f64 / size as f64;
    let mut ty = (y - top) as f64 / size as f64;
    if v["scale_up"] == "bicubic" {
        tx = tx * tx * (3.0 - 2.0 * tx);
        ty = ty * ty * (3.0 - 2.0 * ty);
    }
    let adjacent_x = cell(s, (left + size).min(p.width - 1), top, p, v);
    let adjacent_y = cell(s, left, (top + size).min(p.height - 1), p, v);
    let diagonal = cell(
        s,
        (left + size).min(p.width - 1),
        (top + size).min(p.height - 1),
        p,
        v,
    );
    let weights = [
        (1.0 - tx) * (1.0 - ty),
        tx * (1.0 - ty),
        (1.0 - tx) * ty,
        tx * ty,
    ];
    let samples = [first, adjacent_x, adjacent_y, diagonal];
    let alpha: f64 = samples
        .iter()
        .zip(weights)
        .map(|(c, w)| f64::from(c[3]) * w)
        .sum();
    if alpha <= 0.0 {
        return [0; 4];
    }
    let mut out = [0; 4];
    for k in 0..3 {
        let color: f64 = samples
            .iter()
            .zip(weights)
            .map(|(c, w)| f64::from(c[k]) * f64::from(c[3]) * w)
            .sum();
        out[k] = (color / alpha).round().clamp(0.0, 255.0) as u8;
    }
    out[3] = alpha.round().clamp(0.0, 255.0) as u8;
    out
}

pub fn spec() -> EffectSpec {
    EffectSpec {
        id: "effect.distort.pixelate",
        category: "Distort",
        label_key: "effect.pixelate",
        schema: ParamSchema {
            fields: vec![
                d::field("cell_size", ParamKind::Int, 1.0, 100.0, json!(2)),
                d::choice(
                    "scale_down",
                    &["legacy", "nearest", "bilinear", "bicubic"],
                    "legacy",
                ),
                d::choice("scale_up", &["nearest", "bilinear", "bicubic"], "nearest"),
            ],
        },
        contract: d::contract(BorderRule::Clamp, "legacy-3.36"),
        prepare: prepare_noop,
        required_input: d::input_whole,
        passes: &[PassKind::Neighborhood],
        render_cpu: render,
        gpu: Some(GpuKernel::Pixelate),
    }
}
