//! The two presenter paths behind the C interface.
use std::ffi::c_void;
use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use objc::runtime::Object;
use objc::{msg_send, sel, sel_impl};
use op_gpu::presenter::{ViewParams, ViewRenderer};

use crate::color::{self, ColorSpace};

pub const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8Unorm;
pub const LUT_N: u32 = 33;
const BACKGROUND: [f32; 4] = [51.0 / 255.0, 51.0 / 255.0, 51.0 / 255.0, 1.0];

#[derive(Debug)]
pub enum SpikeError {
    Invalid(String),
    Gpu(String),
    Busy,
    NoFrame,
    Surface(String),
    SizeMismatch,
    Suspended,
}
impl std::fmt::Display for SpikeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Invalid(m) => write!(f, "invalid: {m}"),
            Self::Gpu(m) => write!(f, "gpu: {m}"),
            Self::Busy => write!(f, "busy: every ring slot is in use"),
            Self::NoFrame => write!(f, "no finished frame"),
            Self::Surface(m) => write!(f, "surface: {m}"),
            Self::SizeMismatch => write!(f, "ring surfaces do not match the view size"),
            Self::Suspended => write!(f, "drawable pipeline is suspended"),
        }
    }
}
pub type SResult<T> = Result<T, SpikeError>;

#[derive(Default)]
pub struct ErrorLog {
    pub count: AtomicU64,
    pub last: Mutex<String>,
}

pub struct Context {
    pub instance: wgpu::Instance,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub errors: Arc<ErrorLog>,
    pub live_presenters: Arc<AtomicU64>,
    pub live_ring_textures: Arc<AtomicU64>,
    pub live_surfaces: Arc<AtomicU64>,
}

impl Context {
    pub fn new() -> SResult<Self> {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::METAL,
            flags: wgpu::InstanceFlags::from_build_config().with_env(),
            ..Default::default()
        });
        let adapter =
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
                .map_err(|e| SpikeError::Gpu(format!("adapter: {e}")))?;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("presenter spike"),
            required_limits: adapter.limits(),
            ..Default::default()
        }))
        .map_err(|e| SpikeError::Gpu(format!("device: {e}")))?;
        let errors = Arc::new(ErrorLog::default());
        let log = errors.clone();
        device.on_uncaptured_error(Arc::new(move |e: wgpu::Error| {
            log.count.fetch_add(1, Ordering::SeqCst);
            if let Ok(mut last) = log.last.lock() {
                *last = e.to_string();
            }
            eprintln!("op-present-spike: wgpu uncaptured error: {e}");
        }));
        Ok(Self {
            instance,
            device,
            queue,
            errors,
            live_presenters: Arc::new(AtomicU64::new(0)),
            live_ring_textures: Arc::new(AtomicU64::new(0)),
            live_surfaces: Arc::new(AtomicU64::new(0)),
        })
    }

    pub fn wait_idle(&self) -> SResult<()> {
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map(|_| ())
            .map_err(|e| SpikeError::Gpu(format!("poll: {e}")))
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    pub width_pt: f64,
    pub height_pt: f64,
    pub scale: f64,
    pub zoom: f64,
    pub origin_x: f64,
    pub origin_y: f64,
}

impl View {
    pub fn pixels(&self) -> (u32, u32) {
        let px = |v: f64| ((v * self.scale).round() as i64).clamp(1, 16384) as u32;
        (px(self.width_pt), px(self.height_pt))
    }
    fn params(&self) -> ViewParams {
        ViewParams {
            zoom: self.zoom as f32,
            origin_x: self.origin_x as f32,
            origin_y: self.origin_y as f32,
            background: BACKGROUND,
        }
    }
}

#[derive(Default, Clone)]
pub struct Stats {
    pub frames: u64,
    pub configures: u64,
    pub tag_kept_by_configure: u64,
    pub tag_changed_by_configure: u64,
    pub retags: u64,
    pub surface_errors: u64,
    pub ring_violations: u64,
    pub last_acquire_ms: f64,
    pub last_render_ms: f64,
    pub last_error: String,
}

pub struct Frame {
    pub slot: u32,
    pub width: u32,
    pub height: u32,
    pub seq: u64,
    pub doc_generation: u64,
    pub iosurface: *mut c_void,
    pub acquire_ms: f64,
    pub render_ms: f64,
}

