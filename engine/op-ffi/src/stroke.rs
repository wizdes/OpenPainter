//! Live brush strokes (M38): `op_stroke_begin`, `op_stroke_append`,
//! `op_stroke_end` and `op_stroke_cancel`. The Mac sends pointer samples while
//! the pointer moves; the canvas shows the engine's preview after each append,
//! `op_stroke_end` commits one history step and `op_stroke_cancel` leaves the
//! document as it was. One stroke per document at a time.
use crate::{OpEngine, OpStatus, engine, guard, json_bytes, status};
use op_tools::StrokeSample;

/// One pointer sample in document pixels. `pressure` is 0..1 (1 for a mouse);
/// `tilt_x` and `tilt_y` are -1..1 (0 for a mouse); `time_ms` must not go back.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct OpStrokeSample {
    pub x: f64,
    pub y: f64,
    pub pressure: f64,
    pub tilt_x: f64,
    pub tilt_y: f64,
    pub time_ms: u64,
}

/// Starts a Paintbrush, Eraser or Pencil stroke on the active layer. `json`:
/// `{"tool","width","color":"rrggbbaa","opacity":0..255,"antialias","blend",
/// "hardness":0..1,"pressure_size","pressure_opacity"}`. Returns OP_ERR_BUSY
/// while another stroke, a preview or a move runs on the document, and
/// OP_ERR_BAD_ARGUMENT for bad options.
#[unsafe(no_mangle)]
pub extern "C" fn op_stroke_begin(
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
        let params = match json_bytes(json, len) {
            Ok(v) => v,
            Err(s) => return s,
        };
        let (settings, hardness) = match op_engine::live_stroke::brush_from_json(&params) {
            Ok(b) => b,
            Err(x) => return status(x),
        };
        match e.core().stroke_begin_hard(doc, settings, hardness, 0) {
            Ok(_) => OpStatus::OP_OK,
            Err(x) => status(x),
        }
    })
}

/// Adds samples to the document's stroke and publishes the new preview; the
/// document's canvas views draw it at once. Writes the preview's serial (the
/// `preview_serial` of the frame stamps that show it) to `out_serial` when it
/// is not null. A bad sample changes nothing and returns OP_ERR_BAD_ARGUMENT.
#[unsafe(no_mangle)]
pub extern "C" fn op_stroke_append(
    p: *mut OpEngine,
    doc: u64,
    samples: *const OpStrokeSample,
    count: usize,
    out_serial: *mut u64,
) -> OpStatus {
    guard(|| {
        let e = match engine(p) {
            Ok(e) => e,
            Err(s) => return s,
        };
        if samples.is_null() && count > 0 || count > 1 << 20 {
            return OpStatus::OP_ERR_BAD_ARGUMENT;
        }
        let raw: &[OpStrokeSample] = if count == 0 {
            &[]
        } else {
            // SAFETY: checked non-null; the caller passes `count` samples.
            unsafe { std::slice::from_raw_parts(samples, count) }
        };
        let list: Vec<StrokeSample> = raw
            .iter()
            .map(|s| StrokeSample {
                x: s.x,
                y: s.y,
                pressure: s.pressure,
                timestamp_ms: s.time_ms,
                tilt_x: s.tilt_x,
                tilt_y: s.tilt_y,
            })
            .collect();
        if let Err(x) = e.core().stroke_append(doc, &list) {
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

/// Commits the document's stroke as one history step named after the tool.
/// Returns OP_ERR_BUSY (and drops the stroke) when the document changed under it.
#[unsafe(no_mangle)]
pub extern "C" fn op_stroke_end(p: *mut OpEngine, doc: u64) -> OpStatus {
    guard(|| {
        let e = match engine(p) {
            Ok(e) => e,
            Err(s) => return s,
        };
        let result = e.core().stroke_commit(doc);
        crate::view::doc_touched(p, doc);
        match result {
            Ok(()) => OpStatus::OP_OK,
            Err(x) => status(x),
        }
    })
}

/// Drops the document's stroke; the document and its history stay as they were.
/// Returns OP_ERR_BAD_ARGUMENT when no stroke runs.
#[unsafe(no_mangle)]
pub extern "C" fn op_stroke_cancel(p: *mut OpEngine, doc: u64) -> OpStatus {
    guard(|| {
        let e = match engine(p) {
            Ok(e) => e,
            Err(s) => return s,
        };
        let result = e.core().stroke_cancel(doc);
        crate::view::doc_touched(p, doc);
        match result {
            Ok(()) => OpStatus::OP_OK,
            Err(x) => status(x),
        }
    })
}
