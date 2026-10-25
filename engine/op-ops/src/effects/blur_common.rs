//! Provisional paint.net 5.x blur math based on the documented controls.
use crate::effect::*;
use op_core::doc::{DocState, SelectionState};
use serde_json::Value;

pub fn prepare(doc: &DocState, _: &SelectionState, p: &Value) -> Result<Prepared> {
    Ok(Prepared {
        width: doc.width as i32,
        height: doc.height as i32,
        halo: p["radius"].as_i64().unwrap_or(4) as i32,
        ..Prepared::default()
    })
}

pub fn average(
    sample: &PixelReader<'_>,
    x: i32,
    y: i32,
    prep: &Prepared,
    p: &Value,
    circle: bool,
) -> [u8; 4] {
    let radius = p["radius"].as_i64().unwrap_or(4) as i32;
    if radius == 0 {
        return sample(x, y);
    }
    let gamma = p["gamma_boost"].as_i64().unwrap_or(0);
    let exponent = if gamma >= 0 {
        1.0 + gamma as f64 / 50.0
    } else {
        1.0 / (1.0 - gamma as f64 / 50.0)
    };
    let quality = p["quality"].as_i64().unwrap_or(10);
    let stride = if circle {
        (11 - quality).clamp(1, 10) as i32
    } else {
        1
    };
    let mut rgb = [0.0f64; 3];
    let mut alpha = 0u64;
    let mut area = 0u64;
    for dy in -radius..=radius {
        for dx in -radius..=radius {
            if circle
                && (dx * dx + dy * dy > radius * radius
                    || dx.rem_euclid(stride) != 0
                    || dy.rem_euclid(stride) != 0)
            {
                continue;
            }
            let sx = x + dx;
            let sy = y + dy;
            if sx < 0 || sy < 0 || sx >= prep.width || sy >= prep.height {
                continue;
            }
            let c = sample(sx, sy);
            area += 1;
            alpha += u64::from(c[3]);
            for channel in 0..3 {
                rgb[channel] += (f64::from(c[channel]) / 255.0).powf(exponent) * f64::from(c[3]);
            }
        }
    }
    if area == 0 {
        return sample(x, y);
    }
    if alpha == 0 {
        return [0, 0, 0, 0];
    }
    [0, 1, 2, 3].map(|channel| {
        if channel == 3 {
            (alpha / area) as u8
        } else {
            ((rgb[channel] / alpha as f64).powf(1.0 / exponent) * 255.0)
                .round()
                .clamp(0.0, 255.0) as u8
        }
    })
}
