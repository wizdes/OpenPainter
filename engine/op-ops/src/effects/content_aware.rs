//! PatchMatch fill based on Barnes et al. (2009), sections 3 and 4.
//! Matching uses CIELAB D65 after the standard sRGB transfer function. Alpha
//! is retained as RGBA8; a source patch is legal only when all its pixels are
//! outside the hole and have nonzero alpha. All searches use one fixed PRNG
//! stream and a serial scan, so thread count cannot change the result.
use crate::effect::{self, *};
use op_core::doc::{DocState, SelectionSource, SelectionState};
use op_core::selection::RasterMode;
use op_core::tiles::{TILE_SIZE, TileCoord, TileData};

const R: isize = 3;
const ITERS: usize = 5;

pub fn spec() -> EffectSpec {
    EffectSpec {
        id: "effect.fill.content_aware",
        category: "Edit",
        label_key: "effect.content_aware",
        schema: ParamSchema::default(),
        contract: ArithmeticContract {
            alpha_form: AlphaForm::Straight,
            working_space: WorkingSpace::Encoded,
            quantization_points: &["CIELAB match", "RGBA8 vote", "selection coverage"],
            rounding: RoundingRule::NearestHalfUp,
            border: BorderRule::Clamp,
            target: "own",
        },
        prepare: effect::prepare_noop,
        required_input: effect::pointwise_input,
        passes: &[PassKind::Iterative],
        render_cpu: |sample, x, y, _, _| sample(x, y),
        gpu: None,
    }
}

#[derive(Clone)]
struct Level {
    w: usize,
    h: usize,
    rgba: Vec<u8>,
    mask: Vec<u8>,
}
impl Level {
    fn n(&self) -> usize {
        self.w * self.h
    }
}

fn srgb(v: u8) -> f32 {
    let v = v as f32 / 255.0;
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}
fn lab(rgb: &[u8]) -> [f32; 3] {
    let r = srgb(rgb[0]);
    let g = srgb(rgb[1]);
    let b = srgb(rgb[2]);
    let x = (0.4124564 * r + 0.3575761 * g + 0.1804375 * b) / 0.95047;
    let y = 0.2126729 * r + 0.7151522 * g + 0.0721750 * b;
    let z = (0.0193339 * r + 0.119_192 * g + 0.9503041 * b) / 1.08883;
    let f = |t: f32| {
        if t > 0.008856452 {
            t.cbrt()
        } else {
            7.787037 * t + 16.0 / 116.0
        }
    };
    let (x, y, z) = (f(x), f(y), f(z));
    [116.0 * y - 16.0, 500.0 * (x - y), 200.0 * (y - z)]
}
fn labs(rgba: &[u8]) -> Vec<[f32; 3]> {
    rgba.chunks_exact(4).map(lab).collect()
}

