//! Canvas views (M12): `op_view_attach`, `op_view_set`, `op_view_detach` and the
//! read-only helpers the app and its tests use.
//!
//! How it works:
//! - Each engine gets one view host the first time a view is attached. The host
//!   owns one wgpu device (Metal) and one render thread, which is the only thread
//!   that draws (ARCHITECTURE section 4: one render thread owns the GPU queue).
//! - A view is a retained `CAMetalLayer` (op-gpu `layer::MetalLayerTarget`) plus an
//!   op-gpu `ViewRenderer` holding the document texture, its mips and the
//!   document -> display table built by op-color from the document profile and
//!   the display ICC that Swift sends.
//! - `op_view_set` only stores the newest state and wakes the render thread, so it
//!   never blocks on the GPU. States that arrive between two frames are merged:
//!   only the newest is drawn.
//! - The render thread also redraws a view when its document's revision changes
//!   (checked every 50 ms while idle).
//! - `op_view_detach`, `op_doc_close` (for the document's views) and
//!   `op_engine_free` return only after the view's in-flight frame has finished
//!   and its layer has been released (ARCHITECTURE section 6).
use crate::{OpEngine, OpStatus, engine, guard};
use std::collections::{HashMap, VecDeque};
use std::ffi::c_void;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use parking_lot::{Condvar, Mutex};

/// `OpViewState::flags`: draw the pixel grid (shown only at 400% and above).
pub const OP_VIEW_PIXEL_GRID: u32 = 1;
/// `OpViewState::flags`: the Mac is going to sleep. The engine drops the view's
/// drawable surface and refuses frames until a state without this flag arrives.
pub const OP_VIEW_SUSPENDED: u32 = 2;

/// Where and how a view shows its document. Sizes and positions are in points;
/// the drawable is `width_pt * scale` by `height_pt * scale` pixels.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct OpViewState {
    pub width_pt: f64,
    pub height_pt: f64,
    /// Backing scale factor of the window's screen (1 or 2 on current Macs).
    pub scale: f64,
    /// Points per document pixel: 1.0 is 100%. Allowed range 0.001 to 64.
    pub zoom: f64,
    /// Position of the document's top-left corner, in points from the view's
    /// top-left corner (y grows down).
    pub origin_x: f64,
    pub origin_y: f64,
    /// ICC bytes of the window screen's color space, borrowed for the call.
    /// Length 0 keeps the profile sent before. Until one is sent, no display
    /// conversion happens and the layer keeps whatever tag it has.
    pub display_icc: *const u8,
    pub display_icc_len: usize,
    /// `OP_VIEW_PIXEL_GRID` | `OP_VIEW_SUSPENDED`.
    pub flags: u32,
}

/// What a view holds now (for tests and diagnostics).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct OpViewInfo {
    pub doc: u64,
    pub width_pt: f64,
    pub height_pt: f64,
    pub scale: f64,
    pub zoom: f64,
    pub origin_x: f64,
    pub origin_y: f64,
    pub flags: u32,
    /// 1 when the layer's colorspace equals the display ICC sent last.
    pub layer_tag_is_display: u8,
    /// 1 when a document -> display table is in use.
    pub has_display_table: u8,
    pub suspended: u8,
    pub _pad: u8,
    pub display_icc_len: u64,
    /// Frames presented since attach.
    pub frames: u64,
    pub drawable_width: u32,
    pub drawable_height: u32,
    pub configures: u64,
    pub tag_changed_by_configure: u64,
    pub retags: u64,
    pub table_builds: u64,
    pub mip_builds: u64,
    pub doc_uploads: u64,
    pub doc_revision: u64,
    /// Frames that failed (surface, color or upload errors, panics).
    pub frame_errors: u64,
    /// OpStatus of the last failed frame, or OP_OK.
    pub last_error: u32,
}

/// Engine-wide view counters, for leak checks. All zero before the first attach.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OpViewCounters {
    pub live_views: u64,
    /// CAMetalLayers the engine retains.
    pub live_layers: u64,
    pub live_surfaces: u64,
    pub wgpu_textures: u64,
    pub wgpu_texture_views: u64,
    pub wgpu_buffers: u64,
    pub wgpu_bind_groups: u64,
    pub validation_errors: u64,
    pub frames_total: u64,
}

