//! Document lifecycle (M19): `op_doc_new` (File > New), `op_doc_open_opaint`,
//! `op_doc_save` (.opaint and flattened PNG), `op_doc_mark_saved` (after the
//! app wrote an ImageIO format itself) and `op_doc_replace` (Revert). Each call
//! waits for the document's queued commands, then returns.
use crate::{OpEngine, OpStatus, bytes, engine, guard, json_bytes, status};
use op_engine::SaveFormat;
use serde_json::Value;

fn path_arg(ptr: *const u8, len: usize) -> Result<String, OpStatus> {
    let b = bytes(ptr, len)?;
    let s = std::str::from_utf8(b).map_err(|_| OpStatus::OP_ERR_BAD_ARGUMENT)?;
    if s.is_empty() {
        return Err(OpStatus::OP_ERR_BAD_ARGUMENT);
    }
    Ok(s.to_owned())
}

/// File > New: a one-layer image named "Background" filled with
/// `background_rgba` (0xRRGGBBAA, straight alpha; 0 is Transparent) at `dpi`
/// pixels per inch. The new image is not modified.
#[unsafe(no_mangle)]
pub extern "C" fn op_doc_new(
    p: *mut OpEngine,
    width: u32,
    height: u32,
    dpi: f64,
    background_rgba: u32,
    out_doc: *mut u64,
) -> OpStatus {
    guard(|| {
        if out_doc.is_null() {
            return OpStatus::OP_ERR_BAD_ARGUMENT;
        }
        let e = match engine(p) {
            Ok(e) => e,
            Err(s) => return s,
        };
        match e
            .core()
            .new_document(width, height, dpi, background_rgba.to_be_bytes())
        {
            Ok(id) => {
                unsafe { *out_doc = id };
                OpStatus::OP_OK
            }
            Err(x) => status(x),
        }
    })
}

/// Opens a `.opaint` file (UTF-8 path). A damaged file returns
/// OP_ERR_BAD_ARGUMENT; a file that cannot be read returns OP_ERR_INTERNAL.
#[unsafe(no_mangle)]
pub extern "C" fn op_doc_open_opaint(
    p: *mut OpEngine,
    path: *const u8,
    len: usize,
    out_doc: *mut u64,
) -> OpStatus {
    guard(|| {
        if out_doc.is_null() {
            return OpStatus::OP_ERR_BAD_ARGUMENT;
        }
        let e = match engine(p) {
            Ok(e) => e,
            Err(s) => return s,
        };
        let path = match path_arg(path, len) {
            Ok(s) => s,
            Err(s) => return s,
        };
        match e.core().open_opaint(path) {
            Ok(id) => {
                unsafe { *out_doc = id };
                OpStatus::OP_OK
            }
            Err(x) => status(x),
        }
    })
}

/// Saves the document's current history step. `json`:
/// `{"path":"/x.opaint","format":"opaint"|"png","mark_saved":true}`.
/// `mark_saved` (default true) makes that step the saved one, so `doc.info`
/// reports `dirty: false` until the history moves off it; Save a Copy passes
/// false. The file is replaced only after it is fully written.
#[unsafe(no_mangle)]
pub extern "C" fn op_doc_save(p: *mut OpEngine, doc: u64, json: *const u8, len: usize) -> OpStatus {
    guard(|| {
        let e = match engine(p) {
            Ok(e) => e,
            Err(s) => return s,
        };
        let params = match json_bytes(json, len) {
            Ok(v) => v,
            Err(s) => return s,
        };
        let Some(path) = params
            .get("path")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
        else {
            return OpStatus::OP_ERR_BAD_ARGUMENT;
        };
        let format =
            match SaveFormat::from_name(params.get("format").and_then(Value::as_str).unwrap_or(""))
            {
                Ok(f) => f,
                Err(x) => return status(x),
            };
        let mark = params
            .get("mark_saved")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        match e.core().save_document(doc, path, format, mark) {
            Ok(()) => OpStatus::OP_OK,
            Err(x) => status(x),
        }
    })
}

/// Marks the current step saved after the app wrote the file itself.
/// `revision` is the `doc.info` revision the pixels were read at; when the
/// document changed since, this returns OP_ERR_BUSY and the mark stays.
#[unsafe(no_mangle)]
pub extern "C" fn op_doc_mark_saved(p: *mut OpEngine, doc: u64, revision: u64) -> OpStatus {
    guard(|| {
        let e = match engine(p) {
            Ok(e) => e,
            Err(s) => return s,
        };
        match e.core().mark_saved(doc, revision) {
            Ok(()) => OpStatus::OP_OK,
            Err(x) => status(x),
        }
    })
}

/// Revert: `doc` takes `source`'s content as a fresh, saved, one-step
/// history. The document id, its views and its window stay; `source` stays
/// open for the caller to close.
#[unsafe(no_mangle)]
pub extern "C" fn op_doc_replace(p: *mut OpEngine, doc: u64, source: u64) -> OpStatus {
    guard(|| {
        let e = match engine(p) {
            Ok(e) => e,
            Err(s) => return s,
        };
        match e.core().replace_content(doc, source) {
            Ok(()) => OpStatus::OP_OK,
            Err(x) => status(x),
        }
    })
}
