//! Line geometry follows OpenPDN `src/tools/LineTool.cs` (MIT).
//! Copyright dotPDN LLC, Rick Brewster, Tom Jackson, and contributors.
//! Raster pixels use OpenPainter's selection rasterizer, not GDI+.
use op_core::doc::{DocError, DocState, LayerId, SelectionState, ToolCompositeMode};
use op_core::pixel::Rgba8;
use op_core::selection::{PolygonSet, RasterMode, Selection};
use op_core::tiles::{TILE_SIZE, TileCoord, TileData};
use op_ops::composite::tool_composite;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}
impl Point {
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineKind {
    Straight,
    Bezier,
    Spline,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dash {
    Solid,
    Dash,
    Dot,
    DashDot,
    DashDotDot,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cap {
    Flat,
    Square,
    Round,
    Arrow,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Join {
    Miter,
    Round,
    Bevel,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineState {
    Creating,
    Editing,
    Committing,
}
#[derive(Clone, Copy, Debug)]
pub struct LineOptions {
    pub kind: LineKind,
    pub width: f64,
    pub antialias: bool,
    pub dash: Dash,
    pub start_cap: Cap,
    pub end_cap: Cap,
    pub join: Join,
    pub miter_limit: f64,
    pub primary: [u8; 4],
    pub blend: ToolCompositeMode,
}
#[derive(Debug)]
pub enum LineError {
    BadInput(&'static str),
    Document(DocError),
}
impl std::fmt::Display for LineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BadInput(s) => write!(f, "{s}"),
            Self::Document(e) => write!(f, "{e}"),
        }
    }
}
impl std::error::Error for LineError {}
impl From<DocError> for LineError {
    fn from(e: DocError) -> Self {
        Self::Document(e)
    }
}
type Result<T> = std::result::Result<T, LineError>;

fn valid(p: Point) -> bool {
    p.x.is_finite() && p.y.is_finite() && p.x.abs() < 1e9 && p.y.abs() < 1e9
}
fn validate(points: &[Point], o: LineOptions) -> Result<()> {
    if !points.iter().copied().all(valid)
        || match o.kind {
            LineKind::Straight => points.len() < 2 || points.len() > 1024,
            LineKind::Bezier => points.len() != 4,
            LineKind::Spline => points.len() < 2 || points.len() > 1024,
        }
    {
        return Err(LineError::BadInput("invalid line points"));
    }
    if !o.width.is_finite()
        || !(0.1..=2000.0).contains(&o.width)
        || !o.miter_limit.is_finite()
        || !(1.0..=100.0).contains(&o.miter_limit)
    {
        return Err(LineError::BadInput("invalid line style"));
    }
    Ok(())
}
fn mid(a: Point, b: Point) -> Point {
    Point::new((a.x + b.x) * 0.5, (a.y + b.y) * 0.5)
}
fn dist_line(p: Point, a: Point, b: Point) -> f64 {
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    let len = dx.hypot(dy);
    if len == 0.0 {
        (p.x - a.x).hypot(p.y - a.y)
    } else {
        ((p.x - a.x) * dy - (p.y - a.y) * dx).abs() / len
    }
}
fn flatten_cubic(p: [Point; 4], out: &mut Vec<Point>, depth: u8) {
    if depth >= 20
        || (dist_line(p[1], p[0], p[3]).max(dist_line(p[2], p[0], p[3])) <= 0.025
            && (p[3].x - p[0].x).hypot(p[3].y - p[0].y) > 0.0)
    {
        out.push(p[3]);
        return;
    }
    let a = mid(p[0], p[1]);
    let b = mid(p[1], p[2]);
    let c = mid(p[2], p[3]);
    let d = mid(a, b);
    let e = mid(b, c);
    let f = mid(d, e);
    flatten_cubic([p[0], a, d, f], out, depth + 1);
    flatten_cubic([f, e, c, p[3]], out, depth + 1);
}
/// Flattens in image pixels. The 0.025 px control-point bound leaves margin under 0.1 px.
pub fn flatten(kind: LineKind, points: &[Point]) -> Result<Vec<Point>> {
    let dummy = LineOptions {
        kind,
        width: 1.0,
        antialias: true,
        dash: Dash::Solid,
        start_cap: Cap::Flat,
        end_cap: Cap::Flat,
        join: Join::Miter,
        miter_limit: 4.0,
        primary: [0; 4],
        blend: ToolCompositeMode::Normal,
    };
    validate(points, dummy)?;
    if kind == LineKind::Straight {
        return Ok(points.to_vec());
    }
    let mut out = vec![points[0]];
    if kind == LineKind::Bezier {
        flatten_cubic([points[0], points[1], points[2], points[3]], &mut out, 0);
    } else {
        for i in 0..points.len() - 1 {
            let a = points[i];
            let b = points[i + 1];
            let before = points[i.saturating_sub(1)];
            let after = points[(i + 2).min(points.len() - 1)];
            let p1 = Point::new(a.x + (b.x - before.x) / 6.0, a.y + (b.y - before.y) / 6.0);
            let p2 = Point::new(b.x - (after.x - a.x) / 6.0, b.y - (after.y - a.y) / 6.0);
            flatten_cubic([a, p1, p2, b], &mut out, 0);
        }
    }
    Ok(out)
}
pub struct LineSession {
    base: DocState,
    pub base_revision: u64,
    pub target_layer: LayerId,
    pub points: Vec<Point>,
    pub options: LineOptions,
    pub selected_handle: Option<usize>,
    pub state: LineState,
    selection: Option<Arc<op_core::selection::MaskGrid>>,
    generation: u64,
}
impl LineSession {
    pub fn new(base: DocState, points: Vec<Point>, options: LineOptions) -> Result<Self> {
        validate(&points, options)?;
        let target_layer = base
            .layers
            .get(base.active_layer)
            .ok_or(LineError::BadInput("no active layer"))?
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
            points,
            options,
            selected_handle: None,
            state: LineState::Creating,
            selection,
            generation: 0,
        })
    }
    pub fn generation(&self) -> u64 {
        self.generation
    }
    pub fn update_point(&mut self, index: usize, point: Point) -> Result<()> {
        if index >= self.points.len() || !valid(point) {
            return Err(LineError::BadInput("invalid line handle"));
        }
        self.points[index] = point;
        self.selected_handle = Some(index);
        self.state = LineState::Editing;
        self.generation += 1;
        Ok(())
    }
    pub fn update_points(&mut self, points: Vec<Point>) -> Result<()> {
        validate(&points, self.options)?;
        self.points = points;
        self.selected_handle = None;
        self.state = LineState::Editing;
        self.generation += 1;
        Ok(())
    }
    pub fn update_style(&mut self, options: LineOptions) -> Result<()> {
        validate(&self.points, options)?;
        self.options = options;
        self.state = LineState::Editing;
        self.generation += 1;
        Ok(())
    }
    pub fn update_all(
        &mut self,
        points: Option<Vec<Point>>,
        handle: Option<(usize, Point)>,
        options: Option<LineOptions>,
    ) -> Result<()> {
        let mut next = points.unwrap_or_else(|| self.points.clone());
        if let Some((index, point)) = handle {
            if index >= next.len() || !valid(point) {
                return Err(LineError::BadInput("invalid line handle"));
            }
            next[index] = point;
        }
        let options = options.unwrap_or(self.options);
        validate(&next, options)?;
        self.points = next;
        self.options = options;
        self.selected_handle = handle.map(|(index, _)| index);
        self.state = LineState::Editing;
        self.generation += 1;
        Ok(())
    }
    pub fn preview(&self) -> Result<DocState> {
        let path = flatten(self.options.kind, &self.points)?;
        let mask = rasterize(&path, self.options, self.base.width, self.base.height);
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
                for y in 0..h {
                    for x in 0..w {
                        let px = tx as usize * TILE_SIZE + x;
                        let py = ty as usize * TILE_SIZE + y;
                        let n = py * self.base.width as usize + px;
                        let sel = self.selection.as_ref().map_or(255, |s| s.bytes[n]);
                        let cov = ((u16::from(mask[n]) * u16::from(sel) + 127) / 255) as u8;
                        if cov == 0 {
                            continue;
                        }
                        let k = (y * TILE_SIZE + x) * 4;
                        let dst = Rgba8::from(<[u8; 4]>::try_from(&old[k..k + 4]).expect("pixel"));
                        let result = tool_composite(
                            dst,
                            Rgba8::from(self.options.primary),
                            self.options.blend,
                            cov,
                        );
                        tile[k..k + 4].copy_from_slice(&<[u8; 4]>::from(result));
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
    pub fn finish(mut self) -> Result<DocState> {
        self.state = LineState::Committing;
        self.preview()
    }
}

fn add_polygon(mask: &mut [u8], points: Vec<Point>, width: u32, height: u32, aa: bool) {
    if points.len() < 3 {
        return;
    }
    let min_x = points.iter().map(|p| p.x).fold(f64::INFINITY, f64::min);
    let max_x = points.iter().map(|p| p.x).fold(f64::NEG_INFINITY, f64::max);
    let min_y = points.iter().map(|p| p.y).fold(f64::INFINITY, f64::min);
    let max_y = points.iter().map(|p| p.y).fold(f64::NEG_INFINITY, f64::max);
    let x0 = (min_x.floor() as i64 - 1).clamp(0, i64::from(width)) as u32;
    let x1 = (max_x.ceil() as i64 + 1).clamp(0, i64::from(width)) as u32;
    let y0 = (min_y.floor() as i64 - 1).clamp(0, i64::from(height)) as u32;
    let y1 = (max_y.ceil() as i64 + 1).clamp(0, i64::from(height)) as u32;
    if x0 >= x1 || y0 >= y1 {
        return;
    }
    let shape = PolygonSet(vec![
        points
            .into_iter()
            .map(|p| (p.x - f64::from(x0), p.y - f64::from(y0)))
            .collect(),
    ]);
    let selection = Selection::active_geometry(shape);
    let local = selection.rasterize(
        x1 - x0,
        y1 - y0,
        if aa {
            RasterMode::Antialiased
        } else {
            RasterMode::Pixelated
        },
    );
    for y in y0..y1 {
        for x in x0..x1 {
            let dst = y as usize * width as usize + x as usize;
            let src = (y - y0) as usize * (x1 - x0) as usize + (x - x0) as usize;
            mask[dst] = mask[dst].max(local.bytes[src]);
        }
    }
}
fn circle(mask: &mut [u8], p: Point, r: f64, w: u32, h: u32, aa: bool) {
    let n = 32;
    let poly = (0..n)
        .map(|i| {
            let t = std::f64::consts::TAU * i as f64 / n as f64;
            Point::new(p.x + r * t.cos(), p.y + r * t.sin())
        })
        .collect();
    add_polygon(mask, poly, w, h, aa);
}
fn segment(
    mask: &mut [u8],
    a: Point,
    b: Point,
    r: f64,
    caps: (Cap, Cap),
    size: (u32, u32),
    aa: bool,
) {
    let (start, end) = caps;
    let (w, h) = size;
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    let len = dx.hypot(dy);
    if len == 0.0 {
        if start == Cap::Round || end == Cap::Round {
            circle(mask, a, r, w, h, aa);
        }
        return;
    }
    let ux = dx / len;
    let uy = dy / len;
    let nx = -uy * r;
    let ny = ux * r;
    let sa = if start == Cap::Square {
        Point::new(a.x - ux * r, a.y - uy * r)
    } else {
        a
    };
    let eb = if end == Cap::Square {
        Point::new(b.x + ux * r, b.y + uy * r)
    } else {
        b
    };
    let poly = vec![
        Point::new(sa.x + nx, sa.y + ny),
        Point::new(eb.x + nx, eb.y + ny),
        Point::new(eb.x - nx, eb.y - ny),
        Point::new(sa.x - nx, sa.y - ny),
    ];
    add_polygon(mask, poly, w, h, aa);
    if start == Cap::Round {
        circle(mask, a, r, w, h, aa);
    }
    if end == Cap::Round {
        circle(mask, b, r, w, h, aa);
    }
}
fn arrow(mask: &mut [u8], tip: Point, other: Point, r: f64, w: u32, h: u32, aa: bool) {
    let dx = tip.x - other.x;
    let dy = tip.y - other.y;
    let len = dx.hypot(dy);
    if len == 0.0 {
        return;
    }
    let ux = dx / len;
    let uy = dy / len;
    let size = (r * 4.0).max(3.0);
    let base = Point::new(tip.x - ux * size, tip.y - uy * size);
    add_polygon(
        mask,
        vec![
            tip,
            Point::new(base.x - uy * size * 0.5, base.y + ux * size * 0.5),
            Point::new(base.x + uy * size * 0.5, base.y - ux * size * 0.5),
        ],
        w,
        h,
        aa,
    );
}
fn join(mask: &mut [u8], a: Point, b: Point, c: Point, o: LineOptions, w: u32, h: u32) {
    let (ux, uy) = (b.x - a.x, b.y - a.y);
    let (vx, vy) = (c.x - b.x, c.y - b.y);
    let al = ux.hypot(uy);
    let bl = vx.hypot(vy);
    if al == 0.0 || bl == 0.0 {
        return;
    }
    let (ux, uy, vx, vy) = (ux / al, uy / al, vx / bl, vy / bl);
    let cross = ux * vy - uy * vx;
    if cross.abs() < 1e-10 {
        return;
    }
    let sign = -cross.signum();
    let r = o.width * 0.5;
    let p = Point::new(b.x - uy * r * sign, b.y + ux * r * sign);
    let q = Point::new(b.x - vy * r * sign, b.y + vx * r * sign);
    if o.join == Join::Round {
        circle(mask, b, r, w, h, o.antialias);
        return;
    }
    let mut shape = vec![b, p, q];
    if o.join == Join::Miter {
        let denom = ux * vy - uy * vx;
        let t = ((q.x - p.x) * vy - (q.y - p.y) * vx) / denom;
        let m = Point::new(p.x + ux * t, p.y + uy * t);
        if (m.x - b.x).hypot(m.y - b.y) <= o.miter_limit * r {
            shape = vec![b, p, m, q];
        }
    }
    add_polygon(mask, shape, w, h, o.antialias);
}
pub fn rasterize(path: &[Point], o: LineOptions, w: u32, h: u32) -> Vec<u8> {
    let mut mask = vec![0; w as usize * h as usize];
    if path.len() < 2 {
        return mask;
    }
    let r = o.width * 0.5;
    let pattern: &[f64] = match o.dash {
        Dash::Solid => &[],
        Dash::Dash => &[3.0, 1.0],
        Dash::Dot => &[0.1, 1.9],
        Dash::DashDot => &[3.0, 1.0, 0.1, 1.0],
        Dash::DashDotDot => &[3.0, 1.0, 0.1, 1.0, 0.1, 1.0],
    };
    let mut pattern_i = 0;
    let mut remaining = if pattern.is_empty() {
        f64::INFINITY
    } else {
        pattern[0] * o.width
    };
    for (i, pair) in path.windows(2).enumerate() {
        let a = pair[0];
        let b = pair[1];
        let dx = b.x - a.x;
        let dy = b.y - a.y;
        let len = dx.hypot(dy);
        if len == 0.0 {
            if path.len() == 2 {
                segment(
                    &mut mask,
                    a,
                    b,
                    r,
                    (o.start_cap, o.end_cap),
                    (w, h),
                    o.antialias,
                );
            }
            continue;
        }
        if pattern.is_empty() {
            segment(
                &mut mask,
                a,
                b,
                r,
                (
                    if i == 0 { o.start_cap } else { Cap::Flat },
                    if i + 2 == path.len() {
                        o.end_cap
                    } else {
                        Cap::Flat
                    },
                ),
                (w, h),
                o.antialias,
            );
            if i > 0 {
                join(&mut mask, path[i - 1], a, b, o, w, h);
            }
        } else {
            let mut offset = 0.0;
            while offset < len - 1e-9 {
                let step = remaining.min(len - offset);
                if pattern_i % 2 == 0 {
                    let start = Point::new(a.x + dx * offset / len, a.y + dy * offset / len);
                    let end = Point::new(
                        a.x + dx * (offset + step) / len,
                        a.y + dy * (offset + step) / len,
                    );
                    segment(
                        &mut mask,
                        start,
                        end,
                        r,
                        (Cap::Round, Cap::Round),
                        (w, h),
                        o.antialias,
                    );
                }
                offset += step;
                remaining -= step;
                if remaining <= 1e-9 {
                    pattern_i = (pattern_i + 1) % pattern.len();
                    remaining = pattern[pattern_i] * o.width;
                }
            }
        }
    }
    if o.start_cap == Cap::Arrow {
        arrow(&mut mask, path[0], path[1], r, w, h, o.antialias);
    }
    if o.end_cap == Cap::Arrow {
        arrow(
            &mut mask,
            *path.last().unwrap(),
            path[path.len() - 2],
            r,
            w,
            h,
            o.antialias,
        );
    }
    mask
}

/// Strokes a closed path, including the join where its last edge meets its first.
pub fn rasterize_closed(path: &[Point], options: LineOptions, w: u32, h: u32) -> Vec<u8> {
    let mut mask = rasterize(path, options, w, h);
    if options.dash == Dash::Solid && path.len() >= 4 && path.first() == path.last() {
        join(
            &mut mask,
            path[path.len() - 2],
            path[0],
            path[1],
            options,
            w,
            h,
        );
    }
    mask
}