pub struct Presenter {
    pub ctx: *const Context,
    renderer: ViewRenderer,
    doc_icc: Vec<u8>,
    doc_generation: u64,
    view: View,
    seq: u64,
    pub stats: Stats,
    pub target: Target,
}

pub enum Target {
    Layer(LayerTarget),
    Ring(RingTarget),
}

/// A wgpu surface that keeps the context's live-surface count.
pub struct CountedSurface {
    surface: wgpu::Surface<'static>,
    live: Arc<AtomicU64>,
}

impl Drop for CountedSurface {
    fn drop(&mut self) {
        self.live.fetch_sub(1, Ordering::SeqCst);
    }
}

impl std::ops::Deref for CountedSurface {
    type Target = wgpu::Surface<'static>;
    fn deref(&self) -> &Self::Target {
        &self.surface
    }
}

pub struct LayerTarget {
    layer: *mut Object, // retained
    surface: Option<CountedSurface>,
    configured: Option<(u32, u32)>,
    display_icc: Option<Vec<u8>>,
    display_space: Option<ColorSpace>,
    capture: bool,
    captured: Option<(u32, u32, Vec<u8>)>,
}

const FREE: u8 = 0;
const WRITING: u8 = 1;
const READY: u8 = 2;
const PRESENTING: u8 = 3;

struct Slot {
    texture: wgpu::Texture,
    iosurface: *mut c_void,
    width: u32,
    height: u32,
    state: Arc<AtomicU8>,
    seq: u64,
    doc_generation: u64,
    submitted_at: Option<Instant>,
    done_at: Arc<Mutex<Option<Instant>>>,
}

pub struct RingTarget {
    slots: Vec<Slot>,
    live: Arc<AtomicU64>,
    suspended: bool,
}

impl Drop for RingTarget {
    fn drop(&mut self) {
        self.live
            .fetch_sub(self.slots.len() as u64, Ordering::SeqCst);
    }
}

impl Drop for LayerTarget {
    fn drop(&mut self) {
        self.surface = None;
        // SAFETY: balances the retain taken in Presenter::new_layer.
        unsafe {
            let () = msg_send![self.layer, release];
        }
    }
}

impl Drop for Presenter {
    fn drop(&mut self) {
        // SAFETY: the context outlives its presenters (the C contract).
        let ctx = unsafe { &*self.ctx };
        ctx.live_presenters.fetch_sub(1, Ordering::SeqCst);
    }
}

impl Presenter {
    fn new(ctx: &Context, target: Target) -> Self {
        ctx.live_presenters.fetch_add(1, Ordering::SeqCst);
        Self {
            ctx,
            renderer: ViewRenderer::new(&ctx.device, FORMAT),
            doc_icc: Vec::new(),
            doc_generation: 0,
            view: View {
                width_pt: 1.0,
                height_pt: 1.0,
                scale: 1.0,
                zoom: 1.0,
                origin_x: 0.0,
                origin_y: 0.0,
            },
            seq: 0,
            stats: Stats::default(),
            target,
        }
    }

    fn ctx(&self) -> &Context {
        // SAFETY: the context outlives its presenters (the C contract).
        unsafe { &*self.ctx }
    }

    /// # Safety
    /// `layer` must be a live CAMetalLayer.
    pub unsafe fn new_layer(ctx: &Context, layer: *mut Object) -> SResult<Self> {
        // SAFETY: class check before use; retain is balanced in Drop.
        unsafe {
            let class = objc::runtime::Class::get("CAMetalLayer")
                .ok_or_else(|| SpikeError::Invalid("no CAMetalLayer class".into()))?;
            let ok: bool = msg_send![layer, isKindOfClass: class];
            if !ok {
                return Err(SpikeError::Invalid("not a CAMetalLayer".into()));
            }
            let _: *mut Object = msg_send![layer, retain];
        }
        let mut target = LayerTarget {
            layer,
            surface: None,
            configured: None,
            display_icc: None,
            display_space: None,
            capture: false,
            captured: None,
        };
        target.surface = Some(Self::make_surface(ctx, layer)?);
        Ok(Self::new(ctx, Target::Layer(target)))
    }

