//! Re-entry, cancellation and dispatch order through the extern "C" functions.
use op_ffi::*;
use serde_json::Value;
use std::ffi::{CString, c_void};
use std::ptr;
use std::sync::Mutex;
use std::sync::atomic::{AtomicPtr, Ordering};
use std::time::{Duration, Instant};

/// Shared with the callback through the `user` pointer.
#[derive(Default)]
struct Seen {
    engine: AtomicPtr<OpEngine>,
    /// (kind, doc, ticket, payload)
    events: Mutex<Vec<(u32, u64, u64, Value)>>,
    /// Status of each op_* call made from inside the callback.
    reentry: Mutex<Vec<(&'static str, OpStatus)>>,
    reentry_new_was_null: Mutex<Vec<bool>>,
}
unsafe extern "C" fn record(event: *const OpEvent, user: *mut c_void) {
    let seen = unsafe { &*(user as *const Seen) };
    let ev = unsafe { &*event };
    let payload = unsafe { std::slice::from_raw_parts(ev.json_ptr, ev.json_len) };
    seen.events.lock().unwrap().push((
        ev.kind,
        ev.doc,
        ev.ticket,
        serde_json::from_slice(payload).unwrap(),
    ));
}
unsafe extern "C" fn reenter(event: *const OpEvent, user: *mut c_void) {
    unsafe { record(event, user) };
    let seen = unsafe { &*(user as *const Seen) };
    let e = seen.engine.load(Ordering::SeqCst);
    let doc = unsafe { (*event).doc };
    let q = CString::new("doc.info").unwrap();
    let c = CString::new("layer.add").unwrap();
    let mut out = OpBuffer {
        ptr: ptr::null_mut(),
        len: 0,
    };
    let mut id = 0u64;
    let path = b"/nonexistent.png";
    let calls = [
        (
            "op_query",
            op_query(e, doc, q.as_ptr(), ptr::null(), 0, &mut out),
        ),
        (
            "op_dispatch",
            op_dispatch(e, doc, c.as_ptr(), ptr::null(), 0, &mut id),
        ),
        ("op_cancel", op_cancel(e, 1)),
        ("op_doc_close", op_doc_close(e, doc)),
        (
            "op_import_pixels",
            op_import_pixels(e, 1, 1, [0u8; 4].as_ptr(), 4, ptr::null(), 0, &mut id),
        ),
        (
            "op_doc_open_png",
            op_doc_open_png(e, path.as_ptr(), path.len(), &mut id),
        ),
        (
            "op_view_attach",
            op_view_attach(e, doc, ptr::null_mut(), &mut id),
        ),
    ];
    seen.reentry.lock().unwrap().extend(calls);
    let nested = op_engine_new(ptr::null(), None, ptr::null_mut());
    seen.reentry_new_was_null
        .lock()
        .unwrap()
        .push(nested.is_null());
    if !nested.is_null() {
        op_engine_free(nested);
    }
    // Must be a no-op from inside a callback.
    op_engine_free(e);
}
fn new_engine(seen: &Seen, cb: OpEventFn) -> *mut OpEngine {
    let e = op_engine_new(ptr::null(), cb, seen as *const Seen as *mut c_void);
    assert!(!e.is_null());
    seen.engine.store(e, Ordering::SeqCst);
    e
}
fn import(e: *mut OpEngine, w: u32, h: u32) -> u64 {
    let px: Vec<u8> = (0..w * h).flat_map(|i| [i as u8, 9, 9, 255]).collect();
    let mut id = 0;
    assert_eq!(
        op_import_pixels(e, w, h, px.as_ptr(), px.len(), ptr::null(), 0, &mut id),
        OpStatus::OP_OK
    );
    id
}
fn dispatch(e: *mut OpEngine, doc: u64, id: &str, json: &str) -> u64 {
    let mut ticket = 0;
    let c = CString::new(id).unwrap();
    assert_eq!(
        op_dispatch(e, doc, c.as_ptr(), json.as_ptr(), json.len(), &mut ticket),
        OpStatus::OP_OK,
        "{id} {json}"
    );
    ticket
}
fn info(e: *mut OpEngine, doc: u64) -> Value {
    let q = CString::new("doc.info").unwrap();
    let mut out = OpBuffer {
        ptr: ptr::null_mut(),
        len: 0,
    };
    assert_eq!(
        op_query(e, doc, q.as_ptr(), ptr::null(), 0, &mut out),
        OpStatus::OP_OK
    );
    let v = serde_json::from_slice(unsafe { std::slice::from_raw_parts(out.ptr, out.len) });
    op_buffer_free(&mut out);
    v.unwrap()
}
fn wait_finished(seen: &Seen, ticket: u64, limit: Duration) -> Value {
    let end = Instant::now() + limit;
    while Instant::now() < end {
        if let Some((_, _, _, v)) = seen
            .events
            .lock()
            .unwrap()
            .iter()
            .find(|(k, _, t, _)| *k == 4 && *t == ticket)
        {
            return v.clone();
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    panic!("no JobFinished for ticket {ticket} within {limit:?}");
}

#[test]
fn every_op_call_from_inside_the_callback_returns_busy() {
    let seen = Seen::default();
    let e = new_engine(&seen, Some(reenter));
    let doc = import(e, 1, 1);
    let t = dispatch(e, doc, "layer.add", "{}");
    let done = wait_finished(&seen, t, Duration::from_secs(5));
    assert_eq!(done["status"], "ok");
    let calls = seen.reentry.lock().unwrap().clone();
    assert!(!calls.is_empty());
    for (name, status) in &calls {
        assert_eq!(*status, OpStatus::OP_ERR_BUSY, "{name} from a callback");
    }
    assert!(seen.reentry_new_was_null.lock().unwrap().iter().all(|n| *n));
    // The engine still works after the callbacks tried to close and free it.
    assert_eq!(info(e, doc)["layers"].as_array().unwrap().len(), 2);
    assert_eq!(op_doc_close(e, doc), OpStatus::OP_OK);
    op_engine_free(e);
}

#[test]
fn cancel_stops_running_command_and_reports_cancelled() {
    let seen = Seen::default();
    let e = new_engine(&seen, Some(record));
    // 1024x512 = 8 tiles, 100 ms each when not cancelled.
    let doc = import(e, 1024, 512);
    let t = dispatch(e, doc, "test.slow_invert", r#"{"delay_ms":100}"#);
    std::thread::sleep(Duration::from_millis(150));
    let start = Instant::now();
    assert_eq!(op_cancel(e, t), OpStatus::OP_OK);
    let done = wait_finished(&seen, t, Duration::from_secs(3));
    assert!(start.elapsed() < Duration::from_millis(300));
    assert_eq!(done["status"], "cancelled", "{done}");
    assert_eq!(done["code"], OpStatus::OP_ERR_CANCELLED as u32);
    let i = info(e, doc);
    assert_eq!(i["history_labels"].as_array().unwrap().len(), 1);
    assert_eq!(i["revision"], 0);
    // A finished ticket can still be cancelled: a harmless no-op.
    assert_eq!(op_cancel(e, t), OpStatus::OP_OK);
    // A ticket that was never issued is a bad argument.
    assert_eq!(op_cancel(e, 1_000_000), OpStatus::OP_ERR_BAD_ARGUMENT);
    assert_eq!(op_doc_close(e, doc), OpStatus::OP_OK);
    op_engine_free(e);
}

#[test]
fn dispatches_run_in_call_order_and_each_ticket_finishes_once() {
    let seen = Seen::default();
    let e = new_engine(&seen, Some(record));
    let doc = import(e, 2, 2);
    let mut tickets = vec![dispatch(
        e,
        doc,
        "preview.begin",
        r#"{"command":"image.invert_colors","params":{}}"#,
    )];
    for _ in 0..100 {
        tickets.push(dispatch(e, doc, "preview.update", "{}"));
    }
    let commit = dispatch(e, doc, "preview.commit", "{}");
    tickets.push(commit);
    let done = wait_finished(&seen, commit, Duration::from_secs(5));
    assert_eq!(done["status"], "ok", "{done}");
    for t in &tickets {
        let v = wait_finished(&seen, *t, Duration::from_secs(5));
        assert!(
            ["ok", "dropped"].contains(&v["status"].as_str().unwrap()),
            "ticket {t}: {v}"
        );
    }
    let events = seen.events.lock().unwrap().clone();
    for t in &tickets {
        let n = events
            .iter()
            .filter(|(k, _, x, _)| *k == 4 && x == t)
            .count();
        assert_eq!(n, 1, "ticket {t} finished {n} times");
    }
    assert_eq!(
        info(e, doc)["history_labels"].as_array().unwrap().len(),
        2,
        "exactly one history step"
    );
    assert_eq!(op_doc_close(e, doc), OpStatus::OP_OK);
    op_engine_free(e);
}

#[test]
fn read_only_after_panic_is_reported_by_dispatch_and_info() {
    let seen = Seen::default();
    let e = new_engine(&seen, Some(record));
    let doc = import(e, 1, 1);
    let t = dispatch(e, doc, "test.panic", "{}");
    let done = wait_finished(&seen, t, Duration::from_secs(5));
    assert_eq!(done["status"], "panic");
    assert_eq!(done["code"], OpStatus::OP_ERR_PANIC as u32);
    assert_eq!(info(e, doc)["read_only"], true);
    let t = dispatch(e, doc, "layer.add", "{}");
    let done = wait_finished(&seen, t, Duration::from_secs(5));
    assert_eq!(done["status"], "read_only");
    assert_eq!(done["code"], OpStatus::OP_ERR_READ_ONLY as u32);
    assert_eq!(info(e, doc)["layers"].as_array().unwrap().len(), 1);
    assert_eq!(op_doc_close(e, doc), OpStatus::OP_OK);
    op_engine_free(e);
}