/// Fill outside the document, in display encoding (a classic mid grey).
const BACKGROUND: [f32; 4] = [160.0 / 255.0, 160.0 / 255.0, 160.0 / 255.0, 1.0];
/// Points per checkerboard square.
const CHECKER_PT: f64 = 8.0;
/// The pixel grid shows at 400% and above.
const GRID_MIN_ZOOM: f64 = 4.0;
const LUT_N: u32 = 33;
/// Present timestamps kept per view until read.
const FRAME_LOG_CAP: usize = 16384;

#[derive(Clone)]
struct Request {
    width_pt: f64,
    height_pt: f64,
    scale: f64,
    zoom: f64,
    origin_x: f64,
    origin_y: f64,
    flags: u32,
    display_icc: Option<Arc<[u8]>>,
    /// Bumped whenever the display ICC bytes change.
    display_gen: u64,
    /// False until the first op_view_set.
    set: bool,
}

#[cfg(target_os = "macos")]
struct Render {
    target: op_gpu::layer::MetalLayerTarget,
    renderer: op_gpu::presenter::ViewRenderer,
    doc_revision: Option<u64>,
    doc_profile: Option<Arc<[u8]>>,
    display_gen: u64,
    has_table: bool,
    table_builds: u64,
    doc_uploads: u64,
    frame_errors: u64,
    last_error: OpStatus,
}

#[cfg(not(target_os = "macos"))]
struct Render;

struct FrameLog {
    frames: u64,
    times_ms: VecDeque<f64>,
}

struct View {
    doc: u64,
    created: Instant,
    request: Mutex<Request>,
    dirty: std::sync::atomic::AtomicBool,
    /// None after detach. The render thread holds this lock for a whole frame.
    render: Mutex<Option<Render>>,
    log: Mutex<FrameLog>,
    frame_cv: Condvar,
}

struct Gpu {
    instance: wgpu::Instance,
    device: wgpu::Device,
    queue: wgpu::Queue,
    validation_errors: Arc<AtomicU64>,
}

impl Gpu {
    fn new() -> Result<Self, String> {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::METAL,
            flags: wgpu::InstanceFlags::from_build_config().with_env(),
            ..Default::default()
        });
        let adapter =
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
                .map_err(|e| format!("adapter: {e}"))?;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("OpenPainter canvas"),
            required_limits: adapter.limits(),
            ..Default::default()
        }))
        .map_err(|e| format!("device: {e}"))?;
        let validation_errors = Arc::new(AtomicU64::new(0));
        let count = validation_errors.clone();
        device.on_uncaptured_error(Arc::new(move |e: wgpu::Error| {
            count.fetch_add(1, Ordering::SeqCst);
            eprintln!("op-ffi view: wgpu uncaptured error: {e}");
        }));
        Ok(Self {
            instance,
            device,
            queue,
            validation_errors,
        })
    }
}

#[derive(Default)]
struct HostState {
    views: HashMap<u64, Arc<View>>,
    stop: bool,
}

struct Host {
    /// The owning `OpEngine`, valid until `engine_freeing` has joined the thread.
    engine: usize,
    state: Mutex<HostState>,
    wake: Condvar,
    thread: Mutex<Option<JoinHandle<()>>>,
    gpu: Mutex<Option<Arc<Gpu>>>,
    #[cfg(target_os = "macos")]
    counters: Arc<op_gpu::layer::LayerCounters>,
    next_view: AtomicU64,
    frames_total: AtomicU64,
}

/// One host per live engine, keyed by the engine's address.
static HOSTS: Mutex<Vec<(usize, Arc<Host>)>> = Mutex::new(Vec::new());

fn find_host(p: *mut OpEngine) -> Option<Arc<Host>> {
    HOSTS
        .lock()
        .iter()
        .find(|(k, _)| *k == p as usize)
        .map(|(_, h)| h.clone())
}