    fn make_surface(ctx: &Context, layer: *mut Object) -> SResult<CountedSurface> {
        // SAFETY: the presenter keeps the layer retained for the surface's lifetime.
        let surface = unsafe {
            ctx.instance
                .create_surface_unsafe(wgpu::SurfaceTargetUnsafe::CoreAnimationLayer(
                    layer as *mut c_void,
                ))
        }
        .map_err(|e| SpikeError::Gpu(format!("create surface: {e}")))?;
        ctx.live_surfaces.fetch_add(1, Ordering::SeqCst);
        Ok(CountedSurface {
            surface,
            live: ctx.live_surfaces.clone(),
        })
    }

    pub fn new_ring(ctx: &Context) -> Self {
        Self::new(
            ctx,
            Target::Ring(RingTarget {
                slots: Vec::new(),
                live: ctx.live_ring_textures.clone(),
                suspended: false,
            }),
        )
    }

    pub fn set_document(
        &mut self,
        rgba: &[u8],
        width: u32,
        height: u32,
        icc: &[u8],
    ) -> SResult<()> {
        if ColorSpace::from_icc(icc).is_none() {
            return Err(SpikeError::Invalid("document ICC is not readable".into()));
        }
        let ctx = self.ctx;
        // SAFETY: see ctx().
        let ctx = unsafe { &*ctx };
        self.renderer
            .set_document(&ctx.device, &ctx.queue, width, height, rgba)
            .map_err(|e| SpikeError::Invalid(e.to_string()))?;
        self.doc_icc = icc.to_vec();
        self.doc_generation += 1;
        self.rebuild_lut()
    }

    pub fn set_display_profile(&mut self, icc: &[u8]) -> SResult<()> {
        let Target::Layer(t) = &mut self.target else {
            return Err(SpikeError::Invalid(
                "path B does not convert to the display".into(),
            ));
        };
        let space = ColorSpace::from_icc(icc)
            .ok_or_else(|| SpikeError::Invalid("display ICC is not readable".into()))?;
        t.display_icc = Some(icc.to_vec());
        t.display_space = Some(space);
        self.ensure_tag();
        self.rebuild_lut()
    }

    fn rebuild_lut(&mut self) -> SResult<()> {
        let Target::Layer(t) = &self.target else {
            return Ok(());
        };
        let (Some(display), false) = (t.display_icc.as_ref(), self.doc_icc.is_empty()) else {
            return Ok(());
        };
        let lut = color::build_lut(&self.doc_icc, display, LUT_N).map_err(SpikeError::Invalid)?;
        let ctx = self.ctx;
        // SAFETY: see ctx().
        let ctx = unsafe { &*ctx };
        self.renderer.set_lut(&ctx.device, &ctx.queue, Some(&lut));
        Ok(())
    }

    /// Path A: makes sure the layer carries the display tag; counts a retag.
    fn ensure_tag(&mut self) {
        let Target::Layer(t) = &self.target else {
            return;
        };
        let Some(space) = &t.display_space else {
            return;
        };
        // SAFETY: layer is retained.
        let tag = unsafe { color::layer_tag(t.layer) };
        if tag.icc.as_deref() != t.display_icc.as_deref() {
            // SAFETY: layer is retained.
            unsafe { color::set_layer_tag(t.layer, space) };
            self.stats.retags += 1;
        }
    }

    pub fn set_view(&mut self, view: View) -> SResult<()> {
        let ok = |v: f64| v.is_finite() && v > 0.0;
        if !ok(view.width_pt) || !ok(view.height_pt) || !ok(view.scale) || !ok(view.zoom) {
            return Err(SpikeError::Invalid(format!("bad view {view:?}")));
        }
        if !view.origin_x.is_finite() || !view.origin_y.is_finite() {
            return Err(SpikeError::Invalid(format!("bad view {view:?}")));
        }
        self.view = view;
        Ok(())
    }

    pub fn set_capture(&mut self, on: bool) -> SResult<()> {
        match &mut self.target {
            Target::Layer(t) => {
                t.capture = on;
                Ok(())
            }
            Target::Ring(_) => Err(SpikeError::Invalid(
                "path B: read the IOSurface instead".into(),
            )),
        }
    }

    pub fn captured(&self) -> Option<&(u32, u32, Vec<u8>)> {
        match &self.target {
            Target::Layer(t) => t.captured.as_ref(),
            Target::Ring(_) => None,
        }
    }

