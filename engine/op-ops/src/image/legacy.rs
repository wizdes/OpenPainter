//! The four Paint.NET 3.36 FitSurface paths from MIT OpenPDN Surface.cs,
//! commit cca476b0df2a2f70996e6b9486ec45327631568c.
use super::Resampling;

fn nearest(src: &[u8], sw: u32, sh: u32, dw: u32, dh: u32) -> Vec<u8> {
    let mut out = vec![0; dw as usize * dh as usize * 4];
    for y in 0..dh {
        for x in 0..dw {
            let sx = x as u64 * sw as u64 / dw as u64;
            let sy = y as u64 * sh as u64 / dh as u64;
            let i = (sy * sw as u64 + sx) as usize * 4;
            let d = (y * dw + x) as usize * 4;
            out[d..d + 4].copy_from_slice(&src[i..i + 4]);
        }
    }
    out
}

fn bilinear(src: &[u8], sw: u32, sh: u32, dw: u32, dh: u32) -> Vec<u8> {
    if sw < 2 || sh < 2 || dw < 2 || dh < 2 {
        return super_sample(src, sw, sh, dw, dh);
    }
    let mut out = vec![0; dw as usize * dh as usize * 4];
    for y in 0..dh {
        for x in 0..dw {
            let fy = ((y * (sh - 1)) as f32) / (dh - 1) as f32;
            let fx = ((x * (sw - 1)) as f32) / (dw - 1) as f32;
            let sy = fy.floor() as u32;
            let sx = fx.floor() as u32;
            let yf = (256.0 * (fy - sy as f32)) as u32;
            let xf = (256.0 * (fx - sx as f32)) as u32;
            let points = [
                (sx, sy),
                (sx.saturating_add(1).min(sw - 1), sy),
                (sx, sy.saturating_add(1).min(sh - 1)),
                (
                    sx.saturating_add(1).min(sw - 1),
                    sy.saturating_add(1).min(sh - 1),
                ),
            ];
            let weights = [
                (256 - xf) * (256 - yf),
                xf * (256 - yf),
                (256 - xf) * yf,
                xf * yf,
            ];
            let mut af = 0u32;
            let mut rgb = [0u64; 3];
            for ((px, py), w) in points.into_iter().zip(weights) {
                let p = &src[((py * sw + px) as usize * 4)..][..4];
                af += p[3] as u32 * w;
                for c in 0..3 {
                    rgb[c] += p[3] as u64 * p[c] as u64 * w as u64;
                }
            }
            let d = (y * dw + x) as usize * 4;
            out[d + 3] = ((af + 32768) >> 16) as u8;
            if out[d + 3] != 0 {
                for c in 0..3 {
                    out[d + c] = (rgb[c] / af as u64) as u8;
                }
            }
        }
    }
    out
}

fn r(x: f64) -> f64 {
    let cube = |v: f64| v.max(0.0).powi(3);
    (cube(x + 2.0) - 4.0 * cube(x + 1.0) + 6.0 * cube(x) - 4.0 * cube(x - 1.0)) / 6.0
}
fn bicubic(src: &[u8], sw: u32, sh: u32, dw: u32, dh: u32) -> Vec<u8> {
    if sw < 2 || sh < 2 || dw < 2 || dh < 2 {
        return super_sample(src, sw, sh, dw, dh);
    }
    let mut out = vec![0; dw as usize * dh as usize * 4];
    for y in 0..dh {
        let fy = y as f64 * (sh - 1) as f64 / (dh - 1) as f64;
        let iy = fy.floor() as i64;
        for x in 0..dw {
            let fx = x as f64 * (sw - 1) as f64 / (dw - 1) as f64;
            let ix = fx.floor() as i64;
            let mut total = 0.0;
            let mut asum = 0.0;
            let mut rgb = [0.0; 3];
            for n in -1..=2 {
                let sy = iy + n;
                if sy < 0 || sy >= sh as i64 {
                    continue;
                }
                let wy = r(fy - iy as f64 - n as f64);
                for m in -1..=2 {
                    let sx = ix + m;
                    if sx < 0 || sx >= sw as i64 {
                        continue;
                    }
                    let w = r(m as f64 - (fx - ix as f64)) * wy;
                    let p = &src[((sy as u32 * sw + sx as u32) as usize * 4)..][..4];
                    let a = p[3] as f64;
                    total += w;
                    asum += a * w;
                    for c in 0..3 {
                        rgb[c] += p[c] as f64 * a * w;
                    }
                }
            }
            let d = (y * dw + x) as usize * 4;
            let alpha = asum / total;
            out[d + 3] = alpha.round().clamp(0.0, 255.0) as u8;
            if alpha != 0.0 {
                for c in 0..3 {
                    out[d + c] = (rgb[c] / asum).round().clamp(0.0, 255.0) as u8;
                }
            }
        }
    }
    out
}

fn super_sample(src: &[u8], sw: u32, sh: u32, dw: u32, dh: u32) -> Vec<u8> {
    if sw == dw && sh == dh {
        return src.to_vec();
    }
    if sw <= dw || sh <= dh {
        return if sw < 2 || sh < 2 || dw < 2 || dh < 2 {
            nearest(src, sw, sh, dw, dh)
        } else {
            bicubic(src, sw, sh, dw, dh)
        };
    }
    let mut out = vec![0; dw as usize * dh as usize * 4];
    for y in 0..dh {
        let top = y as f64 * sh as f64 / dh as f64;
        let bottom = (y + 1) as f64 * sh as f64 / dh as f64;
        for x in 0..dw {
            let left = x as f64 * sw as f64 / dw as f64;
            let right = (x + 1) as f64 * sw as f64 / dw as f64;
            let mut asum = 0.0;
            let mut rgb = [0.0; 3];
            for sy in top.floor() as u32..(bottom.ceil() as u32).min(sh) {
                let wy = (bottom.min(sy as f64 + 1.0) - top.max(sy as f64)).max(0.0);
                for sx in left.floor() as u32..(right.ceil() as u32).min(sw) {
                    let wx = (right.min(sx as f64 + 1.0) - left.max(sx as f64)).max(0.0);
                    let p = &src[((sy * sw + sx) as usize * 4)..][..4];
                    let a = p[3] as f64;
                    let w = wx * wy;
                    asum += a * w;
                    for c in 0..3 {
                        rgb[c] += p[c] as f64 * a * w;
                    }
                }
            }
            let d = (y * dw + x) as usize * 4;
            out[d + 3] = (asum / ((right - left) * (bottom - top)))
                .round()
                .clamp(0.0, 255.0) as u8;
            if asum != 0.0 {
                for c in 0..3 {
                    out[d + c] = (rgb[c] / asum).round().clamp(0.0, 255.0) as u8;
                }
            }
        }
    }
    out
}

/// Reproduce the older FitSurface paths. Nearest retains hidden RGB bytes; filtered
/// output becomes transparent black when the computed alpha is zero.
pub fn resize(src: &[u8], sw: u32, sh: u32, dw: u32, dh: u32, mode: Resampling) -> Vec<u8> {
    assert_eq!(src.len(), sw as usize * sh as usize * 4);
    assert!(sw > 0 && sh > 0 && dw > 0 && dh > 0);
    match mode {
        Resampling::Nearest => nearest(src, sw, sh, dw, dh),
        Resampling::Bilinear => bilinear(src, sw, sh, dw, dh),
        Resampling::Bicubic => bicubic(src, sw, sh, dw, dh),
        Resampling::SuperSampling => super_sample(src, sw, sh, dw, dh),
        _ => super::resize(src, sw, sh, dw, dh, mode, false),
    }
}