fn host_for(p: *mut OpEngine) -> Arc<Host> {
    let mut hosts = HOSTS.lock();
    if let Some((_, h)) = hosts.iter().find(|(k, _)| *k == p as usize) {
        return h.clone();
    }
    let host = Arc::new(Host {
        engine: p as usize,
        state: Mutex::new(HostState::default()),
        wake: Condvar::new(),
        thread: Mutex::new(None),
        gpu: Mutex::new(None),
        #[cfg(target_os = "macos")]
        counters: Arc::default(),
        next_view: AtomicU64::new(1),
        frames_total: AtomicU64::new(0),
    });
    let worker = host.clone();
    *host.thread.lock() = Some(
        std::thread::Builder::new()
            .name("op-render".into())
            .spawn(move || render_loop(worker))
            .expect("spawn render thread"),
    );
    hosts.push((p as usize, host.clone()));
    host
}

impl Host {
    fn gpu(&self) -> Result<Arc<Gpu>, OpStatus> {
        let mut slot = self.gpu.lock();
        if let Some(g) = slot.as_ref() {
            return Ok(g.clone());
        }
        let g = Arc::new(Gpu::new().map_err(|e| {
            eprintln!("op-ffi view: no GPU for the canvas: {e}");
            OpStatus::OP_ERR_INTERNAL
        })?);
        *slot = Some(g.clone());
        Ok(g)
    }

    fn view(&self, id: u64) -> Option<Arc<View>> {
        self.state.lock().views.get(&id).cloned()
    }

    fn engine(&self) -> &OpEngine {
        // SAFETY: engine_freeing joins this host's thread and removes every view
        // before the OpEngine is freed, so the pointer is live while it is used.
        unsafe { &*(self.engine as *const OpEngine) }
    }
}

/// Runs `f` in an autorelease pool on macOS (drawables and surfaces are
/// autoreleased Objective-C objects; the render thread has no run loop).
fn pooled<T>(f: impl FnOnce() -> T) -> T {
    #[cfg(target_os = "macos")]
    return op_gpu::layer::autoreleasepool(f);
    #[cfg(not(target_os = "macos"))]
    f()
}

/// Removes a view and waits for its in-flight frame, then releases its layer.
fn finish_view(host: &Host, view: &View) {
    pooled(|| finish_view_inner(host, view))
}

fn finish_view_inner(host: &Host, view: &View) {
    let render = view.render.lock().take();
    if render.is_some() {
        if let Some(gpu) = host.gpu.lock().clone() {
            // Work that references the drawable finishes before the layer goes.
            let _ = gpu.device.poll(wgpu::PollType::wait_indefinitely());
        }
        drop(render);
    }
    view.frame_cv.notify_all();
}

#[cfg(target_os = "macos")]
fn raise_thread_priority() {
    unsafe extern "C" {
        fn pthread_set_qos_class_self_np(qos: u32, relative_priority: i32) -> i32;
    }
    const QOS_CLASS_USER_INTERACTIVE: u32 = 0x21;
    // SAFETY: sets the calling thread's own QoS class; no pointers involved.
    let rc = unsafe { pthread_set_qos_class_self_np(QOS_CLASS_USER_INTERACTIVE, 0) };
    if rc != 0 {
        eprintln!("op-ffi view: could not raise the render thread's QoS class ({rc})");
    }
}
#[cfg(not(target_os = "macos"))]
fn raise_thread_priority() {}

fn render_loop(host: Arc<Host>) {
    // Frames are on the user's critical path, like the main thread.
    raise_thread_priority();
    loop {
        let views: Vec<Arc<View>> = {
            let mut st = host.state.lock();
            if st.stop {
                return;
            }
            let any_dirty = st.views.values().any(|v| v.dirty.load(Ordering::Acquire));
            if !any_dirty {
                host.wake.wait_for(&mut st, Duration::from_millis(50));
            }
            if st.stop {
                return;
            }
            st.views.values().cloned().collect()
        };
        for v in views {
            if !v.dirty.load(Ordering::Acquire) && !doc_changed(&host, &v) {
                continue;
            }
            v.dirty.store(false, Ordering::Release);
            let ok = catch_unwind(AssertUnwindSafe(|| pooled(|| render_one(&host, &v))));
            if ok.is_err() {
                record_error(&v, OpStatus::OP_ERR_PANIC);
            }
        }
    }
}

#[cfg(target_os = "macos")]
fn record_error(v: &View, status: OpStatus) {
    if let Some(r) = v.render.lock().as_mut() {
        r.frame_errors += 1;
        r.last_error = status;
    }
}
#[cfg(not(target_os = "macos"))]
fn record_error(_v: &View, _status: OpStatus) {}

