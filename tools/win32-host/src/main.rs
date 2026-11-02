#![cfg(windows)]

use op_ffi::*;
use std::ffi::c_void;
use std::ptr;

#[link(name = "user32")]
unsafe extern "system" {
    fn CreateDesktopW(
        name: *const u16,
        device: *const u16,
        devmode: *const c_void,
        flags: u32,
        access: u32,
        security: *const c_void,
    ) -> isize;
    fn SetThreadDesktop(desktop: isize) -> i32;
    fn GetThreadDesktop(thread: u32) -> isize;
    fn CloseDesktop(desktop: isize) -> i32;
    fn CreateWindowExW(
        ex_style: u32,
        class: *const u16,
        name: *const u16,
        style: u32,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        parent: isize,
        menu: isize,
        instance: isize,
        param: *const c_void,
    ) -> isize;
    fn DestroyWindow(hwnd: isize) -> i32;
    fn ShowWindow(hwnd: isize, command: i32) -> i32;
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetCurrentThreadId() -> u32;
}

const DESKTOP_ALL_ACCESS: u32 = 0x000f01ff;
const WS_OVERLAPPEDWINDOW: u32 = 0x00cf0000;

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}
fn check(ok: bool, what: &str) -> Result<(), String> {
    if ok { Ok(()) } else { Err(what.to_owned()) }
}
fn status(got: OpStatus, what: &str) -> Result<(), String> {
    check(got == OpStatus::OP_OK, &format!("{what}: {got:?}"))
}
fn state(width: f64, height: f64, icc: &[u8]) -> OpViewState {
    OpViewState {
        width_pt: width,
        height_pt: height,
        scale: 1.0,
        zoom: 1.0,
        origin_x: 0.0,
        origin_y: 0.0,
        display_icc: if icc.is_empty() {
            ptr::null()
        } else {
            icc.as_ptr()
        },
        display_icc_len: icc.len(),
        flags: 0,
    }
}
fn info(engine: *mut OpEngine, view: u64) -> Result<OpViewInfo, String> {
    let mut out = OpViewInfo::default();
    status(op_view_info(engine, view, &mut out), "op_view_info")?;
    Ok(out)
}
fn wait(engine: *mut OpEngine, view: u64, frames: u64) -> Result<(), String> {
    status(
        op_view_wait_frames(engine, view, frames, 10_000, ptr::null_mut()),
        "op_view_wait_frames",
    )
}
fn attach(engine: *mut OpEngine, doc: u64, hwnd: isize) -> Result<u64, String> {
    let mut view = 0;
    status(
        op_view_attach(engine, doc, hwnd as *mut c_void, &mut view),
        "op_view_attach",
    )?;
    check(view != 0, "op_view_attach returned zero")?;
    Ok(view)
}

fn scenario(name: &str, failures: &mut usize, check: impl FnOnce() -> Result<(), String>) {
    match check() {
        Ok(()) => println!("{name}: PASS"),
        Err(error) => {
            eprintln!("{name}: FAIL: {error}");
            *failures += 1;
        }
    }
}

