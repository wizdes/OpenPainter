//! Discrete Poisson healing: source differences guide four-neighbor edges;
//! pixels outside the brush mask are fixed Dirichlet boundary values.
use super::{CloneSource, Result, StrokeEffect, StrokeSession};
use op_core::doc::DocState;
use op_core::pixel::round_unit_to_u8;
use op_core::tiles::{TILE_SIZE, TileCoord, TileData};
use op_ops::composite::composite_tile;
use std::collections::BTreeMap;

struct Reader<'a> {
    doc: &'a DocState,
    cache: BTreeMap<(usize, i32, i32), Vec<u8>>,
    image: BTreeMap<(i32, i32), Vec<u8>>,
}
impl<'a> Reader<'a> {
    fn new(doc: &'a DocState) -> Self {
        Self {
            doc,
            cache: BTreeMap::new(),
            image: BTreeMap::new(),
        }
    }
    fn pixel(&mut self, source: CloneSource, x: i32, y: i32) -> Result<Option<[u8; 4]>> {
        if x < 0 || y < 0 || x as u32 >= self.doc.width || y as u32 >= self.doc.height {
            return Ok(None);
        }
        let coord = TileCoord {
            x: x / TILE_SIZE as i32,
            y: y / TILE_SIZE as i32,
        };
        let tile = match source {
            CloneSource::Layer(layer) => {
                if !self.cache.contains_key(&(layer, coord.x, coord.y)) {
                    self.cache
                        .insert((layer, coord.x, coord.y), self.doc.read_tile(layer, coord)?);
                }
                &self.cache[&(layer, coord.x, coord.y)]
            }
            CloneSource::Image => {
                if !self.image.contains_key(&(coord.x, coord.y)) {
                    self.image.insert(
                        (coord.x, coord.y),
                        composite_tile(self.doc, coord)?.as_bytes().to_vec(),
                    );
                }
                &self.image[&(coord.x, coord.y)]
            }
        };
        let k = ((y as usize % TILE_SIZE) * TILE_SIZE + x as usize % TILE_SIZE) * 4;
        Ok(Some(tile[k..k + 4].try_into().expect("RGBA pixel")))
    }
}

fn coverage(s: &StrokeSession, x: i32, y: i32) -> f32 {
    if x < 0 || y < 0 || x as u32 >= s.base.width || y as u32 >= s.base.height {
        return 0.0;
    }
    let tile = (x / TILE_SIZE as i32, y / TILE_SIZE as i32);
    let i = (y as usize % TILE_SIZE) * TILE_SIZE + x as usize % TILE_SIZE;
    let brush = s.mask.get(&tile).map_or(0.0, |m| m[i]);
    let selected = s
        .selection
        .as_ref()
        .map_or(255, |m| m.bytes[y as usize * m.width as usize + x as usize]);
    brush * selected as f32 / 255.0 * s.settings.color[3] as f32 / 255.0
}

