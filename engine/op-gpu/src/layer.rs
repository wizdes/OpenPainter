//! Canvas presenter target on macOS (path A from M09, `docs/presenter-decision.md`):
//! a wgpu surface on a `CAMetalLayer` that the engine retains, tagged with the
//! display color space so Core Animation does not convert a second time.
//!
//! wgpu 27.0.4 does not touch `CAMetalLayer.colorspace` on configure. This target
//! still reads the tag before and after every configure, counts any change, and
//! sets the tag again before the frame is shown.
#![allow(unexpected_cfgs)] // objc 0.2's msg_send! expands to cfg(feature = "cargo-clippy").

use std::ffi::c_void;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use objc::runtime::{Class, Object};
use objc::{msg_send, sel, sel_impl};

type CFTypeRef = *const c_void;
type CGColorSpaceRef = *mut c_void;

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFDataCreate(alloc: CFTypeRef, bytes: *const u8, len: isize) -> CFTypeRef;
    fn CFDataGetLength(data: CFTypeRef) -> isize;
    fn CFDataGetBytePtr(data: CFTypeRef) -> *const u8;
    fn CFRelease(obj: CFTypeRef);
}

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGColorSpaceCreateWithICCData(data: CFTypeRef) -> CGColorSpaceRef;
    fn CGColorSpaceCopyICCData(space: CGColorSpaceRef) -> CFTypeRef;
    fn CGColorSpaceRelease(space: CGColorSpaceRef);
}

#[link(name = "QuartzCore", kind = "framework")]
unsafe extern "C" {}

/// Why a layer operation failed.
#[derive(Debug)]
pub enum LayerError {
    NotAMetalLayer,
    BadIcc,
    Surface(String),
    Suspended,
}

impl std::fmt::Display for LayerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotAMetalLayer => write!(f, "not a CAMetalLayer"),
            Self::BadIcc => write!(f, "ICC data is not a readable color space"),
            Self::Surface(m) => write!(f, "surface: {m}"),
            Self::Suspended => write!(f, "drawable pipeline is suspended"),
        }
    }
}

/// An owned CGColorSpace made from ICC data.
pub struct ColorSpace(CGColorSpaceRef);

// SAFETY: CGColorSpace is an immutable, thread-safe Core Foundation object.
unsafe impl Send for ColorSpace {}

impl ColorSpace {
    pub fn from_icc(icc: &[u8]) -> Option<Self> {
        // SAFETY: CFDataCreate copies the bytes; both objects are released below or in Drop.
        unsafe {
            let data = CFDataCreate(std::ptr::null(), icc.as_ptr(), icc.len() as isize);
            if data.is_null() {
                return None;
            }
            let space = CGColorSpaceCreateWithICCData(data);
            CFRelease(data);
            (!space.is_null()).then_some(Self(space))
        }
    }
}

impl Drop for ColorSpace {
    fn drop(&mut self) {
        // SAFETY: we own one reference from CGColorSpaceCreateWithICCData.
        unsafe { CGColorSpaceRelease(self.0) }
    }
}

/// Counters shared by every layer target of one owner (for leak checks).
#[derive(Default)]
pub struct LayerCounters {
    pub live_targets: AtomicU64,
    pub live_surfaces: AtomicU64,
}

/// Per-target statistics.
#[derive(Default, Clone, Debug)]
pub struct LayerStats {
    pub configures: u64,
    pub tag_changed_by_configure: u64,
    pub retags: u64,
    pub surface_errors: u64,
    pub drawable: (u32, u32),
}

struct CountedSurface {
    surface: wgpu::Surface<'static>,
    counters: Arc<LayerCounters>,
}

impl Drop for CountedSurface {
    fn drop(&mut self) {
        self.counters.live_surfaces.fetch_sub(1, Ordering::SeqCst);
    }
}

/// A retained `CAMetalLayer` with its wgpu surface.
pub struct MetalLayerTarget {
    layer: *mut Object, // retained
    surface: Option<CountedSurface>,
    configured: Option<(u32, u32)>,
    display_icc: Option<Vec<u8>>,
    display_space: Option<ColorSpace>,
    counters: Arc<LayerCounters>,
    pub stats: LayerStats,
}