fn run() -> Result<(), String> {
    // A private desktop keeps every HWND away from the user's visible desktop.
    let desktop_name = wide(&format!("OpenPainter-W3-{}", std::process::id()));
    let desktop = unsafe {
        CreateDesktopW(
            desktop_name.as_ptr(),
            ptr::null(),
            ptr::null(),
            0,
            DESKTOP_ALL_ACCESS,
            ptr::null(),
        )
    };
    check(desktop != 0, "CreateDesktopW failed")?;
    let original_desktop = unsafe { GetThreadDesktop(GetCurrentThreadId()) };
    check(original_desktop != 0, "GetThreadDesktop failed")?;
    if unsafe { SetThreadDesktop(desktop) } == 0 {
        return Err("SetThreadDesktop failed".into());
    }
    let class = wide("STATIC");
    let title = wide("OpenPainter W3 test host");
    let hwnd = unsafe {
        CreateWindowExW(
            0,
            class.as_ptr(),
            title.as_ptr(),
            WS_OVERLAPPEDWINDOW,
            0,
            0,
            256,
            256,
            0,
            0,
            0,
            ptr::null(),
        )
    };
    check(hwnd != 0, "CreateWindowExW failed")?;
    unsafe { ShowWindow(hwnd, 5) };
    let engine = op_engine_new(ptr::null(), None, ptr::null_mut());
    check(!engine.is_null(), "op_engine_new failed")?;
    let pixels = vec![128u8; 64 * 48 * 4];
    let mut doc = 0;
    status(
        op_import_pixels(
            engine,
            64,
            48,
            pixels.as_ptr(),
            pixels.len(),
            ptr::null(),
            0,
            &mut doc,
        ),
        "op_import_pixels",
    )?;

    let mut failures = 0;
    scenario("resize", &mut failures, || {
        let view = attach(engine, doc, hwnd)?;
        let result = (|| {
            status(
                op_view_set(engine, view, &state(64.0, 48.0, &[])),
                "initial set",
            )?;
            wait(engine, view, 1)?;
            let before = info(engine, view)?;
            check(
                (before.drawable_width, before.drawable_height) == (64, 48),
                "initial drawable size",
            )?;
            status(
                op_view_set(engine, view, &state(128.0, 96.0, &[])),
                "larger set",
            )?;
            wait(engine, view, 2)?;
            let after = info(engine, view)?;
            check(
                (after.drawable_width, after.drawable_height) == (128, 96),
                "resized drawable size",
            )?;
            check(
                after.configures > before.configures,
                "surface was not configured again",
            )
        })();
        let detach = status(op_view_detach(engine, view), "resize detach");
        result.and(detach)
    });
    scenario("switch", &mut failures, || {
        let view = attach(engine, doc, hwnd)?;
        let result = (|| {
            status(
                op_view_set(engine, view, &state(64.0, 48.0, &[])),
                "initial set",
            )?;
            wait(engine, view, 1)?;
            let before = info(engine, view)?;
            let p3 =
                op_color::Profile::builtin("display-p3").map_err(|e| format!("P3 profile: {e}"))?;
            status(
                op_view_set(engine, view, &state(64.0, 48.0, &p3.bytes())),
                "display switch",
            )?;
            wait(engine, view, 2)?;
            let after = info(engine, view)?;
            check(
                after.table_builds > before.table_builds && after.has_display_table == 1,
                "display table was not rebuilt",
            )
        })();
        let detach = status(op_view_detach(engine, view), "switch detach");
        result.and(detach)
    });
    scenario("create-destroy", &mut failures, || {
        for _ in 0..10 {
            let view = attach(engine, doc, hwnd)?;
            let result = (|| {
                status(
                    op_view_set(engine, view, &state(64.0, 48.0, &[])),
                    "cycle set",
                )?;
                wait(engine, view, 1)
            })();
            let detach = status(op_view_detach(engine, view), "cycle detach");
            result.and(detach)?;
        }
        let mut counts = OpViewCounters::default();
        status(op_view_counters(engine, &mut counts), "op_view_counters")?;
        check(
            counts.live_views == 0 && counts.live_layers == 0 && counts.live_surfaces == 0,
            "views or surfaces leaked",
        )
    });

    scenario("device-lost", &mut failures, || {
        let view = attach(engine, doc, hwnd)?;
        let result = (|| {
            status(
                op_view_set(engine, view, &state(64.0, 48.0, &[])),
                "initial set",
            )?;
            wait(engine, view, 1)?;
            let before = info(engine, view)?;
            op_ffi::view_test_lose_device(engine);
            status(
                op_view_set(engine, view, &state(128.0, 96.0, &[])),
                "set after the device was lost",
            )?;
            wait(engine, view, 2)?;
            let after = info(engine, view)?;
            check(
                (after.drawable_width, after.drawable_height) == (128, 96),
                "canvas did not draw at its new size after recovery",
            )?;
            check(
                after.configures > before.configures,
                "surface was not configured on the new device",
            )
        })();
        let detach = status(op_view_detach(engine, view), "device-lost detach");
        result.and(detach)
    });

    println!("RemoveDevice: NOT RUN (the device-lost scenario uses wgpu Device::destroy instead)");
    status(op_doc_close(engine, doc), "op_doc_close")?;
    op_engine_free(engine);
    check(unsafe { DestroyWindow(hwnd) } != 0, "DestroyWindow failed")?;
    // A thread cannot close the desktop it is on.
    check(
        unsafe { SetThreadDesktop(original_desktop) } != 0,
        "SetThreadDesktop back failed",
    )?;
    check(unsafe { CloseDesktop(desktop) } != 0, "CloseDesktop failed")?;
    check(failures == 0, &format!("{failures} host scenarios failed"))
}

fn main() {
    if let Err(e) = run() {
        eprintln!("W3 host: FAIL: {e}");
        std::process::exit(1);
    }
}
