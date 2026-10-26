//! The Edit menu's clipboard (M34). Copy, Copy Merged and Cut read pixels with
//! the `clipboard.copy` query; Paste and Paste into New Layer start a move
//! session with the pasted pixels floating over the active layer.
//!
//! paint.net (https://www.getpaint.net/doc/latest/EditMenu.html): Copy takes
//! the selected region of the active layer (the whole layer with no
//! selection), Copy Merged the same region of all layers, and Cut copies and
//! then erases. Paste puts the pixels on the active layer, selects them and
//! turns on Move Selected Pixels. Paste into New Layer first adds a layer
//! above the active one and makes it active.
use crate::{Actor, Error, EventKind, Result, bad, doc_error, flatten, internal};
use op_core::doc::{DocState, PolygonSet, SelectionSource, SelectionState};
use op_core::selection::RasterMode;
use op_core::tiles::{TILE_SIZE, TileCoord};
use op_tools::r#move::{FloatImage, MoveOptions, MoveSession};
use serde_json::json;
use std::sync::Arc;
use std::sync::atomic::Ordering;

/// What `Engine::paste_begin` pastes.
#[derive(Clone, Debug)]
pub struct PasteRequest {
    pub width: u32,
    pub height: u32,
    /// Straight RGBA8, row by row.
    pub rgba: Vec<u8>,
    /// The pasted image's ICC profile; None or empty means sRGB.
    pub icc: Option<Vec<u8>>,
    /// The top-left corner in document pixels (it may lie off the canvas).
    pub x: i64,
    pub y: i64,
    /// Paste into New Layer: add a layer above the active one first.
    pub new_layer: bool,
    pub options: MoveOptions,
}

/// The answer to `Engine::paste_begin`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PasteStarted {
    pub session: u64,
    /// The pasted profile's description when the pixels were converted to the
    /// document's profile; None when the profiles match.
    pub converted_from: Option<String>,
}

/// History labels and command ids of the two paste steps.
pub const PASTE: &str = "Paste";
pub const PASTE_NEW_LAYER: &str = "Paste into New Layer";

/// The command id of a paste step, for the History panel's icons.
pub fn paste_command_id(label: &str) -> &'static str {
    if label == PASTE_NEW_LAYER {
        "edit.paste_into_new_layer"
    } else {
        "edit.paste"
    }
}

fn layer_rgba(s: &DocState, layer: usize) -> Result<Vec<u8>> {
    let (w, h) = (s.width as usize, s.height as usize);
    let mut out = vec![0; w * h * 4];
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
            for y in 0..(h - ty as usize * TILE_SIZE).min(TILE_SIZE) {
                let n = (w - tx as usize * TILE_SIZE).min(TILE_SIZE) * 4;
                let dst = ((ty as usize * TILE_SIZE + y) * w + tx as usize * TILE_SIZE) * 4;
                out[dst..dst + n].copy_from_slice(&tile[y * TILE_SIZE * 4..y * TILE_SIZE * 4 + n]);
            }
        }
    }
    Ok(out)
}

/// `clipboard.copy`: the active layer's pixels (all layers flattened with
/// `merged`) under the selection, the whole canvas with no selection. Partial
/// coverage scales alpha (a * c / 255, rounded). The answer is cropped to the
/// covered bounds: 16 bytes of x, y, width and height (u32, little endian),
/// then straight RGBA8 row by row. A selection that covers no pixel is an error.
pub(crate) fn copy_pixels(s: &DocState, merged: bool) -> Result<Vec<u8>> {
    let (w, h) = (s.width as usize, s.height as usize);
    let src = if merged {
        flatten(s)?
    } else {
        layer_rgba(s, s.active_layer)?
    };
    let coverage = match s.selection {
        SelectionState::None => None,
        _ => Some(s.rasterize_selection(RasterMode::Antialiased)),
    };
    let cov = |i: usize| coverage.as_ref().map_or(255, |m| m.bytes[i]);
    let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0, 0);
    for y in 0..h {
        for x in 0..w {
            if cov(y * w + x) > 0 {
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x + 1);
                y1 = y1.max(y + 1);
            }
        }
    }
    if x1 <= x0 || y1 <= y0 {
        return Err(bad("the selection covers no pixels"));
    }
    let (cw, ch) = (x1 - x0, y1 - y0);
    let mut out = Vec::with_capacity(16 + cw * ch * 4);
    for v in [x0, y0, cw, ch] {
        out.extend_from_slice(&(v as u32).to_le_bytes());
    }
    for y in y0..y1 {
        for x in x0..x1 {
            let i = y * w + x;
            let p = &src[i * 4..i * 4 + 4];
            let c = cov(i) as u32;
            let a = ((p[3] as u32 * c + 127) / 255) as u8;
            if a == 0 {
                out.extend_from_slice(&[0; 4]);
            } else {
                out.extend_from_slice(&[p[0], p[1], p[2], a]);
            }
        }
    }
    Ok(out)
}