#[cfg(target_os = "macos")]
fn doc_changed(host: &Host, v: &View) -> bool {
    let Ok(snap) = host.engine().core().snapshot(v.doc) else {
        return false;
    };
    let seen = v.render.lock().as_ref().and_then(|r| r.doc_revision);
    seen.is_some_and(|rev| rev != snap.revision)
}
#[cfg(not(target_os = "macos"))]
fn doc_changed(_host: &Host, _v: &View) -> bool {
    false
}

#[cfg(target_os = "macos")]
fn render_one(host: &Host, v: &View) {
    use op_gpu::presenter::{CanvasParams, Lut3d, ViewParams};
    let Ok(gpu) = host.gpu() else { return };
    let req = v.request.lock().clone();
    if !req.set {
        return;
    }
    let mut guard = v.render.lock();
    let Some(r) = guard.as_mut() else { return };
    let fail = |r: &mut Render, status: OpStatus, msg: String| {
        eprintln!("op-ffi view: frame failed: {msg}");
        r.frame_errors += 1;
        r.last_error = status;
    };
    if req.flags & OP_VIEW_SUSPENDED != 0 {
        r.target.suspend();
        return;
    }
    if let Err(e) = r.target.resume(&gpu.instance) {
        return fail(r, OpStatus::OP_ERR_INTERNAL, e.to_string());
    }
    // Document pixels: uploaded on first use and whenever the revision changes.
    let mut need_table = false;
    let snap = match host.engine().core().snapshot(v.doc) {
        Ok(s) => s,
        Err(_) => return fail(r, OpStatus::OP_ERR_NO_SUCH_DOC, "document closed".into()),
    };
    if r.doc_revision != Some(snap.revision) {
        let pixels = match snap.pixels(false) {
            Ok(p) => p,
            Err(e) => return fail(r, OpStatus::OP_ERR_INTERNAL, format!("pixels: {e}")),
        };
        let (w, h) = (snap.state.width, snap.state.height);
        if let Err(e) = r
            .renderer
            .set_document(&gpu.device, &gpu.queue, w, h, &pixels)
        {
            return fail(r, OpStatus::OP_ERR_INTERNAL, format!("upload {w}x{h}: {e}"));
        }
        r.doc_uploads += 1;
        r.doc_revision = Some(snap.revision);
        if r.doc_profile != snap.state.profile {
            r.doc_profile = snap.state.profile.clone();
            need_table = true;
        }
    }
    if r.display_gen != req.display_gen {
        if let Some(icc) = &req.display_icc
            && let Err(e) = r.target.set_display_icc(icc)
        {
            return fail(r, OpStatus::OP_ERR_BAD_ARGUMENT, e.to_string());
        }
        r.display_gen = req.display_gen;
        need_table = true;
    }
    if need_table && let Some(display) = &req.display_icc {
        match display_table(r.doc_profile.as_deref(), display) {
            Ok(data) => {
                let lut = Lut3d { n: LUT_N, data };
                r.renderer.set_lut(&gpu.device, &gpu.queue, Some(&lut));
                r.has_table = true;
                r.table_builds += 1;
            }
            Err(e) => {
                // Shown unconverted; the failure is counted and printed.
                r.renderer.set_lut(&gpu.device, &gpu.queue, None);
                r.has_table = false;
                return fail(r, OpStatus::OP_ERR_INTERNAL, format!("display table: {e}"));
            }
        }
    }
    let px = |pt: f64| ((pt * req.scale).round() as i64).clamp(1, 16384) as u32;
    let (width, height) = (px(req.width_pt), px(req.height_pt));
    let canvas = CanvasParams {
        view: ViewParams {
            zoom: (req.zoom * req.scale) as f32,
            origin_x: (req.origin_x * req.scale) as f32,
            origin_y: (req.origin_y * req.scale) as f32,
            background: BACKGROUND,
        },
        checker_px: ((CHECKER_PT * req.scale).round() as u32).max(1),
        pixel_grid: req.flags & OP_VIEW_PIXEL_GRID != 0 && req.zoom >= GRID_MIN_ZOOM - 1e-9,
        use_mips: true,
    };
    let renderer = &mut r.renderer;
    let result = r
        .target
        .present(&gpu.device, &gpu.queue, width, height, |enc, target| {
            renderer
                .encode_canvas(&gpu.device, &gpu.queue, enc, target, &canvas)
                .map_err(|e| e.to_string())
        });
    if let Err(e) = result {
        return fail(r, OpStatus::OP_ERR_INTERNAL, e.to_string());
    }
    let _ = gpu.device.poll(wgpu::PollType::wait_indefinitely());
    drop(guard);
    let mut log = v.log.lock();
    log.frames += 1;
    if log.times_ms.len() == FRAME_LOG_CAP {
        log.times_ms.pop_front();
    }
    log.times_ms
        .push_back(v.created.elapsed().as_secs_f64() * 1e3);
    drop(log);
    host.frames_total.fetch_add(1, Ordering::Relaxed);
    v.frame_cv.notify_all();
}
#[cfg(not(target_os = "macos"))]
fn render_one(_host: &Host, _v: &View) {}