    fn configure(&mut self, width: u32, height: u32) -> SResult<()> {
        let ctx = self.ctx;
        // SAFETY: see ctx().
        let ctx = unsafe { &*ctx };
        let Target::Layer(t) = &mut self.target else {
            unreachable!()
        };
        let surface = t.surface.as_ref().ok_or(SpikeError::Suspended)?;
        // SAFETY: layer is retained.
        let before = unsafe { color::layer_tag(t.layer) };
        surface.configure(
            &ctx.device,
            &wgpu::SurfaceConfiguration {
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                format: FORMAT,
                width,
                height,
                present_mode: wgpu::PresentMode::Fifo,
                desired_maximum_frame_latency: 2,
                alpha_mode: wgpu::CompositeAlphaMode::Opaque,
                view_formats: vec![],
            },
        );
        // SAFETY: layer is retained.
        let after = unsafe { color::layer_tag(t.layer) };
        if before.icc == after.icc && before.name == after.name {
            self.stats.tag_kept_by_configure += 1;
        } else {
            self.stats.tag_changed_by_configure += 1;
        }
        t.configured = Some((width, height));
        self.stats.configures += 1;
        self.ensure_tag();
        Ok(())
    }

    pub fn render(&mut self) -> SResult<Frame> {
        if self.renderer.document_size().is_none() {
            return Err(SpikeError::Invalid("no document".into()));
        }
        self.seq += 1;
        let result = match self.target {
            Target::Layer(_) => self.render_layer(),
            Target::Ring(_) => self.render_ring(),
        };
        match &result {
            Ok(f) => {
                self.stats.frames += 1;
                self.stats.last_acquire_ms = f.acquire_ms;
                self.stats.last_render_ms = f.render_ms;
            }
            Err(e) => self.stats.last_error = e.to_string(),
        }
        result
    }

