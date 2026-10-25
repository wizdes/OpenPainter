//! The Edit menu's clipboard (M34): `op_paste_begin` starts Paste or Paste
//! into New Layer as a move session (then `op_move_update`, `op_move_finish`
//! and `op_move_cancel` as for any move), and `op_png_decode` reads a
//! pasteboard PNG exactly. Copy reads pixels with the `clipboard.copy` and
//! `clipboard.profile_icc` queries (`op_query`).
use crate::{
    OpBuffer, OpEngine, OpPixelBuffer, OpStatus, bytes, engine, guard, json_bytes, owned, status,
};
use op_engine::PasteRequest;
use serde_json::{Value, json};

fn optional(ptr: *const u8, len: usize) -> Result<Option<Vec<u8>>, OpStatus> {
    if len == 0 {
        return Ok(None);
    }
    bytes(ptr, len).map(|b| Some(b.to_vec()))
}

/// Starts a paste on `doc`. `json`: `{"width", "height", "x", "y",
/// "new_layer", "transform", "resampling"}`: the size of `rgba` (straight
/// RGBA8), the top-left corner in document pixels (it may lie off the canvas),
/// whether to add a layer above the active one first, and the move options
/// (see `op_move_update`). `icc` is the image's profile (empty: sRGB); pixels
/// in another profile are converted to the document's. Writes
/// `{"session", "converted_from"}` to `out` (free with op_buffer_free);
/// `converted_from` is the pasted profile's description, or null when no
/// conversion was needed. OP_ERR_BUSY while a stroke, preview, move or tool
/// session runs.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub extern "C" fn op_paste_begin(
    p: *mut OpEngine,
    doc: u64,
    json: *const u8,
    len: usize,
    rgba: *const u8,
    rgba_len: usize,
    icc: *const u8,
    icc_len: usize,
    out: *mut OpBuffer,
) -> OpStatus {
    guard(|| {
        if out.is_null() {
            return OpStatus::OP_ERR_BAD_ARGUMENT;
        }
        let e = match engine(p) {
            Ok(e) => e,
            Err(s) => return s,
        };
        let v = match json_bytes(json, len) {
            Ok(v) => v,
            Err(s) => return s,
        };
        let pixels = match bytes(rgba, rgba_len) {
            Ok(b) => b.to_vec(),
            Err(s) => return s,
        };
        let profile = match optional(icc, icc_len) {
            Ok(v) => v,
            Err(s) => return s,
        };
        let options = match crate::move_tool::options(&v) {
            Ok(o) => o,
            Err(s) => return s,
        };
        let int = |k: &str| v.get(k).and_then(Value::as_i64);
        let (Some(width), Some(height)) = (
            int("width").and_then(|n| u32::try_from(n).ok()),
            int("height").and_then(|n| u32::try_from(n).ok()),
        ) else {
            return OpStatus::OP_ERR_BAD_ARGUMENT;
        };
        let request = PasteRequest {
            width,
            height,
            rgba: pixels,
            icc: profile,
            x: int("x").unwrap_or(0),
            y: int("y").unwrap_or(0),
            new_layer: v.get("new_layer").and_then(Value::as_bool) == Some(true),
            options,
        };
        let result = e.core().paste_begin(doc, request);
        crate::view::doc_touched(p, doc);
        match result {
            Ok(started) => {
                let answer =
                    json!({"session": started.session, "converted_from": started.converted_from});
                unsafe { *out = owned(answer.to_string().into_bytes()) };
                OpStatus::OP_OK
            }
            Err(x) => status(x),
        }
    })
}

/// Decodes a PNG exactly (straight RGBA8, colors under zero alpha kept) into
/// `out_pixels` (free with op_pixels_free) and its ICC profile into `out_icc`
/// (empty when the PNG has none; free with op_buffer_free). Needs no engine.
#[unsafe(no_mangle)]
pub extern "C" fn op_png_decode(
    png: *const u8,
    len: usize,
    out_pixels: *mut OpPixelBuffer,
    out_icc: *mut OpBuffer,
) -> OpStatus {
    guard(|| {
        if out_pixels.is_null() || out_icc.is_null() {
            return OpStatus::OP_ERR_BAD_ARGUMENT;
        }
        let data = match bytes(png, len) {
            Ok(b) => b,
            Err(s) => return s,
        };
        match op_engine::decode_png(data) {
            Ok(d) => {
                let (width, height) = (d.width, d.height);
                let b = owned(d.pixels);
                unsafe {
                    *out_pixels = OpPixelBuffer {
                        ptr: b.ptr,
                        len: b.len,
                        width,
                        height,
                    };
                    *out_icc = owned(d.icc.unwrap_or_default());
                }
                OpStatus::OP_OK
            }
            Err(x) => status(x),
        }
    })
}