fn downsample(a: &Level) -> Level {
    let w = a.w.div_ceil(2);
    let h = a.h.div_ceil(2);
    let mut rgba = vec![0; w * h * 4];
    let mut mask = vec![0; w * h];
    // Separable 1-4-6-4-1 Gaussian, sampled at even coordinates.
    let kernel = [1u32, 4, 6, 4, 1];
    for y in 0..h {
        for x in 0..w {
            let mut sum = [0u32; 4];
            let mut weight = 0;
            for (ky, &wy) in kernel.iter().enumerate() {
                for (kx, &wx) in kernel.iter().enumerate() {
                    let sx = (2 * x as isize + kx as isize - 2).clamp(0, a.w as isize - 1) as usize;
                    let sy = (2 * y as isize + ky as isize - 2).clamp(0, a.h as isize - 1) as usize;
                    let si = sy * a.w + sx;
                    let ww = wx * wy;
                    if a.mask[si] == 0 {
                        for (c, value) in sum.iter_mut().enumerate() {
                            *value += ww * a.rgba[4 * si + c] as u32;
                        }
                        weight += ww;
                    }
                }
            }
            let i = y * w + x;
            let divisor = weight.max(1);
            for c in 0..4 {
                rgba[4 * i + c] = ((sum[c] + weight / 2) / divisor) as u8;
            }
            if weight == 0 {
                rgba[4 * i + 3] = 255;
            }
            for sy in 2 * y..(2 * y + 2).min(a.h) {
                for sx in 2 * x..(2 * x + 2).min(a.w) {
                    mask[i] = mask[i].max(a.mask[sy * a.w + sx]);
                }
            }
        }
    }
    Level { w, h, rgba, mask }
}
fn legal_sources(a: &Level) -> Vec<usize> {
    let mut bad = vec![0u32; (a.w + 1) * (a.h + 1)];
    for y in 0..a.h {
        let mut row = 0;
        for x in 0..a.w {
            let i = y * a.w + x;
            row += u32::from(a.mask[i] > 0 || a.rgba[4 * i + 3] == 0);
            bad[(y + 1) * (a.w + 1) + x + 1] = bad[y * (a.w + 1) + x + 1] + row;
        }
    }
    let mut out = Vec::new();
    if a.w < 7 || a.h < 7 {
        return out;
    }
    for y in 3..a.h - 3 {
        for x in 3..a.w - 3 {
            let x0 = x - 3;
            let x1 = x + 4;
            let y0 = y - 3;
            let y1 = y + 4;
            let count = bad[y1 * (a.w + 1) + x1] + bad[y0 * (a.w + 1) + x0]
                - bad[y0 * (a.w + 1) + x1]
                - bad[y1 * (a.w + 1) + x0];
            if count == 0 {
                out.push(y * a.w + x);
            }
        }
    }
    out
}
fn targets(a: &Level) -> Vec<usize> {
    // A 7x7 vote still covers every pixel when centers are three apart.
    // Use dense centers for smaller images, where quality matters most.
    let step = spacing(a);
    let mut seen = vec![false; a.n()];
    for (i, &m) in a.mask.iter().enumerate() {
        if m > 0 {
            let x = i % a.w;
            let y = i / a.w;
            for yy in y.saturating_sub(3)..=(y + 3).min(a.h - 1) {
                for xx in x.saturating_sub(3)..=(x + 3).min(a.w - 1) {
                    seen[yy * a.w + xx] = true;
                }
            }
        }
    }
    seen.iter()
        .enumerate()
        .filter_map(|(i, &v)| {
            (v && (i % a.w).is_multiple_of(step) && (i / a.w).is_multiple_of(step)).then_some(i)
        })
        .collect()
}
fn spacing(a: &Level) -> usize {
    if a.n() >= 8_000_000 {
        3
    } else if a.n() >= 1_000_000 {
        2
    } else {
        1
    }
}
fn error(
    a: &Level,
    target: &[[f32; 3]],
    source: &[[f32; 3]],
    t: usize,
    s: usize,
    best: f32,
) -> f32 {
    let tx = t % a.w;
    let ty = t / a.w;
    let sx = s % a.w;
    let sy = s / a.w;
    let mut sum = 0.0;
    for dy in -R..=R {
        for dx in -R..=R {
            let xx = tx as isize + dx;
            let yy = ty as isize + dy;
            if xx < 0 || yy < 0 || xx >= a.w as isize || yy >= a.h as isize {
                continue;
            }
            let ti = yy as usize * a.w + xx as usize;
            let si = (sy as isize + dy) as usize * a.w + (sx as isize + dx) as usize;
            let d0 = target[ti][0] - source[si][0];
            let d1 = target[ti][1] - source[si][1];
            let d2 = target[ti][2] - source[si][2];
            sum += d0 * d0 + d1 * d1 + d2 * d2;
            if sum > best {
                return sum;
            }
        }
    }
    sum
}
fn rng(state: &mut u64) -> u64 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    *state
}
fn vote(a: &Level, field: &[usize], errors: &[f32], centers: &[usize], filled: &mut [u8]) {
    let mut sums = vec![[0.0f32; 4]; a.n()];
    let mut weights = vec![0.0f32; a.n()];
    for &t in centers {
        let s = field[t];
        if s == usize::MAX {
            continue;
        }
        let tx = t % a.w;
        let ty = t / a.w;
        let sx = s % a.w;
        let sy = s / a.w;
        let weight = 1.0 / (1.0 + errors[t] / 49.0);
        for dy in -R..=R {
            for dx in -R..=R {
                let xx = tx as isize + dx;
                let yy = ty as isize + dy;
                if xx < 0 || yy < 0 || xx >= a.w as isize || yy >= a.h as isize {
                    continue;
                }
                let ti = yy as usize * a.w + xx as usize;
                if a.mask[ti] == 0 {
                    continue;
                }
                let si = ((sy as isize + dy) as usize * a.w + (sx as isize + dx) as usize) * 4;
                for (c, value) in sums[ti].iter_mut().enumerate() {
                    *value += weight * a.rgba[si + c] as f32;
                }
                weights[ti] += weight;
            }
        }
    }
    for i in 0..a.n() {
        if a.mask[i] > 0 && weights[i] > 0.0 {
            for c in 0..4 {
                filled[4 * i + c] = (sums[i][c] / weights[i]).round().clamp(0.0, 255.0) as u8;
            }
        }
    }
}

