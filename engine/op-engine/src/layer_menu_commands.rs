//! Remaining Layers menu commands: active-layer transforms and file import.
use super::{
    CommandSpec, Outcome, Param, ParamKind, ParamSchema, Result, RunCtx, bad, doc_error,
    image_commands, internal,
};
use op_color::{Profile, RenderingIntent};
use op_core::doc::DocState;
use op_core::tiles::{TILE_SIZE, TileCoord, TileData};
use serde_json::Value;
use std::path::Path;

fn always(_: &DocState) -> bool {
    true
}

fn transform(ctx: &RunCtx, s: &DocState, kind: u8) -> Result<Outcome> {
    ctx.check()?;
    let index = s.active_layer;
    let src = image_commands::read(s, index)?;
    let mut pixels = vec![0; src.len()];
    for y in 0..s.height {
        ctx.check()?;
        for x in 0..s.width {
            let (sx, sy) = image_commands::transform_source(s.width, s.height, kind, x, y);
            let from = (sy as usize * s.width as usize + sx as usize) * 4;
            let to = (y as usize * s.width as usize + x as usize) * 4;
            pixels[to..to + 4].copy_from_slice(&src[from..from + 4]);
        }
    }
    // Keep imported pixels that lie outside the canvas, including pixels in
    // the unused part of a tile that crosses the canvas edge.
    let mut next = s.clone();
    for (coord, _) in s.layers[index].tiles.iter() {
        if coord.x >= 0 && coord.y >= 0 {
            let width = (u64::from(coord.x as u32) + 1) * TILE_SIZE as u64;
            let height = (u64::from(coord.y as u32) + 1) * TILE_SIZE as u64;
            next.width = next.width.max(u32::try_from(width).map_err(bad)?);
            next.height = next.height.max(u32::try_from(height).map_err(bad)?);
        }
    }
    for ty in 0..s.height.div_ceil(TILE_SIZE as u32) {
        for tx in 0..s.width.div_ceil(TILE_SIZE as u32) {
            ctx.check()?;
            let coord = TileCoord {
                x: tx as i32,
                y: ty as i32,
            };
            let mut tile = s.read_tile(index, coord).map_err(internal)?;
            let tile_width = (s.width - tx * TILE_SIZE as u32).min(TILE_SIZE as u32) as usize;
            let tile_height = (s.height - ty * TILE_SIZE as u32).min(TILE_SIZE as u32) as usize;
            for y in 0..tile_height {
                let from = ((ty as usize * TILE_SIZE + y) * s.width as usize
                    + tx as usize * TILE_SIZE)
                    * 4;
                let to = y * TILE_SIZE * 4;
                tile[to..to + tile_width * 4].copy_from_slice(&pixels[from..from + tile_width * 4]);
            }
            next = next
                .write_tile(index, coord, TileData::try_from(tile).map_err(internal)?)
                .map_err(internal)?;
        }
    }
    next.width = s.width;
    next.height = s.height;
    Ok(Outcome::State(next))
}

fn flip_h(ctx: &RunCtx, s: &DocState, _: &Value) -> Result<Outcome> {
    transform(ctx, s, 0)
}
fn flip_v(ctx: &RunCtx, s: &DocState, _: &Value) -> Result<Outcome> {
    transform(ctx, s, 1)
}
fn rotate_180(ctx: &RunCtx, s: &DocState, _: &Value) -> Result<Outcome> {
    transform(ctx, s, 4)
}

fn import_from_file(ctx: &RunCtx, s: &DocState, params: &Value) -> Result<Outcome> {
    let path = Path::new(params["path"].as_str().unwrap());
    let bytes =
        std::fs::read(path).map_err(|e| bad(format!("cannot read {}: {e}", path.display())))?;
    ctx.check()?;
    let mut image = op_formats::decode(&bytes)
        .map_err(|e| bad(format!("cannot decode {}: {e}", path.display())))?;
    let src = match image.icc.as_deref() {
        Some(icc) => {
            Profile::from_icc(icc).map_err(|e| bad(format!("invalid embedded profile: {e}")))?
        }
        None => Profile::builtin("srgb").map_err(internal)?,
    };
    let dst = match s.profile.as_deref() {
        Some(icc) => Profile::from_icc(icc).map_err(internal)?,
        None => Profile::builtin("srgb").map_err(internal)?,
    };
    if src.hash() != dst.hash() {
        op_color::convert_rgba(
            &src,
            &dst,
            RenderingIntent::Perceptual,
            true,
            &mut image.rgba8,
        )
        .map_err(|e| bad(format!("cannot convert image profile: {e}")))?;
    }
    ctx.check()?;
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .filter(|n| !n.is_empty())
        .ok_or_else(|| bad("path has no file name"))?;
    let index = s.active_layer + 1;
    let next = s
        .add_layer(index, name)
        .map_err(doc_error)?
        .set_active_layer(index)
        .map_err(internal)?;
    // Write against the image dimensions. The layer retains complete edge and
    // out-of-canvas tiles; the document dimensions still clip rendering.
    let mut expanded = next;
    expanded.width = image.width;
    expanded.height = image.height;
    expanded = image_commands::write(expanded, index, &image.rgba8)?;
    expanded.width = s.width;
    expanded.height = s.height;
    ctx.check()?;
    Ok(Outcome::State(expanded))
}

pub(super) fn specs() -> Vec<CommandSpec> {
    [
        (
            "layer.flip_horizontal",
            flip_h as fn(&RunCtx, &DocState, &Value) -> Result<Outcome>,
        ),
        ("layer.flip_vertical", flip_v),
        ("layer.rotate_180", rotate_180),
    ]
    .into_iter()
    .map(|(id, run)| CommandSpec {
        id,
        params: ParamSchema::default(),
        enabled: always,
        run,
        previewable: false,
    })
    .chain(std::iter::once(CommandSpec {
        id: "layer.import_from_file",
        params: ParamSchema {
            fields: vec![Param {
                key: "path",
                kind: ParamKind::String,
                min: None,
                max: None,
                default: None,
            }],
        },
        enabled: |s| s.can_add_layer(),
        run: import_from_file,
        previewable: false,
    }))
    .collect()
}

pub(super) fn history_label(id: &str) -> Option<&'static str> {
    match id {
        "layer.flip_horizontal" => Some("Flip Layer Horizontal"),
        "layer.flip_vertical" => Some("Flip Layer Vertical"),
        "layer.rotate_180" => Some("Rotate Layer 180°"),
        "layer.import_from_file" => Some("Import From File"),
        _ => None,
    }
}
