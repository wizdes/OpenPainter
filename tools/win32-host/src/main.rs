#![cfg(windows)]

use op_ffi::*;
use std::ffi::CString;
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
fn clean(engine: *mut OpEngine, view: u64) -> Result<(), String> {
    let view_info = info(engine, view)?;
    check(
        view_info.frame_errors == 0,
        &format!(
            "frame_errors={} last_error={}",
            view_info.frame_errors, view_info.last_error
        ),
    )?;
    let mut counters = OpViewCounters::default();
    status(op_view_counters(engine, &mut counters), "op_view_counters")?;
    check(
        counters.validation_errors == 0,
        &format!("validation_errors={}", counters.validation_errors),
    )
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

fn effect_gpu_live(engine: *mut OpEngine, doc: u64) -> Result<(), String> {
    let command = CString::new("effect.adjust.sepia").unwrap();
    let mut ticket = 0;
    status(
        op_dispatch(engine, doc, command.as_ptr(), ptr::null(), 0, &mut ticket),
        "effect dispatch",
    )?;
    check(ticket != 0, "effect dispatch returned no ticket")?;
    let query = CString::new("doc.info").unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        let mut buffer = OpBuffer {
            ptr: ptr::null_mut(),
            len: 0,
        };
        status(
            op_query(engine, doc, query.as_ptr(), ptr::null(), 0, &mut buffer),
            "effect GPU info",
        )?;
        let bytes = unsafe { std::slice::from_raw_parts(buffer.ptr, buffer.len) };
        let text = String::from_utf8_lossy(bytes).into_owned();
        op_buffer_free(&mut buffer);
        if text.contains("\"gpu\":true") && text.contains("\"gpu_runs\":1") {
            return Ok(());
        }
        if std::time::Instant::now() >= deadline {
            return Err(format!("effect GPU did not become live: {text}"));
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

fn wait_errors(engine: *mut OpEngine, view: u64, wanted: u64) -> Result<(), String> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    loop {
        let current = info(engine, view)?;
        if current.frame_errors >= wanted {
            return Ok(());
        }
        if std::time::Instant::now() >= deadline {
            return Err(format!(
                "frame errors stayed at {}, wanted {wanted}",
                current.frame_errors
            ));
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
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
            )?;
            clean(engine, view)
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
            )?;
            clean(engine, view)
        })();
        let detach = status(op_view_detach(engine, view), "switch detach");
        result.and(detach)
    });
    scenario("create-destroy", &mut failures, || {
        for _ in 0..10 {
            let cycle_hwnd = unsafe {
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
            check(cycle_hwnd != 0, "cycle CreateWindowExW failed")?;
            unsafe { ShowWindow(cycle_hwnd, 5) };
            let view = attach(engine, doc, cycle_hwnd)?;
            let result = (|| {
                status(
                    op_view_set(engine, view, &state(64.0, 48.0, &[])),
                    "cycle set",
                )?;
                wait(engine, view, 1)?;
                clean(engine, view)
            })();
            let detach = status(op_view_detach(engine, view), "cycle detach");
            result.and(detach)?;
            check(
                unsafe { DestroyWindow(cycle_hwnd) } != 0,
                "cycle DestroyWindow failed",
            )?;
        }
        let mut counts = OpViewCounters::default();
        status(op_view_counters(engine, &mut counts), "op_view_counters")?;
        check(
            counts.live_views == 0 && counts.live_layers == 0 && counts.live_surfaces == 0,
            "views or surfaces leaked",
        )
    });
    // Windows allows one swap chain per window. Two views on two windows both
    // draw. A third on a window already in use gets a named error, and the
    // others keep drawing.
    scenario("multiple-views", &mut failures, || {
        let other = unsafe {
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
        check(other != 0, "second CreateWindowExW failed")?;
        unsafe { ShowWindow(other, 5) };
        let first = attach(engine, doc, hwnd)?;
        let second = attach(engine, doc, other)?;
        let third = attach(engine, doc, hwnd);
        let result = (|| {
            status(
                op_view_set(engine, first, &state(64.0, 48.0, &[])),
                "first view set",
            )?;
            status(
                op_view_set(engine, second, &state(64.0, 48.0, &[])),
                "second view set",
            )?;
            wait(engine, first, 1)?;
            wait(engine, second, 1)?;
            clean(engine, first)?;
            clean(engine, second)?;
            if let Ok(third) = third {
                status(
                    op_view_set(engine, third, &state(64.0, 48.0, &[])),
                    "third view set",
                )?;
                std::thread::sleep(std::time::Duration::from_millis(300));
                let failed = info(engine, third)?;
                check(
                    failed.frame_errors > 0 && failed.last_error != 0,
                    "a second swap chain on one window was not named",
                )?;
            }
            let before = info(engine, first)?;
            status(
                op_view_set(engine, first, &state(128.0, 96.0, &[])),
                "first view set after the third",
            )?;
            wait(engine, first, before.frames + 1)?;
            clean(engine, first)
        })();
        let detach_third = match third {
            Ok(third) => status(op_view_detach(engine, third), "third view detach"),
            Err(_) => Ok(()),
        };
        let a = status(op_view_detach(engine, first), "first view detach");
        let b = status(op_view_detach(engine, second), "second view detach");
        check(
            unsafe { DestroyWindow(other) } != 0,
            "second DestroyWindow failed",
        )?;
        result.and(detach_third).and(a).and(b)
    });
    scenario("configure-error", &mut failures, || {
        let broken = attach(engine, doc, hwnd)?;
        op_ffi::view_test_fail_next_configure();
        let result = (|| {
            status(
                op_view_set(engine, broken, &state(64.0, 48.0, &[])),
                "configure error set",
            )?;
            std::thread::sleep(std::time::Duration::from_millis(150));
            let failed = info(engine, broken)?;
            check(
                failed.frame_errors > 0 && failed.last_error != 0,
                "configure error was not named",
            )?;
            let other_hwnd = unsafe {
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
            check(other_hwnd != 0, "other CreateWindowExW failed")?;
            let other = attach(engine, doc, other_hwnd)?;
            status(
                op_view_set(engine, other, &state(64.0, 48.0, &[])),
                "other view set",
            )?;
            let other_result = wait(engine, other, 1).and_then(|_| clean(engine, other));
            let detach = status(op_view_detach(engine, other), "other view detach");
            let destroy = check(
                unsafe { DestroyWindow(other_hwnd) } != 0,
                "other DestroyWindow failed",
            );
            other_result.and(detach).and(destroy)
        })();
        result.and(status(
            op_view_detach(engine, broken),
            "configure error detach",
        ))
    });
    scenario("suspended-view", &mut failures, || {
        let view = attach(engine, doc, hwnd)?;
        let result = (|| {
            let mut paused = state(64.0, 48.0, &[]);
            paused.flags = OP_VIEW_SUSPENDED;
            status(op_view_set(engine, view, &paused), "suspend set")?;
            std::thread::sleep(std::time::Duration::from_millis(100));
            check(
                info(engine, view)?.frames == 0,
                "suspended view drew a frame",
            )?;
            status(
                op_view_set(engine, view, &state(64.0, 48.0, &[])),
                "resume set",
            )?;
            wait(engine, view, 1)?;
            clean(engine, view)
        })();
        result.and(status(op_view_detach(engine, view), "suspended detach"))
    });
    scenario("destroy-before-frame", &mut failures, || {
        let doomed = unsafe {
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
        check(doomed != 0, "doomed CreateWindowExW failed")?;
        let view = attach(engine, doc, doomed)?;
        check(
            unsafe { DestroyWindow(doomed) } != 0,
            "doomed DestroyWindow failed",
        )?;
        status(
            op_view_set(engine, view, &state(64.0, 48.0, &[])),
            "doomed set",
        )?;
        std::thread::sleep(std::time::Duration::from_millis(250));
        let failed = info(engine, view)?;
        check(
            failed.frame_errors > 0 && failed.last_error != 0,
            "destroyed HWND had no named frame error",
        )?;
        status(op_view_detach(engine, view), "doomed detach")
    });

    if std::env::var("OP_GPU_ADAPTER").as_deref() == Ok("warp") {
        scenario("RemoveDevice-idle", &mut failures, || {
            let view = attach(engine, doc, hwnd)?;
            let result = (|| {
                status(
                    op_view_set(engine, view, &state(64.0, 48.0, &[])),
                    "initial set",
                )?;
                wait(engine, view, 1)?;
                effect_gpu_live(engine, doc)?;
                std::thread::sleep(std::time::Duration::from_millis(200));
                let before = info(engine, view)?;
                op_ffi::view_test_remove_device(engine)?;
                wait(engine, view, before.frames + 1)?;
                let after = info(engine, view)?;
                check(
                    (after.drawable_width, after.drawable_height) == (64, 48),
                    "idle canvas did not draw after recovery",
                )?;
                check(
                    after.configures > before.configures,
                    "surface was not configured on the new device",
                )?;
                let mut counters = OpViewCounters::default();
                status(op_view_counters(engine, &mut counters), "recovery counters")?;
                check(
                    counters.validation_errors == 0,
                    "recovery validation_errors is nonzero",
                )
            })();
            let detach = status(op_view_detach(engine, view), "RemoveDevice detach");
            result.and(detach)
        });
        scenario(
            "RemoveDevice-retry-three-errors-second-view",
            &mut failures,
            || {
                let view = attach(engine, doc, hwnd)?;
                let result = (|| {
                    status(
                        op_view_set(engine, view, &state(64.0, 48.0, &[])),
                        "retry initial set",
                    )?;
                    wait(engine, view, 1)?;
                    let p3 = op_color::Profile::builtin("display-p3")
                        .map_err(|e| format!("recovery P3 profile: {e}"))?;
                    let next_frame = info(engine, view)?.frames + 1;
                    status(
                        op_view_set(engine, view, &state(64.0, 48.0, &p3.bytes())),
                        "retry P3 set",
                    )?;
                    wait(engine, view, next_frame)?;
                    for index in 0..3 {
                        let before = info(engine, view)?.frame_errors;
                        op_ffi::view_test_fail_next_configure();
                        status(
                            op_view_set(engine, view, &state(65.0 + index as f64, 48.0, &[])),
                            "retry error set",
                        )?;
                        wait_errors(engine, view, before + 1)?;
                    }
                    let before = info(engine, view)?;
                    check(
                        before.frame_errors >= 3 && before.configures > 0,
                        "three frame errors were not recorded",
                    )?;
                    op_ffi::view_test_fail_next_recreate();
                    op_ffi::view_test_remove_device(engine)?;
                    std::thread::sleep(std::time::Duration::from_millis(250));
                    let pending = info(engine, view)?;
                    check(
                        pending.frame_errors >= before.frame_errors,
                        "frame error count went backward during recovery",
                    )?;
                    check(
                        pending.configures >= before.configures,
                        "configure count went backward during recovery",
                    )?;
                    check(
                        op_view_wait_frames(engine, view, pending.frames + 1, 100, ptr::null_mut())
                            == OpStatus::OP_ERR_BUSY,
                        "live view did not stay registered during failed recreate",
                    )?;
                    wait(engine, view, pending.frames + 1)?;
                    let after = info(engine, view)?;
                    check(
                        after.frame_errors >= pending.frame_errors
                            && after.configures > before.configures,
                        "recovery did not keep its counters or configure a new surface",
                    )?;
                    check(
                        after.doc_uploads > before.doc_uploads
                            && after.table_builds >= before.table_builds
                            && after.mip_builds >= before.mip_builds,
                        "recovery did not keep upload, table, or mip counts",
                    )?;
                    status(
                        op_view_set(engine, view, &state(67.0, 48.0, &[])),
                        "length-zero profile set",
                    )?;
                    check(
                        info(engine, view)?.display_icc_len == p3.bytes().len() as u64,
                        "length-zero set forgot the app's P3 profile",
                    )?;
                    // Windows allows one swap chain per window, after recovery too.
                    // The second view gets a named error, and the first keeps drawing.
                    let second = attach(engine, doc, hwnd)?;
                    let second_result = (|| {
                        status(
                            op_view_set(engine, second, &state(64.0, 48.0, &[])),
                            "second view set",
                        )?;
                        wait_errors(engine, second, 1)?;
                        let next = info(engine, view)?.frames + 1;
                        status(
                            op_view_set(engine, view, &state(68.0, 48.0, &[])),
                            "first view set beside the second",
                        )?;
                        wait(engine, view, next)
                    })();
                    let second_detach =
                        status(op_view_detach(engine, second), "second view detach");
                    second_result.and(second_detach)
                })();
                let detach = status(op_view_detach(engine, view), "retry view detach");
                result.and(detach)
            },
        );
        scenario("RemoveDevice-surface-retry", &mut failures, || {
            let view = attach(engine, doc, hwnd)?;
            let result = (|| {
                status(
                    op_view_set(engine, view, &state(64.0, 48.0, &[])),
                    "surface retry set",
                )?;
                wait(engine, view, 1)?;
                let before = info(engine, view)?;
                op_ffi::view_test_fail_next_surface_recreate();
                op_ffi::view_test_remove_device(engine)?;
                wait_errors(engine, view, before.frame_errors + 1)?;
                let pending = info(engine, view)?;
                check(
                    pending.configures >= before.configures,
                    "surface failure lost configure count",
                )?;
                check(
                    op_view_wait_frames(engine, view, pending.frames + 1, 100, ptr::null_mut())
                        == OpStatus::OP_ERR_BUSY,
                    "surface failure detached a live view",
                )?;
                wait(engine, view, pending.frames + 1)?;
                let after = info(engine, view)?;
                check(
                    after.configures > before.configures
                        && after.frame_errors >= pending.frame_errors,
                    "surface retry did not recover with its counts",
                )?;
                let mut counters = OpViewCounters::default();
                status(
                    op_view_counters(engine, &mut counters),
                    "surface retry counters",
                )?;
                check(
                    counters.live_layers == 1,
                    &format!("surface retry live_layers={}", counters.live_layers),
                )
            })();
            let detach = status(op_view_detach(engine, view), "surface retry detach");
            result.and(detach)
        });
        scenario("frame-panic-keeps-device", &mut failures, || {
            let view = attach(engine, doc, hwnd)?;
            let result = (|| {
                status(
                    op_view_set(engine, view, &state(64.0, 48.0, &[])),
                    "panic initial set",
                )?;
                wait(engine, view, 1)?;
                let before = info(engine, view)?;
                op_ffi::view_test_panic_next_frame();
                status(
                    op_view_set(engine, view, &state(64.0, 48.0, &[])),
                    "panic retry set",
                )?;
                wait_errors(engine, view, before.frame_errors + 1)?;
                wait(engine, view, before.frames + 1)?;
                let after = info(engine, view)?;
                check(
                    after.configures == before.configures,
                    "a frame panic rebuilt a device that Windows did not remove",
                )
            })();
            let detach = status(op_view_detach(engine, view), "panic view detach");
            result.and(detach)
        });
    } else {
        println!("RemoveDevice-idle: NOT RUN (OP_GPU_ADAPTER is not warp)");
        println!(
            "RemoveDevice-retry-three-errors-second-view: NOT RUN (OP_GPU_ADAPTER is not warp)"
        );
        println!("RemoveDevice-surface-retry: NOT RUN (OP_GPU_ADAPTER is not warp)");
        println!("frame-panic-keeps-device: NOT RUN (OP_GPU_ADAPTER is not warp)");
    }

    let mut final_counters = OpViewCounters::default();
    status(
        op_view_counters(engine, &mut final_counters),
        "final counters",
    )?;
    if final_counters.validation_errors != 0 {
        eprintln!(
            "validation-errors: FAIL: {}",
            final_counters.validation_errors
        );
        failures += 1;
    } else {
        println!("validation-errors: PASS (0)");
    }

    status(op_doc_close(engine, doc), "op_doc_close")?;
    op_engine_free(engine);
    check(unsafe { DestroyWindow(hwnd) } != 0, "DestroyWindow failed")?;
    // A thread cannot close the desktop it is on.
    if unsafe { SetThreadDesktop(original_desktop) } == 0 {
        eprintln!(
            "desktop-close: SetThreadDesktop back failed: {}; Windows closes the private desktop when this process exits",
            std::io::Error::last_os_error()
        );
    } else {
        check(unsafe { CloseDesktop(desktop) } != 0, "CloseDesktop failed")?;
        println!("desktop-close: PASS");
    }
    check(failures == 0, &format!("{failures} host scenarios failed"))
}

fn main() {
    if let Err(e) = run() {
        eprintln!("W3 host: FAIL: {e}");
        std::process::exit(1);
    }
}