/// Fill selected pixels. `progress(done,total)` fires after every EM iteration.
/// No partial result is returned on cancellation or error.
pub fn fill_rgba(
    input: &[u8],
    width: usize,
    height: usize,
    coverage: &[u8],
    check: impl Fn() -> Result<()>,
    progress: impl Fn(usize, usize),
) -> Result<Vec<u8>> {
    if width == 0
        || height == 0
        || input.len() != width * height * 4
        || coverage.len() != width * height
    {
        return Err(EffectError::Parameters(
            "invalid image or selection dimensions".into(),
        ));
    }
    if !coverage.iter().any(|&v| v > 0) {
        return Err(EffectError::Parameters(
            "content-aware fill needs a nonempty selection".into(),
        ));
    }
    let mut canonical = input.to_vec();
    for (i, &mask) in coverage.iter().enumerate() {
        if mask > 0 {
            canonical[4 * i..4 * i + 4].copy_from_slice(&[0, 0, 0, 255]);
        }
    }
    let original = Level {
        w: width,
        h: height,
        rgba: canonical,
        mask: coverage.to_vec(),
    };
    if legal_sources(&original).is_empty() {
        return Err(EffectError::Parameters(
            "selection has no legal opaque source patches".into(),
        ));
    }
    let mut pyramid = vec![original.clone()];
    while pyramid.len() < 4 {
        let last = pyramid.last().unwrap();
        if last.w < 32 || last.h < 32 {
            break;
        }
        let next = downsample(last);
        if legal_sources(&next).is_empty() {
            break;
        }
        pyramid.push(next);
    }
    let total = pyramid.len() * ITERS;
    let mut done = 0;
    let mut previous: Option<Level> = None;
    let mut previous_field: Option<Vec<usize>> = None;
    let mut state = 0x6a09e667f3bcc909u64;
    for a in pyramid.iter().rev() {
        check()?;
        let sources = legal_sources(a);
        let centers = targets(a);
        let source_lab = labs(&a.rgba);
        let mut filled = a.rgba.clone();
        if let Some(prev) = &previous {
            for y in 0..a.h {
                for x in 0..a.w {
                    let i = y * a.w + x;
                    if a.mask[i] > 0 {
                        let pi = ((y / 2).min(prev.h - 1) * prev.w + (x / 2).min(prev.w - 1)) * 4;
                        filled[4 * i..4 * i + 4].copy_from_slice(&prev.rgba[pi..pi + 4]);
                    }
                }
            }
        }
        let mut field = vec![usize::MAX; a.n()];
        let mut errors = vec![f32::INFINITY; a.n()];
        let mut legal = vec![false; a.n()];
        for &s in &sources {
            legal[s] = true;
        }
        for &t in &centers {
            let mut chosen = sources[rng(&mut state) as usize % sources.len()];
            if let (Some(prev), Some(prev_field)) = (&previous, &previous_field) {
                let step = spacing(prev);
                let px = ((t % a.w / 2).min(prev.w - 1) / step) * step;
                let py = ((t / a.w / 2).min(prev.h - 1) / step) * step;
                let ps = prev_field[py * prev.w + px];
                if ps != usize::MAX {
                    let sx = (2 * (ps % prev.w)).min(a.w - 1);
                    let sy = (2 * (ps / prev.w)).min(a.h - 1);
                    let candidate = sy * a.w + sx;
                    if legal[candidate] {
                        chosen = candidate;
                    }
                }
            }
            field[t] = chosen;
        }
        for iter in 0..ITERS {
            check()?;
            let mut target_lab = source_lab.clone();
            for (i, &m) in a.mask.iter().enumerate() {
                if m > 0 {
                    target_lab[i] = lab(&filled[4 * i..4 * i + 4]);
                }
            }
            let forward = iter % 2 == 0;
            let scan: Box<dyn Iterator<Item = &usize>> = if forward {
                Box::new(centers.iter())
            } else {
                Box::new(centers.iter().rev())
            };
            for &t in scan {
                let try_source = |s: usize, field: &mut [usize], errors: &mut [f32]| {
                    if s >= a.n() || !legal[s] {
                        return;
                    }
                    let e = error(a, &target_lab, &source_lab, t, s, errors[t]);
                    if e < errors[t] || (e == errors[t] && s < field[t]) {
                        field[t] = s;
                        errors[t] = e;
                    }
                };
                try_source(field[t], &mut field, &mut errors);
                let x = t % a.w;
                let y = t / a.w;
                let step = spacing(a);
                let near = if forward {
                    [
                        (x.checked_sub(step), Some(y), step as isize, 0isize),
                        (Some(x), y.checked_sub(step), 0, step as isize),
                    ]
                } else {
                    [
                        (
                            x.checked_add(step).filter(|&v| v < a.w),
                            Some(y),
                            -(step as isize),
                            0,
                        ),
                        (
                            Some(x),
                            y.checked_add(step).filter(|&v| v < a.h),
                            0,
                            -(step as isize),
                        ),
                    ]
                };
                for (nx, ny, dx, dy) in near {
                    if let (Some(nx), Some(ny)) = (nx, ny) {
                        let ns = field[ny * a.w + nx];
                        if ns != usize::MAX {
                            let cx = ns as isize % a.w as isize + dx;
                            let cy = ns as isize / a.w as isize + dy;
                            if cx >= 0 && cy >= 0 && cx < a.w as isize && cy < a.h as isize {
                                try_source(
                                    cy as usize * a.w + cx as usize,
                                    &mut field,
                                    &mut errors,
                                );
                            }
                        }
                    }
                }
                let mut radius = a.w.max(a.h);
                while radius > 0 {
                    let best = field[t];
                    let bx = best % a.w;
                    let by = best / a.w;
                    let lo_x = bx.saturating_sub(radius).max(3);
                    let hi_x = (bx + radius).min(a.w - 4);
                    let lo_y = by.saturating_sub(radius).max(3);
                    let hi_y = (by + radius).min(a.h - 4);
                    if lo_x <= hi_x && lo_y <= hi_y {
                        let rx = lo_x + rng(&mut state) as usize % (hi_x - lo_x + 1);
                        let ry = lo_y + rng(&mut state) as usize % (hi_y - lo_y + 1);
                        try_source(ry * a.w + rx, &mut field, &mut errors);
                    }
                    radius /= 2;
                }
            }
            vote(a, &field, &errors, &centers, &mut filled);
            done += 1;
            progress(done, total);
        }
        previous_field = Some(field);
        previous = Some(Level {
            w: a.w,
            h: a.h,
            rgba: filled,
            mask: a.mask.clone(),
        });
    }
    let filled = &previous.unwrap().rgba;
    let mut output = input.to_vec();
    for (i, &c) in coverage.iter().enumerate() {
        if c > 0 {
            for ch in 0..4 {
                output[4 * i + ch] =
                    effect::coverage_lerp(input[4 * i + ch], filled[4 * i + ch], c);
            }
        }
    }
    Ok(output)
}

