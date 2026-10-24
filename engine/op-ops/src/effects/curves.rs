// Ported from OpenPDN 3.36 at cca476b (MIT), SplineInterpolator,
// CurvesEffectConfigToken and UnaryPixelOps. Copyright dotPDN LLC and contributors.
use crate::effect::*;
use crate::effects::intensity;
use serde_json::{Value, json};

pub fn spec() -> EffectSpec {
    EffectSpec {
        id: "effect.adjust.curves",
        category: "Adjustments",
        label_key: "effect.curves",
        schema: ParamSchema {
            fields: vec![
                Param {
                    key: "ColorTransferMode",
                    kind: ParamKind::Choice(&["Luminosity", "Rgb"]),
                    min: None,
                    max: None,
                    default: Some(json!("Luminosity")),
                },
                Param {
                    key: "ControlPoints",
                    kind: ParamKind::Curve,
                    min: None,
                    max: None,
                    default: Some(json!([[[0, 0], [255, 255]]])),
                },
            ],
        },
        contract: ArithmeticContract {
            alpha_form: AlphaForm::Straight,
            working_space: WorkingSpace::Encoded,
            quantization_points: &[
                "natural cubic spline f64",
                "lookup byte truncation",
                "intensity truncation",
            ],
            rounding: RoundingRule::NearestHalfUp,
            border: BorderRule::Clamp,
            target: "legacy-3.36",
        },
        prepare,
        required_input: pointwise_input,
        passes: &[PassKind::Pointwise],
        render_cpu,
        gpu: Some(GpuKernel::Curves),
    }
}

fn prepare(
    _: &op_core::doc::DocState,
    _: &op_core::doc::SelectionState,
    params: &Value,
) -> Result<Prepared> {
    let rgb = params["ColorTransferMode"] == "Rgb";
    let channels = params["ControlPoints"].as_array().unwrap();
    if channels.len() != if rgb { 3 } else { 1 } {
        return Err(EffectError::Parameters(
            "ControlPoints channel count does not match ColorTransferMode".into(),
        ));
    }
    let mut tables = Box::new([[0u8; 256]; 3]);
    for (channel, points) in channels.iter().enumerate() {
        let mut pairs: Vec<(f64, f64)> = points
            .as_array()
            .unwrap()
            .iter()
            .map(|point| {
                (
                    point[0].as_u64().unwrap() as f64,
                    point[1].as_u64().unwrap() as f64,
                )
            })
            .collect();
        pairs.sort_by(|a, b| a.0.total_cmp(&b.0));
        if pairs.windows(2).any(|pair| pair[0].0 == pair[1].0) {
            return Err(EffectError::Parameters(
                "duplicate curve x coordinate".into(),
            ));
        }
        let second = second_derivatives(&pairs);
        for (value, out) in tables[channel].iter_mut().enumerate() {
            *out = interpolate(&pairs, &second, value as f64).clamp(0.0, 255.0) as u8;
        }
    }
    Ok(Prepared {
        channel_lut: Some(tables),
    })
}

fn second_derivatives(points: &[(f64, f64)]) -> Vec<f64> {
    let n = points.len();
    let mut second = vec![0.0; n];
    let mut u = vec![0.0; n];
    for i in 1..n - 1 {
        let width = points[i + 1].0 - points[i - 1].0;
        let sig = (points[i].0 - points[i - 1].0) / width;
        let p = sig * second[i - 1] + 2.0;
        second[i] = (sig - 1.0) / p;
        let slope = (points[i + 1].1 - points[i].1) / (points[i + 1].0 - points[i].0)
            - (points[i].1 - points[i - 1].1) / (points[i].0 - points[i - 1].0);
        u[i] = (6.0 * slope / width - sig * u[i - 1]) / p;
    }
    for i in (0..n - 1).rev() {
        second[i] = second[i] * second[i + 1] + u[i];
    }
    second
}

fn interpolate(points: &[(f64, f64)], second: &[f64], x: f64) -> f64 {
    let mut lo = 0;
    let mut hi = points.len() - 1;
    while hi - lo > 1 {
        let mid = (hi + lo) >> 1;
        if points[mid].0 > x {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    let h = points[hi].0 - points[lo].0;
    let a = (points[hi].0 - x) / h;
    let b = (x - points[lo].0) / h;
    a * points[lo].1
        + b * points[hi].1
        + ((a * a * a - a) * second[lo] + (b * b * b - b) * second[hi]) * (h * h) / 6.0
}

fn render_cpu(
    sample: &PixelReader<'_>,
    x: i32,
    y: i32,
    prepared: &Prepared,
    params: &Value,
) -> [u8; 4] {
    let mut pixel = sample(x, y);
    let Some(tables) = prepared.channel_lut.as_ref() else {
        return pixel;
    };
    if params["ColorTransferMode"] == "Rgb" {
        for (channel, table) in tables.iter().enumerate() {
            pixel[channel] = table[pixel[channel] as usize];
        }
    } else {
        let old = intensity(pixel);
        let diff = i16::from(tables[0][old as usize]) - i16::from(old);
        for channel in &mut pixel[..3] {
            *channel = (i16::from(*channel) + diff).clamp(0, 255) as u8;
        }
    }
    pixel
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn spline_hits_every_control_point_and_identity() {
        for points in [
            vec![(0.0, 0.0), (255.0, 255.0)],
            vec![(0.0, 0.0), (64.0, 130.0), (128.0, 160.0), (255.0, 255.0)],
            vec![(0.0, 0.0), (96.0, 150.0), (255.0, 255.0)],
            vec![(0.0, 0.0), (128.0, 90.0), (255.0, 255.0)],
            vec![(0.0, 0.0), (192.0, 120.0), (255.0, 255.0)],
            vec![(0.0, 255.0), (255.0, 0.0)],
        ] {
            let second = second_derivatives(&points);
            for &(x, y) in &points {
                assert!((interpolate(&points, &second, x) - y).abs() < 1e-9);
            }
        }
    }
}
