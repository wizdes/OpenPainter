//! Paint.NET 3.36 GaussianBlurEffect.cs, OpenPDN commit cca476b (MIT).
//! 5.1 Gamma Boost is a power transform; quality 1..3 samples every 8, 4, 2 offsets.
//! Both options are provisional until a 5.1 capture is available.
use crate::effect::*;
use op_core::doc::{DocState, SelectionState};
use op_core::tiles::{TILE_SIZE, TileCoord};
use serde_json::{Value, json};

#[derive(Clone, Copy, Debug, Default)]
pub struct GaussianColumn {
    wa: i32,
    wc: i32,
    sums: [i32; 4],
}

fn gamma_power(params: &Value) -> f64 {
    let base = match params["gamma"].as_str().unwrap_or("linear") {
        "srgb" => 2.2,
        "custom" => params["gamma_exponent"].as_f64().unwrap_or(2.2),
        _ if !params["linear_light"].as_bool().unwrap_or(true) => 2.2,
        _ => 1.0,
    };
    let boost = params["gamma_boost"].as_f64().unwrap_or(0.0);
    base * 2.0_f64.powf(boost / 100.0)
}

fn prepare(doc: &DocState, _: &SelectionState, params: &Value) -> Result<Prepared> {
    let width = doc.width as usize;
    let height = doc.height as usize;
    let radius = params["radius"].as_i64().unwrap_or(2) as i32;
    let stride = 1 << (4 - params["quality"].as_i64().unwrap_or(4));
    let power = gamma_power(params);
    if radius == 0 {
        return Ok(Prepared {
            width: width as i32,
            height: height as i32,
            halo: 0,
            gaussian: None,
            noise: None,
            palette: None,
            channel_lut: None,
        });
    }
    let mut pixels = vec![[0u8; 4]; width * height];
    for ty in 0..height.div_ceil(TILE_SIZE) {
        for tx in 0..width.div_ceil(TILE_SIZE) {
            let tile = doc
                .read_tile(
                    doc.active_layer,
                    TileCoord {
                        x: tx as i32,
                        y: ty as i32,
                    },
                )
                .map_err(|e| EffectError::Tile(e.to_string()))?;
            for py in 0..TILE_SIZE.min(height - ty * TILE_SIZE) {
                for px in 0..TILE_SIZE.min(width - tx * TILE_SIZE) {
                    let i = (py * TILE_SIZE + px) * 4;
                    pixels[(ty * TILE_SIZE + py) * width + tx * TILE_SIZE + px]
                        .copy_from_slice(&tile[i..i + 4]);
                }
            }
        }
    }
    let mut columns = vec![GaussianColumn::default(); width * height];
    for y in 0..height {
        for x in 0..width {
            let column = &mut columns[y * width + x];
            for dy in -radius..=radius {
                if dy % stride != 0 {
                    continue;
                }
                let yy = y as i32 + dy;
                if yy < 0 || yy >= height as i32 {
                    continue;
                }
                let c = pixels[yy as usize * width + x];
                let wy = 16 * (radius + 1 - dy.abs()) as i64;
                column.wa += wy as i32;
                let alpha_weight = wy * (i64::from(c[3]) + i64::from(c[3] >> 7));
                column.wc += alpha_weight as i32;
                let weight = alpha_weight >> 8;
                for (channel, byte) in c.iter().enumerate() {
                    let value = if channel < 3 && power != 1.0 {
                        ((f64::from(*byte) / 255.0).powf(power) * 255.0).round() as i64
                    } else {
                        i64::from(*byte)
                    };
                    column.sums[channel] += (weight * value) as i32;
                }
            }
        }
    }
    Ok(Prepared {
        width: width as i32,
        height: height as i32,
        halo: radius,
        gaussian: Some(std::sync::Arc::new(columns)),
        noise: None,
        palette: None,
        channel_lut: None,
    })
}
fn render(sample: &PixelReader<'_>, x: i32, y: i32, prep: &Prepared, params: &Value) -> [u8; 4] {
    let radius = params["radius"].as_i64().unwrap_or(2) as i32;
    let stride = 1 << (4 - params["quality"].as_i64().unwrap_or(4));
    if radius == 0 {
        return sample(x, y);
    }
    let power = gamma_power(params);
    let mut wa_sum = 0i64;
    let mut wc_sum = 0i64;
    let mut sums = [0i64; 4];
    for dx in -radius..=radius {
        if dx % stride != 0 {
            continue;
        }
        let xx = x + dx;
        if xx < 0 || xx >= prep.width {
            continue;
        }
        let wx = 16 * (radius + 1 - dx.abs()) as i64;
        if let Some(columns) = &prep.gaussian {
            let column = columns[y as usize * prep.width as usize + xx as usize];
            wa_sum += wx * i64::from(column.wa);
            wc_sum += wx * i64::from(column.wc);
            for (channel, sum) in sums.iter_mut().enumerate() {
                *sum += wx * i64::from(column.sums[channel]);
            }
            continue;
        }
        let mut wa_col = 0i64;
        let mut wc_col = 0i64;
        let mut col = [0i64; 4];
        for dy in -radius..=radius {
            if dy % stride != 0 {
                continue;
            }
            let yy = y + dy;
            if yy < 0 || yy >= prep.height {
                continue;
            }
            let c = sample(xx, yy);
            let wy = 16 * (radius + 1 - dy.abs()) as i64;
            wa_col += wy;
            let alpha_weight = wy * (i64::from(c[3]) + i64::from(c[3] >> 7));
            wc_col += alpha_weight;
            let weight = alpha_weight >> 8;
            for channel in 0..4 {
                let value = if channel < 3 && power != 1.0 {
                    ((f64::from(c[channel]) / 255.0).powf(power) * 255.0).round() as i64
                } else {
                    i64::from(c[channel])
                };
                col[channel] += weight * value;
            }
        }
        wa_sum += wx * wa_col;
        wc_sum += wx * wc_col;
        for channel in 0..4 {
            sums[channel] += wx * col[channel];
        }
    }
    wc_sum >>= 8;
    if wa_sum == 0 || wc_sum == 0 {
        return [0; 4];
    }
    let mut out = [0; 4];
    for channel in 0..3 {
        let mut value = (sums[channel] / wc_sum).clamp(0, 255) as f64;
        if power != 1.0 {
            value = (value / 255.0).powf(1.0 / power) * 255.0;
        }
        out[channel] = value.round().clamp(0.0, 255.0) as u8;
    }
    out[3] = (sums[3] / wa_sum).clamp(0, 255) as u8;
    out
}
pub fn spec() -> EffectSpec {
    EffectSpec {
        id: "effect.blur.gaussian",
        category: "Blurs",
        label_key: "effect.gaussian_blur",
        schema: ParamSchema {
            fields: vec![
                Param {
                    key: "radius",
                    kind: ParamKind::Int,
                    min: Some(0.0),
                    max: Some(200.0),
                    default: Some(json!(2)),
                },
                Param {
                    key: "gamma",
                    kind: ParamKind::Choice(&["linear", "srgb", "custom"]),
                    min: None,
                    max: None,
                    default: Some(json!("linear")),
                },
                Param {
                    key: "gamma_exponent",
                    kind: ParamKind::Float,
                    min: Some(0.1),
                    max: Some(5.0),
                    default: Some(json!(2.2)),
                },
                Param {
                    key: "gamma_boost",
                    kind: ParamKind::Float,
                    min: Some(-100.0),
                    max: Some(100.0),
                    default: Some(json!(0.0)),
                },
                Param {
                    key: "linear_light",
                    kind: ParamKind::Bool,
                    min: None,
                    max: None,
                    default: Some(json!(true)),
                },
                Param {
                    key: "quality",
                    kind: ParamKind::Int,
                    min: Some(1.0),
                    max: Some(4.0),
                    default: Some(json!(4)),
                },
            ],
        },
        contract: ArithmeticContract {
            alpha_form: AlphaForm::Straight,
            working_space: WorkingSpace::Encoded,
            quantization_points: &[
                "vertical alpha weight shift",
                "horizontal alpha weight shift",
                "output byte",
            ],
            rounding: RoundingRule::NearestHalfUp,
            border: BorderRule::Transparent,
            target: "legacy-3.36",
        },
        prepare,
        required_input: neighborhood_input,
        passes: &[PassKind::Neighborhood],
        render_cpu: render,
        gpu: Some(GpuKernel::Gaussian),
    }
}

/// Small-image reference for the default legacy radius used by GPU parity.
pub fn reference_rgba8(width: u32, height: u32, bytes: &[u8]) -> Vec<u8> {
    reference_rgba8_with_radius(width, height, bytes, 2)
}

pub fn reference_rgba8_with_radius(width: u32, height: u32, bytes: &[u8], radius: u32) -> Vec<u8> {
    let prepared = Prepared {
        width: width as i32,
        height: height as i32,
        halo: radius as i32,
        gaussian: None,
        noise: None,
        palette: None,
        channel_lut: None,
    };
    let params = json!({"radius":radius,"gamma":"linear","gamma_exponent":2.2,"quality":4});
    let sample = |x: i32, y: i32| {
        if x < 0 || y < 0 || x >= width as i32 || y >= height as i32 {
            return [0; 4];
        }
        let i = (y as usize * width as usize + x as usize) * 4;
        bytes[i..i + 4].try_into().unwrap()
    };
    let mut out = vec![0; bytes.len()];
    for y in 0..height as i32 {
        for x in 0..width as i32 {
            let i = (y as usize * width as usize + x as usize) * 4;
            out[i..i + 4].copy_from_slice(&render(&sample, x, y, &prepared, &params));
        }
    }
    out
}
