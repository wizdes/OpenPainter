// Ported from OpenPDN 3.36 at cca476b (MIT), AutoLevelEffect, HistogramRgb,
// and UnaryPixelOps.Level. Copyright dotPDN LLC and contributors.
use crate::effect::*;
use op_core::doc::{DocState, SelectionSource, SelectionState};
use op_core::selection::RasterMode;
use op_core::tiles::TILE_SIZE;

pub fn spec() -> EffectSpec {
    EffectSpec {
        id: "effect.adjust.auto_level",
        category: "Adjustments",
        label_key: "effect.auto_level",
        schema: ParamSchema::default(),
        contract: ArithmeticContract {
            alpha_form: AlphaForm::Straight,
            working_space: WorkingSpace::Encoded,
            quantization_points: &[
                "histogram percentile",
                "gamma float32",
                "output byte truncation",
            ],
            rounding: RoundingRule::NearestHalfUp,
            border: BorderRule::Clamp,
            target: "legacy-3.36",
        },
        prepare,
        required_input: pointwise_input,
        passes: &[PassKind::Reduction, PassKind::Pointwise],
        render_cpu: |sample, x, y, prepared, _| {
            let mut pixel = sample(x, y);
            if let Some(lut) = &prepared.channel_lut {
                for channel in 0..3 {
                    pixel[channel] = lut[channel][usize::from(pixel[channel])];
                }
            }
            pixel
        },
        gpu: None,
    }
}

pub(crate) fn prepare(
    doc: &DocState,
    selection: &SelectionState,
    _: &serde_json::Value,
) -> Result<Prepared> {
    // OpenPDN 3.36 builds its histogram from selected pixels. The soft mask
    // contributes a pixel once when coverage is nonzero, regardless of weight.
    let geometry = matches!(
        selection,
        SelectionState::Active(SelectionSource::Geometry(_))
    )
    .then(|| doc.rasterize_selection(RasterMode::Antialiased));
    let mask = match selection {
        SelectionState::Active(SelectionSource::Coverage(mask)) => Some(mask.as_ref()),
        _ => geometry.as_deref(),
    };
    if let Some(mask) = mask
        && (mask.width != doc.width
            || mask.height != doc.height
            || mask.bytes.len() != doc.width as usize * doc.height as usize)
    {
        return Err(EffectError::InvalidMask);
    }
    let mut hist = [[0u64; 256]; 3];
    let rect = Rect::new(0, 0, doc.width, doc.height);
    for coord in tile_coords(rect) {
        let tile = doc
            .read_tile(doc.active_layer, coord)
            .map_err(|error| EffectError::Tile(error.to_string()))?;
        for py in 0..TILE_SIZE {
            let y = coord.y as usize * TILE_SIZE + py;
            if y >= doc.height as usize {
                break;
            }
            for px in 0..TILE_SIZE {
                let x = coord.x as usize * TILE_SIZE + px;
                if x >= doc.width as usize {
                    break;
                }
                if mask.is_some_and(|mask| mask.bytes[y * doc.width as usize + x] == 0) {
                    continue;
                }
                let index = (py * TILE_SIZE + px) * 4;
                for channel in 0..3 {
                    hist[channel][usize::from(tile[index + channel])] += 1;
                }
            }
        }
    }
    let mut lut = Box::new([[0u8; 256]; 3]);
    for channel in 0..3 {
        let count: u64 = hist[channel].iter().sum();
        if count == 0 {
            return Ok(Prepared::default());
        }
        let percentile = |fraction: f32| -> u8 {
            let mut sum = 0u64;
            for (value, occurrences) in hist[channel].iter().enumerate() {
                sum += occurrences;
                if (sum as f32) > count as f32 * fraction {
                    return value as u8;
                }
            }
            0
        };
        let low = percentile(0.005);
        let high = percentile(0.995);
        if high <= low {
            return Ok(Prepared::default());
        }
        let weighted: u64 = hist[channel]
            .iter()
            .enumerate()
            .map(|(value, n)| value as u64 * n)
            .sum();
        let mean = ((weighted as f32 / count as f32) + 0.5) as u8;
        let gamma = if low < mean && mean < high {
            ((0.5f64).ln() / (f64::from(mean - low) / f64::from(high - low)).ln()).clamp(0.1, 10.0)
                as f32
        } else {
            1.0
        };
        for (value, output) in lut[channel].iter_mut().enumerate() {
            *output = if value <= usize::from(low) {
                0
            } else if value >= usize::from(high) {
                255
            } else {
                (255.0
                    * (f64::from((value as u8) - low) / f64::from(high - low))
                        .powf(f64::from(gamma))) as u8
            };
        }
    }
    Ok(Prepared {
        channel_lut: Some(lut),
        ..Prepared::default()
    })
}
