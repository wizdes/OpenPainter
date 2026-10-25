//! Move Selected Pixels and Move Selection (M33 UI over the M32 engine):
//! `op_move_begin`, `op_move_update`, `op_move_finish` and `op_move_cancel`.
//! One session per tool use: begin lifts the floating pixels (or the outline),
//! each update replaces the whole transform and publishes a new preview, finish
//! writes one history step ("Move Selected Pixels" or "Move Selection") and
//! cancel leaves the document and its history as they were. Any engine command
//! sent while a move runs finishes the move first (op-engine `command`).
use crate::{OpEngine, OpStatus, engine, guard, json_bytes, status};
use op_ops::image::Resampling;
use op_tools::r#move::{Affine, MoveKind, MoveOptions};
use serde_json::Value;

/// The resampling names the move accepts: the engine's five move modes
/// (`nearest`, `bilinear`, `bicubic`, `super_sampling`, `best_quality`) and the
/// other names `image.resize` takes.
pub(crate) fn resampling(name: &str) -> Option<Resampling> {
    Some(match name {
        "nearest" | "nearest_neighbor" => Resampling::Nearest,
        "bilinear" => Resampling::Bilinear,
        "bilinear_low_quality" => Resampling::BilinearLowQuality,
        "bicubic" => Resampling::Bicubic,
        "bicubic_smooth" => Resampling::BicubicSmooth,
        "super_sampling" => Resampling::SuperSampling,
        "fant" => Resampling::Fant,
        "lanczos" => Resampling::Lanczos,
        "adaptive_sharp" => Resampling::AdaptiveSharp,
        "best_quality" => Resampling::BestQuality,
        _ => return None,
    })
}

/// `{"transform":[a,b,c,d,tx,ty],"resampling","copy"}`; every key is optional
/// (identity, bicubic, no copy). The transform maps document points x' = a x +
/// c y + tx, y' = b x + d y + ty.
pub(crate) fn options(v: &Value) -> Result<MoveOptions, OpStatus> {
    let bad = OpStatus::OP_ERR_BAD_ARGUMENT;
    let transform = match v.get("transform") {
        None | Some(Value::Null) => Affine::identity(),
        Some(Value::Array(a)) if a.len() == 6 => {
            let mut m = [0.0; 6];
            for (slot, x) in m.iter_mut().zip(a) {
                *slot = x.as_f64().filter(|f| f.is_finite()).ok_or(bad)?;
            }
            Affine(m)
        }
        Some(_) => return Err(bad),
    };
    let resampling = match v.get("resampling") {
        None | Some(Value::Null) => Resampling::Bicubic,
        Some(Value::String(s)) => resampling(s).ok_or(bad)?,
        Some(_) => return Err(bad),
    };
    let copy = match v.get("copy") {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(_) => return Err(bad),
    };
    Ok(MoveOptions {
        transform,
        resampling,
        copy,
    })
}

/// Starts a move session on the active layer. `json`: `{"kind":"pixels" |
/// "selection", "transform", "resampling", "copy"}` (see `op_move_update`).
/// With no selection, "pixels" moves the whole layer. Returns OP_ERR_BUSY while
/// a stroke, a preview or another move runs, and OP_ERR_BAD_ARGUMENT for bad
/// options or a transform that cannot be inverted.
#[unsafe(no_mangle)]
pub extern "C" fn op_move_begin(
    p: *mut OpEngine,
    doc: u64,
    json: *const u8,
    len: usize,
) -> OpStatus {
    guard(|| {
        let e = match engine(p) {
            Ok(e) => e,
            Err(s) => return s,
        };
        let v = match json_bytes(json, len) {
            Ok(v) => v,
            Err(s) => return s,
        };
        let kind = match v.get("kind").and_then(Value::as_str) {
            Some("pixels") => MoveKind::Pixels,
            Some("selection") => MoveKind::Selection,
            _ => return OpStatus::OP_ERR_BAD_ARGUMENT,
        };
        let opts = match options(&v) {
            Ok(o) => o,
            Err(s) => return s,
        };
        let result = e.core().move_begin(doc, kind, opts);
        crate::view::doc_touched(p, doc);
        match result {
            Ok(_) => OpStatus::OP_OK,
            Err(x) => status(x),
        }
    })
}

/// Replaces the session's whole transform (not a delta), its resampling and
/// its copy flag, and publishes the new preview. `json`: `{"transform":
/// [a,b,c,d,tx,ty], "resampling", "copy"}`. Writes the preview serial to
/// `out_serial` when it is not null. OP_ERR_BAD_ARGUMENT when no move runs or
/// the options are bad (the preview stays as it was).
#[unsafe(no_mangle)]
pub extern "C" fn op_move_update(
    p: *mut OpEngine,
    doc: u64,
    json: *const u8,
    len: usize,
    out_serial: *mut u64,
) -> OpStatus {
    guard(|| {
        let e = match engine(p) {
            Ok(e) => e,
            Err(s) => return s,
        };
        let v = match json_bytes(json, len) {
            Ok(v) => v,
            Err(s) => return s,
        };
        let opts = match options(&v) {
            Ok(o) => o,
            Err(s) => return s,
        };
        if let Err(x) = e.core().move_update(doc, opts) {
            return status(x);
        }
        crate::view::doc_touched(p, doc);
        if !out_serial.is_null() {
            let serial = e.core().snapshot(doc).map_or(0, |s| s.preview_serial);
            // SAFETY: checked non-null.
            unsafe { *out_serial = serial };
        }
        OpStatus::OP_OK
    })
}

/// Commits the session as one history step. OP_ERR_BAD_ARGUMENT when no move
/// runs; OP_ERR_BUSY (and the move is dropped) when the active layer changed.
#[unsafe(no_mangle)]
pub extern "C" fn op_move_finish(p: *mut OpEngine, doc: u64) -> OpStatus {
    guard(|| {
        let e = match engine(p) {
            Ok(e) => e,
            Err(s) => return s,
        };
        let result = e.core().move_finish(doc);
        crate::view::doc_touched(p, doc);
        match result {
            Ok(()) => OpStatus::OP_OK,
            Err(x) => status(x),
        }
    })
}

/// Drops the session: pixels, selection and history stay as they were.
/// OP_ERR_BAD_ARGUMENT when no move runs.
#[unsafe(no_mangle)]
pub extern "C" fn op_move_cancel(p: *mut OpEngine, doc: u64) -> OpStatus {
    guard(|| {
        let e = match engine(p) {
            Ok(e) => e,
            Err(s) => return s,
        };
        let result = e.core().move_cancel(doc);
        crate::view::doc_touched(p, doc);
        match result {
            Ok(()) => OpStatus::OP_OK,
            Err(x) => status(x),
        }
    })
}
