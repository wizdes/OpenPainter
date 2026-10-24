//! M09 presenter go/no-go spike. See `include/op_present_spike.h` for the C interface
//! and `docs/presenter-decision.md` for the result. macOS only; on other systems this
//! crate is empty.
#![allow(clippy::missing_safety_doc)]
// objc 0.2's msg_send! expands to cfg(feature = "cargo-clippy").
#![allow(unexpected_cfgs)]

#[cfg(target_os = "macos")]
mod color;
#[cfg(target_os = "macos")]
mod spike;

#[cfg(target_os = "macos")]
pub use ffi::*;

#[cfg(target_os = "macos")]
mod ffi {
    use std::cell::RefCell;
    use std::ffi::{c_char, c_void};
    use std::panic::{AssertUnwindSafe, catch_unwind};

    use crate::spike::{Context, Frame, Presenter, SpikeError, View};

    pub type OpPsStatus = i32;
    pub const OP_PS_OK: i32 = 0;
    pub const OP_PS_NULL: i32 = 1;
    pub const OP_PS_INVALID: i32 = 2;
    pub const OP_PS_GPU: i32 = 3;
    pub const OP_PS_BUSY: i32 = 4;
    pub const OP_PS_NO_FRAME: i32 = 5;
    pub const OP_PS_SURFACE: i32 = 6;
    pub const OP_PS_SIZE_MISMATCH: i32 = 7;
    pub const OP_PS_SUSPENDED: i32 = 8;
    pub const OP_PS_PANIC: i32 = 99;

    #[repr(C)]
    pub struct OpPsView {
        pub width_pt: f64,
        pub height_pt: f64,
        pub scale: f64,
        pub zoom: f64,
        pub origin_x: f64,
        pub origin_y: f64,
    }

    #[repr(C)]
    pub struct OpPsFrame {
        pub slot: u32,
        pub width: u32,
        pub height: u32,
        pub seq: u64,
        pub doc_generation: u64,
        pub iosurface: *mut c_void,
        pub acquire_ms: f64,
        pub render_ms: f64,
    }

    #[repr(C)]
    pub struct OpPsStats {
        pub frames: u64,
        pub validation_errors: u64,
        pub configures: u64,
        pub tag_kept_by_configure: u64,
        pub tag_changed_by_configure: u64,
        pub retags: u64,
        pub surface_errors: u64,
        pub ring_violations: u64,
        pub doc_generation: u64,
        pub last_acquire_ms: f64,
        pub last_render_ms: f64,
        pub drawable_width: u32,
        pub drawable_height: u32,
        pub pixel_format: u32,
        pub layer_tag_is_display: u8,
        pub framebuffer_only: u8,
        pub _pad: [u8; 2],
        pub colorspace_name: [c_char; 96],
        pub last_error: [c_char; 256],
    }

    #[repr(C)]
    pub struct OpPsReport {
        pub live_presenters: u64,
        pub live_ring_textures: u64,
        pub live_surfaces: u64,
        pub wgpu_textures: u64,
        pub wgpu_texture_views: u64,
        pub wgpu_buffers: u64,
        pub wgpu_bind_groups: u64,
        pub wgpu_render_pipelines: u64,
        pub wgpu_texture_memory: u64,
        pub validation_errors: u64,
    }

    pub struct OpPsContext(Context);
    pub struct OpPsPresenter(Presenter);

    thread_local! {
        static LAST_ERROR: RefCell<String> = const { RefCell::new(String::new()) };
    }

    fn set_error(msg: String) {
        LAST_ERROR.with(|e| *e.borrow_mut() = msg);
    }

    fn status(e: SpikeError) -> OpPsStatus {
        let code = match &e {
            SpikeError::Invalid(_) => OP_PS_INVALID,
            SpikeError::Gpu(_) => OP_PS_GPU,
            SpikeError::Busy => OP_PS_BUSY,
            SpikeError::NoFrame => OP_PS_NO_FRAME,
            SpikeError::Surface(_) => OP_PS_SURFACE,
            SpikeError::SizeMismatch => OP_PS_SIZE_MISMATCH,
            SpikeError::Suspended => OP_PS_SUSPENDED,
        };
        set_error(e.to_string());
        code
    }