/// `clipboard.profile_icc`: the document's ICC bytes, or the engine's sRGB
/// profile when the document has none (it is treated as sRGB).
pub(crate) fn profile_icc(s: &DocState) -> Result<Vec<u8>> {
    match &s.profile {
        Some(bytes) => Ok(bytes.to_vec()),
        None => Ok(op_color::Profile::builtin("srgb")
            .map_err(internal)?
            .bytes()
            .to_vec()),
    }
}

/// True when converting from `src` to `dst` changes no color of a 16-level
/// grid (the same profile under other bytes, such as two sRGB files).
fn same_colors(src: &op_color::Profile, dst: &op_color::Profile) -> Result<bool> {
    let levels: Vec<u8> = (0..16).map(|i| (i * 17) as u8).collect();
    let mut probe = Vec::with_capacity(16 * 16 * 16 * 4);
    for &r in &levels {
        for &g in &levels {
            for &b in &levels {
                probe.extend_from_slice(&[r, g, b, 255]);
            }
        }
    }
    let before = probe.clone();
    op_color::convert_rgba(
        src,
        dst,
        op_color::RenderingIntent::Perceptual,
        false,
        &mut probe,
    )
    .map_err(bad)?;
    Ok(probe == before)
}

/// Converts pasted pixels to the document's profile (perceptual intent, the
/// Color Profile dialog's default). Returns the pasted profile's description
/// when the pixels changed profile.
pub(crate) fn convert_to_document(
    doc: &DocState,
    icc: Option<&[u8]>,
    rgba: &mut [u8],
) -> Result<Option<String>> {
    let src = match icc {
        Some(b) if !b.is_empty() => op_color::Profile::from_icc(b).map_err(bad)?,
        _ => op_color::Profile::builtin("srgb").map_err(internal)?,
    };
    if src.color_space() != lcms2::ColorSpaceSignature::RgbData {
        return Err(bad("the pasted image's profile is not RGB"));
    }
    let dst = match &doc.profile {
        Some(b) => op_color::Profile::from_icc(b).map_err(bad)?,
        None => op_color::Profile::builtin("srgb").map_err(internal)?,
    };
    if src.hash() == dst.hash() || same_colors(&src, &dst)? {
        return Ok(None);
    }
    op_color::convert_rgba(
        &src,
        &dst,
        op_color::RenderingIntent::Perceptual,
        false,
        rgba,
    )
    .map_err(bad)?;
    Ok(Some(src.description().to_owned()))
}

impl Actor {
    /// Starts a paste: a move session whose floating pixels are `req`'s, with
    /// the selection set to the pasted bounds. Only `move_finish` writes
    /// history (one step, the new layer included); `move_cancel` leaves the
    /// document and its history as they were.
    pub(crate) fn paste_begin(&mut self, mut req: PasteRequest) -> Result<PasteStarted> {
        if self.read_only.load(Ordering::Acquire) {
            return Err(Error::ReadOnly);
        }
        if self.preview.is_some()
            || self.stroke.is_some()
            || self.move_session.is_some()
            || self.gradient_session.is_some()
            || self.line_session.is_some()
            || self.shape_session.is_some()
            || self.text_session.is_some()
        {
            return Err(Error::Busy);
        }
        let len = (req.width as usize)
            .checked_mul(req.height as usize)
            .and_then(|n| n.checked_mul(4))
            .ok_or_else(|| bad("dimensions overflow"))?;
        if req.width == 0 || req.height == 0 || req.rgba.len() != len || len > 1024 * 1024 * 1024 {
            return Err(bad("invalid pixel dimensions or length"));
        }
        let current = self.history.current_state().clone();
        let converted_from = convert_to_document(&current, req.icc.as_deref(), &mut req.rgba)?;
        let mut base = current;
        let label = if req.new_layer {
            let index = base.active_layer + 1;
            let name = format!("Layer {}", base.layers.len() + 1);
            base = base.add_layer(index, name).map_err(doc_error)?;
            base = base.set_active_layer(index).map_err(internal)?;
            PASTE_NEW_LAYER
        } else {
            PASTE
        };
        let rect = PolygonSet::rectangle(
            req.x as f64,
            req.y as f64,
            req.width as f64,
            req.height as f64,
        );
        base = base.set_selection(SelectionState::Active(SelectionSource::Geometry(Arc::new(
            rect,
        ))));
        let float = FloatImage {
            x: req.x,
            y: req.y,
            width: req.width,
            height: req.height,
            rgba: req.rgba,
        };
        let session = MoveSession::paste(base, float, req.options)
            .map_err(bad)?
            .with_label(label);
        let state = session.preview().map_err(bad)?;
        let id = self.next_session.fetch_add(1, Ordering::Relaxed);
        self.move_session = Some((id, session));
        self.publish(Some(state));
        Ok(PasteStarted {
            session: id,
            converted_from,
        })
    }

    /// After a paste step commits: the Layers panel may have a new layer.
    pub(crate) fn paste_finished(&self) {
        self.event(EventKind::LayersChanged, 0, json!({}));
    }
}

/// Decodes a PNG from the pasteboard exactly: straight RGBA8 (colors under
/// zero alpha kept) and its ICC profile.
pub fn decode_png(bytes: &[u8]) -> Result<op_formats::png::DecodedImage> {
    op_formats::png::decode_png(bytes).map_err(bad)
}