// SAFETY: the layer pointer is only used through thread-safe CAMetalLayer calls
// (nextDrawable, drawableSize, colorspace inside a CATransaction) and retain/release.
unsafe impl Send for MetalLayerTarget {}

pub const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8Unorm;

impl MetalLayerTarget {
    /// Retains `layer` and makes a surface on it.
    ///
    /// # Safety
    /// `layer` must be a live Objective-C object (its class is checked here).
    pub unsafe fn new(
        instance: &wgpu::Instance,
        layer: *mut c_void,
        counters: Arc<LayerCounters>,
    ) -> Result<Self, LayerError> {
        let layer = layer as *mut Object;
        // SAFETY: class check before use; the retain is balanced in Drop.
        unsafe {
            let class = Class::get("CAMetalLayer").ok_or(LayerError::NotAMetalLayer)?;
            let ok: bool = msg_send![layer, isKindOfClass: class];
            if !ok {
                return Err(LayerError::NotAMetalLayer);
            }
            let _: *mut Object = msg_send![layer, retain];
        }
        counters.live_targets.fetch_add(1, Ordering::SeqCst);
        let mut target = Self {
            layer,
            surface: None,
            configured: None,
            display_icc: None,
            display_space: None,
            counters,
            stats: LayerStats::default(),
        };
        target.resume(instance)?;
        Ok(target)
    }

    /// The retained layer, for identity checks.
    pub fn layer_ptr(&self) -> *mut c_void {
        self.layer.cast()
    }

    pub fn is_suspended(&self) -> bool {
        self.surface.is_none()
    }

    /// Drops the surface (on sleep). Frames are refused until `resume`.
    pub fn suspend(&mut self) {
        self.surface = None;
        self.configured = None;
    }

