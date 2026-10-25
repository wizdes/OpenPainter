//! Gradient math ported from OpenPDN 3.36, `src/Core/GradientRenderers.cs`
//! and `src/Core/GradientRenderer.cs` (MIT).
//! Copyright dotPDN LLC, Rick Brewster, Tom Jackson, and contributors.
//! Transparency mode runs from the primary alpha to 255 minus the secondary alpha.
use op_core::doc::{DocError, DocState, LayerId, SelectionState, ToolCompositeMode};
use op_core::selection::RasterMode;
use op_core::tiles::{TILE_SIZE, TileCoord, TileData};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GradientType {
    Linear,
    LinearReflected,
    Diamond,
    Radial,
    Conical,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GradientMode {
    Color,
    Transparency,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Handle {
    Start,
    End,
}
#[derive(Clone, Copy, Debug)]
pub struct GradientOptions {
    pub shape: GradientType,
    pub mode: GradientMode,
    pub primary: [u8; 4],
    pub secondary: [u8; 4],
    pub antialias: bool,
    pub blend: ToolCompositeMode,
}
#[derive(Debug)]
pub enum GradientError {
    BadInput(&'static str),
    Document(DocError),
}
impl std::fmt::Display for GradientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BadInput(s) => write!(f, "{s}"),
            Self::Document(e) => write!(f, "{e}"),
        }
    }
}
impl std::error::Error for GradientError {}
impl From<DocError> for GradientError {
    fn from(e: DocError) -> Self {
        Self::Document(e)
    }
}
pub type Result<T> = std::result::Result<T, GradientError>;