    /// Runs `f` with panics caught; a panic becomes OP_PS_PANIC.
    fn guard(f: impl FnOnce() -> Result<(), OpPsStatus>) -> OpPsStatus {
        match catch_unwind(AssertUnwindSafe(f)) {
            Ok(Ok(())) => OP_PS_OK,
            Ok(Err(code)) => code,
            Err(payload) => {
                let msg = payload
                    .downcast_ref::<&str>()
                    .map(|s| s.to_string())
                    .or_else(|| payload.downcast_ref::<String>().cloned())
                    .unwrap_or_else(|| "unknown panic".into());
                set_error(format!("panic: {msg}"));
                OP_PS_PANIC
            }
        }
    }

    fn null() -> OpPsStatus {
        set_error("null pointer".into());
        OP_PS_NULL
    }

    unsafe fn presenter<'a>(p: *mut OpPsPresenter) -> Result<&'a mut Presenter, OpPsStatus> {
        // SAFETY: caller passes a live presenter or null.
        unsafe { p.as_mut() }.map(|p| &mut p.0).ok_or_else(null)
    }

    unsafe fn bytes<'a>(ptr: *const u8, len: usize) -> Result<&'a [u8], OpPsStatus> {
        if ptr.is_null() {
            return if len == 0 { Ok(&[]) } else { Err(null()) };
        }
        // SAFETY: caller passes `len` readable bytes.
        Ok(unsafe { std::slice::from_raw_parts(ptr, len) })
    }

    fn write_frame(out: *mut OpPsFrame, f: Frame) {
        if let Some(out) = unsafe { out.as_mut() } {
            *out = OpPsFrame {
                slot: f.slot,
                width: f.width,
                height: f.height,
                seq: f.seq,
                doc_generation: f.doc_generation,
                iosurface: f.iosurface,
                acquire_ms: f.acquire_ms,
                render_ms: f.render_ms,
            };
        }
    }

