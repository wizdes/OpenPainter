//! M53 through the C interface: `layer.pixels` (op_query), then
//! `selection.from_mask` and `layer.remove_background` (op_dispatch) with a
//! base64 mask, one history step each.
use op_ffi::*;
use serde_json::{Value, json};
use std::ffi::CString;
use std::ptr;

fn query(e: *mut OpEngine, doc: u64, id: &str, params: &Value) -> Vec<u8> {
    let mut out = OpBuffer {
        ptr: ptr::null_mut(),
        len: 0,
    };
    let q = CString::new(id).unwrap();
    let body = params.to_string();
    assert_eq!(
        op_query(e, doc, q.as_ptr(), body.as_ptr(), body.len(), &mut out),
        OpStatus::OP_OK,
        "{id}"
    );
    let v = unsafe { std::slice::from_raw_parts(out.ptr, out.len) }.to_vec();
    op_buffer_free(&mut out);
    v
}

fn dispatch(e: *mut OpEngine, doc: u64, id: &str, params: &Value) -> OpStatus {
    let mut ticket = 0;
    let c = CString::new(id).unwrap();
    let body = params.to_string();
    op_dispatch(e, doc, c.as_ptr(), body.as_ptr(), body.len(), &mut ticket)
}

fn labels(e: *mut OpEngine, doc: u64) -> Vec<Value> {
    // Any synchronous call waits for queued operations.
    let _ = op_doc_mark_saved(e, doc, u64::MAX);
    let info: Value = serde_json::from_slice(&query(e, doc, "doc.info", &json!({}))).unwrap();
    info["history_labels"].as_array().unwrap().clone()
}

#[test]
fn remove_background_through_the_c_interface() {
    let e = op_engine_new(ptr::null(), None, ptr::null_mut());
    // Four pixels: alpha 255, 200, 100, 255.
    let px = [
        10, 20, 30, 255, 40, 50, 60, 200, 70, 80, 90, 100, 1, 2, 3, 255,
    ];
    let mut doc = 0;
    assert_eq!(
        op_import_pixels(e, 4, 1, px.as_ptr(), px.len(), ptr::null(), 0, &mut doc),
        OpStatus::OP_OK
    );
    assert_eq!(query(e, doc, "layer.pixels", &json!({})), px);

    // Mask bytes 0, 128, 255, 1 are "AID/AQ==" in base64.
    assert_eq!(
        dispatch(e, doc, "selection.from_mask", &json!({"mask": "AID/AQ=="})),
        OpStatus::OP_OK
    );
    assert_eq!(labels(e, doc).last().unwrap(), "Select Foreground");
    assert_eq!(
        query(e, doc, "doc.selection_mask", &json!({})),
        [0, 128, 255, 1]
    );

    assert_eq!(
        dispatch(
            e,
            doc,
            "layer.remove_background",
            &json!({"mask": "AID/AQ=="})
        ),
        OpStatus::OP_OK
    );
    let l = labels(e, doc);
    assert_eq!(l.len(), 3);
    assert_eq!(l.last().unwrap(), "Remove Background");
    // 255*0 -> 0 (transparent black); 200*128/255 = 100.39 -> 100;
    // 100*255/255 = 100; 255*1/255 = 1.
    assert_eq!(
        query(e, doc, "layer.pixels", &json!({})),
        [0, 0, 0, 0, 40, 50, 60, 100, 70, 80, 90, 100, 1, 2, 3, 1]
    );

    // A bad mask is refused at once and adds nothing.
    assert_eq!(
        dispatch(e, doc, "layer.remove_background", &json!({"mask": 7})),
        OpStatus::OP_ERR_BAD_ARGUMENT
    );
    assert_eq!(labels(e, doc).len(), 3);
    op_engine_free(e);
}
