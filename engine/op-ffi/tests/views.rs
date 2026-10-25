//! Canvas view functions (M12): null pointers, bad states, stale ids, detach while
//! a frame renders, document close and engine free with live views, and leaks
//! over 100 attach/detach cycles. The views draw into real CAMetalLayers that are
//! not in a window.
#![cfg(target_os = "macos")]
#![allow(unexpected_cfgs)] // objc 0.2's msg_send! expands to cfg(feature = "cargo-clippy").

use objc::runtime::{Class, Object};
use objc::{msg_send, sel, sel_impl};
use op_ffi::*;
use std::ffi::c_void;
use std::ptr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

fn gpu_tests() -> bool {
    if std::env::var("OP_GPU_TESTS").as_deref() == Ok("0") {
        println!("SKIP OP_GPU_TESTS=0");
        return false;
    }
    true
}

/// A layer and its retain count right after creation.
struct Layer(*mut Object, usize);
unsafe impl Send for Layer {}
impl Layer {
    fn new(class: &str) -> Self {
        let class = Class::get(class).expect("QuartzCore class");
        let obj: *mut Object = unsafe { msg_send![class, new] };
        assert!(!obj.is_null());
        // Creating a layer opens an implicit transaction that retains it; commit
        // it so the baseline is the caller's own reference.
        let tx = Class::get("CATransaction").unwrap();
        let () = unsafe { msg_send![tx, flush] };
        let base: usize = unsafe { msg_send![obj, retainCount] };
        Self(obj, base)
    }
    fn metal() -> Self {
        Self::new("CAMetalLayer")
    }
    fn ptr(&self) -> *mut c_void {
        self.0.cast()
    }
    /// Retains beyond the creation baseline.
    fn extra_retains(&self) -> isize {
        let n: usize = unsafe { msg_send![self.0, retainCount] };
        n as isize - self.1 as isize
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

fn p3_icc() -> Vec<u8> {
    op_color::Profile::builtin("display-p3")
        .unwrap()
        .bytes()
        .to_vec()
}

fn state(zoom: f64, icc: &[u8]) -> OpViewState {
    OpViewState {
        width_pt: 64.0,
        height_pt: 48.0,
        scale: 2.0,
        zoom,
        origin_x: 3.5,
        origin_y: -2.0,
        display_icc: if icc.is_empty() {
            ptr::null()
        } else {
            icc.as_ptr()
        },
        display_icc_len: icc.len(),
        flags: 0,
    }
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

fn counters(e: *mut OpEngine) -> OpViewCounters {
    let mut c = OpViewCounters::default();
    assert_eq!(op_view_counters(e, &mut c), OpStatus::OP_OK);
    c
}

fn wait(e: *mut OpEngine, v: u64, n: u64) -> u64 {
    let mut frames = 0;
    assert_eq!(
        op_view_wait_frames(e, v, n, 10_000, &mut frames),
        OpStatus::OP_OK,
        "waiting for {n} frames"
    );
    frames
}

#[test]
fn null_pointers_and_bad_states() {
    let e = engine();
    let d = doc(e, 8, 8);
    let layer = Layer::metal();
    let mut v = 0u64;
    let s = state(1.0, &[]);
    assert_eq!(
        op_view_attach(ptr::null_mut(), d, layer.ptr(), &mut v),
        OpStatus::OP_ERR_BAD_ARGUMENT
    );
    assert_eq!(
        op_view_attach(e, d, ptr::null_mut(), &mut v),
        OpStatus::OP_ERR_BAD_ARGUMENT
    );
    assert_eq!(
        op_view_attach(e, d, layer.ptr(), ptr::null_mut()),
        OpStatus::OP_ERR_BAD_ARGUMENT
    );
    assert_eq!(
        op_view_set(ptr::null_mut(), 1, &s),
        OpStatus::OP_ERR_BAD_ARGUMENT
    );
    assert_eq!(
        op_view_set(e, 1, ptr::null()),
        OpStatus::OP_ERR_BAD_ARGUMENT
    );
    assert_eq!(
        op_view_detach(ptr::null_mut(), 1),
        OpStatus::OP_ERR_BAD_ARGUMENT
    );
    assert_eq!(
        op_view_info(e, 1, ptr::null_mut()),
        OpStatus::OP_ERR_BAD_ARGUMENT
    );
    assert_eq!(
        op_view_counters(e, ptr::null_mut()),
        OpStatus::OP_ERR_BAD_ARGUMENT
    );
    let mut n = 0usize;
    assert_eq!(
        op_view_take_frame_times(e, 1, ptr::null_mut(), 4, &mut n),
        OpStatus::OP_ERR_BAD_ARGUMENT
    );
    // Before any attach there is no view host: zero counters, stale ids.
    assert_eq!(counters(e), OpViewCounters::default());
    assert_eq!(op_view_set(e, 1, &s), OpStatus::OP_ERR_NO_SUCH_DOC);
    if !gpu_tests() {
        op_engine_free(e);
        return;
    }
    // Not a CAMetalLayer.
    let plain = Layer::new("CALayer");
    assert_eq!(
        op_view_attach(e, d, plain.ptr(), &mut v),
        OpStatus::OP_ERR_BAD_ARGUMENT
    );
    assert_eq!(plain.extra_retains(), 0, "a refused layer is not retained");
    let v = attach(e, d, &layer);
    let good = state(1.0, &[]);
    let bad: Vec<(&str, OpViewState)> = vec![
        (
            "zero width",
            OpViewState {
                width_pt: 0.0,
                ..good
            },
        ),
        (
            "nan zoom",
            OpViewState {
                zoom: f64::NAN,
                ..good
            },
        ),
        ("zoom too big", OpViewState { zoom: 65.0, ..good }),
        ("zoom zero", OpViewState { zoom: 0.0, ..good }),
        (
            "bad scale",
            OpViewState {
                scale: -1.0,
                ..good
            },
        ),
        (
            "huge drawable",
            OpViewState {
                width_pt: 9000.0,
                ..good
            },
        ),
        (
            "inf origin",
            OpViewState {
                origin_x: f64::INFINITY,
                ..good
            },
        ),
        ("unknown flag", OpViewState { flags: 4, ..good }),
        (
            "icc null with length",
            OpViewState {
                display_icc: ptr::null(),
                display_icc_len: 10,
                ..good
            },
        ),
    ];
    for (what, s) in &bad {
        assert_eq!(
            op_view_set(e, v, s),
            OpStatus::OP_ERR_BAD_ARGUMENT,
            "{what}"
        );
    }
    let junk = vec![7u8; 200];
    assert_eq!(
        op_view_set(e, v, &state(1.0, &junk)),
        OpStatus::OP_ERR_BAD_ARGUMENT,
        "junk ICC"
    );
    // Refused states change nothing.
    let i = info(e, v);
    assert_eq!(i.width_pt, 0.0);
    assert_eq!(i.frames, 0);
    assert_eq!(op_view_detach(e, v), OpStatus::OP_OK);
    op_engine_free(e);
}

#[test]
fn state_round_trip_frames_color_tag_mips_and_grid() {
    if !gpu_tests() {
        return;
    }
    let e = engine();
    let d = doc(e, 300, 200);
    let layer = Layer::metal();
    let v = attach(e, d, &layer);
    let srgb = srgb_icc();
    let s = state(1.5, &srgb);
    assert_eq!(op_view_set(e, v, &s), OpStatus::OP_OK);
    wait(e, v, 1);
    let i = info(e, v);
    assert_eq!(i.doc, d);
    assert_eq!(
        (
            i.width_pt,
            i.height_pt,
            i.scale,
            i.zoom,
            i.origin_x,
            i.origin_y,
            i.flags
        ),
        (64.0, 48.0, 2.0, 1.5, 3.5, -2.0, 0)
    );
    assert_eq!((i.drawable_width, i.drawable_height), (128, 96));
    assert_eq!(i.display_icc_len, srgb.len() as u64);
    assert_eq!(i.layer_tag_is_display, 1, "layer tagged with the display");
    assert_eq!(i.has_display_table, 1);
    assert_eq!(i.table_builds, 1);
    assert_eq!(i.doc_uploads, 1);
    assert_eq!(i.tag_changed_by_configure, 0);
    assert_eq!(i.frame_errors, 0, "last error {}", i.last_error);
    assert_eq!(i.mip_builds, 0, "no mips at 150%");
    // A new display profile rebuilds the table and retags the layer.
    let p3 = p3_icc();
    let before = i.frames;
    assert_eq!(op_view_set(e, v, &state(1.5, &p3)), OpStatus::OP_OK);
    wait(e, v, before + 1);
    let i = info(e, v);
    assert_eq!(i.table_builds, 2);
    assert_eq!(i.layer_tag_is_display, 1);
    assert_eq!(i.display_icc_len, p3.len() as u64);
    // Length 0 keeps the profile; zoom 25% builds mips once.
    for (k, zoom) in [0.25, 0.3, 0.1].into_iter().enumerate() {
        let n = info(e, v).frames;
        assert_eq!(op_view_set(e, v, &state(zoom, &[])), OpStatus::OP_OK);
        wait(e, v, n + 1);
        let i = info(e, v);
        assert_eq!(i.mip_builds, 1, "step {k}");
        assert_eq!(i.table_builds, 2, "step {k}");
        assert_eq!(i.zoom, zoom);
    }
    // Grid flag round trip at 800%.
    let n = info(e, v).frames;
    let grid = OpViewState {
        flags: OP_VIEW_PIXEL_GRID,
        ..state(8.0, &[])
    };
    assert_eq!(op_view_set(e, v, &grid), OpStatus::OP_OK);
    wait(e, v, n + 1);
    assert_eq!(info(e, v).flags, OP_VIEW_PIXEL_GRID);
    // A document edit is picked up without a new op_view_set.
    let n = info(e, v).frames;
    let c = std::ffi::CString::new("layer.add").unwrap();
    let mut ticket = 0;
    assert_eq!(
        op_dispatch(e, d, c.as_ptr(), ptr::null(), 0, &mut ticket),
        OpStatus::OP_OK
    );
    wait(e, v, n + 1);
    assert_eq!(info(e, v).doc_uploads, 2);
    // Frame times: increasing, one per frame.
    let mut times = vec![0f64; 64];
    let mut len = 0usize;
    assert_eq!(
        op_view_take_frame_times(e, v, times.as_mut_ptr(), times.len(), &mut len),
        OpStatus::OP_OK
    );
    assert_eq!(len as u64, info(e, v).frames);
    assert!(times[..len].windows(2).all(|w| w[1] >= w[0]));
    assert_eq!(
        op_view_take_frame_times(e, v, times.as_mut_ptr(), times.len(), &mut len),
        OpStatus::OP_OK
    );
    assert_eq!(len, 0, "times are moved out");
    // Suspend drops the surface, resume brings it back.
    let n = info(e, v).frames;
    let sleep = OpViewState {
        flags: OP_VIEW_SUSPENDED,
        ..state(1.0, &[])
    };
    assert_eq!(op_view_set(e, v, &sleep), OpStatus::OP_OK);
    let mut frames = 0;
    assert_eq!(
        op_view_wait_frames(e, v, n + 1, 300, &mut frames),
        OpStatus::OP_ERR_BUSY,
        "no frame while suspended"
    );
    assert_eq!(info(e, v).suspended, 1);
    assert_eq!(counters(e).live_surfaces, 0);
    assert_eq!(op_view_set(e, v, &state(1.0, &[])), OpStatus::OP_OK);
    wait(e, v, n + 1);
    assert_eq!(info(e, v).suspended, 0);
    assert_eq!(counters(e).live_surfaces, 1);
    assert_eq!(counters(e).validation_errors, 0);
    assert_eq!(op_view_detach(e, v), OpStatus::OP_OK);
    op_engine_free(e);
}

/// M20: a preview session (Layer Properties) shows on the canvas, and Cancel
/// shows the document again, with no op_view_set in between.
#[test]
fn preview_pixels_show_and_cancel_restores() {
    if !gpu_tests() {
        return;
    }
    let e = engine();
    let d = doc(e, 64, 48);
    let layer = Layer::metal();
    let v = attach(e, d, &layer);
    let srgb = srgb_icc();
    assert_eq!(op_view_set(e, v, &state(1.0, &srgb)), OpStatus::OP_OK);
    wait(e, v, 1);
    assert_eq!(info(e, v).doc_uploads, 1);
    let send = |cmd: &str, json: &str| {
        let c = std::ffi::CString::new(cmd).unwrap();
        let mut ticket = 0;
        assert_eq!(
            op_dispatch(e, d, c.as_ptr(), json.as_ptr(), json.len(), &mut ticket),
            OpStatus::OP_OK
        );
    };
    let uploads = |n: u64| {
        let end = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while info(e, v).doc_uploads < n && std::time::Instant::now() < end {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        info(e, v).doc_uploads
    };
    send(
        "preview.begin",
        r#"{"command":"layer.properties","params":{"index":0,"name":"B","visible":true,"blend":"normal","opacity":10}}"#,
    );
    assert_eq!(uploads(2), 2, "the preview was uploaded");
    send("preview.cancel", "{}");
    assert_eq!(uploads(3), 3, "the plain document came back");
    assert_eq!(info(e, v).frame_errors, 0);
    assert_eq!(op_view_detach(e, v), OpStatus::OP_OK);
    op_engine_free(e);
}

#[test]
fn stale_ids_and_document_close() {
    if !gpu_tests() {
        return;
    }
    let e = engine();
    let d = doc(e, 16, 16);
    let layer = Layer::metal();
    let mut out = 0;
    assert_eq!(
        op_view_attach(e, 999, layer.ptr(), &mut out),
        OpStatus::OP_ERR_NO_SUCH_DOC
    );
    let v = attach(e, d, &layer);
    assert!(layer.extra_retains() > 0, "the engine retains the layer");
    assert_eq!(op_view_detach(e, v + 100), OpStatus::OP_ERR_NO_SUCH_DOC);
    assert_eq!(op_view_detach(e, v), OpStatus::OP_OK);
    assert_eq!(layer.extra_retains(), 0, "detach releases the layer");
    let s = state(1.0, &[]);
    let mut i = OpViewInfo::default();
    let mut n = 0usize;
    let mut t = [0f64; 2];
    assert_eq!(op_view_detach(e, v), OpStatus::OP_ERR_NO_SUCH_DOC);
    assert_eq!(op_view_set(e, v, &s), OpStatus::OP_ERR_NO_SUCH_DOC);
    assert_eq!(op_view_info(e, v, &mut i), OpStatus::OP_ERR_NO_SUCH_DOC);
    assert_eq!(
        op_view_take_frame_times(e, v, t.as_mut_ptr(), 2, &mut n),
        OpStatus::OP_ERR_NO_SUCH_DOC
    );
    assert_eq!(
        op_view_wait_frames(e, v, 1, 10, ptr::null_mut()),
        OpStatus::OP_ERR_NO_SUCH_DOC
    );
    // Closing a document detaches its views and releases their layers.
    let v2 = attach(e, d, &layer);
    assert_eq!(
        op_view_set(e, v2, &state(1.0, &srgb_icc())),
        OpStatus::OP_OK
    );
    wait(e, v2, 1);
    assert_eq!(op_doc_close(e, d), OpStatus::OP_OK);
    assert_eq!(layer.extra_retains(), 0);
    assert_eq!(op_view_set(e, v2, &s), OpStatus::OP_ERR_NO_SUCH_DOC);
    assert_eq!(op_view_detach(e, v2), OpStatus::OP_ERR_NO_SUCH_DOC);
    let c = counters(e);
    assert_eq!((c.live_views, c.live_layers, c.live_surfaces), (0, 0, 0));
    assert_eq!(
        op_view_attach(e, d, layer.ptr(), &mut out),
        OpStatus::OP_ERR_NO_SUCH_DOC
    );
    // Freeing the engine with an attached, rendering view releases it too.
    let d2 = doc(e, 16, 16);
    let v3 = attach(e, d2, &layer);
    assert_eq!(
        op_view_set(e, v3, &state(1.0, &srgb_icc())),
        OpStatus::OP_OK
    );
    wait(e, v3, 1);
    op_engine_free(e);
    assert_eq!(layer.extra_retains(), 0, "engine free releases the layer");
}

#[test]
fn detach_during_render() {
    if !gpu_tests() {
        return;
    }
    let e = engine();
    let d = doc(e, 1024, 768);
    let srgb = srgb_icc();
    for round in 0..10 {
        let layer = Layer::metal();
        let v = attach(e, d, &layer);
        assert_eq!(op_view_set(e, v, &state(1.0, &srgb)), OpStatus::OP_OK);
        wait(e, v, 1);
        let stop = Arc::new(AtomicBool::new(false));
        let sets = Arc::new(AtomicU64::new(0));
        let ep = e as usize;
        let spammer = {
            let (stop, sets) = (stop.clone(), sets.clone());
            std::thread::spawn(move || {
                let e = ep as *mut OpEngine;
                let mut k = 0u64;
                let mut last = OpStatus::OP_OK;
                while !stop.load(Ordering::SeqCst) {
                    k += 1;
                    let zoom = [0.2, 0.7, 1.0, 3.0][(k % 4) as usize];
                    last = op_view_set(e, v, &state(zoom, &[]));
                    if last != OpStatus::OP_OK {
                        break;
                    }
                    sets.fetch_add(1, Ordering::SeqCst);
                }
                last
            })
        };
        let n = info(e, v).frames;
        wait(e, v, n + 3);
        // Detach while the render thread is busy with this view.
        assert_eq!(op_view_detach(e, v), OpStatus::OP_OK, "round {round}");
        assert_eq!(layer.extra_retains(), 0, "round {round}: layer released");
        stop.store(true, Ordering::SeqCst);
        let last = spammer.join().unwrap();
        assert!(
            last == OpStatus::OP_ERR_NO_SUCH_DOC || last == OpStatus::OP_OK,
            "round {round}: {last:?}"
        );
        assert!(sets.load(Ordering::SeqCst) > 0);
    }
    let c = counters(e);
    assert_eq!((c.live_views, c.live_layers, c.live_surfaces), (0, 0, 0));
    assert_eq!(c.validation_errors, 0);
    op_engine_free(e);
}

#[test]
fn hundred_attach_detach_cycles_leave_no_leaks() {
    if !gpu_tests() {
        return;
    }
    let e = engine();
    let d = doc(e, 512, 300);
    let srgb = srgb_icc();
    let cycle = |zoom: f64| {
        let layer = Layer::metal();
        let v = attach(e, d, &layer);
        assert_eq!(op_view_set(e, v, &state(zoom, &srgb)), OpStatus::OP_OK);
        wait(e, v, 1);
        assert_eq!(op_view_detach(e, v), OpStatus::OP_OK);
        assert_eq!(layer.extra_retains(), 0);
    };
    // One warm-up cycle so lazily made device objects exist in the baseline.
    cycle(0.5);
    let base = counters(e);
    assert_eq!(
        (base.live_views, base.live_layers, base.live_surfaces),
        (0, 0, 0)
    );
    // While a view is alive the counters are not empty.
    let layer = Layer::metal();
    let v = attach(e, d, &layer);
    assert_eq!(op_view_set(e, v, &state(0.5, &srgb)), OpStatus::OP_OK);
    wait(e, v, 1);
    let alive = counters(e);
    assert_eq!(
        (alive.live_views, alive.live_layers, alive.live_surfaces),
        (1, 1, 1)
    );
    assert!(
        alive.wgpu_textures > base.wgpu_textures,
        "{alive:?} vs {base:?}"
    );
    assert!(alive.wgpu_buffers > base.wgpu_buffers);
    assert_eq!(op_view_detach(e, v), OpStatus::OP_OK);
    drop(layer);
    for k in 0..100 {
        cycle([0.25, 1.0, 4.0][k % 3]);
    }
    let after = counters(e);
    println!("baseline {base:?}\nafter    {after:?}");
    assert_eq!(after.live_views, base.live_views);
    assert_eq!(after.live_layers, base.live_layers);
    assert_eq!(after.live_surfaces, base.live_surfaces);
    assert_eq!(after.wgpu_textures, base.wgpu_textures);
    assert_eq!(after.wgpu_texture_views, base.wgpu_texture_views);
    assert_eq!(after.wgpu_buffers, base.wgpu_buffers);
    assert_eq!(after.wgpu_bind_groups, base.wgpu_bind_groups);
    assert_eq!(after.validation_errors, 0);
    assert!(after.frames_total >= base.frames_total + 101);
    op_engine_free(e);
}
