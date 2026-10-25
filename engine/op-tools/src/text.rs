//! The Text tool's editable session. Behavior follows OpenPDN `src/tools/TextTool.cs`
//! (MIT; copyright dotPDN LLC, Rick Brewster, Tom Jackson, and contributors): the text
//! stays editable until Finish, which commits one history step.
//!
//! The engine never shapes text. The platform (CoreText on the Mac) lays the text out and
//! hands over raster runs: coverage runs (one byte per pixel, drawn with the primary color)
//! and color-glyph runs (premultiplied RGBA, such as color emoji). This session composites
//! those runs onto the base state, clipped by the selection.
use op_core::doc::{DocError, DocState, LayerId, SelectionState, ToolCompositeMode};
use op_core::pixel::Rgba8;
use op_core::selection::{MaskGrid, RasterMode};
use op_core::tiles::{TILE_SIZE, TileCoord, TileData};
use op_ops::composite::tool_composite;
use std::collections::BTreeMap;
use std::sync::Arc;

/// The largest run side, in pixels.
pub const MAX_RUN_SIDE: u32 = 16_384;
/// The most pixels all runs of one session may hold together.
pub const MAX_RUN_PIXELS: u64 = 64 * 1024 * 1024;
/// The most runs one session may hold.
pub const MAX_RUNS: usize = 4096;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunKind {
    /// One coverage byte per pixel; the pixel color is the primary color.
    Coverage,
    /// Four bytes per pixel, premultiplied RGBA (color glyphs).
    Color,
}

/// One raster run. `x` and `y` are the run's top-left corner relative to the session
/// origin, in document pixels.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextRun {
    pub kind: RunKind,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub pixels: Arc<[u8]>,
}
impl TextRun {
    fn bytes_per_pixel(&self) -> usize {
        match self.kind {
            RunKind::Coverage => 1,
            RunKind::Color => 4,
        }
    }
    fn validate(&self) -> Result<()> {
        if self.width > MAX_RUN_SIDE || self.height > MAX_RUN_SIDE {
            return Err(TextError::BadInput("text run is too large"));
        }
        let expected = self.width as usize * self.height as usize * self.bytes_per_pixel();
        if self.pixels.len() != expected {
            return Err(TextError::BadInput("text run data does not match its size"));
        }
        if self.kind == RunKind::Color
            && self
                .pixels
                .chunks_exact(4)
                .any(|p| p[0] > p[3] || p[1] > p[3] || p[2] > p[3])
        {
            return Err(TextError::BadInput("color run is not premultiplied"));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextStyle {
    /// Straight RGBA. Coverage runs use it; its alpha also scales color runs.
    pub primary: [u8; 4],
    pub blend: ToolCompositeMode,
}

/// The editable text, kept with the session until Finish. `caret` is a byte offset into
/// `text` (the Mac side converts from UTF-16 before it sends it).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TextModel {
    pub text: String,
    pub caret: usize,
}
impl TextModel {
    pub fn new(text: String, caret: usize) -> Result<Self> {
        if caret > text.len() || !text.is_char_boundary(caret) {
            return Err(TextError::BadInput("caret is not on a character boundary"));
        }
        Ok(Self { text, caret })
    }
}

#[derive(Debug)]
pub enum TextError {
    BadInput(&'static str),
    Document(DocError),
}
impl std::fmt::Display for TextError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BadInput(s) => write!(f, "{s}"),
            Self::Document(e) => write!(f, "{e}"),
        }
    }
}
impl std::error::Error for TextError {}
impl From<DocError> for TextError {
    fn from(e: DocError) -> Self {
        Self::Document(e)
    }
}
pub type Result<T> = std::result::Result<T, TextError>;

/// One `tool.text.update`: every field is optional.
#[derive(Clone, Debug, Default)]
pub struct TextUpdate {
    pub runs: Option<Vec<TextRun>>,
    pub origin: Option<(i32, i32)>,
    pub style: Option<TextStyle>,
    pub model: Option<TextModel>,
}