/// Search clockwise in 8-pixel radial rings, starting after the dab diameter.
/// Score 32 equally spaced border pixels by squared RGBA difference. Reject any
/// patch that overlaps the dab or crosses the canvas. Ties keep the first patch.
/// All reads use the frozen pre-stroke layer, so this search cannot self-sample.
fn spot_offset(
    s: &StrokeSession,
    dest: &mut Reader<'_>,
    cx: i32,
    cy: i32,
) -> Result<Option<(i32, i32)>> {
    let radius = (s.settings.width / 2.0).ceil() as i32;
    let max_radius = (s.base.width.max(s.base.height) as i32).min(512);
    let mut best = None;
    let mut best_score = u64::MAX;
    for distance in ((2 * radius + 2)..=max_radius).step_by(8) {
        for direction in 0..32 {
            let angle = direction as f64 * std::f64::consts::TAU / 32.0;
            let sx = cx + (distance as f64 * angle.cos()).round() as i32;
            let sy = cy + (distance as f64 * angle.sin()).round() as i32;
            if sx - radius - 2 < 0
                || sy - radius - 2 < 0
                || sx + radius + 2 >= s.base.width as i32
                || sy + radius + 2 >= s.base.height as i32
            {
                continue;
            }
            let mut score = 0u64;
            for i in 0..32 {
                let a = i as f64 * std::f64::consts::TAU / 32.0;
                let bx = ((radius + 1) as f64 * a.cos()).round() as i32;
                let by = ((radius + 1) as f64 * a.sin()).round() as i32;
                let d = dest
                    .pixel(CloneSource::Layer(s.layer_index()?), cx + bx, cy + by)?
                    .unwrap();
                let src = dest
                    .pixel(CloneSource::Layer(s.layer_index()?), sx + bx, sy + by)?
                    .unwrap();
                for c in 0..4 {
                    let delta = d[c] as i32 - src[c] as i32;
                    score += (delta * delta) as u64;
                }
            }
            if score < best_score {
                best_score = score;
                best = Some((cx - sx, cy - sy));
            }
        }
        if best_score == 0 {
            break;
        }
    }
    Ok(best)
}