    /// Makes the surface again (on wake). Does nothing when it exists.
    pub fn resume(&mut self, instance: &wgpu::Instance) -> Result<(), LayerError> {
        if self.surface.is_some() {
            return Ok(());
        }
        // SAFETY: the layer stays retained for the surface's lifetime.
        let surface = unsafe {
            instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::CoreAnimationLayer(
                self.layer.cast(),
            ))
        }
        .map_err(|e| LayerError::Surface(format!("create surface: {e}")))?;
        self.counters.live_surfaces.fetch_add(1, Ordering::SeqCst);
        self.surface = Some(CountedSurface {
            surface,
            counters: self.counters.clone(),
        });
        self.configured = None;
        Ok(())
    }

    /// Sets the display profile the layer is tagged with.
    pub fn set_display_icc(&mut self, icc: &[u8]) -> Result<(), LayerError> {
        let space = ColorSpace::from_icc(icc).ok_or(LayerError::BadIcc)?;
        self.display_icc = Some(icc.to_vec());
        self.display_space = Some(space);
        self.ensure_tag();
        Ok(())
    }

    pub fn display_icc(&self) -> Option<&[u8]> {
        self.display_icc.as_deref()
    }

    /// Reads the layer's current colorspace as ICC bytes.
    pub fn layer_tag_icc(&self) -> Option<Vec<u8>> {
        // SAFETY: the layer is retained; the getter returns a borrowed space or null.
        unsafe {
            let space: CGColorSpaceRef = msg_send![self.layer, colorspace];
            if space.is_null() {
                return None;
            }
            let data = CGColorSpaceCopyICCData(space);
            if data.is_null() {
                return None;
            }
            let bytes =
                std::slice::from_raw_parts(CFDataGetBytePtr(data), CFDataGetLength(data) as usize)
                    .to_vec();
            CFRelease(data);
            Some(bytes)
        }
    }

    /// True when the layer carries the display tag (or no display is known yet).
    pub fn tag_is_display(&self) -> bool {
        match &self.display_icc {
            Some(icc) => self.layer_tag_icc().as_deref() == Some(icc.as_slice()),
            None => false,
        }
    }

    /// Sets the tag again if anything changed it. Counts a retag.
    fn ensure_tag(&mut self) {
        let Some(space) = &self.display_space else {
            return;
        };
        if self.layer_tag_icc().as_deref() == self.display_icc.as_deref() {
            return;
        }
        // SAFETY: the layer is retained. A CATransaction keeps the change explicit on
        // threads without a run loop; actions are off so nothing animates.
        unsafe {
            let tx = Class::get("CATransaction").expect("QuartzCore is linked");
            let () = msg_send![tx, begin];
            let () = msg_send![tx, setDisableActions: true];
            let () = msg_send![self.layer, setColorspace: space.0];
            let () = msg_send![tx, commit];
        }
        self.stats.retags += 1;
    }

    fn configure(
        &mut self,
        device: &wgpu::Device,
        width: u32,
        height: u32,
    ) -> Result<(), LayerError> {
        let surface = self.surface.as_ref().ok_or(LayerError::Suspended)?;
        let before = self.layer_tag_icc();
        surface.surface.configure(
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
        // wgpu set layer properties on this thread. A thread without a run loop
        // never commits that implicit transaction, and an open transaction keeps
        // the layer retained, so commit it now.
        flush_transaction();
        if self.layer_tag_icc() != before {
            self.stats.tag_changed_by_configure += 1;
        }
        self.configured = Some((width, height));
        self.stats.configures += 1;
        self.stats.drawable = (width, height);
        self.ensure_tag();
        Ok(())
    }

    /// Acquires a drawable of `width` x `height` pixels (configuring when the size
    /// changed), lets `draw` record into it, submits and presents.
    pub fn present(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        width: u32,
        height: u32,
        draw: impl FnOnce(&mut wgpu::CommandEncoder, &wgpu::TextureView) -> Result<(), String>,
    ) -> Result<(), LayerError> {
        if self.surface.is_none() {
            return Err(LayerError::Suspended);
        }
        if self.configured != Some((width, height)) {
            self.configure(device, width, height)?;
        }
        self.ensure_tag();
        let mut frame = None;
        for _ in 0..2 {
            let surface = self.surface.as_ref().ok_or(LayerError::Suspended)?;
            match surface.surface.get_current_texture() {
                Ok(t) => {
                    frame = Some(t);
                    break;
                }
                Err(wgpu::SurfaceError::Outdated | wgpu::SurfaceError::Lost) => {
                    self.stats.surface_errors += 1;
                    self.configure(device, width, height)?;
                }
                Err(e) => {
                    self.stats.surface_errors += 1;
                    return Err(LayerError::Surface(format!("acquire: {e}")));
                }
            }
        }
        let frame = frame.ok_or_else(|| LayerError::Surface("acquire failed twice".into()))?;
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("canvas frame"),
        });
        draw(&mut encoder, &view).map_err(LayerError::Surface)?;
        queue.submit(Some(encoder.finish()));
        frame.present();
        flush_transaction();
        Ok(())
    }
}

/// Commits any implicit Core Animation transaction open on this thread.
fn flush_transaction() {
    // SAFETY: a class method with no arguments.
    unsafe {
        let tx = Class::get("CATransaction").expect("QuartzCore is linked");
        let () = msg_send![tx, flush];
    }
}

impl Drop for MetalLayerTarget {
    fn drop(&mut self) {
        self.surface = None;
        flush_transaction();
        self.counters.live_targets.fetch_sub(1, Ordering::SeqCst);
        // SAFETY: balances the retain taken in `new`.
        unsafe {
            let () = msg_send![self.layer, release];
        }
    }
}

/// Runs `f` inside an Objective-C autorelease pool. Threads without a run loop
/// (the render thread, test threads) have no pool, so every drawable and every
/// autoreleased object that wgpu and Core Animation hand out would stay alive.
pub fn autoreleasepool<T>(f: impl FnOnce() -> T) -> T {
    let mut out = None;
    objc::rc::autoreleasepool(|| out = Some(f()));
    out.expect("autoreleasepool ran the closure")
}
