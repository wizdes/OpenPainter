//! M19 C interface: New, .opaint open and save, PNG export, the saved marker,
//! mark_saved after an app-side write, and replace (Revert).
use op_ffi::*;
use std::ffi::CString;
use std::ptr;

fn info(e: *mut OpEngine, doc: u64) -> serde_json::Value {
    let mut out = OpBuffer {
        ptr: ptr::null_mut(),
        len: 0,
    };
    let q = CString::new("doc.info").unwrap();
    assert_eq!(
        op_query(e, doc, q.as_ptr(), ptr::null(), 0, &mut out),
        OpStatus::OP_OK
    );
    let v =
        serde_json::from_slice(unsafe { std::slice::from_raw_parts(out.ptr, out.len) }).unwrap();
    op_buffer_free(&mut out);
    v
}
fn run(e: *mut OpEngine, doc: u64, id: &str) {
    let mut ticket = 0;
    let c = CString::new(id).unwrap();
    assert_eq!(
        op_dispatch(e, doc, c.as_ptr(), ptr::null(), 0, &mut ticket),
        OpStatus::OP_OK
    );
    // Any synchronous call waits for queued commands; mark_saved with a stale
    // revision is a cheap one.
    let _ = op_doc_mark_saved(e, doc, u64::MAX);
}
fn save(e: *mut OpEngine, doc: u64, json: &str) -> OpStatus {
    op_doc_save(e, doc, json.as_ptr(), json.len())
}

#[test]
fn new_save_open_mark_and_replace() {
    let dir = tempfile::tempdir().unwrap();
    let e = op_engine_new(ptr::null(), None, ptr::null_mut());
    let mut doc = 0;
    assert_eq!(
        op_doc_new(e, 4, 2, 72.0, 0x102030ff, &mut doc),
        OpStatus::OP_OK
    );
    assert_eq!(
        op_doc_new(e, 0, 2, 72.0, 0, &mut doc),
        OpStatus::OP_ERR_BAD_ARGUMENT
    );
    assert_eq!(
        op_doc_new(e, 4, 2, 72.0, 0, ptr::null_mut()),
        OpStatus::OP_ERR_BAD_ARGUMENT
    );
    assert_eq!(
        op_doc_new(e, 4, 2, 72.0, 0x102030ff, &mut doc),
        OpStatus::OP_OK
    );
    assert_eq!(info(e, doc)["dirty"], false);
    run(e, doc, "layer.add");
    assert_eq!(info(e, doc)["dirty"], true);

    let opaint = dir.path().join("a.opaint");
    let json = format!(
        r#"{{"path":{:?},"format":"opaint"}}"#,
        opaint.to_str().unwrap()
    );
    assert_eq!(save(e, doc, &json), OpStatus::OP_OK);
    assert_eq!(info(e, doc)["dirty"], false, "save clears the mark");
    run(e, doc, "edit.undo");
    assert_eq!(info(e, doc)["dirty"], true);
    run(e, doc, "edit.redo");
    assert_eq!(info(e, doc)["dirty"], false);

    let png = dir.path().join("a.png");
    let json = format!(
        r#"{{"path":{:?},"format":"png","mark_saved":false}}"#,
        png.to_str().unwrap()
    );
    run(e, doc, "edit.undo");
    assert_eq!(save(e, doc, &json), OpStatus::OP_OK);
    assert_eq!(info(e, doc)["dirty"], true, "a copy leaves the mark");
    assert!(png.exists());
    assert_eq!(
        save(e, doc, r#"{"path":"/x.gif","format":"gif"}"#),
        OpStatus::OP_ERR_BAD_ARGUMENT
    );
    assert_eq!(
        save(e, doc, r#"{"format":"png"}"#),
        OpStatus::OP_ERR_BAD_ARGUMENT
    );
    assert_eq!(
        save(e, 999, r#"{"path":"/tmp/x.png","format":"png"}"#),
        OpStatus::OP_ERR_NO_SUCH_DOC
    );

    let revision = info(e, doc)["revision"].as_u64().unwrap();
    assert_eq!(
        op_doc_mark_saved(e, doc, revision + 1),
        OpStatus::OP_ERR_BUSY
    );
    assert_eq!(op_doc_mark_saved(e, doc, revision), OpStatus::OP_OK);
    assert_eq!(info(e, doc)["dirty"], false);

    let path = opaint.to_str().unwrap();
    let mut reopened = 0;
    assert_eq!(
        op_doc_open_opaint(e, path.as_ptr(), path.len(), &mut reopened),
        OpStatus::OP_OK
    );
    assert_eq!(info(e, reopened)["layers"].as_array().unwrap().len(), 2);
    let bad = "/no/such/file.opaint";
    assert_ne!(
        op_doc_open_opaint(e, bad.as_ptr(), bad.len(), &mut reopened),
        OpStatus::OP_OK
    );

    assert_eq!(op_doc_replace(e, doc, reopened), OpStatus::OP_OK);
    let i = info(e, doc);
    assert_eq!(i["layers"].as_array().unwrap().len(), 2);
    assert_eq!(i["history_labels"].as_array().unwrap().len(), 1);
    assert_eq!(i["dirty"], false);
    assert_eq!(op_doc_close(e, reopened), OpStatus::OP_OK);
    assert_eq!(
        op_doc_replace(e, doc, reopened),
        OpStatus::OP_ERR_NO_SUCH_DOC
    );
    op_engine_free(e);
}