pub struct TextSession {
    base: DocState,
    pub base_revision: u64,
    pub target_layer: LayerId,
    pub origin: (i32, i32),
    pub style: TextStyle,
    runs: Vec<TextRun>,
    model: TextModel,
    selection: Option<Arc<MaskGrid>>,
    generation: u64,
}
impl TextSession {
    pub fn new(
        base: DocState,
        origin: (i32, i32),
        style: TextStyle,
        runs: Vec<TextRun>,
        model: TextModel,
    ) -> Result<Self> {
        validate_runs(&runs)?;
        let target_layer = base
            .layers
            .get(base.active_layer)
            .ok_or(TextError::BadInput("no active layer"))?
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
            origin,
            style,
            runs,
            model,
            selection,
            generation: 0,
        })
    }
    pub fn update(&mut self, update: TextUpdate) -> Result<()> {
        if let Some(runs) = &update.runs {
            validate_runs(runs)?;
        }
        if let Some(runs) = update.runs {
            self.runs = runs;
        }
        if let Some(origin) = update.origin {
            self.origin = origin;
        }
        if let Some(style) = update.style {
            self.style = style;
        }
        if let Some(model) = update.model {
            self.model = model;
        }
        self.generation = self.generation.saturating_add(1);
        Ok(())
    }
    pub fn generation(&self) -> u64 {
        self.generation
    }
    pub fn model(&self) -> &TextModel {
        &self.model
    }
    pub fn runs(&self) -> &[TextRun] {
        &self.runs
    }
    /// The document rectangle the runs cover, clipped to the canvas: (x, y, width, height).
    pub fn bounds(&self) -> Option<(u32, u32, u32, u32)> {
        let (w, h) = (i64::from(self.base.width), i64::from(self.base.height));
        let mut out: Option<(i64, i64, i64, i64)> = None;
        for r in &self.runs {
            let x0 = (i64::from(self.origin.0) + i64::from(r.x)).clamp(0, w);
            let y0 = (i64::from(self.origin.1) + i64::from(r.y)).clamp(0, h);
            let x1 = (i64::from(self.origin.0) + i64::from(r.x) + i64::from(r.width)).clamp(0, w);
            let y1 = (i64::from(self.origin.1) + i64::from(r.y) + i64::from(r.height)).clamp(0, h);
            if x0 >= x1 || y0 >= y1 {
                continue;
            }
            out = Some(match out {
                None => (x0, y0, x1, y1),
                Some((a, b, c, d)) => (a.min(x0), b.min(y0), c.max(x1), d.max(y1)),
            });
        }
        out.map(|(a, b, c, d)| (a as u32, b as u32, (c - a) as u32, (d - b) as u32))
    }
    /// The base state with the runs composited onto the target layer. Finish commits
    /// exactly this state, so the preview equals the commit.
    pub fn preview(&self) -> Result<DocState> {
        let layer = self
            .base
            .layers
            .iter()
            .position(|l| l.id == self.target_layer)
            .ok_or(TextError::BadInput("the text layer is gone"))?;
        let (dw, dh) = (i64::from(self.base.width), i64::from(self.base.height));
        let mut tiles: BTreeMap<(i32, i32), Vec<u8>> = BTreeMap::new();
        let src_primary = Rgba8::from(self.style.primary);
        for run in &self.runs {
            let rx = i64::from(self.origin.0) + i64::from(run.x);
            let ry = i64::from(self.origin.1) + i64::from(run.y);
            let x0 = rx.clamp(0, dw);
            let y0 = ry.clamp(0, dh);
            let x1 = (rx + i64::from(run.width)).clamp(0, dw);
            let y1 = (ry + i64::from(run.height)).clamp(0, dh);
            for y in y0..y1 {
                for x in x0..x1 {
                    let lx = (x - rx) as usize;
                    let ly = (y - ry) as usize;
                    let i = ly * run.width as usize + lx;
                    let (src, raw) = match run.kind {
                        RunKind::Coverage => (src_primary, run.pixels[i]),
                        RunKind::Color => {
                            let p = &run.pixels[i * 4..i * 4 + 4];
                            let a = p[3];
                            if a == 0 {
                                continue;
                            }
                            let straight = |c: u8| {
                                ((u32::from(c) * 255 + u32::from(a) / 2) / u32::from(a)).min(255)
                                    as u8
                            };
                            let src =
                                Rgba8::from([straight(p[0]), straight(p[1]), straight(p[2]), 255]);
                            (src, scale(a, self.style.primary[3]))
                        }
                    };
                    let n = y as usize * self.base.width as usize + x as usize;
                    let sel = self.selection.as_ref().map_or(255, |s| s.bytes[n]);
                    let cov = scale(raw, sel);
                    if cov == 0 {
                        continue;
                    }
                    let coord = (
                        (x as usize / TILE_SIZE) as i32,
                        (y as usize / TILE_SIZE) as i32,
                    );
                    let tile = match tiles.entry(coord) {
                        std::collections::btree_map::Entry::Occupied(e) => e.into_mut(),
                        std::collections::btree_map::Entry::Vacant(e) => {
                            e.insert(self.base.read_tile(
                                layer,
                                TileCoord {
                                    x: coord.0,
                                    y: coord.1,
                                },
                            )?)
                        }
                    };
                    let k = ((y as usize % TILE_SIZE) * TILE_SIZE + x as usize % TILE_SIZE) * 4;
                    let dst = Rgba8::from(<[u8; 4]>::try_from(&tile[k..k + 4]).expect("pixel"));
                    let out = tool_composite(dst, src, self.style.blend, cov);
                    tile[k..k + 4].copy_from_slice(&<[u8; 4]>::from(out));
                }
            }
        }
        let mut next = self.base.clone();
        for ((tx, ty), data) in tiles {
            let coord = TileCoord { x: tx, y: ty };
            if data != self.base.read_tile(layer, coord)? {
                next = next.write_tile(
                    layer,
                    coord,
                    TileData::try_from(data).map_err(DocError::from)?,
                )?;
            }
        }
        Ok(next)
    }
    pub fn finish(self) -> Result<DocState> {
        self.preview()
    }
}

fn validate_runs(runs: &[TextRun]) -> Result<()> {
    if runs.len() > MAX_RUNS {
        return Err(TextError::BadInput("too many text runs"));
    }
    let mut total = 0u64;
    for r in runs {
        r.validate()?;
        total += u64::from(r.width) * u64::from(r.height);
    }
    if total > MAX_RUN_PIXELS {
        return Err(TextError::BadInput("text runs hold too many pixels"));
    }
    Ok(())
}

fn scale(a: u8, b: u8) -> u8 {
    ((u16::from(a) * u16::from(b) + 127) / 255) as u8
}
