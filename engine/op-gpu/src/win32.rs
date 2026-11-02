//! A DX12 surface for a caller-owned HWND. The caller keeps the window alive until detach.
use std::ffi::c_void;
use std::num::NonZeroIsize;
use std::os::windows::ffi::OsStringExt;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use wgpu::rwh::{RawDisplayHandle, RawWindowHandle, Win32WindowHandle, WindowsDisplayHandle};

pub const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8Unorm;

#[link(name = "user32")]
unsafe extern "system" {
    fn IsWindow(hwnd: isize) -> i32;
    fn GetDC(hwnd: isize) -> isize;
    fn ReleaseDC(hwnd: isize, dc: isize) -> i32;
}
#[link(name = "gdi32")]
unsafe extern "system" {
    fn GetICMProfileW(dc: isize, count: *mut u32, path: *mut u16) -> i32;
}

#[derive(Default)]
pub struct WindowCounters {
    pub live_targets: AtomicU64,
    pub live_surfaces: AtomicU64,
}

#[derive(Default, Clone)]
pub struct WindowStats {
    pub configures: u64,
    pub drawable: (u32, u32),
    pub surface_errors: u64,
    pub tag_changed_by_configure: u64,
    pub retags: u64,
}

pub struct Win32Target {
    hwnd: isize,
    surface: Option<wgpu::Surface<'static>>,
    configured: Option<(u32, u32)>,
    display_icc: Vec<u8>,
    counters: Arc<WindowCounters>,
    pub stats: WindowStats,
}

// The HWND is used only for surface creation. The caller owns its lifetime.
unsafe impl Send for Win32Target {}

impl Win32Target {
    /// The caller must keep `hwnd` alive through detach.
    pub unsafe fn new(
        instance: &wgpu::Instance,
        hwnd: *mut c_void,
        counters: Arc<WindowCounters>,
    ) -> Result<Self, String> {
        let value = hwnd as isize;
        if value == 0 || unsafe { IsWindow(value) } == 0 {
            return Err("invalid HWND".into());
        }
        let display_icc = display_profile(hwnd)?;
        let mut target = Self {
            hwnd: value,
            surface: None,
            configured: None,
            display_icc,
            counters,
            stats: WindowStats::default(),
        };
        target.resume(instance)?;
        target.counters.live_targets.fetch_add(1, Ordering::SeqCst);
        Ok(target)
    }

    pub fn display_icc(&self) -> &[u8] {
        &self.display_icc
    }
    pub fn tag_is_display(&self) -> bool {
        false
    }
    pub fn is_suspended(&self) -> bool {
        self.surface.is_none()
    }
    pub fn suspend(&mut self) {
        if self.surface.take().is_some() {
            self.counters.live_surfaces.fetch_sub(1, Ordering::SeqCst);
        }
        self.configured = None;
    }
    pub fn resume(&mut self, instance: &wgpu::Instance) -> Result<(), String> {
        if self.surface.is_some() {
            return Ok(());
        }
        let hwnd = NonZeroIsize::new(self.hwnd).ok_or("invalid HWND")?;
        let surface = unsafe {
            instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle {
                raw_display_handle: RawDisplayHandle::Windows(WindowsDisplayHandle::new()),
                raw_window_handle: RawWindowHandle::Win32(Win32WindowHandle::new(hwnd)),
            })
        }
        .map_err(|e| format!("create DX12 HWND surface: {e}"))?;
        self.surface = Some(surface);
        self.counters.live_surfaces.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    pub fn set_display_icc(&mut self, icc: &[u8]) -> Result<(), String> {
        self.display_icc = icc.to_vec();
        Ok(())
    }
    pub fn present(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        width: u32,
        height: u32,
        draw: impl FnOnce(&mut wgpu::CommandEncoder, &wgpu::TextureView) -> Result<(), String>,
    ) -> Result<(), String> {
        let _warp_guard = crate::windows_warp_guard().map_err(|e| e.to_string())?;
        let surface = self.surface.as_ref().ok_or("HWND surface is suspended")?;
        if self.configured != Some((width, height)) {
            surface.configure(
                device,
                &wgpu::SurfaceConfiguration {
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                    format: FORMAT,
                    width,
                    height,
                    present_mode: wgpu::PresentMode::Fifo,
                    desired_maximum_frame_latency: 2,
                    alpha_mode: wgpu::CompositeAlphaMode::Opaque,
                    view_formats: vec![],
                },
            );
            self.configured = Some((width, height));
            self.stats.configures += 1;
            self.stats.drawable = (width, height);
        }
        let frame = surface.get_current_texture().map_err(|e| {
            self.stats.surface_errors += 1;
            format!("acquire HWND frame: {e}")
        })?;
        let view = frame.texture.create_view(&Default::default());
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Windows canvas frame"),
        });
        draw(&mut encoder, &view)?;
        queue.submit(Some(encoder.finish()));
        frame.present();
        if _warp_guard.is_some() {
            crate::poll_gpu(device, "WARP view frame").map_err(|e| e.to_string())?;
        }
        Ok(())
    }
}

impl Drop for Win32Target {
    fn drop(&mut self) {
        self.suspend();
        self.counters.live_targets.fetch_sub(1, Ordering::SeqCst);
    }
}

fn display_profile(hwnd: *mut c_void) -> Result<Vec<u8>, String> {
    let hwnd = hwnd as isize;
    let dc = unsafe { GetDC(hwnd) };
    if dc == 0 {
        return Err("GetDC for display profile failed".into());
    }
    let result = (|| {
        let mut count = 0u32;
        unsafe { GetICMProfileW(dc, &mut count, std::ptr::null_mut()) };
        if count == 0 || count > 32768 {
            return Err("GetICMProfileW did not return a path length".into());
        }
        let mut wide = vec![0u16; count as usize];
        if unsafe { GetICMProfileW(dc, &mut count, wide.as_mut_ptr()) } == 0 {
            return Err(format!(
                "GetICMProfileW failed: {}",
                std::io::Error::last_os_error()
            ));
        }
        let end = wide
            .iter()
            .position(|&unit| unit == 0)
            .unwrap_or(wide.len());
        let path = std::ffi::OsString::from_wide(&wide[..end]);
        std::fs::read(&path).map_err(|e| format!("read Windows display ICC {:?}: {e}", path))
    })();
    unsafe { ReleaseDC(hwnd, dc) };
    result
}
