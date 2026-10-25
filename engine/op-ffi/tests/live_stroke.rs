//! M38 live strokes through the C interface: begin, append, end and cancel, the
//! history step, the partial tile uploads of an attached view and the frame
//! stamps that name the preview each frame shows.
//! not in a window.
#![cfg(target_os = "macos")]
#![allow(unexpected_cfgs)] // objc 0.2's msg_send! expands to cfg(feature = "cargo-clippy").

use objc::runtime::{Class, Object};
use objc::{msg_send, sel, sel_impl};
use op_ffi::*;
use std::ffi::c_void;
use std::ptr;

fn gpu_tests() -> bool {
    if std::env::var("OP_GPU_TESTS").as_deref() == Ok("0") {
        println!("SKIP OP_GPU_TESTS=0");
        return false;
    }
    true
}

/// A CAMetalLayer that is not in a window.
struct Layer(*mut Object);
unsafe impl Send for Layer {}
impl Layer {
    fn new(class: &str) -> Self {
        let class = Class::get(class).expect("QuartzCore class");
        let obj: *mut Object = unsafe { msg_send![class, new] };
        assert!(!obj.is_null());
        Self(obj)
    }
    fn metal() -> Self {
        Self::new("CAMetalLayer")
    }
    fn ptr(&self) -> *mut c_void {
        self.0.cast()
    }
}
impl Drop for Layer {
    fn drop(&mut self) {
        unsafe {
            let () = msg_send![self.0, release];
        }
    }
}

fn engine() -> *mut OpEngine {
    let e = op_engine_new(ptr::null(), None, ptr::null_mut());
    assert!(!e.is_null());
    e
}

fn doc(e: *mut OpEngine, w: u32, h: u32) -> u64 {
    let px: Vec<u8> = (0..w * h)
        .flat_map(|i| {
            [
                (i % w) as u8,
                (i / w) as u8,
                128,
                if i % 7 == 0 { 0 } else { 255 },
            ]
        })
        .collect();
    let mut id = 0;
    assert_eq!(
        op_import_pixels(e, w, h, px.as_ptr(), px.len(), ptr::null(), 0, &mut id),
        OpStatus::OP_OK
    );
    id
}

fn srgb_icc() -> Vec<u8> {
    op_color::Profile::builtin("srgb").unwrap().bytes().to_vec()
}

fn attach(e: *mut OpEngine, doc: u64, layer: &Layer) -> u64 {
    let mut v = 0;
    assert_eq!(op_view_attach(e, doc, layer.ptr(), &mut v), OpStatus::OP_OK);
    assert!(v != 0);
    v
}

fn info(e: *mut OpEngine, v: u64) -> OpViewInfo {
    let mut i = OpViewInfo::default();
    assert_eq!(op_view_info(e, v, &mut i), OpStatus::OP_OK);
    i
}

fn query_json(e: *mut OpEngine, doc: u64, id: &str, params: &str) -> serde_json::Value {
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
    serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null)
}

fn pixels(e: *mut OpEngine, doc: u64, preview: bool) -> Vec<u8> {
    let mut out = OpBuffer {
        ptr: ptr::null_mut(),
        len: 0,
    };
    let params = format!("{{\"preview\":{preview}}}");
    let q = std::ffi::CString::new("doc.pixels").unwrap();
    assert_eq!(
        op_query(e, doc, q.as_ptr(), params.as_ptr(), params.len(), &mut out),
        OpStatus::OP_OK
    );
    let bytes = unsafe { std::slice::from_raw_parts(out.ptr, out.len) }.to_vec();
    op_buffer_free(&mut out);
    bytes
}

fn begin(e: *mut OpEngine, doc: u64, json: &str) -> OpStatus {
    op_stroke_begin(e, doc, json.as_ptr(), json.len())
}

fn line(y: f64, n: usize) -> Vec<OpStrokeSample> {
    (0..n)
        .map(|i| OpStrokeSample {
            x: 10.0 + i as f64 * 4.0,
            y,
            pressure: 1.0,
            tilt_x: 0.0,
            tilt_y: 0.0,
            time_ms: i as u64,
        })
        .collect()
}

