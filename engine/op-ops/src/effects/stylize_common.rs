//! Shared 3x3 convolution from OpenPDN ColorDifferenceEffect and EmbossEffect,
//! commit cca476b, MIT. Source RGB is straight and alpha is ignored.
use crate::effect::{PixelReader, Prepared, Result};
use op_core::doc::{DocState, SelectionState};
use serde_json::Value;

pub(crate) fn prepare(doc: &DocState, _: &SelectionState, _: &Value) -> Result<Prepared> {
    Ok(Prepared {
        width: doc.width as i32,
        height: doc.height as i32,
        halo: 1,
        ..Prepared::default()
    })
}

pub(crate) fn weights(angle: f64, center: f64) -> [[f64; 3]; 3] {
    let r = angle.to_radians();
    let d = std::f64::consts::FRAC_PI_4;
    [
        [(r + d).cos(), (r + 2.0 * d).cos(), (r + 3.0 * d).cos()],
        [r.cos(), center, (r + 4.0 * d).cos()],
        [(r - d).cos(), (r - 2.0 * d).cos(), (r - 3.0 * d).cos()],
    ]
}

pub(crate) fn render(
    sample: &PixelReader<'_>,
    x: i32,
    y: i32,
    prepared: &Prepared,
    angle: f64,
    emboss: bool,
    relief: bool,
) -> [u8; 4] {
    let w = weights(angle, f64::from(relief));
    let mut sums = [0.0; 3];
    for (fy, row) in w.iter().enumerate() {
        let sy = y + fy as i32 - 1;
        if sy < 0 || sy >= prepared.height {
            continue;
        }
        for (fx, weight) in row.iter().enumerate() {
            let sx = x + fx as i32 - 1;
            if sx < 0 || sx >= prepared.width {
                continue;
            }
            let color = sample(sx, sy);
            if emboss {
                sums[0] += weight * f64::from(super::intensity(color));
            } else {
                for c in 0..3 {
                    sums[c] += weight * f64::from(color[c]);
                }
            }
        }
    }
    if emboss {
        let gray = (sums[0] as i32 + 128).clamp(0, 255) as u8;
        [gray, gray, gray, 255]
    } else {
        [
            (sums[0] as i32).clamp(0, 255) as u8,
            (sums[1] as i32).clamp(0, 255) as u8,
            (sums[2] as i32).clamp(0, 255) as u8,
            255,
        ]
    }
}
