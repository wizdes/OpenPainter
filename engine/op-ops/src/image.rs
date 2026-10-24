//! Whole-image pixel transforms. Input and output are straight RGBA8.
//! Filtering uses premultiplied alpha; zero-alpha results become transparent black.
pub mod legacy;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resampling {
    Nearest,
    Bilinear,
    BilinearLowQuality,
    Bicubic,
    BicubicSmooth,
    SuperSampling,
    Fant,
    Lanczos,
    AdaptiveSharp,
    BestQuality,
}

fn sinc(x: f64) -> f64 {
    if x.abs() < 1e-9 {
        1.0
    } else {
        let x = x * std::f64::consts::PI;
        x.sin() / x
    }
}
fn kernel(mode: Resampling, x: f64) -> f64 {
    let x = x.abs();
    match mode {
        Resampling::Bilinear | Resampling::BilinearLowQuality | Resampling::Fant => {
            (1.0 - x).max(0.0)
        }
        Resampling::Bicubic | Resampling::AdaptiveSharp | Resampling::BestQuality => {
            if x < 1.0 {
                1.5 * x * x * x - 2.5 * x * x + 1.0
            } else if x < 2.0 {
                -0.5 * x * x * x + 2.5 * x * x - 4.0 * x + 2.0
            } else {
                0.0
            }
        }
        Resampling::BicubicSmooth => {
            if x < 1.0 {
                (4.0 - 6.0 * x * x + 3.0 * x * x * x) / 6.0
            } else if x < 2.0 {
                (2.0 - x).powi(3) / 6.0
            } else {
                0.0
            }
        }
        Resampling::Lanczos if x < 3.0 => sinc(x) * sinc(x / 3.0),
        _ => 0.0,
    }
}
fn srgb_decode(x: f64) -> f64 {
    if x <= 0.04045 {
        x / 12.92
    } else {
        ((x + 0.055) / 1.055).powf(2.4)
    }
}
fn srgb_encode(x: f64) -> f64 {
    if x <= 0.0031308 {
        x * 12.92
    } else {
        1.055 * x.powf(1.0 / 2.4) - 0.055
    }
}
fn byte(x: f64) -> u8 {
    (x * 255.0).round().clamp(0.0, 255.0) as u8
}
fn taps(src: u32, dst: u32, pos: u32, mode: Resampling) -> Vec<(usize, f64)> {
    let scale = src as f64 / dst as f64;
    let center = (pos as f64 + 0.5) * scale - 0.5;
    if mode == Resampling::Nearest {
        return vec![(
            ((pos as u64 * src as u64 / dst as u64).min(src as u64 - 1)) as usize,
            1.0,
        )];
    }
    let area = matches!(mode, Resampling::SuperSampling) && scale > 1.0;
    if area {
        let lo = pos as f64 * scale;
        let hi = (pos + 1) as f64 * scale;
        return ((lo.floor() as usize)..((hi.ceil() as usize).min(src as usize)))
            .filter_map(|i| {
                let w = (hi.min(i as f64 + 1.0) - lo.max(i as f64)).max(0.0) / scale;
                (w > 0.0).then_some((i, w))
            })
            .collect();
    }
    let mode = if mode == Resampling::SuperSampling {
        Resampling::Bilinear
    } else {
        mode
    };
    let radius = if mode == Resampling::Lanczos {
        3.0
    } else if matches!(
        mode,
        Resampling::Bicubic
            | Resampling::BicubicSmooth
            | Resampling::AdaptiveSharp
            | Resampling::BestQuality
    ) {
        2.0
    } else {
        1.0
    };
    let stretch = if mode == Resampling::BilinearLowQuality {
        1.0
    } else {
        scale.max(1.0)
    };
    let start = (center - radius * stretch).floor() as i64;
    let end = (center + radius * stretch).ceil() as i64;
    let mut weights = std::collections::BTreeMap::<usize, f64>::new();
    for i in start..=end {
        let w = kernel(mode, (i as f64 - center) / stretch);
        if w != 0.0 {
            *weights
                .entry(i.clamp(0, src as i64 - 1) as usize)
                .or_default() += w;
        }
    }
    let sum: f64 = weights.values().sum();
    weights.into_iter().map(|(i, w)| (i, w / sum)).collect()
}
/// Resize RGBA8 with clamp borders and one output quantization. Alpha is filtered linearly.
pub fn resize(
    src: &[u8],
    sw: u32,
    sh: u32,
    dw: u32,
    dh: u32,
    mode: Resampling,
    gamma: bool,
) -> Vec<u8> {
    assert_eq!(src.len(), sw as usize * sh as usize * 4);
    assert!(sw > 0 && sh > 0 && dw > 0 && dh > 0);
    // Provisional automatic choice until 5.1 captures are available.
    let mode = if matches!(mode, Resampling::AdaptiveSharp | Resampling::BestQuality) {
        if (dw as f64 / sw as f64).min(dh as f64 / sh as f64) < 0.5 {
            Resampling::Fant
        } else if dw > sw || dh > sh {
            Resampling::Lanczos
        } else {
            Resampling::Bicubic
        }
    } else {
        mode
    };
    let xt: Vec<_> = (0..dw).map(|x| taps(sw, dw, x, mode)).collect();
    let yt: Vec<_> = (0..dh).map(|y| taps(sh, dh, y, mode)).collect();
    let mut out = vec![0; dw as usize * dh as usize * 4];
    for y in 0..dh as usize {
        for x in 0..dw as usize {
            if mode == Resampling::Nearest {
                let si = (yt[y][0].0 * sw as usize + xt[x][0].0) * 4;
                out[(y * dw as usize + x) * 4..(y * dw as usize + x + 1) * 4]
                    .copy_from_slice(&src[si..si + 4]);
                continue;
            }
            let mut acc = [0.0; 4];
            for &(sy, wy) in &yt[y] {
                for &(sx, wx) in &xt[x] {
                    let p = &src[(sy * sw as usize + sx) * 4..][..4];
                    let a = p[3] as f64 / 255.0;
                    let w = wx * wy;
                    acc[3] += a * w;
                    for c in 0..3 {
                        let v = p[c] as f64 / 255.0;
                        acc[c] += (if gamma { srgb_decode(v) } else { v }) * a * w;
                    }
                }
            }
            let i = (y * dw as usize + x) * 4;
            out[i + 3] = byte(acc[3]);
            if acc[3] > 0.0 && out[i + 3] > 0 {
                for c in 0..3 {
                    let v = (acc[c] / acc[3]).clamp(0.0, 1.0);
                    out[i + c] = byte(if gamma { srgb_encode(v) } else { v });
                }
            }
        }
    }
    out
}
