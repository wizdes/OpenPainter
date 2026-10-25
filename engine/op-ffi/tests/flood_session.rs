//! M31 through the C interface: a live Magic Wand session with op_dispatch
//! ("preview.begin" / "preview.update" / "preview.commit") and the live
//! selection through op_query with {"preview":true}.
use op_ffi::*;
use serde_json::{Value, json};
use std::ffi::CString;
use std::ptr;
use std::time::{Duration, Instant};

fn query(e: *mut OpEngine, doc: u64, id: &str, params: &Value) -> Vec<u8> {
    let mut out = OpBuffer {
        ptr: ptr::null_mut(),
        len: 0,
    };
    let q = CString::new(id).unwrap();
    let body = params.to_string();
    assert_eq!(
        op_query(e, doc, q.as_ptr(), body.as_ptr(), body.len(), &mut out),
        OpStatus::OP_OK
    );
    let v = unsafe { std::slice::from_raw_parts(out.ptr, out.len) }.to_vec();
    op_buffer_free(&mut out);
    v
}
fn dispatch(e: *mut OpEngine, doc: u64, id: &str, params: &Value) {
    let mut ticket = 0;
    let c = CString::new(id).unwrap();
    let body = params.to_string();
    assert_eq!(
        op_dispatch(e, doc, c.as_ptr(), body.as_ptr(), body.len(), &mut ticket),
        OpStatus::OP_OK,
        "{id}"
    );
}
fn info(e: *mut OpEngine, doc: u64) -> Value {
    serde_json::from_slice(&query(e, doc, "doc.info", &json!({}))).unwrap()
}

#[test]
fn live_wand_session_through_the_c_interface() {
    let e = op_engine_new(ptr::null(), None, ptr::null_mut());
    let mut px = Vec::new();
    for x in 0..4u8 {
        px.extend_from_slice(&[x * 60, 0, 0, 255]);
    }
    let mut doc = 0;
    assert_eq!(
        op_import_pixels(e, 4, 1, px.as_ptr(), px.len(), ptr::null(), 0, &mut doc),
        OpStatus::OP_OK
    );
    let begin = json!({"command":"tool.magic_wand","params":{"x":0,"y":0,"tolerance":0.0}});
    dispatch(e, doc, "preview.begin", &begin);
    dispatch(e, doc, "preview.update", &json!({"tolerance":0.5}));
    // The live selection arrives once the render lands.
    let start = Instant::now();
    let live = loop {
        let m = query(e, doc, "doc.selection_mask", &json!({"preview":true}));
        if m != query(e, doc, "doc.selection_mask", &json!({})) {
            break m;
        }
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "no live selection"
        );
        std::thread::sleep(Duration::from_millis(5));
    };
    assert_eq!(live[0], 255);
    assert_eq!(info(e, doc)["history_labels"].as_array().unwrap().len(), 1);
    dispatch(e, doc, "preview.commit", &json!({}));
    // Any synchronous call waits for queued operations.
    let _ = op_doc_mark_saved(e, doc, u64::MAX);
    let labels = info(e, doc)["history_labels"].clone();
    assert_eq!(labels.as_array().unwrap().len(), 2);
    assert_eq!(labels[1], "Magic Wand");
    assert_eq!(query(e, doc, "doc.selection_mask", &json!({})), live);
    op_engine_free(e);
}
