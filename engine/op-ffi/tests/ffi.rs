use op_ffi::*;
use std::ffi::{CString, c_void};
use std::ptr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
static CALLBACKS: AtomicUsize = AtomicUsize::new(0);
static REENTRY: AtomicUsize = AtomicUsize::new(0);
static mut ENGINE: *mut OpEngine = ptr::null_mut();
unsafe extern "C" fn callback(event: *const OpEvent, _user: *mut c_void) {
    CALLBACKS.fetch_add(1, Ordering::SeqCst);
    assert!(!event.is_null());
    let mut out = OpBuffer {
        ptr: ptr::null_mut(),
        len: 0,
    };
    let query = CString::new("doc.info").unwrap();
    let result = op_query(
        unsafe { ENGINE },
        unsafe { (*event).doc },
        query.as_ptr(),
        ptr::null(),
        0,
        &mut out,
    );
    if result == OpStatus::OP_ERR_BUSY {
        REENTRY.fetch_add(1, Ordering::SeqCst);
    }
}
fn imported(e: *mut OpEngine) -> u64 {
    let mut id = 0;
    assert_eq!(
        op_import_pixels(e, 1, 1, [1, 2, 3, 255].as_ptr(), 4, ptr::null(), 0, &mut id),
        OpStatus::OP_OK
    );
    id
}
fn dispatch(e: *mut OpEngine, doc: u64, id: &str, json: &str) -> u64 {
    let mut ticket = 0;
    let c = CString::new(id).unwrap();
    assert_eq!(
        op_dispatch(e, doc, c.as_ptr(), json.as_ptr(), json.len(), &mut ticket),
        OpStatus::OP_OK
    );
    ticket
}
#[test]
fn null_bad_json_stale_and_buffer_ownership() {
    let e = op_engine_new(ptr::null(), None, ptr::null_mut());
    assert!(!e.is_null());
    let id = imported(e);
    let mut out = OpBuffer {
        ptr: ptr::null_mut(),
        len: 0,
    };
    let q = CString::new("doc.info").unwrap();
    assert_eq!(
        op_query(ptr::null_mut(), id, q.as_ptr(), ptr::null(), 0, &mut out),
        OpStatus::OP_ERR_BAD_ARGUMENT
    );
    assert_eq!(
        op_query(e, 9999, q.as_ptr(), ptr::null(), 0, &mut out),
        OpStatus::OP_ERR_NO_SUCH_DOC
    );
    assert_eq!(
        op_query(e, id, q.as_ptr(), b"{".as_ptr(), 1, &mut out),
        OpStatus::OP_ERR_BAD_ARGUMENT
    );
    assert_eq!(
        op_query(e, id, q.as_ptr(), ptr::null(), 0, &mut out),
        OpStatus::OP_OK
    );
    assert!(out.len > 0);
    op_buffer_free(&mut out);
    op_buffer_free(&mut out);
    assert_eq!(op_doc_close(e, id), OpStatus::OP_OK);
    assert_eq!(op_doc_close(e, id), OpStatus::OP_ERR_NO_SUCH_DOC);
    op_engine_free(e);
}
#[test]
fn panic_pending_jobs_free_and_callback_lifetime() {
    CALLBACKS.store(0, Ordering::SeqCst);
    REENTRY.store(0, Ordering::SeqCst);
    let e = op_engine_new(ptr::null(), Some(callback), ptr::null_mut());
    unsafe { ENGINE = e };
    let id = imported(e);
    dispatch(e, id, "test.panic", "{}");
    for _ in 0..100 {
        let mut out = OpBuffer {
            ptr: ptr::null_mut(),
            len: 0,
        };
        let q = CString::new("doc.info").unwrap();
        assert_eq!(
            op_query(e, id, q.as_ptr(), ptr::null(), 0, &mut out),
            OpStatus::OP_OK
        );
        op_buffer_free(&mut out);
        if CALLBACKS.load(Ordering::SeqCst) > 0 {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(CALLBACKS.load(Ordering::SeqCst) > 0);
    assert!(REENTRY.load(Ordering::SeqCst) > 0);
    let pending = dispatch(
        e,
        id,
        "preview.begin",
        r#"{"command":"test.slow_invert","params":{"delay_ms":200}}"#,
    );
    assert_eq!(op_cancel(e, pending), OpStatus::OP_OK);
    assert_eq!(op_doc_close(e, id), OpStatus::OP_OK);
    op_engine_free(e);
    let count = CALLBACKS.load(Ordering::SeqCst);
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(CALLBACKS.load(Ordering::SeqCst), count);
    unsafe { ENGINE = ptr::null_mut() };
}
#[test]
fn free_waits_for_pending_preview_work() {
    let engine = op_engine_new(ptr::null(), None, ptr::null_mut());
    let doc = imported(engine);
    let ticket = dispatch(
        engine,
        doc,
        "preview.begin",
        r#"{"command":"test.slow_invert","params":{"delay_ms":250}}"#,
    );
    assert!(ticket > 0);
    std::thread::sleep(Duration::from_millis(20));
    let start = std::time::Instant::now();
    op_engine_free(engine);
    assert!(start.elapsed() >= Duration::from_millis(150));
}