/// The 33^3 document -> display table; a document without a profile is sRGB.
fn display_table(doc: Option<&[u8]>, display: &[u8]) -> Result<Vec<[f32; 4]>, String> {
    let src = match doc {
        Some(bytes) => op_color::Profile::from_icc(bytes),
        None => op_color::Profile::builtin("srgb"),
    }
    .map_err(|e| format!("document profile: {e}"))?;
    let dst = op_color::Profile::from_icc(display).map_err(|e| format!("display profile: {e}"))?;
    let table = op_color::display_table(&src, &dst, LUT_N).map_err(|e| e.to_string())?;
    Ok(table.into_iter().map(|c| [c[0], c[1], c[2], 1.0]).collect())
}

/// Called first thing in `op_engine_free`: stops the render thread and releases
/// every view and layer of this engine.
pub(crate) fn engine_freeing(p: *mut OpEngine) {
    let host = {
        let mut hosts = HOSTS.lock();
        let Some(i) = hosts.iter().position(|(k, _)| *k == p as usize) else {
            return;
        };
        hosts.swap_remove(i).1
    };
    host.state.lock().stop = true;
    host.wake.notify_all();
    if let Some(j) = host.thread.lock().take() {
        let _ = j.join();
    }
    let views: Vec<Arc<View>> = host.state.lock().views.drain().map(|(_, v)| v).collect();
    for v in views {
        finish_view(&host, &v);
    }
}

/// Called by `op_doc_close` before the document goes: detaches its views.
pub(crate) fn doc_closing(p: *mut OpEngine, doc: u64) {
    let Some(host) = find_host(p) else { return };
    let views: Vec<Arc<View>> = {
        let mut st = host.state.lock();
        let ids: Vec<u64> = st
            .views
            .iter()
            .filter(|(_, v)| v.doc == doc)
            .map(|(k, _)| *k)
            .collect();
        ids.iter().filter_map(|k| st.views.remove(k)).collect()
    };
    for v in views {
        finish_view(&host, &v);
    }
}

/// Attaches a view of `doc` to a `CAMetalLayer`. The engine retains the layer
/// until the view is detached. Nothing is drawn until the first `op_view_set`.
/// Returns OP_ERR_NO_SUCH_DOC for an unknown document, OP_ERR_BAD_ARGUMENT for a
/// null pointer or an object that is not a CAMetalLayer, OP_ERR_INTERNAL when no
/// GPU device can be made, and OP_ERR_UNSUPPORTED off macOS.
#[unsafe(no_mangle)]
pub extern "C" fn op_view_attach(
    p: *mut OpEngine,
    doc: u64,
    layer: *mut c_void,
    out_view: *mut u64,
) -> OpStatus {
    guard(|| {
        if out_view.is_null() || layer.is_null() {
            return OpStatus::OP_ERR_BAD_ARGUMENT;
        }
        let e = match engine(p) {
            Ok(e) => e,
            Err(s) => return s,
        };
        if e.core().snapshot(doc).is_err() {
            return OpStatus::OP_ERR_NO_SUCH_DOC;
        }
        pooled(|| attach(p, doc, layer, out_view))
    })
}