#[test]
fn begin_append_end_and_cancel() {
    let e = engine();
    let d = doc(e, 300, 200);
    let original = pixels(e, d, false);
    let brush = r#"{"tool":"paintbrush","width":9,"color":"ff0000ff","hardness":0.5}"#;
    assert_eq!(begin(e, d, brush), OpStatus::OP_OK);
    assert_eq!(
        begin(e, d, brush),
        OpStatus::OP_ERR_BUSY,
        "one stroke at a time"
    );
    let s = line(50.0, 30);
    let mut serial = 0;
    assert_eq!(
        op_stroke_append(e, d, s.as_ptr(), 10, &mut serial),
        OpStatus::OP_OK
    );
    assert!(serial > 0);
    let mut later = 0;
    assert_eq!(
        op_stroke_append(e, d, s[10..].as_ptr(), 20, &mut later),
        OpStatus::OP_OK
    );
    assert!(later > serial, "each append shows a new preview");
    assert_ne!(pixels(e, d, true), original);
    assert_eq!(pixels(e, d, false), original);
    // A bad sample is refused and changes nothing.
    let bad = [OpStrokeSample {
        pressure: 2.0,
        ..s[29]
    }];
    let shown = pixels(e, d, true);
    assert_eq!(
        op_stroke_append(e, d, bad.as_ptr(), 1, ptr::null_mut()),
        OpStatus::OP_ERR_BAD_ARGUMENT
    );
    assert_eq!(pixels(e, d, true), shown);
    assert_eq!(op_stroke_cancel(e, d), OpStatus::OP_OK);
    assert_eq!(pixels(e, d, true), original);
    let info = query_json(e, d, "doc.info", "{}");
    assert_eq!(info["history_labels"], serde_json::json!(["Open"]));
    assert_eq!(op_stroke_cancel(e, d), OpStatus::OP_ERR_BAD_ARGUMENT);
    assert_eq!(op_stroke_end(e, d), OpStatus::OP_ERR_BAD_ARGUMENT);

    assert_eq!(
        begin(e, d, r#"{"tool":"pencil","color":"00ff00ff"}"#),
        OpStatus::OP_OK
    );
    for chunk in s.chunks(3) {
        assert_eq!(
            op_stroke_append(e, d, chunk.as_ptr(), chunk.len(), ptr::null_mut()),
            OpStatus::OP_OK
        );
    }
    assert_eq!(op_stroke_end(e, d), OpStatus::OP_OK);
    let info = query_json(e, d, "doc.info", "{}");
    assert_eq!(
        info["history_labels"],
        serde_json::json!(["Open", "Pencil"])
    );
    assert_eq!(info["dirty"], serde_json::json!(true));
    let after = pixels(e, d, false);
    let at = ((50 * 300 + 50) * 4) as usize;
    assert_eq!(&after[at..at + 4], &[0, 255, 0, 255]);
    // Bad options and null pointers.
    assert_eq!(
        begin(e, d, r#"{"tool":"clone"}"#),
        OpStatus::OP_ERR_BAD_ARGUMENT
    );
    assert_eq!(
        begin(e, d, r#"{"width":-1}"#),
        OpStatus::OP_ERR_BAD_ARGUMENT
    );
    assert_eq!(begin(e, 999, "{}"), OpStatus::OP_ERR_NO_SUCH_DOC);
    assert_eq!(
        op_stroke_append(e, d, ptr::null(), 3, ptr::null_mut()),
        OpStatus::OP_ERR_BAD_ARGUMENT
    );
    op_engine_free(e);
}

#[test]
fn an_attached_view_uploads_only_stroked_tiles_and_stamps_each_frame() {
    if !gpu_tests() {
        return;
    }
    let e = engine();
    let d = doc(e, 1200, 900);
    let layer = Layer::metal();
    let v = attach(e, d, &layer);
    let icc = srgb_icc();
    let st = OpViewState {
        width_pt: 300.0,
        height_pt: 200.0,
        scale: 1.0,
        zoom: 0.25,
        origin_x: 0.0,
        origin_y: 0.0,
        display_icc: icc.as_ptr(),
        display_icc_len: icc.len(),
        flags: 0,
    };
    assert_eq!(op_view_set(e, v, &st), OpStatus::OP_OK);
    assert_eq!(
        op_view_wait_frames(e, v, 1, 20_000, ptr::null_mut()),
        OpStatus::OP_OK
    );
    let first = info(e, v);
    assert_eq!(first.doc_uploads, 1);
    assert_eq!(
        begin(e, d, r#"{"width":6,"color":"0000ffff"}"#),
        OpStatus::OP_OK
    );
    let s = line(100.0, 10); // x 10..46, y 100: tile (0, 0) only
    let mut serial = 0;
    let before = std::time::Instant::now();
    assert_eq!(
        op_stroke_append(e, d, s.as_ptr(), s.len(), &mut serial),
        OpStatus::OP_OK
    );
    // The append wakes the render thread: no 50 ms idle wait.
    let mut stamps = Vec::new();
    while before.elapsed().as_secs() < 20 {
        let mut buf = [OpFrameStamp::default(); 64];
        let mut n = 0;
        assert_eq!(
            op_view_take_frame_stamps(e, v, buf.as_mut_ptr(), buf.len(), &mut n),
            OpStatus::OP_OK
        );
        stamps.extend_from_slice(&buf[..n]);
        if stamps.iter().any(|s| s.preview_serial >= serial) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    let shown = stamps
        .iter()
        .find(|s| s.preview_serial >= serial)
        .expect("a frame shows the stroke");
    assert!(shown.time_ns > 0);
    let after = info(e, v);
    assert_eq!(after.doc_uploads, 1, "no full upload for a stroke");
    assert!(after.region_uploads >= 1);
    assert_eq!(
        after.region_tiles, after.region_uploads,
        "one tile per change"
    );
    assert_eq!(after.frame_errors, 0);
    // Commit: the same tiles, so the next frames upload nothing new.
    assert_eq!(op_stroke_end(e, d), OpStatus::OP_OK);
    let frames = after.frames;
    assert_eq!(
        op_view_wait_frames(e, v, frames + 1, 20_000, ptr::null_mut()),
        OpStatus::OP_OK
    );
    let done = info(e, v);
    assert_eq!(done.doc_uploads, 1);
    assert_eq!(done.region_tiles, after.region_tiles);
    // Null and oversized buffers are refused.
    let mut n = 0;
    assert_eq!(
        op_view_take_frame_stamps(e, v, ptr::null_mut(), 4, &mut n),
        OpStatus::OP_ERR_BAD_ARGUMENT
    );
    assert_eq!(op_view_detach(e, v), OpStatus::OP_OK);
    op_engine_free(e);
}
