//! M57 crash recovery: turn recovery records on for a document, list the
//! records left by a crash, open one, discard one. Records and their format
//! are described in docs/ARCHITECTURE.md ("Recovery records").
//! `doc.recovery` (op_query) reports the acknowledged revision and the last
//! write error; `doc.last_file_error` the last save error as text.
use crate::{OpBuffer, OpEngine, OpStatus, bytes, engine, guard, json_bytes, owned, status};
use op_engine::recovery::{self, RecoveryOptions};
use serde_json::Value;
use std::time::Duration;

fn options(params: &Value) -> RecoveryOptions {
    let mut o = RecoveryOptions::default();
    if let Some(ms) = params.get("settle_ms").and_then(Value::as_u64) {
        o.settle = Duration::from_millis(ms);
    }
    if let Some(ms) = params.get("max_interval_ms").and_then(Value::as_u64) {
        o.max_interval = Duration::from_millis(ms);
    }
    if let Some(n) = params.get("test_fail_after_bytes").and_then(Value::as_u64) {
        o.hooks.fail_after_bytes = Some(n);
    }
    o
}

fn dir_arg(params: &Value) -> Result<String, OpStatus> {
    params
        .get("dir")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .ok_or(OpStatus::OP_ERR_BAD_ARGUMENT)
}

/// Turns on recovery records for `doc`. `json`: `{"dir":"<root>/<doc-uuid>",
/// "original_url":"file:///x.opaint"|null, "settle_ms":30000,
/// "max_interval_ms":120000}`. Calling it again only updates `original_url`.
#[unsafe(no_mangle)]
pub extern "C" fn op_recovery_attach(
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
        let dir = match dir_arg(&params) {
            Ok(d) => d,
            Err(s) => return s,
        };
        let url = params
            .get("original_url")
            .and_then(Value::as_str)
            .map(str::to_owned);
        match e.core().recovery_attach(doc, dir, url, options(&params)) {
            Ok(()) => OpStatus::OP_OK,
            Err(x) => status(x),
        }
    })
}

/// Writes a record now if the document changed since the last one, and
/// waits for it. Mostly for tests; the writer writes by itself.
#[unsafe(no_mangle)]
pub extern "C" fn op_recovery_flush(p: *mut OpEngine, doc: u64) -> OpStatus {
    guard(|| {
        let e = match engine(p) {
            Ok(e) => e,
            Err(s) => return s,
        };
        match e.core().recovery_flush(doc) {
            Ok(_) => OpStatus::OP_OK,
            Err(x) => status(x),
        }
    })
}

/// Stops recovery for `doc`. With `delete_record` false the record stays on
/// disk and its folder is let go, as after a crash (tests use this); with
/// true the folder is deleted, as on a clean close.
#[unsafe(no_mangle)]
pub extern "C" fn op_recovery_detach(p: *mut OpEngine, doc: u64, delete_record: bool) -> OpStatus {
    guard(|| {
        let e = match engine(p) {
            Ok(e) => e,
            Err(s) => return s,
        };
        e.core().recovery_detach(doc, delete_record);
        OpStatus::OP_OK
    })
}

/// Waits up to `timeout_ms` for recovery writers that were told to stop
/// (closed documents) to finish. The app calls it on Quit. OP_OK when all
/// finished, OP_ERR_BUSY when some were still running at the limit.
#[unsafe(no_mangle)]
pub extern "C" fn op_recovery_wait_stopped(p: *mut OpEngine, timeout_ms: u64) -> OpStatus {
    guard(|| {
        let e = match engine(p) {
            Ok(e) => e,
            Err(s) => return s,
        };
        if e.core()
            .recovery_wait_stopped(std::time::Duration::from_millis(timeout_ms))
        {
            OpStatus::OP_OK
        } else {
            OpStatus::OP_ERR_BUSY
        }
    })
}

/// Lists the records under the UTF-8 folder `root` that no live process
/// owns, as a JSON array (needs no engine). Each entry: `uuid`, `dir`,
/// `valid`, `problem`, `revision`, `original_url`, `time_unix`, `width`,
/// `height`, `layers`, `snapshot_bytes`.
#[unsafe(no_mangle)]
pub extern "C" fn op_recovery_scan(root: *const u8, len: usize, out: *mut OpBuffer) -> OpStatus {
    guard(|| {
        if out.is_null() {
            return OpStatus::OP_ERR_BAD_ARGUMENT;
        }
        let root = match bytes(root, len).map(std::str::from_utf8) {
            Ok(Ok(s)) if !s.is_empty() => s.to_owned(),
            _ => return OpStatus::OP_ERR_BAD_ARGUMENT,
        };
        let list: Vec<Value> = recovery::scan(std::path::Path::new(&root))
            .iter()
            .map(|r| r.to_json())
            .collect();
        let bytes = Value::Array(list).to_string().into_bytes();
        unsafe { *out = owned(bytes) };
        OpStatus::OP_OK
    })
}

/// Opens the record in `{"dir":...}` as a new document marked unsaved, and
/// keeps recording it into the same folder (same JSON options as attach).
/// OP_ERR_BAD_ARGUMENT when the record is damaged, or older than its file
/// unless `"untitled": true` (Recover as Untitled: the document names no file).
#[unsafe(no_mangle)]
pub extern "C" fn op_recovery_open(
    p: *mut OpEngine,
    json: *const u8,
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
        let params = match json_bytes(json, len) {
            Ok(v) => v,
            Err(s) => return s,
        };
        let dir = match dir_arg(&params) {
            Ok(d) => d,
            Err(s) => return s,
        };
        let untitled = params
            .get("untitled")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let opened = if untitled {
            e.core().recovery_open_untitled(dir, options(&params))
        } else {
            e.core().recovery_open(dir, options(&params))
        };
        match opened {
            Ok((id, _)) => {
                unsafe { *out_doc = id };
                OpStatus::OP_OK
            }
            Err(x) => status(x),
        }
    })
}

/// Deletes the record folder in `{"dir":...}` (Discard). OP_ERR_BUSY when a
/// live process owns it.
#[unsafe(no_mangle)]
pub extern "C" fn op_recovery_discard(json: *const u8, len: usize) -> OpStatus {
    guard(|| {
        let params = match json_bytes(json, len) {
            Ok(v) => v,
            Err(s) => return s,
        };
        let dir = match dir_arg(&params) {
            Ok(d) => d,
            Err(s) => return s,
        };
        match recovery::discard(std::path::Path::new(&dir)) {
            Ok(()) => OpStatus::OP_OK,
            Err(x) => status(x),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn options_read_the_json_fields() {
        let o = options(&json!({"settle_ms":10,"max_interval_ms":20,"test_fail_after_bytes":5}));
        assert_eq!(o.settle, Duration::from_millis(10));
        assert_eq!(o.max_interval, Duration::from_millis(20));
        assert_eq!(o.hooks.fail_after_bytes, Some(5));
        assert_eq!(dir_arg(&json!({})), Err(OpStatus::OP_ERR_BAD_ARGUMENT));
    }
}