#[cfg(target_os = "macos")]
fn attach(p: *mut OpEngine, doc: u64, layer: *mut c_void, out_view: *mut u64) -> OpStatus {
    let host = host_for(p);
    let gpu = match host.gpu() {
        Ok(g) => g,
        Err(s) => return s,
    };
    // SAFETY: the caller passes a live Objective-C object; its class is checked.
    let target = match unsafe {
        op_gpu::layer::MetalLayerTarget::new(&gpu.instance, layer, host.counters.clone())
    } {
        Ok(t) => t,
        Err(op_gpu::layer::LayerError::NotAMetalLayer) => return OpStatus::OP_ERR_BAD_ARGUMENT,
        Err(e) => {
            eprintln!("op-ffi view: attach failed: {e}");
            return OpStatus::OP_ERR_INTERNAL;
        }
    };
    let renderer = op_gpu::presenter::ViewRenderer::new(&gpu.device, op_gpu::layer::FORMAT);
    let id = host.next_view.fetch_add(1, Ordering::Relaxed);
    let view = Arc::new(View {
        doc,
        created: Instant::now(),
        request: Mutex::new(Request {
            width_pt: 0.0,
            height_pt: 0.0,
            scale: 1.0,
            zoom: 1.0,
            origin_x: 0.0,
            origin_y: 0.0,
            flags: 0,
            display_icc: None,
            display_gen: 0,
            set: false,
        }),
        dirty: false.into(),
        render: Mutex::new(Some(Render {
            target,
            renderer,
            doc_revision: None,
            doc_profile: None,
            display_gen: 0,
            has_table: false,
            table_builds: 0,
            doc_uploads: 0,
            frame_errors: 0,
            last_error: OpStatus::OP_OK,
        })),
        log: Mutex::new(FrameLog {
            frames: 0,
            times_ms: VecDeque::new(),
        }),
        frame_cv: Condvar::new(),
    });
    host.state.lock().views.insert(id, view);
    // SAFETY: checked non-null by the caller.
    unsafe { *out_view = id };
    OpStatus::OP_OK
}

#[cfg(not(target_os = "macos"))]
fn attach(_p: *mut OpEngine, _doc: u64, _layer: *mut c_void, _out: *mut u64) -> OpStatus {
    OpStatus::OP_ERR_UNSUPPORTED
}

fn checked(state: &OpViewState) -> Result<Option<&[u8]>, OpStatus> {
    let positive = |v: f64| v.is_finite() && v > 0.0;
    let ok = positive(state.width_pt)
        && positive(state.height_pt)
        && positive(state.scale)
        && state.scale <= 8.0
        && state.width_pt * state.scale <= 16384.0
        && state.height_pt * state.scale <= 16384.0
        && state.zoom.is_finite()
        && (0.001..=64.0).contains(&state.zoom)
        && state.origin_x.is_finite()
        && state.origin_y.is_finite()
        && state.origin_x.abs() < 1e9
        && state.origin_y.abs() < 1e9
        && state.flags & !(OP_VIEW_PIXEL_GRID | OP_VIEW_SUSPENDED) == 0;
    if !ok {
        return Err(OpStatus::OP_ERR_BAD_ARGUMENT);
    }
    if state.display_icc_len == 0 {
        return Ok(None);
    }
    let icc = crate::bytes(state.display_icc, state.display_icc_len)?;
    if op_color::Profile::from_icc(icc).is_err() {
        return Err(OpStatus::OP_ERR_BAD_ARGUMENT);
    }
    #[cfg(target_os = "macos")]
    if op_gpu::layer::ColorSpace::from_icc(icc).is_none() {
        return Err(OpStatus::OP_ERR_BAD_ARGUMENT);
    }
    Ok(Some(icc))
}