    fn copy_c(dst: &mut [c_char], s: &str) {
        let n = s.len().min(dst.len() - 1);
        for (d, b) in dst.iter_mut().zip(s.as_bytes()[..n].iter()) {
            *d = *b as c_char;
        }
        dst[n] = 0;
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn op_ps_context_new(out: *mut *mut OpPsContext) -> OpPsStatus {
        guard(|| {
            if out.is_null() {
                return Err(null());
            }
            let ctx = Context::new().map_err(status)?;
            // SAFETY: checked non-null.
            unsafe { *out = Box::into_raw(Box::new(OpPsContext(ctx))) };
            Ok(())
        })
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn op_ps_context_free(ctx: *mut OpPsContext) {
        let _ = guard(|| {
            if !ctx.is_null() {
                // SAFETY: from op_ps_context_new.
                let ctx = unsafe { Box::from_raw(ctx) };
                let _ = ctx.0.wait_idle();
            }
            Ok(())
        });
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn op_ps_context_maintain(ctx: *mut OpPsContext) -> OpPsStatus {
        guard(|| {
            // SAFETY: live context or null.
            let ctx = unsafe { ctx.as_ref() }.ok_or_else(null)?;
            ctx.0.wait_idle().map_err(status)
        })
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn op_ps_context_report(
        ctx: *mut OpPsContext,
        out: *mut OpPsReport,
    ) -> OpPsStatus {
        guard(|| {
            // SAFETY: live context or null.
            let ctx = &unsafe { ctx.as_ref() }.ok_or_else(null)?.0;
            // SAFETY: out may be null.
            let out = unsafe { out.as_mut() }.ok_or_else(null)?;
            use std::sync::atomic::Ordering;
            let hal = ctx.device.get_internal_counters().hal;
            let n = |c: &wgpu::wgt::InternalCounter| c.read().max(0) as u64;
            *out = OpPsReport {
                live_presenters: ctx.live_presenters.load(Ordering::SeqCst),
                live_ring_textures: ctx.live_ring_textures.load(Ordering::SeqCst),
                live_surfaces: ctx.live_surfaces.load(Ordering::SeqCst),
                wgpu_textures: n(&hal.textures),
                wgpu_texture_views: n(&hal.texture_views),
                wgpu_buffers: n(&hal.buffers),
                wgpu_bind_groups: n(&hal.bind_groups),
                wgpu_render_pipelines: n(&hal.render_pipelines),
                wgpu_texture_memory: n(&hal.texture_memory),
                validation_errors: ctx.errors.count.load(Ordering::SeqCst),
            };
            Ok(())
        })
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn op_ps_new_layer(
        ctx: *mut OpPsContext,
        layer: *mut c_void,
        out: *mut *mut OpPsPresenter,
    ) -> OpPsStatus {
        guard(|| {
            // SAFETY: live context or null.
            let ctx = &unsafe { ctx.as_ref() }.ok_or_else(null)?.0;
            if layer.is_null() || out.is_null() {
                return Err(null());
            }
            // SAFETY: caller passes a CAMetalLayer; new_layer checks the class.
            let p = unsafe { Presenter::new_layer(ctx, layer.cast()) }.map_err(status)?;
            // SAFETY: checked non-null.
            unsafe { *out = Box::into_raw(Box::new(OpPsPresenter(p))) };
            Ok(())
        })
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn op_ps_new_ring(
        ctx: *mut OpPsContext,
        out: *mut *mut OpPsPresenter,
    ) -> OpPsStatus {
        guard(|| {
            // SAFETY: live context or null.
            let ctx = &unsafe { ctx.as_ref() }.ok_or_else(null)?.0;
            if out.is_null() {
                return Err(null());
            }
            // SAFETY: checked non-null.
            unsafe { *out = Box::into_raw(Box::new(OpPsPresenter(Presenter::new_ring(ctx)))) };
            Ok(())
        })
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn op_ps_set_ring(
        p: *mut OpPsPresenter,
        surfaces: *const *mut c_void,
        count: u32,
    ) -> OpPsStatus {
        guard(|| {
            let p = unsafe { presenter(p) }?;
            if surfaces.is_null() {
                return Err(null());
            }
            // SAFETY: caller passes `count` IOSurfaceRefs.
            let list = unsafe { std::slice::from_raw_parts(surfaces, count as usize) };
            unsafe { p.set_ring(list) }.map_err(status)
        })
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn op_ps_free(p: *mut OpPsPresenter) {
        let _ = guard(|| {
            if !p.is_null() {
                // SAFETY: from op_ps_new_*.
                let p = unsafe { Box::from_raw(p) };
                // SAFETY: context outlives presenters.
                let _ = unsafe { &*p.0.ctx }.wait_idle();
                drop(p);
            }
            Ok(())
        });
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn op_ps_set_document(
        p: *mut OpPsPresenter,
        rgba: *const u8,
        len: usize,
        width: u32,
        height: u32,
        icc: *const u8,
        icc_len: usize,
    ) -> OpPsStatus {
        guard(|| {
            let p = unsafe { presenter(p) }?;
            let rgba = unsafe { bytes(rgba, len) }?;
            let icc = unsafe { bytes(icc, icc_len) }?;
            p.set_document(rgba, width, height, icc).map_err(status)
        })
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn op_ps_set_display_profile(
        p: *mut OpPsPresenter,
        icc: *const u8,
        len: usize,
    ) -> OpPsStatus {
        guard(|| {
            let p = unsafe { presenter(p) }?;
            let icc = unsafe { bytes(icc, len) }?;
            p.set_display_profile(icc).map_err(status)
        })
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn op_ps_set_view(
        p: *mut OpPsPresenter,
        view: *const OpPsView,
    ) -> OpPsStatus {
        guard(|| {
            let p = unsafe { presenter(p) }?;
            // SAFETY: live struct or null.
            let v = unsafe { view.as_ref() }.ok_or_else(null)?;
            p.set_view(View {
                width_pt: v.width_pt,
                height_pt: v.height_pt,
                scale: v.scale,
                zoom: v.zoom,
                origin_x: v.origin_x,
                origin_y: v.origin_y,
            })
            .map_err(status)
        })
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn op_ps_render(
        p: *mut OpPsPresenter,
        out: *mut OpPsFrame,
    ) -> OpPsStatus {
        guard(|| {
            let p = unsafe { presenter(p) }?;
            let f = p.render().map_err(status)?;
            write_frame(out, f);
            Ok(())
        })
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn op_ps_set_capture(p: *mut OpPsPresenter, on: bool) -> OpPsStatus {
        guard(|| unsafe { presenter(p) }?.set_capture(on).map_err(status))
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn op_ps_copy_capture(
        p: *mut OpPsPresenter,
        out: *mut u8,
        cap: usize,
        width: *mut u32,
        height: *mut u32,
    ) -> OpPsStatus {
        guard(|| {
            let p = unsafe { presenter(p) }?;
            let Some((w, h, bytes)) = p.captured() else {
                set_error("no captured frame".into());
                return Err(OP_PS_NO_FRAME);
            };
            if out.is_null() || width.is_null() || height.is_null() {
                return Err(null());
            }
            if cap < bytes.len() {
                set_error(format!("capture needs {} bytes", bytes.len()));
                return Err(OP_PS_INVALID);
            }
            // SAFETY: caller passes `cap` writable bytes and two u32 slots.
            unsafe {
                std::ptr::copy_nonoverlapping(bytes.as_ptr(), out, bytes.len());
                *width = *w;
                *height = *h;
            }
            Ok(())
        })
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn op_ps_ring_acquire(
        p: *mut OpPsPresenter,
        wait: bool,
        out: *mut OpPsFrame,
    ) -> OpPsStatus {
        guard(|| {
            let p = unsafe { presenter(p) }?;
            let f = p.ring_acquire(wait).map_err(status)?;
            write_frame(out, f);
            Ok(())
        })
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn op_ps_ring_release(p: *mut OpPsPresenter, slot: u32) -> OpPsStatus {
        guard(|| unsafe { presenter(p) }?.ring_release(slot).map_err(status))
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn op_ps_suspend(p: *mut OpPsPresenter) -> OpPsStatus {
        guard(|| unsafe { presenter(p) }?.suspend().map_err(status))
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn op_ps_resume(p: *mut OpPsPresenter) -> OpPsStatus {
        guard(|| unsafe { presenter(p) }?.resume().map_err(status))
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn op_ps_stats(p: *mut OpPsPresenter, out: *mut OpPsStats) -> OpPsStatus {
        guard(|| {
            let p = unsafe { presenter(p) }?;
            // SAFETY: live struct or null.
            let out = unsafe { out.as_mut() }.ok_or_else(null)?;
            let s = &p.stats;
            let layer = p.layer_readback();
            let mut stats = OpPsStats {
                frames: s.frames,
                validation_errors: p.validation_errors(),
                configures: s.configures,
                tag_kept_by_configure: s.tag_kept_by_configure,
                tag_changed_by_configure: s.tag_changed_by_configure,
                retags: s.retags,
                surface_errors: s.surface_errors,
                ring_violations: s.ring_violations,
                doc_generation: p.doc_generation(),
                last_acquire_ms: s.last_acquire_ms,
                last_render_ms: s.last_render_ms,
                drawable_width: layer.as_ref().map_or(0, |l| l.width),
                drawable_height: layer.as_ref().map_or(0, |l| l.height),
                pixel_format: layer.as_ref().map_or(0, |l| l.pixel_format),
                layer_tag_is_display: layer.as_ref().is_some_and(|l| l.tag_is_display) as u8,
                framebuffer_only: layer.as_ref().is_some_and(|l| l.framebuffer_only) as u8,
                _pad: [0; 2],
                colorspace_name: [0; 96],
                last_error: [0; 256],
            };
            copy_c(
                &mut stats.colorspace_name,
                layer
                    .as_ref()
                    .map_or("(path B: Swift owns the layer)", |l| &l.name),
            );
            copy_c(&mut stats.last_error, &s.last_error);
            *out = stats;
            Ok(())
        })
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn op_ps_last_error(buf: *mut c_char, cap: usize) -> usize {
        LAST_ERROR.with(|e| {
            let e = e.borrow();
            if !buf.is_null() && cap > 0 {
                // SAFETY: caller passes `cap` writable bytes.
                let dst = unsafe { std::slice::from_raw_parts_mut(buf, cap) };
                copy_c(dst, &e);
            }
            e.len()
        })
    }

    #[unsafe(no_mangle)]
    #[allow(clippy::too_many_arguments)]
    pub unsafe extern "C" fn op_ps_debug_convert(
        doc_icc: *const u8,
        doc_len: usize,
        display_icc: *const u8,
        display_len: usize,
        rgb: *const f32,
        n: usize,
        exact: *mut f32,
        lut: *mut f32,
    ) -> OpPsStatus {
        guard(|| {
            let doc = unsafe { bytes(doc_icc, doc_len) }?;
            let display = unsafe { bytes(display_icc, display_len) }?;
            if rgb.is_null() {
                return Err(null());
            }
            // SAFETY: caller passes 3 * n floats.
            let input: Vec<[f32; 3]> = unsafe { std::slice::from_raw_parts(rgb, n * 3) }
                .chunks(3)
                .map(|c| [c[0], c[1], c[2]])
                .collect();
            let invalid = |e: String| {
                set_error(e);
                OP_PS_INVALID
            };
            if !exact.is_null() {
                let out = crate::color::convert_exact(doc, display, &input).map_err(invalid)?;
                // SAFETY: caller passes 3 * n writable floats.
                let dst = unsafe { std::slice::from_raw_parts_mut(exact, n * 3) };
                for (d, c) in dst.chunks_mut(3).zip(&out) {
                    d.copy_from_slice(c);
                }
            }
            if !lut.is_null() {
                let table =
                    crate::color::build_lut(doc, display, crate::spike::LUT_N).map_err(invalid)?;
                // SAFETY: caller passes 3 * n writable floats.
                let dst = unsafe { std::slice::from_raw_parts_mut(lut, n * 3) };
                for (d, c) in dst.chunks_mut(3).zip(&input) {
                    d.copy_from_slice(&table.apply(*c));
                }
            }
            Ok(())
        })
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn op_ps_debug_panic() -> OpPsStatus {
        guard(|| panic!("op_ps_debug_panic test hook"))
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    #[test]
    fn panics_are_caught_at_the_boundary() {
        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let code = op_ps_debug_panic();
        std::panic::set_hook(prev);
        assert_eq!(code, OP_PS_PANIC);
        let mut buf = [0 as std::ffi::c_char; 128];
        let n = unsafe { op_ps_last_error(buf.as_mut_ptr(), buf.len()) };
        assert!(n > 0);
        let msg = unsafe { std::ffi::CStr::from_ptr(buf.as_ptr()) };
        assert!(msg.to_string_lossy().contains("test hook"));
    }

    #[test]
    fn null_pointers_return_null_status() {
        assert_eq!(
            unsafe { op_ps_render(std::ptr::null_mut(), std::ptr::null_mut()) },
            OP_PS_NULL
        );
        assert_eq!(
            unsafe { op_ps_context_new(std::ptr::null_mut()) },
            OP_PS_NULL
        );
    }

    #[test]
    fn lut_from_lcms2_is_identity_for_same_profile() {
        // sRGB -> sRGB through lcms2's built-in profile must be close to identity.
        let srgb = lcms2::Profile::new_srgb().icc().unwrap();
        let lut = crate::color::build_lut(&srgb, &srgb, 33).unwrap();
        for (i, c) in lut.data.iter().enumerate() {
            let n = 33usize;
            let want = [
                (i % n) as f32 / 32.0,
                (i / n % n) as f32 / 32.0,
                (i / (n * n)) as f32 / 32.0,
            ];
            for k in 0..3 {
                assert!(
                    (c[k] - want[k]).abs() < 1e-3,
                    "entry {i}: {c:?} vs {want:?}"
                );
            }
        }
        assert!(crate::color::build_lut(b"not icc", &srgb, 33).is_err());
    }
}