pub(super) fn render(s: &mut StrokeSession) -> Result<DocState> {
    if s.mask.is_empty() {
        return Ok(s.base.clone());
    }
    let mut reader = Reader::new(&s.base);
    let offset = match s.effect {
        StrokeEffect::Heal {
            offset: Some(o), ..
        } => Some(o),
        StrokeEffect::SpotHeal { offset: Some(o) } => Some(o),
        StrokeEffect::SpotHeal { offset: None } => {
            let first = s.last.expect("spot dab has a sample");
            let o = spot_offset(
                s,
                &mut reader,
                first.x.floor() as i32,
                first.y.floor() as i32,
            )?;
            if let StrokeEffect::SpotHeal { offset } = &mut s.effect {
                *offset = o;
            }
            o
        }
        _ => None,
    };
    let Some((dx, dy)) = offset else {
        return Ok(s.base.clone());
    };
    let source = match s.effect {
        StrokeEffect::Heal { source, .. } => source,
        _ => CloneSource::Layer(s.layer_index()?),
    };
    let min_x = s.mask.keys().map(|v| v.0).min().unwrap() * TILE_SIZE as i32;
    let min_y = s.mask.keys().map(|v| v.1).min().unwrap() * TILE_SIZE as i32;
    let max_x = ((s.mask.keys().map(|v| v.0).max().unwrap() + 1) * TILE_SIZE as i32)
        .min(s.base.width as i32);
    let max_y = ((s.mask.keys().map(|v| v.1).max().unwrap() + 1) * TILE_SIZE as i32)
        .min(s.base.height as i32);
    let width = (max_x - min_x) as usize;
    let height = (max_y - min_y) as usize;
    let mut index = vec![usize::MAX; width * height];
    let mut coords = Vec::new();
    let mut covs = Vec::new();
    let mut sources = Vec::new();
    let mut targets = Vec::new();
    let layer = s.layer_index()?;
    for y in min_y..max_y {
        for x in min_x..max_x {
            let cov = coverage(s, x, y);
            if cov <= 0.0 {
                continue;
            }
            let Some(src) = reader.pixel(source, x - dx, y - dy)? else {
                continue;
            };
            let dst = reader.pixel(CloneSource::Layer(layer), x, y)?.unwrap();
            index[(y - min_y) as usize * width + (x - min_x) as usize] = coords.len();
            coords.push((x, y));
            covs.push(cov);
            sources.push(src);
            targets.push(dst);
        }
    }
    if coords.is_empty() {
        return Ok(s.base.clone());
    }
    let mut neighbors = vec![[usize::MAX; 4]; coords.len()];
    let mut rhs = vec![[0.0f32; 4]; coords.len()];
    let mut boundary_sum = [0.0f32; 4];
    let mut boundary_count = 0f32;
    for (i, &(x, y)) in coords.iter().enumerate() {
        for (edge, (nx, ny)) in [(x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)]
            .into_iter()
            .enumerate()
        {
            let ni = if nx >= min_x && nx < max_x && ny >= min_y && ny < max_y {
                index[(ny - min_y) as usize * width + (nx - min_x) as usize]
            } else {
                usize::MAX
            };
            neighbors[i][edge] = ni;
            let src_neighbor = reader
                .pixel(source, nx - dx, ny - dy)?
                .unwrap_or(sources[i]);
            let dst_neighbor = reader
                .pixel(CloneSource::Layer(layer), nx, ny)?
                .unwrap_or(targets[i]);
            for c in 0..4 {
                rhs[i][c] += sources[i][c] as f32 - src_neighbor[c] as f32;
                if ni == usize::MAX {
                    rhs[i][c] += dst_neighbor[c] as f32;
                    boundary_sum[c] += dst_neighbor[c] as f32 - src_neighbor[c] as f32;
                }
            }
            if ni == usize::MAX {
                boundary_count += 1.0;
            }
        }
    }
    // A screened Poisson term anchors the interior to the source plus its
    // border tone shift. Pure Dirichlet Poisson erases a linear source ramp
    // against a flat target because a ramp has zero Laplacian.
    const SCREENING: f32 = 1.0;
    let mut solved = vec![[0.0f32; 4]; coords.len()];
    for c in 0..4 {
        let shift = if boundary_count > 0.0 {
            boundary_sum[c] / boundary_count
        } else {
            0.0
        };
        let initial: Vec<f32> = sources.iter().map(|p| p[c] as f32 + shift).collect();
        let b: Vec<f32> = rhs
            .iter()
            .zip(&initial)
            .map(|(p, target)| p[c] + SCREENING * target)
            .collect();
        let solution = conjugate_gradient(&neighbors, &b, initial, SCREENING);
        for (i, value) in solution.into_iter().enumerate() {
            solved[i][c] = value;
        }
    }
    let mut output = s.base.clone();
    let mut tiles: BTreeMap<(i32, i32), Vec<u8>> = BTreeMap::new();
    for (i, &(x, y)) in coords.iter().enumerate() {
        let key = (x / TILE_SIZE as i32, y / TILE_SIZE as i32);
        if let std::collections::btree_map::Entry::Vacant(entry) = tiles.entry(key) {
            entry.insert(s.base.read_tile(layer, TileCoord { x: key.0, y: key.1 })?);
        }
        let tile = tiles.get_mut(&key).unwrap();
        let k = ((y as usize % TILE_SIZE) * TILE_SIZE + x as usize % TILE_SIZE) * 4;
        let a = covs[i].clamp(0.0, 1.0);
        for c in 0..4 {
            tile[k + c] = round_unit_to_u8(
                ((solved[i][c].clamp(0.0, 255.0) * a + targets[i][c] as f32 * (1.0 - a)) / 255.0)
                    .clamp(0.0, 1.0),
            );
        }
    }
    for ((x, y), tile) in tiles {
        output = output.write_tile(
            layer,
            TileCoord { x, y },
            TileData::try_from(tile).map_err(op_core::doc::DocError::from)?,
        )?;
    }
    Ok(output)
}