/// Stores the view's newest state and asks for a frame. Returns at once; the
/// render thread draws the newest state it finds. A bad field returns
/// OP_ERR_BAD_ARGUMENT and changes nothing. A stale view id returns
/// OP_ERR_NO_SUCH_DOC.
#[unsafe(no_mangle)]
pub extern "C" fn op_view_set(p: *mut OpEngine, view: u64, state: *const OpViewState) -> OpStatus {
    guard(|| {
        if state.is_null() {
            return OpStatus::OP_ERR_BAD_ARGUMENT;
        }
        if let Err(s) = engine(p) {
            return s;
        }
        // SAFETY: checked non-null; the caller passes a live struct.
        let state = unsafe { &*state };
        let icc = match checked(state) {
            Ok(i) => i,
            Err(s) => return s,
        };
        let Some(host) = find_host(p) else {
            return OpStatus::OP_ERR_NO_SUCH_DOC;
        };
        let Some(v) = host.view(view) else {
            return OpStatus::OP_ERR_NO_SUCH_DOC;
        };
        {
            let mut r = v.request.lock();
            r.width_pt = state.width_pt;
            r.height_pt = state.height_pt;
            r.scale = state.scale;
            r.zoom = state.zoom;
            r.origin_x = state.origin_x;
            r.origin_y = state.origin_y;
            r.flags = state.flags;
            r.set = true;
            if let Some(icc) = icc
                && r.display_icc.as_deref() != Some(icc)
            {
                r.display_icc = Some(Arc::from(icc));
                r.display_gen += 1;
            }
        }
        v.dirty.store(true, Ordering::Release);
        host.wake.notify_all();
        OpStatus::OP_OK
    })
}

/// Detaches a view: waits for its in-flight frame, drops its surface and
/// releases the layer. A stale id returns OP_ERR_NO_SUCH_DOC.
#[unsafe(no_mangle)]
pub extern "C" fn op_view_detach(p: *mut OpEngine, view: u64) -> OpStatus {
    guard(|| {
        if let Err(s) = engine(p) {
            return s;
        }
        let Some(host) = find_host(p) else {
            return OpStatus::OP_ERR_NO_SUCH_DOC;
        };
        let Some(v) = host.state.lock().views.remove(&view) else {
            return OpStatus::OP_ERR_NO_SUCH_DOC;
        };
        finish_view(&host, &v);
        OpStatus::OP_OK
    })
}

/// Reads a view's current state and statistics.
#[unsafe(no_mangle)]
pub extern "C" fn op_view_info(p: *mut OpEngine, view: u64, out: *mut OpViewInfo) -> OpStatus {
    guard(|| {
        if out.is_null() {
            return OpStatus::OP_ERR_BAD_ARGUMENT;
        }
        if let Err(s) = engine(p) {
            return s;
        }
        let Some(v) = find_host(p).and_then(|h| h.view(view)) else {
            return OpStatus::OP_ERR_NO_SUCH_DOC;
        };
        let req = v.request.lock().clone();
        let mut info = OpViewInfo {
            doc: v.doc,
            width_pt: req.width_pt,
            height_pt: req.height_pt,
            scale: req.scale,
            zoom: req.zoom,
            origin_x: req.origin_x,
            origin_y: req.origin_y,
            flags: req.flags,
            display_icc_len: req.display_icc.as_ref().map_or(0, |i| i.len() as u64),
            frames: v.log.lock().frames,
            ..Default::default()
        };
        fill_render_info(&v, &mut info);
        // SAFETY: checked non-null.
        unsafe { *out = info };
        OpStatus::OP_OK
    })
}

#[cfg(target_os = "macos")]
fn fill_render_info(v: &View, info: &mut OpViewInfo) {
    if let Some(r) = v.render.lock().as_ref() {
        let s = &r.target.stats;
        info.layer_tag_is_display = r.target.tag_is_display() as u8;
        info.has_display_table = r.has_table as u8;
        info.suspended = r.target.is_suspended() as u8;
        info.drawable_width = s.drawable.0;
        info.drawable_height = s.drawable.1;
        info.configures = s.configures;
        info.tag_changed_by_configure = s.tag_changed_by_configure;
        info.retags = s.retags;
        info.table_builds = r.table_builds;
        info.mip_builds = r.renderer.mip_builds();
        info.doc_uploads = r.doc_uploads;
        info.doc_revision = r.doc_revision.unwrap_or(0);
        info.frame_errors = r.frame_errors;
        info.last_error = r.last_error as u32;
    }
}
#[cfg(not(target_os = "macos"))]
fn fill_render_info(_v: &View, _info: &mut OpViewInfo) {}

