use super::{
    CommandSpec, Outcome, Param, ParamKind, ParamSchema, Result, RunCtx, always, bad, internal,
};
use op_core::doc::{DocState, LayerBlendMode, MaskGrid, SelectionSource, SelectionState};
use op_core::selection::{RasterMode, Selection};
use op_core::tiles::{TILE_BYTES, TILE_SIZE, TileCoord, TileData};
use op_ops::image::{self, Resampling};
use serde_json::{Value, json};
use std::sync::Arc;

fn field(key: &'static str, kind: ParamKind, default: Option<Value>) -> Param {
    Param {
        key,
        kind,
        min: None,
        max: None,
        default,
    }
}
fn spec(
    id: &'static str,
    fields: Vec<Param>,
    run: fn(&RunCtx, &DocState, &Value) -> Result<Outcome>,
) -> CommandSpec {
    CommandSpec {
        id,
        params: ParamSchema { fields },
        enabled: always,
        run,
        previewable: false,
    }
}
pub(super) fn specs() -> Vec<CommandSpec> {
    let empty = || Vec::new();
    vec![
        spec(
            "image.resize",
            vec![
                field("width", ParamKind::Int, Some(json!(0))),
                field("height", ParamKind::Int, Some(json!(0))),
                field("percent", ParamKind::Float, Some(json!(0.0))),
                field(
                    "resampling",
                    ParamKind::Choice(&[
                        "nearest",
                        "nearest_neighbor",
                        "bilinear",
                        "bilinear_low_quality",
                        "bicubic",
                        "bicubic_smooth",
                        "super_sampling",
                        "fant",
                        "lanczos",
                        "adaptive_sharp",
                        "best_quality",
                    ]),
                    Some(json!("bicubic")),
                ),
                field("maintain_aspect", ParamKind::Bool, Some(json!(true))),
                field("gamma", ParamKind::Bool, Some(json!(true))),
            ],
            resize,
        ),
        spec(
            "image.canvas_size",
            vec![
                field("width", ParamKind::Int, None),
                field("height", ParamKind::Int, None),
                field(
                    "anchor",
                    ParamKind::Choice(&[
                        "top_left",
                        "top",
                        "top_right",
                        "left",
                        "center",
                        "right",
                        "bottom_left",
                        "bottom",
                        "bottom_right",
                    ]),
                    Some(json!("center")),
                ),
                field(
                    "fill",
                    ParamKind::Choice(&["transparent", "primary", "secondary", "white", "black"]),
                    Some(json!("transparent")),
                ),
                field("primary", ParamKind::String, Some(json!("000000ff"))),
                field("secondary", ParamKind::String, Some(json!("ffffffff"))),
            ],
            canvas,
        ),
        spec("image.flip_horizontal", empty(), flip_h),
        spec("image.flip_vertical", empty(), flip_v),
        spec("image.rotate_90_cw", empty(), cw),
        spec("image.rotate_90_ccw", empty(), ccw),
        spec("image.rotate_180", empty(), rot180),
        spec("image.flatten", empty(), flatten),
        spec("image.auto_crop", empty(), auto_crop),
        spec("image.crop_to_selection", empty(), crop_selection),
    ]
}
fn dims(p: &Value) -> Result<(u32, u32)> {
    let w = u32::try_from(p["width"].as_i64().unwrap()).map_err(bad)?;
    let h = u32::try_from(p["height"].as_i64().unwrap()).map_err(bad)?;
    if w == 0 || h == 0 || w as u64 * h as u64 > 100_000_000 {
        return Err(bad("invalid image size"));
    }
    Ok((w, h))
}
fn read(s: &DocState, layer: usize) -> Result<Vec<u8>> {
    let mut out = vec![0; s.width as usize * s.height as usize * 4];
    for ty in 0..s.height.div_ceil(TILE_SIZE as u32) {
        for tx in 0..s.width.div_ceil(TILE_SIZE as u32) {
            let tile = s
                .read_tile(
                    layer,
                    TileCoord {
                        x: tx as i32,
                        y: ty as i32,
                    },
                )
                .map_err(internal)?;
            for y in 0..(s.height - ty * TILE_SIZE as u32).min(TILE_SIZE as u32) as usize {
                let n = (s.width - tx * TILE_SIZE as u32).min(TILE_SIZE as u32) as usize * 4;
                let dst = ((ty as usize * TILE_SIZE + y) * s.width as usize
                    + tx as usize * TILE_SIZE)
                    * 4;
                out[dst..dst + n].copy_from_slice(&tile[y * TILE_SIZE * 4..y * TILE_SIZE * 4 + n]);
            }
        }
    }
    Ok(out)
}
fn write(mut next: DocState, layer: usize, pixels: &[u8]) -> Result<DocState> {
    let w = next.width as usize;
    let h = next.height as usize;
    for ty in 0..h.div_ceil(TILE_SIZE) {
        for tx in 0..w.div_ceil(TILE_SIZE) {
            let mut tile = vec![0; TILE_BYTES];
            for y in 0..(h - ty * TILE_SIZE).min(TILE_SIZE) {
                let n = (w - tx * TILE_SIZE).min(TILE_SIZE) * 4;
                let src = ((ty * TILE_SIZE + y) * w + tx * TILE_SIZE) * 4;
                tile[y * TILE_SIZE * 4..y * TILE_SIZE * 4 + n]
                    .copy_from_slice(&pixels[src..src + n]);
            }
            if tile.iter().any(|&x| x != 0) {
                next = next
                    .write_tile(
                        layer,
                        TileCoord {
                            x: tx as i32,
                            y: ty as i32,
                        },
                        TileData::try_from(tile).map_err(internal)?,
                    )
                    .map_err(internal)?;
            }
        }
    }
    Ok(next)
}
fn selection_map(
    s: &DocState,
    dw: u32,
    dh: u32,
    mut src: impl FnMut(u32, u32) -> Option<(u32, u32)>,
    affine: [f64; 6],
) -> SelectionState {
    if matches!(s.selection, SelectionState::None) {
        return SelectionState::None;
    }
    if matches!(
        s.selection,
        SelectionState::Active(SelectionSource::Geometry(_))
    ) {
        return Selection::new(s.selection.clone())
            .affine(affine, dw, dh)
            .state()
            .clone();
    }
    let mask = s.rasterize_selection(RasterMode::Antialiased);
    let mut out = vec![0; dw as usize * dh as usize];
    for y in 0..dh {
        for x in 0..dw {
            if let Some((sx, sy)) = src(x, y)
                && sx < s.width
                && sy < s.height
            {
                out[(y * dw + x) as usize] = mask.bytes[(sy * s.width + sx) as usize];
            }
        }
    }
    SelectionState::Active(SelectionSource::Coverage(Arc::new(
        MaskGrid::new(dw, dh, out).unwrap(),
    )))
}
fn finish(
    s: &DocState,
    w: u32,
    h: u32,
    ctx: &RunCtx,
    mut pixel: impl FnMut(usize, &[u8]) -> Result<Vec<u8>>,
    mut source: impl FnMut(u32, u32) -> Option<(u32, u32)>,
    affine: [f64; 6],
) -> Result<Outcome> {
    let mut next = s.empty_image_like(w, h).map_err(internal)?;
    for i in 0..s.layers.len() {
        ctx.check()?;
        next = write(next, i, &pixel(i, &read(s, i)?)?)?;
        ctx.progress(i + 1, s.layers.len());
    }
    let sel = selection_map(s, w, h, &mut source, affine);
    ctx.check()?;
    Ok(Outcome::State(next.set_selection(sel)))
}
fn mode(p: &Value) -> Resampling {
    match p["resampling"].as_str().unwrap() {
        "nearest" | "nearest_neighbor" => Resampling::Nearest,
        "bilinear" => Resampling::Bilinear,
        "bilinear_low_quality" => Resampling::BilinearLowQuality,
        "bicubic_smooth" => Resampling::BicubicSmooth,
        "super_sampling" => Resampling::SuperSampling,
        "fant" => Resampling::Fant,
        "lanczos" => Resampling::Lanczos,
        "adaptive_sharp" => Resampling::AdaptiveSharp,
        "best_quality" => Resampling::BestQuality,
        _ => Resampling::Bicubic,
    }
}
fn resize(ctx: &RunCtx, s: &DocState, p: &Value) -> Result<Outcome> {
    let percent = p["percent"].as_f64().unwrap();
    let mut w = p["width"].as_i64().unwrap();
    let mut h = p["height"].as_i64().unwrap();
    if percent > 0.0 {
        if w != 0 || h != 0 {
            return Err(bad("use percent or dimensions"));
        }
        w = (s.width as f64 * percent / 100.0).round() as i64;
        h = (s.height as f64 * percent / 100.0).round() as i64;
    } else if p["maintain_aspect"].as_bool().unwrap() {
        if w > 0 {
            h = (w as f64 * s.height as f64 / s.width as f64).round() as i64;
        } else if h > 0 && w == 0 {
            w = (h as f64 * s.width as f64 / s.height as f64).round() as i64;
        }
    }
    let (w, h) = dims(&json!({"width":w,"height":h}))?;
    let m = mode(p);
    let gamma = p["gamma"].as_bool().unwrap();
    finish(
        s,
        w,
        h,
        ctx,
        |_, src| {
            Ok(if gamma {
                image::resize(src, s.width, s.height, w, h, m, true)
            } else {
                image::legacy::resize(src, s.width, s.height, w, h, m)
            })
        },
        |x, y| {
            Some((
                ((x as f64 + 0.5) * s.width as f64 / w as f64)
                    .floor()
                    .min(s.width as f64 - 1.0) as u32,
                ((y as f64 + 0.5) * s.height as f64 / h as f64)
                    .floor()
                    .min(s.height as f64 - 1.0) as u32,
            ))
        },
        [
            w as f64 / s.width as f64,
            0.0,
            0.0,
            h as f64 / s.height as f64,
            0.0,
            0.0,
        ],
    )
}
fn color(s: &str) -> Result<[u8; 4]> {
    if s.len() != 8 {
        return Err(bad("color needs eight RGBA hex digits"));
    }
    let mut out = [0; 4];
    for i in 0..4 {
        out[i] = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).map_err(bad)?;
    }
    Ok(out)
}
fn canvas(ctx: &RunCtx, s: &DocState, p: &Value) -> Result<Outcome> {
    let (w, h) = dims(p)?;
    let anchor = p["anchor"].as_str().unwrap();
    let ax = if anchor.ends_with("left") || anchor == "left" {
        0
    } else if anchor.ends_with("right") || anchor == "right" {
        2
    } else {
        1
    };
    let ay = if anchor.starts_with("top") {
        0
    } else if anchor.starts_with("bottom") {
        2
    } else {
        1
    };
    let dx = ((w as i64 - s.width as i64) * ax / 2) as i32;
    let dy = ((h as i64 - s.height as i64) * ay / 2) as i32;
    let fill = match p["fill"].as_str().unwrap() {
        "white" => [255; 4],
        "black" => [0, 0, 0, 255],
        "primary" => color(p["primary"].as_str().unwrap())?,
        "secondary" => color(p["secondary"].as_str().unwrap())?,
        _ => [0; 4],
    };
    finish(
        s,
        w,
        h,
        ctx,
        |i, src| {
            let mut out = vec![0; w as usize * h as usize * 4];
            for y in 0..h {
                for x in 0..w {
                    let d = (y * w + x) as usize * 4;
                    let sx = x as i64 - dx as i64;
                    let sy = y as i64 - dy as i64;
                    if sx >= 0 && sy >= 0 && sx < s.width as i64 && sy < s.height as i64 {
                        let a = (sy as usize * s.width as usize + sx as usize) * 4;
                        out[d..d + 4].copy_from_slice(&src[a..a + 4]);
                    } else if i == 0 {
                        out[d..d + 4].copy_from_slice(&fill);
                    }
                }
            }
            Ok(out)
        },
        |x, y| {
            let sx = x as i64 - dx as i64;
            let sy = y as i64 - dy as i64;
            if sx >= 0 && sy >= 0 {
                Some((sx as u32, sy as u32))
            } else {
                None
            }
        },
        [1.0, 0.0, 0.0, 1.0, dx as f64, dy as f64],
    )
}
fn transform(ctx: &RunCtx, s: &DocState, kind: u8) -> Result<Outcome> {
    let (w, h) = if kind == 2 || kind == 3 {
        (s.height, s.width)
    } else {
        (s.width, s.height)
    };
    let map = move |x: u32, y: u32| -> (u32, u32) {
        match kind {
            0 => (s.width - 1 - x, y),
            1 => (x, s.height - 1 - y),
            2 => (y, s.height - 1 - x),
            3 => (s.width - 1 - y, x),
            _ => (s.width - 1 - x, s.height - 1 - y),
        }
    };
    let affine = match kind {
        0 => [-1.0, 0.0, 0.0, 1.0, s.width as f64, 0.0],
        1 => [1.0, 0.0, 0.0, -1.0, 0.0, s.height as f64],
        2 => [0.0, 1.0, -1.0, 0.0, s.height as f64, 0.0],
        3 => [0.0, -1.0, 1.0, 0.0, 0.0, s.width as f64],
        _ => [-1.0, 0.0, 0.0, -1.0, s.width as f64, s.height as f64],
    };
    finish(
        s,
        w,
        h,
        ctx,
        |_, src| {
            let mut out = vec![0; w as usize * h as usize * 4];
            for y in 0..h {
                for x in 0..w {
                    let (sx, sy) = map(x, y);
                    let a = (sy * s.width + sx) as usize * 4;
                    let d = (y * w + x) as usize * 4;
                    out[d..d + 4].copy_from_slice(&src[a..a + 4]);
                }
            }
            Ok(out)
        },
        |x, y| Some(map(x, y)),
        affine,
    )
}
fn flip_h(c: &RunCtx, s: &DocState, _: &Value) -> Result<Outcome> {
    transform(c, s, 0)
}
fn flip_v(c: &RunCtx, s: &DocState, _: &Value) -> Result<Outcome> {
    transform(c, s, 1)
}
fn cw(c: &RunCtx, s: &DocState, _: &Value) -> Result<Outcome> {
    transform(c, s, 2)
}
fn ccw(c: &RunCtx, s: &DocState, _: &Value) -> Result<Outcome> {
    transform(c, s, 3)
}
fn rot180(c: &RunCtx, s: &DocState, _: &Value) -> Result<Outcome> {
    transform(c, s, 4)
}
fn flatten(ctx: &RunCtx, s: &DocState, _: &Value) -> Result<Outcome> {
    ctx.check()?;
    let mut next = s.empty_image_like(s.width, s.height).map_err(internal)?;
    while next.layers.len() > 1 {
        next = next.delete_layer(1).map_err(internal)?;
    }
    next = next
        .set_layer_blend(0, LayerBlendMode::Normal)
        .map_err(internal)?
        .set_layer_opacity(0, 255)
        .map_err(internal)?
        .set_layer_visible(0, true)
        .map_err(internal)?;
    for ty in 0..s.height.div_ceil(TILE_SIZE as u32) {
        for tx in 0..s.width.div_ceil(TILE_SIZE as u32) {
            ctx.check()?;
            let coord = TileCoord {
                x: tx as i32,
                y: ty as i32,
            };
            let tile = op_ops::composite::composite_tile(s, coord).map_err(internal)?;
            next = next.write_tile(0, coord, tile).map_err(internal)?;
        }
    }
    Ok(Outcome::State(next))
}
fn crop(
    ctx: &RunCtx,
    s: &DocState,
    bounds: (u32, u32, u32, u32),
    mask: Option<&[u8]>,
) -> Result<Outcome> {
    let (x0, y0, w, h) = bounds;
    finish(
        s,
        w,
        h,
        ctx,
        |_, src| {
            let mut out = vec![0; w as usize * h as usize * 4];
            for y in 0..h {
                for x in 0..w {
                    let sx = x + x0;
                    let sy = y + y0;
                    let d = (y * w + x) as usize * 4;
                    let a = (sy * s.width + sx) as usize * 4;
                    if mask.is_none_or(|m| m[(sy * s.width + sx) as usize] > 0) {
                        out[d..d + 4].copy_from_slice(&src[a..a + 4]);
                    }
                }
            }
            Ok(out)
        },
        |x, y| Some((x + x0, y + y0)),
        [1.0, 0.0, 0.0, 1.0, -(x0 as f64), -(y0 as f64)],
    )
}
fn bounds(mask: &[u8], w: u32, h: u32) -> Option<(u32, u32, u32, u32)> {
    let (mut minx, mut miny, mut maxx, mut maxy) = (w, h, 0, 0);
    let mut found = false;
    for y in 0..h {
        for x in 0..w {
            if mask[(y * w + x) as usize] > 0 {
                found = true;
                minx = minx.min(x);
                miny = miny.min(y);
                maxx = maxx.max(x);
                maxy = maxy.max(y);
            }
        }
    }
    found.then_some((minx, miny, maxx - minx + 1, maxy - miny + 1))
}
fn crop_selection(ctx: &RunCtx, s: &DocState, _: &Value) -> Result<Outcome> {
    if matches!(s.selection, SelectionState::None) {
        return Err(bad("no selection"));
    }
    let mask = s.rasterize_selection(RasterMode::Pixelated);
    let b = bounds(&mask.bytes, s.width, s.height).ok_or_else(|| bad("empty selection"))?;
    crop(ctx, s, b, Some(&mask.bytes))
}
fn auto_crop(ctx: &RunCtx, s: &DocState, _: &Value) -> Result<Outcome> {
    let mut mask = vec![0; s.width as usize * s.height as usize];
    for ty in 0..s.height.div_ceil(TILE_SIZE as u32) {
        for tx in 0..s.width.div_ceil(TILE_SIZE as u32) {
            ctx.check()?;
            let tile = op_ops::composite::composite_tile(
                s,
                TileCoord {
                    x: tx as i32,
                    y: ty as i32,
                },
            )
            .map_err(internal)?;
            for y in 0..(s.height - ty * TILE_SIZE as u32).min(TILE_SIZE as u32) {
                for x in 0..(s.width - tx * TILE_SIZE as u32).min(TILE_SIZE as u32) {
                    if tile.as_bytes()[((y as usize * TILE_SIZE + x as usize) * 4) + 3] > 0 {
                        mask[((ty * TILE_SIZE as u32 + y) * s.width + tx * TILE_SIZE as u32 + x)
                            as usize] = 255;
                    }
                }
            }
        }
    }
    let b = bounds(&mask, s.width, s.height).ok_or_else(|| bad("image is fully transparent"))?;
    crop(ctx, s, b, None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::JobTicket;
    use op_core::tiles::TileStore;

    #[test]
    fn flatten_uses_layer_composite_and_keeps_one_layer() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(TileStore::new(dir.path(), 64 * 1024 * 1024).unwrap());
        let mut state = DocState::new(store, 1, 1, 96.0).unwrap();
        let mut bottom = vec![0; TILE_BYTES];
        bottom[..4].copy_from_slice(&[255, 0, 0, 255]);
        state = state
            .write_tile(
                0,
                TileCoord { x: 0, y: 0 },
                TileData::try_from(bottom).unwrap(),
            )
            .unwrap();
        state = state.add_layer(1, "Top").unwrap();
        let mut top = vec![0; TILE_BYTES];
        top[..4].copy_from_slice(&[0, 0, 255, 128]);
        state = state
            .write_tile(
                1,
                TileCoord { x: 0, y: 0 },
                TileData::try_from(top).unwrap(),
            )
            .unwrap();
        let expected = op_ops::composite::composite_tile(&state, TileCoord { x: 0, y: 0 }).unwrap();
        let (events, _) = crossbeam_channel::unbounded();
        let ctx = RunCtx {
            ticket: JobTicket::new(0),
            doc: 1,
            events,
            gpu: crate::gpu_effects::GpuShared::new(crate::EngineOptions {
                force_cpu: true,
                ..Default::default()
            }),
        };
        let Outcome::State(flat) = flatten(&ctx, &state, &json!({})).unwrap() else {
            panic!("flatten returned no state")
        };
        assert_eq!(flat.layers.len(), 1);
        assert_eq!(
            &flat.read_tile(0, TileCoord { x: 0, y: 0 }).unwrap()[..4],
            &expected.as_bytes()[..4]
        );
    }
}
