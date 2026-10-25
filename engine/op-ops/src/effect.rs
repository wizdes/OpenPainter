//! Tile effect contracts and a deterministic CPU scheduler.
use op_core::doc::{DocState, MaskGrid, SelectionSource, SelectionState};
use op_core::selection::RasterMode;
use op_core::tiles::{TILE_BYTES, TILE_SIZE, TileCoord, TileData};
use rayon::prelude::*;
use serde_json::Value;
use std::collections::HashMap;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum EffectError {
    #[error("invalid parameters: {0}")]
    Parameters(String),
    #[error("invalid coverage mask dimensions or byte count")]
    InvalidMask,
    #[error("effect was cancelled")]
    Cancelled,
    #[error("tile operation failed: {0}")]
    Tile(String),
}
pub type Result<T> = std::result::Result<T, EffectError>;

#[derive(Clone, Debug)]
pub enum ParamKind {
    Int,
    Float,
    Bool,
    Choice(&'static [&'static str]),
    String,
    Curve,
    RgbBytes,
    RgbGamma,
}
#[derive(Clone, Debug)]
pub struct Param {
    pub key: &'static str,
    pub kind: ParamKind,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub default: Option<Value>,
}
#[derive(Clone, Debug, Default)]
pub struct ParamSchema {
    pub fields: Vec<Param>,
}
impl ParamSchema {
    pub fn validate(&self, value: Value) -> Result<Value> {
        let obj = value
            .as_object()
            .ok_or_else(|| EffectError::Parameters("parameters must be a JSON object".into()))?;
        for key in obj.keys() {
            if !self.fields.iter().any(|field| field.key == key) {
                return Err(EffectError::Parameters(format!("unknown parameter {key}")));
            }
        }
        let mut out = serde_json::Map::new();
        for field in &self.fields {
            let v = obj
                .get(field.key)
                .cloned()
                .or_else(|| field.default.clone())
                .ok_or_else(|| EffectError::Parameters(format!("missing {}", field.key)))?;
            let valid = match &field.kind {
                ParamKind::Int => v.as_i64().is_some(),
                ParamKind::Float => v.as_f64().is_some_and(f64::is_finite),
                ParamKind::Bool => v.is_boolean(),
                ParamKind::String => v.is_string(),
                ParamKind::Curve => v.as_array().is_some_and(|channels| {
                    !channels.is_empty()
                        && channels.len() <= 3
                        && channels.iter().all(|channel| {
                            channel.as_array().is_some_and(|points| {
                                points.len() >= 2
                                    && points.iter().all(|point| {
                                        point.as_array().is_some_and(|xy| {
                                            xy.len() == 2
                                                && xy
                                                    .iter()
                                                    .all(|n| n.as_u64().is_some_and(|n| n <= 255))
                                        })
                                    })
                            })
                        })
                }),
                ParamKind::RgbBytes => v.as_array().is_some_and(|channels| {
                    channels.len() == 3
                        && channels
                            .iter()
                            .all(|n| n.as_u64().is_some_and(|n| n <= 255))
                }),
                ParamKind::RgbGamma => v.as_array().is_some_and(|channels| {
                    channels.len() == 3
                        && channels.iter().all(|n| {
                            n.as_f64()
                                .is_some_and(|n| n.is_finite() && (0.1..=10.0).contains(&n))
                        })
                }),
                ParamKind::Choice(choices) => v.as_str().is_some_and(|s| choices.contains(&s)),
            };
            if !valid {
                return Err(EffectError::Parameters(format!("invalid {}", field.key)));
            }
            if let Some(n) = v.as_f64()
                && (field.min.is_some_and(|min| n < min) || field.max.is_some_and(|max| n > max))
            {
                return Err(EffectError::Parameters(format!(
                    "{} out of range",
                    field.key
                )));
            }
            out.insert(field.key.into(), v);
        }
        Ok(Value::Object(out))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}
impl Rect {
    pub const fn new(x: i32, y: i32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlphaForm {
    Straight,
    Premultiplied,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkingSpace {
    Encoded,
    Linear,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoundingRule {
    NearestHalfUp,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BorderRule {
    Clamp,
    Wrap,
    Mirror,
    Transparent,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PassKind {
    Pointwise,
    Neighborhood,
    Reduction,
    Warp,
    Iterative,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GpuKernel {
    InvertColors,
    InvertAlpha,
    BlackAndWhite,
    BrightnessContrast,
    Posterize,
    Sepia,
    TemperatureTint,
    Curves,
    Levels,
    Exposure,
    HighlightsShadows,
    Gaussian,
    Fragment,
    Median,
    Motion,
    Radial,
    Zoom,
    EdgeDetect,
    Emboss,
    Relief,
    OilPainting,
    Outline,
    InkSketch,
    PencilSketch,
    Pixelate,
    Bulge,
    Twist,
}
#[derive(Clone, Debug)]
pub struct ArithmeticContract {
    pub alpha_form: AlphaForm,
    pub working_space: WorkingSpace,
    pub quantization_points: &'static [&'static str],
    pub rounding: RoundingRule,
    pub border: BorderRule,
    pub target: &'static str,
}
#[derive(Clone, Debug, Default)]
pub struct Prepared {
    /// Per-channel lookup tables (Auto-Level, Levels, Curves, Exposure).
    pub channel_lut: Option<Box<[[u8; 256]; 3]>>,
    /// Document size; zero means "unbounded", so every tile pixel renders.
    pub width: i32,
    pub height: i32,
    pub halo: i32,
    pub gaussian: Option<std::sync::Arc<Vec<crate::effects::gaussian_blur::GaussianColumn>>>,
}
pub type PixelReader<'a> = dyn Fn(i32, i32) -> [u8; 4] + 'a;
pub struct TileInputs<'a> {
    pub output_rect: Rect,
    pub sample: &'a PixelReader<'a>,
}
pub struct EffectSpec {
    pub id: &'static str,
    pub category: &'static str,
    pub label_key: &'static str,
    pub schema: ParamSchema,
    pub contract: ArithmeticContract,
    pub prepare: fn(&DocState, &SelectionState, &Value) -> Result<Prepared>,
    pub required_input: fn(Rect, &Prepared) -> Rect,
    pub passes: &'static [PassKind],
    pub render_cpu: fn(&PixelReader<'_>, i32, i32, &Prepared, &Value) -> [u8; 4],
    pub gpu: Option<GpuKernel>,
}
impl EffectSpec {
    pub fn default_params(&self) -> Value {
        self.schema
            .validate(Value::Object(serde_json::Map::new()))
            .expect("valid effect defaults")
    }
    /// Produces a complete RGBA8 tile from the immutable input snapshot.
    /// Selection coverage is applied by the scheduler after this render.
    pub fn render_cpu(
        &self,
        input: &TileInputs<'_>,
        prepared: &Prepared,
        params: &Value,
    ) -> Vec<u8> {
        let mut output = vec![0; TILE_BYTES];
        for py in 0..TILE_SIZE {
            for px in 0..TILE_SIZE {
                let x = input.output_rect.x + px as i32;
                let y = input.output_rect.y + py as i32;
                if prepared.width > 0
                    && (x < 0 || x >= prepared.width || y < 0 || y >= prepared.height)
                {
                    continue;
                }
                let pixel = (self.render_cpu)(input.sample, x, y, prepared, params);
                let index = (py * TILE_SIZE + px) * 4;
                output[index..index + 4].copy_from_slice(&pixel);
            }
        }
        output
    }
}
pub fn prepare_noop(doc: &DocState, _: &SelectionState, _: &Value) -> Result<Prepared> {
    Ok(Prepared {
        width: doc.width as i32,
        height: doc.height as i32,
        ..Prepared::default()
    })
}
pub fn pointwise_input(rect: Rect, _: &Prepared) -> Rect {
    rect
}
pub fn neighborhood_input(rect: Rect, prepared: &Prepared) -> Rect {
    let halo = prepared.halo.max(0);
    Rect::new(
        rect.x.saturating_sub(halo),
        rect.y.saturating_sub(halo),
        rect.width.saturating_add((halo as u32).saturating_mul(2)),
        rect.height.saturating_add((halo as u32).saturating_mul(2)),
    )
}
pub fn whole_image_input(_: Rect, prepared: &Prepared) -> Rect {
    Rect::new(0, 0, prepared.width as u32, prepared.height as u32)
}
pub fn blend_samples(samples: &[[u8; 4]]) -> [u8; 4] {
    if samples.is_empty() {
        return [255, 255, 255, 0];
    }
    let alpha: u64 = samples.iter().map(|s| u64::from(s[3])).sum();
    if alpha == 0 {
        return [0; 4];
    }
    let mut out = [0; 4];
    for channel in 0..3 {
        let sum: u64 = samples
            .iter()
            .map(|s| u64::from(s[channel]) * u64::from(s[3]))
            .sum();
        out[channel] = (sum / alpha) as u8;
    }
    out[3] = (alpha / samples.len() as u64) as u8;
    out
}
pub fn registered() -> Vec<EffectSpec> {
    vec![
        crate::effects::auto_level::spec(),
        crate::effects::black_and_white::spec(),
        crate::effects::brightness_contrast::spec(),
        crate::effects::curves::spec(),
        crate::effects::exposure::spec(),
        crate::effects::highlights_shadows::spec(),
        crate::effects::hue_saturation::spec(),
        crate::effects::invert_alpha::spec(),
        crate::effects::invert_colors::spec(),
        crate::effects::levels::spec(),
        crate::effects::posterize::spec(),
        crate::effects::sepia::spec(),
        crate::effects::temperature_tint::spec(),
        crate::effects::ink_sketch::spec(),
        crate::effects::oil_painting::spec(),
        crate::effects::pencil_sketch::spec(),
        crate::effects::fragment::spec(),
        crate::effects::gaussian_blur::spec(),
        crate::effects::median_blur::spec(),
        crate::effects::motion_blur::spec(),
        crate::effects::radial_blur::spec(),
        crate::effects::zoom_blur::spec(),
        crate::effects::bulge::spec(),
        crate::effects::frosted_glass::spec(),
        crate::effects::pixelate::spec(),
        crate::effects::polar_inversion::spec(),
        crate::effects::tile_reflection::spec(),
        crate::effects::twist::spec(),
        crate::effects::edge_detect::spec(),
        crate::effects::emboss::spec(),
        crate::effects::outline::spec(),
        crate::effects::relief::spec(),
        crate::effects::content_aware::spec(),
    ]
}
pub fn find(id: &str) -> Option<EffectSpec> {
    registered().into_iter().find(|spec| spec.id == id)
}
/// Whole-image CPU reference for default GPU effect parameters.
pub fn reference_rgba8(id: &str, width: u32, height: u32, bytes: &[u8]) -> Result<Vec<u8>> {
    let spec = find(id).ok_or_else(|| EffectError::Parameters(format!("unknown effect {id}")))?;
    if bytes.len() != width as usize * height as usize * 4 {
        return Err(EffectError::Parameters("image byte count".into()));
    }
    let params = spec.schema.validate(serde_json::json!({}))?;
    let prepared = Prepared {
        width: width as i32,
        height: height as i32,
        ..Prepared::default()
    };
    let sample = |x: i32, y: i32| {
        if x < 0 || y < 0 || x >= width as i32 || y >= height as i32 {
            return [0; 4];
        }
        let i = (y as usize * width as usize + x as usize) * 4;
        bytes[i..i + 4].try_into().unwrap()
    };
    let mut output = vec![0; bytes.len()];
    for y in 0..height as i32 {
        for x in 0..width as i32 {
            let i = (y as usize * width as usize + x as usize) * 4;
            output[i..i + 4].copy_from_slice(&(spec.render_cpu)(&sample, x, y, &prepared, &params));
        }
    }
    Ok(output)
}
pub fn tile_coords(rect: Rect) -> Vec<TileCoord> {
    if rect.width == 0 || rect.height == 0 {
        return Vec::new();
    }
    let size = TILE_SIZE as i64;
    let left = (rect.x as i64).div_euclid(size).max(0);
    let top = (rect.y as i64).div_euclid(size).max(0);
    let right = (rect.x as i64 + rect.width as i64 - 1).div_euclid(size);
    let bottom = (rect.y as i64 + rect.height as i64 - 1).div_euclid(size);
    (top..=bottom)
        .flat_map(|y| {
            (left..=right).map(move |x| TileCoord {
                x: x as i32,
                y: y as i32,
            })
        })
        .collect()
}
/// Straight-channel interpolation: `(original*(255-c) + effected*c + 127) / 255`.
/// The one shared final rounding step is integer nearest, with half values rounded up.
pub fn coverage_lerp(original: u8, effected: u8, coverage: u8) -> u8 {
    ((u32::from(original) * (255 - u32::from(coverage))
        + u32::from(effected) * u32::from(coverage)
        + 127)
        / 255) as u8
}
/// Selection coverage at one pixel. Geometry selections use the selection
/// engine's antialiased area coverage (`raster`), rasterized once per render.
fn mask_coverage(
    selection: &SelectionState,
    raster: Option<&MaskGrid>,
    x: i32,
    y: i32,
) -> Result<u8> {
    match selection {
        SelectionState::None => Ok(255),
        SelectionState::Active(SelectionSource::Geometry(_)) => {
            let grid = raster.ok_or(EffectError::InvalidMask)?;
            if x < 0 || y < 0 || x as u32 >= grid.width || y as u32 >= grid.height {
                Ok(0)
            } else {
                Ok(grid.bytes[y as usize * grid.width as usize + x as usize])
            }
        }
        SelectionState::Active(SelectionSource::Coverage(mask)) => {
            if mask.bytes.len() != mask.width as usize * mask.height as usize {
                return Err(EffectError::InvalidMask);
            }
            if x < 0 || y < 0 || x as u32 >= mask.width || y as u32 >= mask.height {
                Ok(0)
            } else {
                Ok(mask.bytes[y as usize * mask.width as usize + x as usize])
            }
        }
    }
}
fn border_coordinate(value: i32, length: u32, rule: BorderRule) -> Option<i32> {
    let length = i64::from(length);
    let value = i64::from(value);
    let mapped = match rule {
        BorderRule::Transparent if value < 0 || value >= length => return None,
        BorderRule::Transparent => value,
        BorderRule::Clamp => value.clamp(0, length - 1),
        BorderRule::Wrap => value.rem_euclid(length),
        BorderRule::Mirror if length == 1 => 0,
        BorderRule::Mirror => {
            let period = 2 * (length - 1);
            let p = value.rem_euclid(period);
            if p < length { p } else { period - p }
        }
    };
    i32::try_from(mapped).ok()
}
fn render_one(
    doc: &DocState,
    spec: &EffectSpec,
    coord: TileCoord,
    dirty: Rect,
    prepared: &Prepared,
    params: &Value,
) -> Result<(TileCoord, TileData)> {
    let x0 = coord.x * TILE_SIZE as i32;
    let y0 = coord.y * TILE_SIZE as i32;
    let output = Rect::new(x0, y0, TILE_SIZE as u32, TILE_SIZE as u32);
    let input = if matches!(spec.contract.border, BorderRule::Wrap | BorderRule::Mirror) {
        Rect::new(0, 0, doc.width, doc.height)
    } else {
        (spec.required_input)(output, prepared)
    };
    let mut tiles = HashMap::new();
    for source_coord in tile_coords(input) {
        if source_coord.x as u32 * (TILE_SIZE as u32) < doc.width
            && source_coord.y as u32 * (TILE_SIZE as u32) < doc.height
        {
            tiles.insert(
                source_coord,
                doc.read_tile(doc.active_layer, source_coord)
                    .map_err(|e| EffectError::Tile(e.to_string()))?,
            );
        }
    }
    let original = doc
        .read_tile(doc.active_layer, coord)
        .map_err(|e| EffectError::Tile(e.to_string()))?;
    let mut result = original.clone();
    let sample = |x: i32, y: i32| -> [u8; 4] {
        let Some(x) = border_coordinate(x, doc.width, spec.contract.border) else {
            return [0; 4];
        };
        let Some(y) = border_coordinate(y, doc.height, spec.contract.border) else {
            return [0; 4];
        };
        let source_coord = TileCoord {
            x: x / TILE_SIZE as i32,
            y: y / TILE_SIZE as i32,
        };
        let Some(bytes) = tiles.get(&source_coord) else {
            return [0; 4];
        };
        let index = ((y as usize % TILE_SIZE) * TILE_SIZE + x as usize % TILE_SIZE) * 4;
        bytes[index..index + 4].try_into().unwrap()
    };
    let effected_tile = EffectSpec::render_cpu(
        spec,
        &TileInputs {
            output_rect: output,
            sample: &sample,
        },
        prepared,
        params,
    );
    let geometry_raster = matches!(
        doc.selection,
        SelectionState::Active(SelectionSource::Geometry(_))
    )
    .then(|| doc.rasterize_selection(RasterMode::Antialiased));
    for py in 0..TILE_SIZE {
        for px in 0..TILE_SIZE {
            let x = x0 + px as i32;
            let y = y0 + py as i32;
            if x < 0
                || y < 0
                || x as u32 >= doc.width
                || y as u32 >= doc.height
                || (x as i64) < dirty.x as i64
                || (y as i64) < dirty.y as i64
                || x as i64 >= dirty.x as i64 + dirty.width as i64
                || y as i64 >= dirty.y as i64 + dirty.height as i64
            {
                continue;
            }
            let coverage = mask_coverage(&doc.selection, geometry_raster.as_deref(), x, y)?;
            if coverage == 0 {
                continue;
            }
            let i = (py * TILE_SIZE + px) * 4;
            for channel in 0..4 {
                result[i + channel] =
                    coverage_lerp(original[i + channel], effected_tile[i + channel], coverage);
            }
        }
    }
    Ok((
        coord,
        TileData::try_from(result).map_err(|e| EffectError::Tile(e.to_string()))?,
    ))
}
pub fn render(
    doc: &DocState,
    spec: &EffectSpec,
    dirty: Rect,
    params: Option<Value>,
) -> Result<DocState> {
    render_tiles(doc, spec, dirty, &tile_coords(dirty), false, params)
}
pub fn render_tiles(
    doc: &DocState,
    spec: &EffectSpec,
    dirty: Rect,
    coords: &[TileCoord],
    parallel: bool,
    params: Option<Value>,
) -> Result<DocState> {
    if spec.id == "effect.fill.content_aware" {
        let _ = spec
            .schema
            .validate(params.unwrap_or_else(|| serde_json::json!({})))?;
        return crate::effects::content_aware::render_document(
            doc,
            dirty,
            coords,
            || Ok(()),
            |_, _| {},
        );
    }
    let params = spec
        .schema
        .validate(params.unwrap_or_else(|| serde_json::json!({})))?;
    let prepared = (spec.prepare)(doc, &doc.selection, &params)?;
    let valid: Vec<_> = coords
        .iter()
        .copied()
        .filter(|coord| {
            coord.x >= 0
                && coord.y >= 0
                && (coord.x as u64) * (TILE_SIZE as u64) < doc.width as u64
                && (coord.y as u64) * (TILE_SIZE as u64) < doc.height as u64
        })
        .collect();
    let rendered: Result<Vec<_>> = if parallel {
        valid
            .par_iter()
            .map(|&coord| render_one(doc, spec, coord, dirty, &prepared, &params))
            .collect()
    } else {
        valid
            .iter()
            .map(|&coord| render_one(doc, spec, coord, dirty, &prepared, &params))
            .collect()
    };
    let mut next = doc.clone();
    for (coord, tile) in rendered? {
        next = next
            .write_tile(doc.active_layer, coord, tile)
            .map_err(|e| EffectError::Tile(e.to_string()))?;
    }
    Ok(next)
}

/// Runs `prepare` once, checks cancellation between tiles, and reports finished tiles.
/// The source is always the original snapshot, so neither tile order nor progress
/// callback timing can change the rendered bytes.
pub fn render_tiles_checked(
    doc: &DocState,
    spec: &EffectSpec,
    dirty: Rect,
    coords: &[TileCoord],
    params: Option<Value>,
    check: impl Fn() -> Result<()>,
    progress: impl Fn(usize, usize),
) -> Result<DocState> {
    if spec.id == "effect.fill.content_aware" {
        let _ = spec
            .schema
            .validate(params.unwrap_or_else(|| serde_json::json!({})))?;
        return crate::effects::content_aware::render_document(doc, dirty, coords, check, progress);
    }
    let params = spec
        .schema
        .validate(params.unwrap_or_else(|| serde_json::json!({})))?;
    let prepared = (spec.prepare)(doc, &doc.selection, &params)?;
    let mut rendered = Vec::new();
    for &coord in coords {
        check()?;
        if coord.x < 0
            || coord.y < 0
            || (coord.x as u64) * (TILE_SIZE as u64) >= doc.width as u64
            || (coord.y as u64) * (TILE_SIZE as u64) >= doc.height as u64
        {
            continue;
        }
        rendered.push(render_one(doc, spec, coord, dirty, &prepared, &params)?);
        progress(rendered.len(), coords.len());
    }
    check()?;
    let mut next = doc.clone();
    for (coord, tile) in rendered {
        next = next
            .write_tile(doc.active_layer, coord, tile)
            .map_err(|e| EffectError::Tile(e.to_string()))?;
    }
    Ok(next)
}