pub struct GradientSession {
    base: DocState,
    pub base_revision: u64,
    pub target_layer: LayerId,
    pub options: GradientOptions,
    pub start: (f64, f64),
    pub end: (f64, f64),
    selection: Option<Arc<op_core::selection::MaskGrid>>,
    generation: u64,
}
impl GradientSession {
    pub fn new(
        base: DocState,
        options: GradientOptions,
        start: (f64, f64),
        end: (f64, f64),
    ) -> Result<Self> {
        validate(start)?;
        validate(end)?;
        let target_layer = base
            .layers
            .get(base.active_layer)
            .ok_or(GradientError::BadInput("no active layer"))?
            .id;
        let selection = if matches!(base.selection, SelectionState::None) {
            None
        } else {
            Some(base.rasterize_selection(RasterMode::Antialiased))
        };
        Ok(Self {
            base_revision: base.revision,
            target_layer,
            base,
            options,
            start,
            end,
            selection,
            generation: 0,
        })
    }
    pub fn update(&mut self, handle: Handle, point: (f64, f64)) -> Result<()> {
        validate(point)?;
        match handle {
            Handle::Start => self.start = point,
            Handle::End => self.end = point,
        }
        self.generation = self.generation.saturating_add(1);
        Ok(())
    }
    pub fn generation(&self) -> u64 {
        self.generation
    }
    pub fn preview(&self) -> Result<DocState> {
        let mut next = self.base.clone();
        let layer = self.base.active_layer;
        for ty in 0..self.base.height.div_ceil(TILE_SIZE as u32) {
            for tx in 0..self.base.width.div_ceil(TILE_SIZE as u32) {
                let coord = TileCoord {
                    x: tx as i32,
                    y: ty as i32,
                };
                let old = self.base.read_tile(layer, coord)?;
                let mut tile = old.clone();
                let w = (self.base.width as usize - tx as usize * TILE_SIZE).min(TILE_SIZE);
                let h = (self.base.height as usize - ty as usize * TILE_SIZE).min(TILE_SIZE);
                for ly in 0..h {
                    for lx in 0..w {
                        let x = tx as usize * TILE_SIZE + lx;
                        let y = ty as usize * TILE_SIZE + ly;
                        let coverage = self
                            .selection
                            .as_ref()
                            .map_or(255, |m| m.bytes[y * self.base.width as usize + x]);
                        if coverage == 0 {
                            continue;
                        }
                        let i = (ly * TILE_SIZE + lx) * 4;
                        let dst: [u8; 4] = old[i..i + 4].try_into().expect("four channels");
                        let raw = self.render_pixel(dst, x as f64, y as f64);
                        let v = mix(dst, raw, coverage);
                        tile[i..i + 4].copy_from_slice(&v);
                    }
                }
                if tile != old {
                    next = next.write_tile(
                        layer,
                        coord,
                        TileData::try_from(tile).map_err(DocError::from)?,
                    )?;
                }
            }
        }
        Ok(next)
    }
    pub fn finish(self) -> Result<DocState> {
        self.preview()
    }
    fn render_pixel(&self, dst: [u8; 4], x: f64, y: f64) -> [u8; 4] {
        if !self.options.antialias || self.start == self.end {
            return self.render_at(dst, x, y);
        }
        let mut sum = [0u32; 4];
        for oy in [-0.375, -0.125, 0.125, 0.375] {
            for ox in [-0.375, -0.125, 0.125, 0.375] {
                let p = self.render_at(dst, x + ox, y + oy);
                for c in 0..4 {
                    sum[c] += u32::from(p[c]);
                }
            }
        }
        sum.map(|v| ((v + 8) / 16) as u8)
    }
    fn render_at(&self, dst: [u8; 4], x: f64, y: f64) -> [u8; 4] {
        let a = &self.options;
        let t = self.factor(x, y);
        // OpenPDN indexes a 256-entry color table with trunc(t * 255).
        let index = (t * 255.0) as u8;
        let color = legacy_blend(a.primary, a.secondary, index);
        // OpenPDN ComputeAlphaOnlyValuesFromColors: the end alpha is 255 - secondary.A.
        let (start_alpha, end_alpha) = (i32::from(a.primary[3]), 255 - i32::from(a.secondary[3]));
        let alpha = (start_alpha + (end_alpha - start_alpha) * i32::from(index) / 255) as u8;
        match (a.mode, a.blend) {
            (GradientMode::Color, ToolCompositeMode::Overwrite) => color,
            (GradientMode::Color, ToolCompositeMode::Normal) => over(color, dst),
            (GradientMode::Transparency, ToolCompositeMode::Overwrite) => {
                [dst[0], dst[1], dst[2], alpha]
            }
            (GradientMode::Transparency, ToolCompositeMode::Normal) => {
                [dst[0], dst[1], dst[2], scale(dst[3], alpha)]
            }
        }
    }
    fn factor(&self, x: f64, y: f64) -> f64 {
        if self.start == self.end {
            return 1.0;
        }
        let (vx, vy) = (self.end.0 - self.start.0, self.end.1 - self.start.1);
        let (dx, dy) = (x - self.start.0, y - self.start.1);
        let norm = vx * vx + vy * vy;
        let t = match self.options.shape {
            GradientType::Linear | GradientType::LinearReflected => (dx * vx + dy * vy) / norm,
            GradientType::Diamond => {
                ((dx * vx + dy * vy) / norm).abs() + ((dx * vy - dy * vx) / norm).abs()
            }
            GradientType::Radial => dx.hypot(dy) / vx.hypot(vy),
            GradientType::Conical => {
                let delta = (dy.atan2(dx) - vy.atan2(vx)) / std::f64::consts::PI;
                let wrapped = if delta > 1.0 {
                    delta - 2.0
                } else if delta < -1.0 {
                    delta + 2.0
                } else {
                    delta
                };
                wrapped.abs()
            }
        };
        let t = if self.options.shape == GradientType::LinearReflected {
            t.abs()
        } else {
            t
        };
        t.clamp(0.0, 1.0)
    }
}
fn validate(p: (f64, f64)) -> Result<()> {
    if p.0.is_finite() && p.1.is_finite() && p.0.abs() < 1.0e9 && p.1.abs() < 1.0e9 {
        Ok(())
    } else {
        Err(GradientError::BadInput("invalid gradient handle"))
    }
}
fn scale(a: u8, b: u8) -> u8 {
    ((u16::from(a) * u16::from(b) + 127) / 255) as u8
}
fn mix(a: [u8; 4], b: [u8; 4], coverage: u8) -> [u8; 4] {
    if coverage == 255 {
        return b;
    }
    std::array::from_fn(|i| {
        ((u16::from(a[i]) * u16::from(255 - coverage)
            + u16::from(b[i]) * u16::from(coverage)
            + 127)
            / 255) as u8
    })
}
fn over(src: [u8; 4], dst: [u8; 4]) -> [u8; 4] {
    let sa = f64::from(src[3]) / 255.0;
    let da = f64::from(dst[3]) / 255.0;
    let out = sa + da * (1.0 - sa);
    if out == 0.0 {
        return dst;
    }
    [
        ((f64::from(src[0]) * sa + f64::from(dst[0]) * da * (1.0 - sa)) / out).round() as u8,
        ((f64::from(src[1]) * sa + f64::from(dst[1]) * da * (1.0 - sa)) / out).round() as u8,
        ((f64::from(src[2]) * sa + f64::from(dst[2]) * da * (1.0 - sa)) / out).round() as u8,
        (out * 255.0).round() as u8,
    ]
}

fn legacy_blend(start: [u8; 4], end: [u8; 4], index: u8) -> [u8; 4] {
    let sa = u32::from(scale(255 - index, start[3]));
    let ea = u32::from(scale(index, end[3]));
    let sum = sa + ea;
    if sum == 0 {
        return [0; 4];
    }
    [
        ((u32::from(start[0]) * sa + u32::from(end[0]) * ea) / sum) as u8,
        ((u32::from(start[1]) * sa + u32::from(end[1]) * ea) / sum) as u8,
        ((u32::from(start[2]) * sa + u32::from(end[2]) * ea) / sum) as u8,
        sum as u8,
    ]
}
