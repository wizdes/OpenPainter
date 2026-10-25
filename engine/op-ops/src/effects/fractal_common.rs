//! Shared pixel math for the OpenPDN 3.36 Julia and Mandelbrot fractals.
//! Ported from commit cca476b, MIT; copyright dotPDN LLC and contributors.

pub const BLEND_MODES: &[&str] = &[
    "normal",
    "multiply",
    "additive",
    "color_burn",
    "color_dodge",
    "reflect",
    "glow",
    "overlay",
    "difference",
    "negation",
    "lighten",
    "darken",
    "screen",
    "xor",
];

pub fn provisional_blend(input: [u8; 4], output: [u8; 4], name: &str) -> [u8; 4] {
    use op_core::doc::LayerBlendMode;
    let mode = match name {
        "multiply" => LayerBlendMode::Multiply,
        "additive" => LayerBlendMode::Additive,
        "color_burn" => LayerBlendMode::ColorBurn,
        "color_dodge" => LayerBlendMode::ColorDodge,
        "reflect" => LayerBlendMode::Reflect,
        "glow" => LayerBlendMode::Glow,
        "overlay" => LayerBlendMode::Overlay,
        "difference" => LayerBlendMode::Difference,
        "negation" => LayerBlendMode::Negation,
        "lighten" => LayerBlendMode::Lighten,
        "darken" => LayerBlendMode::Darken,
        "screen" => LayerBlendMode::Screen,
        "xor" => LayerBlendMode::Xor,
        _ => return output,
    };
    let color = crate::composite::composite_pixel(input.into(), output.into(), mode, 255);
    [color.r, color.g, color.b, color.a]
}

#[derive(Clone, Copy)]
pub struct FractalOptions {
    pub factor: f64,
    pub zoom: f64,
    pub angle: f64,
    pub quality: i32,
    pub mandelbrot: bool,
    pub invert: bool,
}
pub fn pixel(x: i32, y: i32, width: i32, height: i32, options: &FractalOptions) -> [u8; 4] {
    let FractalOptions {
        factor,
        zoom,
        angle,
        quality,
        mandelbrot,
        invert,
    } = *options;
    let count = quality * quality + 1;
    let inv_count = 1.0 / f64::from(count);
    let inv_quality = 1.0 / f64::from(quality);
    let inv_h = 1.0 / f64::from(height);
    let inv_zoom = 1.0 / zoom;
    let theta_offset = angle * std::f64::consts::TAU / 360.0;
    let mut sums = [0u32; 4];
    for index in 0..count {
        let i = f64::from(index);
        let u = (2.0 * f64::from(x) - f64::from(width) + i * inv_count) * inv_h;
        let v = (2.0 * f64::from(y) - f64::from(height) + (i * inv_quality).fract()) * inv_h;
        let radius = (u * u + v * v).sqrt();
        let theta = v.atan2(u) + theta_offset;
        let up = radius * theta.cos();
        let vp = radius * theta.sin();
        let value = if mandelbrot {
            mandelbrot_value(up * inv_zoom - 0.7, vp * inv_zoom - 0.29, factor as i32)
        } else {
            let aspect = f64::from(height) / f64::from(width);
            julia_value((up - vp * aspect) * inv_zoom, (vp + up * aspect) * inv_zoom)
        };
        let c = if mandelbrot {
            64.0 + factor * value
        } else {
            factor * value
        };
        for (channel, sum) in sums.iter_mut().enumerate() {
            let offset = if channel == 3 {
                0
            } else if mandelbrot {
                3 - channel
            } else {
                channel + 1
            };
            *sum += (c - 256.0 * offset as f64).clamp(0.0, 255.0) as u32;
        }
    }
    let mut out = [0; 4];
    for (channel, output) in out.iter_mut().enumerate() {
        *output = (sums[channel] / count as u32) as u8;
        if invert && channel < 3 {
            *output = 255 - *output;
        }
    }
    out
}

fn julia_value(mut x: f64, mut y: f64) -> f64 {
    let mut c = 0;
    while c < 256 && x * x + y * y < 10000.0 {
        let t = x;
        x = x * x - y * y + 0.3125;
        y = 2.0 * t * y + 0.03;
        c += 1;
    }
    f64::from(c) - 2.0 + 2.0 * 10000.0f64.ln() / (x * x + y * y).ln()
}

fn mandelbrot_value(r: f64, i: f64, factor: i32) -> f64 {
    let (mut x, mut y, mut c) = (0.0, 0.0, 0);
    while c * factor < 1024 && x * x + y * y < 100000.0 {
        let t = x;
        x = x * x - y * y + r;
        y = 2.0 * t * y + i;
        c += 1;
    }
    f64::from(c) - (y * y + x * x).ln() / 100000.0f64.ln()
}