// CG solves the positive-definite 4-neighbor Dirichlet Laplacian. Stop when
// ||b-Ax|| / ||b|| <= 1e-5; all sums have a fixed order for repeatable bytes.
fn conjugate_gradient(
    neighbors: &[[usize; 4]],
    b: &[f32],
    mut x: Vec<f32>,
    screening: f32,
) -> Vec<f32> {
    let n = b.len();
    let mut r = vec![0.0f32; n];
    let mut p = vec![0.0f32; n];
    let mut ap = vec![0.0f32; n];
    let mut norm_b = 0.0f64;
    let mut rr = 0.0f64;
    for i in 0..n {
        let mut ax = (4.0 + screening) * x[i];
        for &j in &neighbors[i] {
            if j != usize::MAX {
                ax -= x[j];
            }
        }
        r[i] = b[i] - ax;
        p[i] = r[i];
        rr += (r[i] as f64).powi(2);
        norm_b += (b[i] as f64).powi(2);
    }
    let tolerance = norm_b.max(1.0) * 1e-10;
    for _ in 0..n.min(2000) {
        if rr <= tolerance {
            break;
        }
        let mut pap = 0.0f64;
        for i in 0..n {
            let mut v = (4.0 + screening) * p[i];
            for &j in &neighbors[i] {
                if j != usize::MAX {
                    v -= p[j];
                }
            }
            ap[i] = v;
            pap += p[i] as f64 * v as f64;
        }
        if pap <= 0.0 {
            break;
        }
        let alpha = (rr / pap) as f32;
        let mut next_rr = 0.0f64;
        for i in 0..n {
            x[i] += alpha * p[i];
            r[i] -= alpha * ap[i];
            next_rr += (r[i] as f64).powi(2);
        }
        let beta = (next_rr / rr) as f32;
        for i in 0..n {
            p[i] = r[i] + beta * p[i];
        }
        rr = next_rr;
    }
    x
}

#[cfg(test)]
mod tests {
    use super::conjugate_gradient;
    #[test]
    fn analytic_dirichlet_residual() {
        let side = 17;
        let mut neighbors = vec![[usize::MAX; 4]; side * side];
        let mut b = vec![0.0f32; side * side];
        for y in 0..side {
            for x in 0..side {
                let i = y * side + x;
                for (edge, (nx, ny)) in [
                    (x as i32 - 1, y as i32),
                    (x as i32 + 1, y as i32),
                    (x as i32, y as i32 - 1),
                    (x as i32, y as i32 + 1),
                ]
                .into_iter()
                .enumerate()
                {
                    if nx >= 0 && nx < side as i32 && ny >= 0 && ny < side as i32 {
                        neighbors[i][edge] = ny as usize * side + nx as usize;
                    } else {
                        b[i] += 80.0;
                    }
                }
            }
        }
        let x = conjugate_gradient(&neighbors, &b, vec![0.0; b.len()], 0.0);
        let norm_b = b.iter().map(|v| (*v as f64).powi(2)).sum::<f64>().sqrt();
        let mut residual = 0.0f64;
        for (i, &v) in x.iter().enumerate() {
            let mut ax = 4.0 * v;
            for &j in &neighbors[i] {
                if j != usize::MAX {
                    ax -= x[j];
                }
            }
            residual += (b[i] as f64 - ax as f64).powi(2);
            assert!((v - 80.0).abs() < 0.01);
        }
        assert!(residual.sqrt() / norm_b <= 1e-5);

        // A source ramp has zero Laplacian. Screening must preserve it while
        // the outer neighbors stay fixed at the destination value.
        let expected: Vec<f32> = (0..side)
            .flat_map(|y| (0..side).map(move |x| 50.0 + 2.0 * x as f32 + 3.0 * y as f32))
            .collect();
        let mut screened_rhs = vec![0.0f32; expected.len()];
        for i in 0..expected.len() {
            screened_rhs[i] = 5.0 * expected[i];
            for &j in &neighbors[i] {
                if j != usize::MAX {
                    screened_rhs[i] -= expected[j];
                }
            }
        }
        let solved = conjugate_gradient(
            &neighbors,
            &screened_rhs,
            vec![0.0; screened_rhs.len()],
            1.0,
        );
        let norm = screened_rhs
            .iter()
            .map(|v| (*v as f64).powi(2))
            .sum::<f64>()
            .sqrt();
        let mut error = 0.0f64;
        for i in 0..solved.len() {
            let mut ax = 5.0 * solved[i];
            for &j in &neighbors[i] {
                if j != usize::MAX {
                    ax -= solved[j];
                }
            }
            error += (screened_rhs[i] as f64 - ax as f64).powi(2);
        }
        assert!(error.sqrt() / norm <= 1e-5);
    }
}