    fn render_layer(&mut self) -> SResult<Frame> {
        let (width, height) = self.view.pixels();
        let configured = match &self.target {
            Target::Layer(t) => {
                if t.surface.is_none() {
                    return Err(SpikeError::Suspended);
                }
                t.configured
            }
            Target::Ring(_) => unreachable!(),
        };
        if configured != Some((width, height)) {
            self.configure(width, height)?;
        }
        self.ensure_tag();
        let t0 = Instant::now();
        let mut texture = None;
        for _ in 0..2 {
            let Target::Layer(t) = &self.target else {
                unreachable!()
            };
            match t.surface.as_ref().unwrap().get_current_texture() {
                Ok(tex) => {
                    texture = Some(tex);
                    break;
                }
                Err(e @ (wgpu::SurfaceError::Outdated | wgpu::SurfaceError::Lost)) => {
                    self.stats.surface_errors += 1;
                    self.stats.last_error = format!("acquire: {e}");
                    self.configure(width, height)?;
                }
                Err(e) => {
                    self.stats.surface_errors += 1;
                    return Err(SpikeError::Surface(format!("acquire: {e}")));
                }
            }
        }
        let frame = texture.ok_or_else(|| SpikeError::Surface("acquire failed twice".into()))?;
        let t1 = Instant::now();
        let ctx = self.ctx;
        // SAFETY: see ctx().
        let ctx = unsafe { &*ctx };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = ctx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        self.renderer
            .encode(
                &ctx.device,
                &ctx.queue,
                &mut encoder,
                &view,
                &self.view.params(),
            )
            .map_err(|e| SpikeError::Gpu(e.to_string()))?;
        let Target::Layer(t) = &mut self.target else {
            unreachable!()
        };
        let capture = t.capture.then(|| {
            let row = (width * 4).div_ceil(256) * 256;
            let buffer = ctx.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("drawable capture"),
                size: row as u64 * height as u64,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            encoder.copy_texture_to_buffer(
                frame.texture.as_image_copy(),
                wgpu::TexelCopyBufferInfo {
                    buffer: &buffer,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(row),
                        rows_per_image: Some(height),
                    },
                },
                wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
            );
            (buffer, row)
        });
        ctx.queue.submit(Some(encoder.finish()));
        frame.present();
        ctx.wait_idle()?;
        let t2 = Instant::now();
        if let Some((buffer, row)) = capture {
            let (tx, rx) = std::sync::mpsc::channel();
            buffer.map_async(wgpu::MapMode::Read, .., move |r| {
                let _ = tx.send(r);
            });
            ctx.wait_idle()?;
            rx.recv()
                .map_err(|e| SpikeError::Gpu(e.to_string()))?
                .map_err(|e| SpikeError::Gpu(e.to_string()))?;
            let mapped = buffer.get_mapped_range(..);
            let mut bytes = Vec::with_capacity((width * height * 4) as usize);
            for y in 0..height as usize {
                let start = y * row as usize;
                bytes.extend_from_slice(&mapped[start..start + width as usize * 4]);
            }
            drop(mapped);
            buffer.unmap();
            t.captured = Some((width, height, bytes));
        }
        Ok(Frame {
            slot: 0,
            width,
            height,
            seq: self.seq,
            doc_generation: self.doc_generation,
            iosurface: std::ptr::null_mut(),
            acquire_ms: (t1 - t0).as_secs_f64() * 1e3,
            render_ms: (t2 - t1).as_secs_f64() * 1e3,
        })
    }

    /// # Safety
    /// Every pointer must be a live IOSurfaceRef with a BGRA8 pixel format.
    pub unsafe fn set_ring(&mut self, surfaces: &[*mut c_void]) -> SResult<()> {
        if !(2..=3).contains(&surfaces.len()) || surfaces.iter().any(|s| s.is_null()) {
            return Err(SpikeError::Invalid("ring needs 2 or 3 IOSurfaces".into()));
        }
        let ctx = self.ctx;
        // SAFETY: see ctx().
        let ctx = unsafe { &*ctx };
        let Target::Ring(ring) = &mut self.target else {
            return Err(SpikeError::Invalid("path A has no ring".into()));
        };
        if ring
            .slots
            .iter()
            .any(|s| s.state.load(Ordering::SeqCst) == PRESENTING)
        {
            return Err(SpikeError::Busy);
        }
        ctx.wait_idle()?;
        let mut slots = Vec::new();
        for &surface in surfaces {
            // SAFETY: caller contract.
            slots.push(unsafe { wrap_iosurface(ctx, surface)? });
        }
        ring.live
            .fetch_sub(ring.slots.len() as u64, Ordering::SeqCst);
        ring.live.fetch_add(slots.len() as u64, Ordering::SeqCst);
        ring.slots = slots;
        ring.suspended = false;
        Ok(())
    }

    fn render_ring(&mut self) -> SResult<Frame> {
        let ctx = self.ctx;
        // SAFETY: see ctx().
        let ctx = unsafe { &*ctx };
        let (width, height) = self.view.pixels();
        let Target::Ring(ring) = &mut self.target else {
            unreachable!()
        };
        if ring.suspended || ring.slots.is_empty() {
            return Err(SpikeError::Suspended);
        }
        if ring
            .slots
            .iter()
            .any(|s| (s.width, s.height) != (width, height))
        {
            return Err(SpikeError::SizeMismatch);
        }
        // Prefer a free slot; otherwise recycle the oldest unshown ready frame.
        let pick = ring
            .slots
            .iter()
            .position(|s| s.state.load(Ordering::SeqCst) == FREE)
            .or_else(|| {
                ring.slots
                    .iter()
                    .enumerate()
                    .filter(|(_, s)| s.state.load(Ordering::SeqCst) == READY)
                    .min_by_key(|(_, s)| s.seq)
                    .map(|(i, _)| i)
            })
            .ok_or(SpikeError::Busy)?;
        let slot = &mut ring.slots[pick];
        let prev = slot.state.swap(WRITING, Ordering::SeqCst);
        if prev != FREE && prev != READY {
            self.stats.ring_violations += 1;
            slot.state.store(prev, Ordering::SeqCst);
            return Err(SpikeError::Busy);
        }
        slot.seq = self.seq;
        slot.doc_generation = self.doc_generation;
        let view = slot
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = ctx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        let t1 = Instant::now();
        self.renderer
            .encode(
                &ctx.device,
                &ctx.queue,
                &mut encoder,
                &view,
                &self.view.params(),
            )
            .map_err(|e| SpikeError::Gpu(e.to_string()))?;
        ctx.queue.submit(Some(encoder.finish()));
        let state = slot.state.clone();
        let done = slot.done_at.clone();
        *done.lock().unwrap() = None;
        slot.submitted_at = Some(t1);
        // The explicit completion signal: only the GPU finishing moves WRITING -> READY.
        ctx.queue.on_submitted_work_done(move || {
            *done.lock().unwrap() = Some(Instant::now());
            let _ = state.compare_exchange(WRITING, READY, Ordering::SeqCst, Ordering::SeqCst);
        });
        Ok(Frame {
            slot: pick as u32,
            width,
            height,
            seq: self.seq,
            doc_generation: self.doc_generation,
            iosurface: slot.iosurface,
            acquire_ms: 0.0,
            render_ms: (Instant::now() - t1).as_secs_f64() * 1e3,
        })
    }

    pub fn ring_acquire(&mut self, wait: bool) -> SResult<Frame> {
        let ctx = self.ctx;
        // SAFETY: see ctx().
        let ctx = unsafe { &*ctx };
        let poll = if wait {
            wgpu::PollType::wait_indefinitely()
        } else {
            wgpu::PollType::Poll
        };
        ctx.device
            .poll(poll)
            .map_err(|e| SpikeError::Gpu(format!("poll: {e}")))?;
        let Target::Ring(ring) = &mut self.target else {
            return Err(SpikeError::Invalid("path A has no ring".into()));
        };
        let Some(newest) = ring
            .slots
            .iter()
            .enumerate()
            .filter(|(_, s)| s.state.load(Ordering::SeqCst) == READY)
            .max_by_key(|(_, s)| s.seq)
            .map(|(i, _)| i)
        else {
            return Err(SpikeError::NoFrame);
        };
        // Older ready frames are stale: hand them back to the engine.
        let newest_seq = ring.slots[newest].seq;
        for s in &ring.slots {
            if s.seq < newest_seq {
                let _ = s
                    .state
                    .compare_exchange(READY, FREE, Ordering::SeqCst, Ordering::SeqCst);
            }
        }
        let slot = &ring.slots[newest];
        if slot
            .state
            .compare_exchange(READY, PRESENTING, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            self.stats.ring_violations += 1;
            return Err(SpikeError::NoFrame);
        }
        let render_ms = match (slot.submitted_at, *slot.done_at.lock().unwrap()) {
            (Some(a), Some(b)) => (b - a).as_secs_f64() * 1e3,
            _ => 0.0,
        };
        self.stats.last_render_ms = render_ms;
        Ok(Frame {
            slot: newest as u32,
            width: slot.width,
            height: slot.height,
            seq: slot.seq,
            doc_generation: slot.doc_generation,
            iosurface: slot.iosurface,
            acquire_ms: 0.0,
            render_ms,
        })
    }

    pub fn ring_release(&mut self, index: u32) -> SResult<()> {
        let Target::Ring(ring) = &mut self.target else {
            return Err(SpikeError::Invalid("path A has no ring".into()));
        };
        let slot = ring
            .slots
            .get(index as usize)
            .ok_or_else(|| SpikeError::Invalid(format!("no slot {index}")))?;
        if slot
            .state
            .compare_exchange(PRESENTING, FREE, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            self.stats.ring_violations += 1;
            return Err(SpikeError::Invalid(format!(
                "slot {index} was not being presented"
            )));
        }
        Ok(())
    }

    pub fn suspend(&mut self) -> SResult<()> {
        let ctx = self.ctx;
        // SAFETY: see ctx().
        let ctx = unsafe { &*ctx };
        ctx.wait_idle()?;
        match &mut self.target {
            Target::Layer(t) => {
                t.surface = None;
                t.configured = None;
            }
            Target::Ring(ring) => {
                if ring
                    .slots
                    .iter()
                    .any(|s| s.state.load(Ordering::SeqCst) == PRESENTING)
                {
                    return Err(SpikeError::Busy);
                }
                ring.live
                    .fetch_sub(ring.slots.len() as u64, Ordering::SeqCst);
                ring.slots.clear();
                ring.suspended = true;
            }
        }
        Ok(())
    }

    pub fn resume(&mut self) -> SResult<()> {
        let ctx = self.ctx;
        // SAFETY: see ctx().
        let ctx = unsafe { &*ctx };
        if let Target::Layer(t) = &mut self.target
            && t.surface.is_none()
        {
            t.surface = Some(Self::make_surface(ctx, t.layer)?);
            t.configured = None;
        }
        Ok(())
    }

    pub fn doc_generation(&self) -> u64 {
        self.doc_generation
    }

    /// Path A: what the CAMetalLayer looks like right now.
    pub fn layer_readback(&self) -> Option<LayerReadback> {
        let Target::Layer(t) = &self.target else {
            return None;
        };
        // SAFETY: layer is retained.
        unsafe {
            let size: core_graphics_types::geometry::CGSize = msg_send![t.layer, drawableSize];
            let format: u64 = msg_send![t.layer, pixelFormat];
            let fb_only: bool = msg_send![t.layer, framebufferOnly];
            let tag = color::layer_tag(t.layer);
            Some(LayerReadback {
                width: size.width as u32,
                height: size.height as u32,
                pixel_format: format as u32,
                framebuffer_only: fb_only,
                tag_is_display: t.display_icc.is_some() && tag.icc == t.display_icc,
                name: tag.name,
            })
        }
    }

    pub fn validation_errors(&self) -> u64 {
        self.ctx().errors.count.load(Ordering::SeqCst)
    }
}

