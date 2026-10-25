//! M33 moves through the C interface: begin, many updates, finish (one
//! history step), cancel (nothing changes), the copy flag and bad arguments.
use op_ffi::*;
use std::ptr;

fn engine() -> *mut OpEngine {
    let e = op_engine_new(ptr::null(), None, ptr::null_mut());
    assert!(!e.is_null());
    e
}

fn doc(e: *mut OpEngine, w: u32, h: u32) -> (u64, Vec<u8>) {
    let px: Vec<u8> = (0..w * h)
        .flat_map(|i| [(i % w) as u8, (i / w) as u8, 128, 255])
        .collect();
    let mut id = 0;
    assert_eq!(
        op_import_pixels(e, w, h, px.as_ptr(), px.len(), ptr::null(), 0, &mut id),
        OpStatus::OP_OK
    );
    (id, px)
}

fn query(e: *mut OpEngine, doc: u64, id: &str, params: &str) -> Vec<u8> {
    let mut out = OpBuffer {
        ptr: ptr::null_mut(),
        len: 0,
    };
    let q = std::ffi::CString::new(id).unwrap();
    assert_eq!(
        op_query(e, doc, q.as_ptr(), params.as_ptr(), params.len(), &mut out),
        OpStatus::OP_OK
    );
    let bytes = unsafe { std::slice::from_raw_parts(out.ptr, out.len) }.to_vec();
    op_buffer_free(&mut out);
    bytes
}

fn labels(e: *mut OpEngine, doc: u64) -> Vec<String> {
    let v: serde_json::Value = serde_json::from_slice(&query(e, doc, "doc.info", "{}")).unwrap();
    v["history_labels"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s.as_str().unwrap().to_string())
        .collect()
}

fn begin(e: *mut OpEngine, doc: u64, json: &str) -> OpStatus {
    op_move_begin(e, doc, json.as_ptr(), json.len())
}

fn update(e: *mut OpEngine, doc: u64, json: &str) -> OpStatus {
    op_move_update(e, doc, json.as_ptr(), json.len(), ptr::null_mut())
}

#[test]
fn many_updates_commit_one_step() {
    let e = engine();
    let (d, px) = doc(e, 16, 8);
    assert_eq!(begin(e, d, r#"{"kind":"pixels"}"#), OpStatus::OP_OK);
    let mut serial = 0;
    for i in 0..100 {
        let j = format!(
            r#"{{"transform":[1,0,0,1,{},0],"resampling":"nearest"}}"#,
            i % 4
        );
        assert_eq!(
            op_move_update(e, d, j.as_ptr(), j.len(), &mut serial),
            OpStatus::OP_OK
        );
    }
    assert!(serial > 0);
    let preview = query(e, d, "doc.pixels", r#"{"preview":true}"#);
    assert_eq!(query(e, d, "doc.pixels", "{}"), px, "committed bytes wait");
    assert_eq!(op_move_finish(e, d), OpStatus::OP_OK);
    assert_eq!(labels(e, d), ["Open", "Move Selected Pixels"]);
    assert_eq!(query(e, d, "doc.pixels", "{}"), preview);
    // Moved 3 px right: column 3 holds column 0, columns 0..3 are clear.
    let after = query(e, d, "doc.pixels", "{}");
    assert_eq!(&after[3 * 4..3 * 4 + 4], &px[0..4]);
    assert_eq!(&after[0..4], &[0, 0, 0, 0]);
    assert_eq!(op_move_finish(e, d), OpStatus::OP_ERR_BAD_ARGUMENT);
    op_engine_free(e);
}

#[test]
fn cancel_leaves_everything() {
    let e = engine();
    let (d, px) = doc(e, 8, 8);
    assert_eq!(
        begin(
            e,
            d,
            r#"{"kind":"pixels","transform":[0,1,-1,0,8,0],"resampling":"bilinear","copy":true}"#
        ),
        OpStatus::OP_OK
    );
    assert_ne!(query(e, d, "doc.pixels", r#"{"preview":true}"#), px);
    assert_eq!(begin(e, d, r#"{"kind":"pixels"}"#), OpStatus::OP_ERR_BUSY);
    assert_eq!(op_move_cancel(e, d), OpStatus::OP_OK);
    assert_eq!(query(e, d, "doc.pixels", "{}"), px);
    assert_eq!(query(e, d, "doc.pixels", r#"{"preview":true}"#), px);
    assert_eq!(labels(e, d), ["Open"]);
    assert_eq!(op_move_cancel(e, d), OpStatus::OP_ERR_BAD_ARGUMENT);
    op_engine_free(e);
}

#[test]
fn selection_move_changes_no_pixels() {
    let e = engine();
    let (d, px) = doc(e, 8, 8);
    assert_eq!(
        begin(e, d, r#"{"kind":"selection","transform":[1,0,0,1,2,1]}"#),
        OpStatus::OP_OK
    );
    assert_eq!(op_move_finish(e, d), OpStatus::OP_OK);
    assert_eq!(labels(e, d), ["Open", "Move Selection"]);
    assert_eq!(query(e, d, "doc.pixels", "{}"), px);
    op_engine_free(e);
}

#[test]
fn bad_arguments() {
    let e = engine();
    let (d, _) = doc(e, 8, 8);
    for j in [
        r#"{}"#,
        r#"{"kind":"layer"}"#,
        r#"{"kind":"pixels","transform":[1,0,0,1]}"#,
        r#"{"kind":"pixels","transform":[0,0,0,0,0,0]}"#,
        r#"{"kind":"pixels","resampling":"sharpest"}"#,
        r#"{"kind":"pixels","copy":1}"#,
        "not json",
    ] {
        assert_eq!(begin(e, d, j), OpStatus::OP_ERR_BAD_ARGUMENT, "{j}");
    }
    assert_eq!(
        update(e, d, r#"{"transform":[1,0,0,1,1,0]}"#),
        OpStatus::OP_ERR_BAD_ARGUMENT,
        "no session"
    );
    assert_eq!(begin(e, d, r#"{"kind":"pixels"}"#), OpStatus::OP_OK);
    assert_eq!(
        update(e, d, r#"{"transform":[0,0,0,0,0,0]}"#),
        OpStatus::OP_ERR_BAD_ARGUMENT
    );
    assert_eq!(
        begin(e, 999, r#"{"kind":"pixels"}"#),
        OpStatus::OP_ERR_NO_SUCH_DOC
    );
    assert_eq!(op_move_cancel(e, d), OpStatus::OP_OK);
    op_engine_free(e);
}