pub(crate) fn render_document(
    doc: &DocState,
    dirty: Rect,
    coords: &[TileCoord],
    check: impl Fn() -> Result<()>,
    progress: impl Fn(usize, usize),
) -> Result<DocState> {
    let n = doc.width as usize * doc.height as usize;
    let selection = match &doc.selection {
        SelectionState::None => {
            return Err(EffectError::Parameters(
                "content-aware fill needs an active selection".into(),
            ));
        }
        SelectionState::Active(SelectionSource::Coverage(m)) => m.clone(),
        SelectionState::Active(SelectionSource::Geometry(_)) => {
            doc.rasterize_selection(RasterMode::Antialiased)
        }
    };
    if selection.width != doc.width || selection.height != doc.height || selection.bytes.len() != n
    {
        return Err(EffectError::InvalidMask);
    }
    let mut input = vec![0; n * 4];
    for coord in effect::tile_coords(Rect::new(0, 0, doc.width, doc.height)) {
        check()?;
        let tile = doc
            .read_tile(doc.active_layer, coord)
            .map_err(|e| EffectError::Tile(e.to_string()))?;
        for py in 0..TILE_SIZE {
            let y = coord.y as usize * TILE_SIZE + py;
            if y >= doc.height as usize {
                break;
            }
            let x = coord.x as usize * TILE_SIZE;
            let count = TILE_SIZE.min(doc.width as usize - x);
            input[(y * doc.width as usize + x) * 4..(y * doc.width as usize + x + count) * 4]
                .copy_from_slice(&tile[py * TILE_SIZE * 4..(py * TILE_SIZE + count) * 4]);
        }
    }
    let output = fill_rgba(
        &input,
        doc.width as usize,
        doc.height as usize,
        &selection.bytes,
        &check,
        progress,
    )?;
    let mut next = doc.clone();
    for &coord in coords {
        check()?;
        if coord.x < 0
            || coord.y < 0
            || coord.x as u64 * TILE_SIZE as u64 >= doc.width as u64
            || coord.y as u64 * TILE_SIZE as u64 >= doc.height as u64
        {
            continue;
        }
        let mut tile = doc
            .read_tile(doc.active_layer, coord)
            .map_err(|e| EffectError::Tile(e.to_string()))?
            .to_vec();
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
                if (x as i64) < dirty.x as i64
                    || (y as i64) < dirty.y as i64
                    || (x as i64) >= dirty.x as i64 + dirty.width as i64
                    || (y as i64) >= dirty.y as i64 + dirty.height as i64
                {
                    continue;
                }
                let src = (y * doc.width as usize + x) * 4;
                let dst = (py * TILE_SIZE + px) * 4;
                tile[dst..dst + 4].copy_from_slice(&output[src..src + 4]);
            }
        }
        next = next
            .write_tile(
                doc.active_layer,
                coord,
                TileData::try_from(tile).map_err(|e| EffectError::Tile(e.to_string()))?,
            )
            .map_err(|e| EffectError::Tile(e.to_string()))?;
    }
    Ok(next)
}