pub struct LayerReadback {
    pub width: u32,
    pub height: u32,
    pub pixel_format: u32,
    pub framebuffer_only: bool,
    pub tag_is_display: bool,
    pub name: String,
}

#[link(name = "IOSurface", kind = "framework")]
unsafe extern "C" {
    fn IOSurfaceGetWidth(s: *mut c_void) -> usize;
    fn IOSurfaceGetHeight(s: *mut c_void) -> usize;
    fn IOSurfaceGetPixelFormat(s: *mut c_void) -> u32;
}

const BGRA: u32 = u32::from_be_bytes(*b"BGRA");

/// Wraps an IOSurface as a wgpu render target through wgpu-hal's Metal backend.
unsafe fn wrap_iosurface(ctx: &Context, surface: *mut c_void) -> SResult<Slot> {
    use metal::foreign_types::ForeignType;
    // SAFETY: caller passes a live IOSurfaceRef.
    let (width, height, format) = unsafe {
        (
            IOSurfaceGetWidth(surface) as u32,
            IOSurfaceGetHeight(surface) as u32,
            IOSurfaceGetPixelFormat(surface),
        )
    };
    if width == 0 || height == 0 || format != BGRA {
        return Err(SpikeError::Invalid(format!(
            "IOSurface must be BGRA8, got {width}x{height} format {format:#x}"
        )));
    }
    // SAFETY: the hal device is Metal (the instance only enables Metal); the new texture
    // is +1 retained and handed to metal::Texture, which releases it on drop.
    let texture = unsafe {
        let hal = ctx
            .device
            .as_hal::<wgpu::hal::api::Metal>()
            .ok_or_else(|| SpikeError::Gpu("not a Metal device".into()))?;
        let raw_device = hal.raw_device().lock();
        let desc = metal::TextureDescriptor::new();
        desc.set_texture_type(metal::MTLTextureType::D2);
        desc.set_pixel_format(metal::MTLPixelFormat::BGRA8Unorm);
        desc.set_width(width as u64);
        desc.set_height(height as u64);
        desc.set_storage_mode(metal::MTLStorageMode::Shared);
        desc.set_usage(metal::MTLTextureUsage::RenderTarget | metal::MTLTextureUsage::ShaderRead);
        let raw: *mut metal::MTLTexture = msg_send![raw_device.as_ref(),
            newTextureWithDescriptor: desc.as_ref()
            iosurface: surface
            plane: 0usize];
        if raw.is_null() {
            return Err(SpikeError::Gpu(
                "newTextureWithDescriptor:iosurface: failed".into(),
            ));
        }
        let raw = metal::Texture::from_ptr(raw);
        let hal_texture = wgpu::hal::metal::Device::texture_from_raw(
            raw,
            FORMAT,
            metal::MTLTextureType::D2,
            1,
            1,
            wgpu::hal::CopyExtent {
                width,
                height,
                depth: 1,
            },
        );
        drop(raw_device);
        ctx.device.create_texture_from_hal::<wgpu::hal::api::Metal>(
            hal_texture,
            &wgpu::TextureDescriptor {
                label: Some("IOSurface ring slot"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            },
        )
    };
    Ok(Slot {
        texture,
        iosurface: surface,
        width,
        height,
        state: Arc::new(AtomicU8::new(FREE)),
        seq: 0,
        doc_generation: 0,
        submitted_at: None,
        done_at: Arc::new(Mutex::new(None)),
    })
}