/// Reads the engine-wide view counters (live views, retained layers, surfaces,
/// wgpu objects, validation errors, frames).
#[unsafe(no_mangle)]
pub extern "C" fn op_view_counters(p: *mut OpEngine, out: *mut OpViewCounters) -> OpStatus {
    guard(|| {
        if out.is_null() {
            return OpStatus::OP_ERR_BAD_ARGUMENT;
        }
        if let Err(s) = engine(p) {
            return s;
        }
        let mut c = OpViewCounters::default();
        if let Some(host) = find_host(p) {
            c.live_views = host.state.lock().views.len() as u64;
            c.frames_total = host.frames_total.load(Ordering::Relaxed);
            #[cfg(target_os = "macos")]
            {
                c.live_layers = host.counters.live_targets.load(Ordering::SeqCst);
                c.live_surfaces = host.counters.live_surfaces.load(Ordering::SeqCst);
            }
            if let Some(gpu) = host.gpu.lock().clone() {
                let _ = gpu.device.poll(wgpu::PollType::Poll);
                let hal = gpu.device.get_internal_counters().hal;
                let n = |c: &wgpu::wgt::InternalCounter| c.read().max(0) as u64;
                c.wgpu_textures = n(&hal.textures);
                c.wgpu_texture_views = n(&hal.texture_views);
                c.wgpu_buffers = n(&hal.buffers);
                c.wgpu_bind_groups = n(&hal.bind_groups);
                c.validation_errors = gpu.validation_errors.load(Ordering::SeqCst);
            }
        }
        // SAFETY: checked non-null.
        unsafe { *out = c };
        OpStatus::OP_OK
    })
}

/// Moves up to `cap` present timestamps (milliseconds since attach, oldest
/// first) out of the view's log into `out` and writes how many to `out_len`.
/// The log keeps the newest 16384 frames until read.
#[unsafe(no_mangle)]
pub extern "C" fn op_view_take_frame_times(
    p: *mut OpEngine,
    view: u64,
    out: *mut f64,
    cap: usize,
    out_len: *mut usize,
) -> OpStatus {
    guard(|| {
        if out_len.is_null() || (out.is_null() && cap > 0) || cap > FRAME_LOG_CAP {
            return OpStatus::OP_ERR_BAD_ARGUMENT;
        }
        if let Err(s) = engine(p) {
            return s;
        }
        let Some(v) = find_host(p).and_then(|h| h.view(view)) else {
            return OpStatus::OP_ERR_NO_SUCH_DOC;
        };
        let mut log = v.log.lock();
        let n = cap.min(log.times_ms.len());
        for (i, t) in log.times_ms.drain(..n).enumerate() {
            // SAFETY: the caller passes `cap` writable doubles; i < n <= cap.
            unsafe { *out.add(i) = t };
        }
        // SAFETY: checked non-null.
        unsafe { *out_len = n };
        OpStatus::OP_OK
    })
}

/// Waits until the view has presented at least `min_frames` frames, or until
/// `timeout_ms` passes (then OP_ERR_BUSY). Writes the frame count to `out_frames`
/// when it is not null. Returns OP_ERR_NO_SUCH_DOC if the view is detached.
#[unsafe(no_mangle)]
pub extern "C" fn op_view_wait_frames(
    p: *mut OpEngine,
    view: u64,
    min_frames: u64,
    timeout_ms: u32,
    out_frames: *mut u64,
) -> OpStatus {
    guard(|| {
        if let Err(s) = engine(p) {
            return s;
        }
        let Some(host) = find_host(p) else {
            return OpStatus::OP_ERR_NO_SUCH_DOC;
        };
        let Some(v) = host.view(view) else {
            return OpStatus::OP_ERR_NO_SUCH_DOC;
        };
        let deadline = Instant::now() + Duration::from_millis(timeout_ms as u64);
        let mut log = v.log.lock();
        let status = loop {
            if log.frames >= min_frames {
                break OpStatus::OP_OK;
            }
            if v.render.try_lock().is_some_and(|r| r.is_none()) {
                break OpStatus::OP_ERR_NO_SUCH_DOC;
            }
            if v.frame_cv.wait_until(&mut log, deadline).timed_out() {
                break if log.frames >= min_frames {
                    OpStatus::OP_OK
                } else {
                    OpStatus::OP_ERR_BUSY
                };
            }
        };
        if !out_frames.is_null() {
            // SAFETY: checked non-null.
            unsafe { *out_frames = log.frames };
        }
        status
    })
}
