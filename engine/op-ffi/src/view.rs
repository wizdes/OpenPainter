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
//!   (checked every 50 ms while idle). Live strokes wake it at once
//!   (`doc_touched`), and a change that only touched some tiles uploads only
//!   those tiles (M38).
//! - `op_view_detach`, `op_doc_close` (for the document's views) and
//!   `op_engine_free` return only after the view's in-flight frame has finished
//!   and its layer has been released (ARCHITECTURE section 6).
// The view host draws only on macOS (CAMetalLayer); on other systems the
// host and its render loop are compiled but never called.
#![cfg_attr(not(target_os = "macos"), allow(dead_code, clippy::drop_non_drop))]
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

/// An overlay point in document points. Borrowed only during the set call.
#[cfg(windows)]
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct OpOverlayPoint { pub x: f64, pub y: f64 }

/// Windows overlay item kinds: path=1, rectangle=2, ellipse=3, handle=4,
/// pivot=5, crop=6, caret=7, edit frame=8, underline=9, flood handle=10,
/// static zoom rectangle=11.
#[cfg(windows)]
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct OpOverlayItem {
    pub kind: u32,
    /// For a path only: 1 closes it; all other bits are invalid.
    pub flags: u32,
    pub points: *const OpOverlayPoint,
    pub point_count: usize,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}
#[cfg(windows)]
impl Default for OpOverlayItem {
    fn default() -> Self { Self { kind:0, flags:0, points:std::ptr::null(), point_count:0, x:0.0, y:0.0, width:0.0, height:0.0 } }
}

#[cfg(windows)]
fn parse_overlay_items(items: *const OpOverlayItem, count: usize) -> Result<Arc<op_gpu::overlay::Overlay>, OpStatus> {
    use op_gpu::overlay::{Item, Point};
    if count > 10_000 || (count > 0 && items.is_null()) { return Err(OpStatus::OP_ERR_BAD_ARGUMENT); }
    let source = if count == 0 { &[][..] } else { unsafe { std::slice::from_raw_parts(items, count) } };
    let mut parsed=Vec::with_capacity(count);
    let pt=|x:f64,y:f64| Point::new(x as f32,y as f32);
    for i in source {
        if i.flags & !1 != 0 || ![i.x,i.y,i.width,i.height].iter().all(|n|n.is_finite()) { return Err(OpStatus::OP_ERR_BAD_ARGUMENT); }
        let points = if i.point_count == 0 { Vec::new() } else {
            if i.point_count > 1_000_000 || i.points.is_null() { return Err(OpStatus::OP_ERR_BAD_ARGUMENT); }
            unsafe { std::slice::from_raw_parts(i.points,i.point_count) }.iter().map(|p|pt(p.x,p.y)).collect::<Vec<_>>()
        };
        let item=match i.kind {
            1 if i.point_count >= 2 => Item::path(points,i.flags == 1),
            2 if i.point_count == 0 && i.flags == 0 => Item::Rectangle {x:i.x as f32,y:i.y as f32,width:i.width as f32,height:i.height as f32},
            3 if i.point_count == 0 && i.flags == 0 => Item::Ellipse {x:i.x as f32,y:i.y as f32,width:i.width as f32,height:i.height as f32},
            4 if i.point_count == 0 && i.flags == 0 => Item::handle(pt(i.x,i.y)),
            5 if i.point_count == 0 && i.flags == 0 => Item::Pivot(pt(i.x,i.y)),
            6 if i.point_count == 4 && i.flags == 0 => Item::crop([points[0],points[1],points[2],points[3]]),
            7 if i.point_count == 0 && i.flags == 0 => Item::Caret {x:i.x as f32,y:i.y as f32,height:i.height as f32},
            8 if i.point_count == 0 && i.flags == 0 => Item::EditFrame {x:i.x as f32,y:i.y as f32,width:i.width as f32,height:i.height as f32},
            9 if i.point_count == 2 && i.flags == 0 => Item::Underline {from:points[0],to:points[1]},
            10 if i.point_count == 0 && i.flags == 0 => Item::FloodHandle(pt(i.x,i.y)),
            11 if i.point_count == 0 && i.flags == 0 => Item::ZoomRectangle {x:i.x as f32,y:i.y as f32,width:i.width as f32,height:i.height as f32},
            _ => return Err(OpStatus::OP_ERR_BAD_ARGUMENT),
        };
        parsed.push(item);
    }
    op_gpu::overlay::Overlay::new(parsed).map(Arc::new).map_err(|_|OpStatus::OP_ERR_BAD_ARGUMENT)
}

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
    /// Changes uploaded as changed tiles only, not the whole image (M38).
    pub region_uploads: u64,
    /// Tiles uploaded by those partial uploads.
    pub region_tiles: u64,
    /// The fill around the document, red, green, blue in 0..=1 (M56).
    pub background_red: f32,
    pub background_green: f32,
    pub background_blue: f32,
    pub _pad2: f32,
}

/// One presented frame, for latency measurements (M38). `time_ns` is
/// CLOCK_UPTIME_RAW (the clock of `NSEvent.timestamp` and
/// `ProcessInfo.systemUptime`) after the GPU finished the frame.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OpFrameStamp {
    pub time_ns: u64,
    /// The document revision the frame shows.
    pub revision: u64,
    /// The preview (`stroke.append` returns it) the frame shows; 0 for none.
    pub preview_serial: u64,
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
    #[cfg(windows)]
    display_sent: bool,
    /// Bumped whenever the display ICC bytes change.
    display_gen: u64,
    /// False until the first op_view_set.
    set: bool,
    /// Fill outside the document (`op_view_set_background`, M56).
    background: [f32; 4],
    #[cfg(windows)]
    overlay: Option<Arc<op_gpu::overlay::Overlay>>,
    #[cfg(windows)]
    overlay_dash: f64,
}

#[cfg(windows)]
fn recovery_profile(request: &Request, windows_icc: &[u8]) -> Arc<[u8]> {
    if request.display_sent {
        request.display_icc.clone().expect("sent display profile")
    } else {
        Arc::from(windows_icc)
    }
}

#[cfg(all(test, windows))]
mod recovery_profile_tests {
    use super::*;
    #[test]
    fn sent_profile_survives_recovery_and_length_zero_set() {
        let request = Request {
            width_pt: 1.0,
            height_pt: 1.0,
            scale: 1.0,
            zoom: 1.0,
            origin_x: 0.0,
            origin_y: 0.0,
            flags: 0,
            display_icc: Some(Arc::from([1u8, 2, 3].as_slice())),
            display_sent: true,
            display_gen: 1,
            set: true,
            background: BACKGROUND,
            overlay: None,
            overlay_dash: 0.0,
        };
        assert_eq!(&*recovery_profile(&request, &[4, 5]), &[1, 2, 3]);
        let mut automatic = request;
        automatic.display_sent = false;
        assert_eq!(&*recovery_profile(&automatic, &[4, 5]), &[4, 5]);
    }
}

#[cfg(any(target_os = "macos", windows))]
struct Render {
    #[cfg(target_os = "macos")]
    target: op_gpu::layer::MetalLayerTarget,
    #[cfg(windows)]
    target: op_gpu::win32::Win32Target,
    renderer: op_gpu::presenter::ViewRenderer,
    doc_revision: Option<u64>,
    /// `Published::preview_serial` of the uploaded pixels (0: no preview).
    doc_preview: u64,
    doc_profile: Option<Arc<[u8]>>,
    display_gen: u64,
    has_table: bool,
    table_builds: u64,
    doc_uploads: u64,
    frame_errors: u64,
    last_error: OpStatus,
    #[cfg(windows)]
    last_message: Option<String>,
    /// The snapshot whose pixels are on the GPU, for tile diffs.
    shown: Option<Arc<op_engine::Published>>,
    region_uploads: u64,
    region_tiles: u64,
}

#[cfg(not(any(target_os = "macos", windows)))]
struct Render;

struct FrameLog {
    frames: u64,
    times_ms: VecDeque<f64>,
    stamps: VecDeque<OpFrameStamp>,
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
    #[cfg(windows)]
    recovery_errors: AtomicU64,
    #[cfg(windows)]
    saved_info: Mutex<OpViewInfo>,
    #[cfg(windows)]
    retry_at: Mutex<Option<Instant>>,
}

struct Gpu {
    #[cfg(not(windows))]
    instance: wgpu::Instance,
    #[cfg(windows)]
    instance: std::mem::ManuallyDrop<wgpu::Instance>,
    #[cfg(not(windows))]
    device: wgpu::Device,
    #[cfg(windows)]
    device: std::mem::ManuallyDrop<wgpu::Device>,
    #[cfg(not(windows))]
    queue: wgpu::Queue,
    #[cfg(windows)]
    queue: std::mem::ManuallyDrop<wgpu::Queue>,
    validation_errors: Arc<AtomicU64>,
    #[cfg(windows)]
    lost: Arc<std::sync::atomic::AtomicBool>,
}

#[cfg(windows)]
impl Drop for Gpu {
    fn drop(&mut self) {
        let _warp_guard = op_gpu::windows_warp_guard().expect("WARP canvas device release lock");
        // The guard must cover the actual wgpu drops, including when a frame
        // kept the final Arc after the host removed its slot.
        unsafe {
            std::mem::ManuallyDrop::drop(&mut self.queue);
            std::mem::ManuallyDrop::drop(&mut self.device);
            std::mem::ManuallyDrop::drop(&mut self.instance);
        }
    }
}

#[cfg(all(windows, feature = "test-hooks"))]
static FAIL_NEXT_RECREATE: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
#[cfg(all(windows, feature = "test-hooks"))]
static PANIC_NEXT_FRAME: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

impl Gpu {
    #[cfg(windows)]
    fn removed(&self) -> bool {
        use windows::Win32::Graphics::Direct3D12::ID3D12Device;
        let raw: ID3D12Device = unsafe {
            self.device
                .as_hal::<wgpu::hal::api::Dx12>()
                .unwrap()
                .raw_device()
                .clone()
        };
        unsafe { raw.GetDeviceRemovedReason().is_err() }
    }
    fn new() -> Result<Self, String> {
        #[cfg(all(windows, feature = "test-hooks"))]
        if FAIL_NEXT_RECREATE.swap(false, Ordering::SeqCst) {
            return Err("test hook: DX12 device recreate failed".into());
        }
        #[cfg(windows)]
        let _warp_create_guard = op_gpu::windows_warp_guard().map_err(|e| e.to_string())?;
        #[cfg(windows)]
        let (instance, adapter) = op_gpu::windows_adapter().map_err(|e| e.to_string())?;
        #[cfg(not(windows))]
        let (instance, adapter) = {
            let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
                backends: wgpu::Backends::METAL,
                flags: wgpu::InstanceFlags::from_build_config().with_env(),
                ..Default::default()
            });
            let adapter = pollster::block_on(
                instance.request_adapter(&wgpu::RequestAdapterOptions::default()),
            )
            .map_err(|e| format!("adapter: {e}"))?;
            (instance, adapter)
        };
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("OpenPainter canvas"),
            required_limits: adapter.limits(),
            ..Default::default()
        }))
        .map_err(|e| {
            #[cfg(windows)]
            {
                format!("device: {e}; a holder of a removed device may remain in this process")
            }
            #[cfg(not(windows))]
            {
                format!("device: {e}")
            }
        })?;
        let validation_errors = Arc::new(AtomicU64::new(0));
        let count = validation_errors.clone();
        device.on_uncaptured_error(Arc::new(move |e: wgpu::Error| {
            count.fetch_add(1, Ordering::SeqCst);
            eprintln!("op-ffi view: wgpu uncaptured error: {e}");
        }));
        #[cfg(windows)]
        let lost = {
            let lost = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let signal = lost.clone();
            device.set_device_lost_callback(move |reason, message| {
                eprintln!("op-ffi view: DX12 device lost ({reason:?}): {message}");
                signal.store(true, Ordering::SeqCst);
            });
            lost
        };
        Ok(Self {
            #[cfg(not(windows))]
            instance,
            #[cfg(windows)]
            instance: std::mem::ManuallyDrop::new(instance),
            #[cfg(not(windows))]
            device,
            #[cfg(windows)]
            device: std::mem::ManuallyDrop::new(device),
            #[cfg(not(windows))]
            queue,
            #[cfg(windows)]
            queue: std::mem::ManuallyDrop::new(queue),
            validation_errors,
            #[cfg(windows)]
            lost,
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
    #[cfg(windows)]
    recreate_after: Mutex<Option<Instant>>,
    #[cfg(windows)]
    pending_recovery: Mutex<Vec<(Arc<View>, isize, bool, op_gpu::win32::WindowStats, u64)>>,
    #[cfg(target_os = "macos")]
    counters: Arc<op_gpu::layer::LayerCounters>,
    #[cfg(windows)]
    counters: Arc<op_gpu::win32::WindowCounters>,
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
        #[cfg(windows)]
        recreate_after: Mutex::new(None),
        #[cfg(windows)]
        pending_recovery: Mutex::new(Vec::new()),
        #[cfg(target_os = "macos")]
        counters: Arc::default(),
        #[cfg(windows)]
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
            #[cfg(windows)]
            if g.lost.load(Ordering::SeqCst) && g.removed() {
                eprintln!("op-ffi view: recreating lost DX12 canvas device");
                let views: Vec<_> = self.state.lock().views.values().cloned().collect();
                let mut old = Vec::new();
                for view in &views {
                    let mut info = OpViewInfo::default();
                    fill_render_info(view, &mut info);
                    *view.saved_info.lock() = info;
                    if let Some(mut render) = view.render.lock().take() {
                        let hwnd = render.target.hwnd();
                        let stats = render.target.stats.clone();
                        let errors = render.frame_errors;
                        render.target.suspend();
                        old.push((view.clone(), hwnd, render.target.is_panel(), stats, errors));
                    }
                }
                *self.pending_recovery.lock() = old;
                self.engine()
                    .core()
                    .release_effect_gpu_for_canvas_recovery();
                *slot = None;
                // No view renderer, surface, effect context or host slot may
                // retain the removed device when DX12 selects WARP again.
                if self
                    .recreate_after
                    .lock()
                    .is_some_and(|at| Instant::now() < at)
                {
                    return Err(OpStatus::OP_ERR_INTERNAL);
                }
                let replacement = match Gpu::new() {
                    Ok(g) => Arc::new(g),
                    Err(e) => {
                        eprintln!("op-ffi view: recreate DX12 device failed: {e}; retry in 2 s");
                        *self.recreate_after.lock() = Some(Instant::now() + Duration::from_secs(2));
                        for (view, _, _, _, _) in self.pending_recovery.lock().iter() {
                            view.recovery_errors.fetch_add(1, Ordering::SeqCst);
                        }
                        return Err(OpStatus::OP_ERR_INTERNAL);
                    }
                };
                *self.recreate_after.lock() = None;
                self.restore_views(&replacement);
                *slot = Some(replacement.clone());
                return Ok(replacement);
            }
            #[cfg(windows)]
            if !self.pending_recovery.lock().is_empty()
                && self
                    .recreate_after
                    .lock()
                    .is_none_or(|at| Instant::now() >= at)
            {
                *self.recreate_after.lock() = None;
                self.restore_views(g);
            }
            return Ok(g.clone());
        }
        #[cfg(windows)]
        if self
            .recreate_after
            .lock()
            .is_some_and(|at| Instant::now() < at)
        {
            return Err(OpStatus::OP_ERR_INTERNAL);
        }
        #[cfg(windows)]
        if !self.pending_recovery.lock().is_empty() {
            self.engine()
                .core()
                .release_effect_gpu_for_canvas_recovery();
        }
        let g = Arc::new(Gpu::new().map_err(|e| {
            eprintln!("op-ffi view: no GPU for the canvas: {e}");
            #[cfg(windows)]
            {
                *self.recreate_after.lock() = Some(Instant::now() + Duration::from_secs(2));
                for (view, _, _, _, _) in self.pending_recovery.lock().iter() {
                    view.recovery_errors.fetch_add(1, Ordering::SeqCst);
                }
            }
            OpStatus::OP_ERR_INTERNAL
        })?);
        #[cfg(windows)]
        {
            *self.recreate_after.lock() = None;
            self.restore_views(&g);
        }
        *slot = Some(g.clone());
        Ok(g)
    }

    #[cfg(windows)]
    fn restore_views(&self, gpu: &Arc<Gpu>) {
        let pending = std::mem::take(&mut *self.pending_recovery.lock());
        let mut retry = Vec::new();
        for (view, hwnd, panel, stats, errors) in pending {
            if !self
                .state
                .lock()
                .views
                .values()
                .any(|registered| Arc::ptr_eq(registered, &view))
            {
                continue;
            }
            match unsafe {
                if panel { op_gpu::win32::Win32Target::new_panel(&gpu.instance, hwnd as *mut c_void, self.counters.clone()) }
                else { op_gpu::win32::Win32Target::new(&gpu.instance, hwnd as *mut c_void, self.counters.clone()) }
            } {
                Ok(mut target) => {
                    let profile = recovery_profile(&view.request.lock(), target.display_icc());
                    let problem = (!profile.is_empty()).then(|| self
                        .engine()
                        .core()
                        .snapshot(view.doc)
                        .ok()
                        .and_then(|snap| {
                            display_table(snap.state.profile.as_deref(), &profile).err()
                        })).flatten();
                    if let Some(e) = &problem {
                        eprintln!(
                            "op-ffi view: recovery display table unavailable: {e}; drawing without display conversion"
                        );
                    }
                    {
                        let mut request = view.request.lock();
                        if !request.display_sent {
                            request.display_icc = (problem.is_none() && !profile.is_empty()).then_some(profile);
                            request.display_gen += 1;
                        }
                    }
                    target.stats = stats;
                    let previous = *view.saved_info.lock();
                    let render = Render {
                        target,
                        renderer: op_gpu::presenter::ViewRenderer::new(
                            &gpu.device,
                            op_gpu::win32::FORMAT,
                        ),
                        doc_revision: None,
                        doc_preview: 0,
                        doc_profile: None,
                        display_gen: 0,
                        has_table: false,
                        table_builds: previous.table_builds,
                        doc_uploads: previous.doc_uploads,
                        frame_errors: errors
                            + view.recovery_errors.swap(0, Ordering::SeqCst)
                            + u64::from(problem.is_some()),
                        last_error: if problem.is_some() {
                            OpStatus::OP_ERR_INTERNAL
                        } else {
                            OpStatus::OP_OK
                        },
                        last_message: problem.clone(),
                        shown: None,
                        region_uploads: previous.region_uploads,
                        region_tiles: previous.region_tiles,
                    };
                    *view.render.lock() = Some(render);
                    view.dirty.store(true, Ordering::Release);
                }
                Err(e) => {
                    eprintln!("op-ffi view: recreate HWND surface failed: {e}");
                    view.recovery_errors.fetch_add(1, Ordering::SeqCst);
                    retry.push((view, hwnd, panel, stats, errors));
                }
            }
        }
        if !retry.is_empty() {
            self.pending_recovery.lock().extend(retry);
            *self.recreate_after.lock() = Some(Instant::now() + Duration::from_secs(2));
        }
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
            #[cfg(windows)]
            if let Err(e) = op_gpu::poll_gpu(&gpu.device, "view detach") {
                eprintln!("op-ffi view: {e}");
            }
            #[cfg(not(windows))]
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
    #[cfg(windows)]
    let mut last_idle_check = Instant::now() - Duration::from_secs(1);
    loop {
        let views: Vec<Arc<View>> = {
            let mut st = host.state.lock();
            if st.stop {
                return;
            }
            let any_dirty = st.views.values().any(|v| {
                v.dirty.load(Ordering::Acquire) && {
                    #[cfg(windows)]
                    {
                        v.retry_at.lock().is_none_or(|at| Instant::now() >= at)
                    }
                    #[cfg(not(windows))]
                    {
                        true
                    }
                }
            });
            if !any_dirty {
                host.wake.wait_for(&mut st, Duration::from_millis(50));
            }
            if st.stop {
                return;
            }
            st.views.values().cloned().collect()
        };
        #[cfg(windows)]
        let gpu_lost = {
            // wgpu runs the device-lost callback only from a poll, and nothing
            // else polls this device. Without this the canvas never recovers.
            let gpu = host.gpu.lock().clone();
            gpu.is_some_and(|gpu| {
                let result = catch_unwind(AssertUnwindSafe(|| {
                    if let Err(e) = gpu.device.poll(wgpu::PollType::Poll) {
                        eprintln!("op-ffi view: canvas device poll failed: {e}");
                    }
                    if last_idle_check.elapsed() >= Duration::from_secs(1) {
                        last_idle_check = Instant::now();
                        if gpu.removed() {
                            gpu.lost.store(true, Ordering::SeqCst);
                        }
                    }
                }));
                if result.is_err() {
                    eprintln!("op-ffi view: canvas device poll panicked");
                    if gpu.removed() {
                        gpu.lost.store(true, Ordering::SeqCst);
                    }
                    for view in &views {
                        view.recovery_errors.fetch_add(1, Ordering::SeqCst);
                    }
                }
                gpu.lost.load(Ordering::SeqCst) && gpu.removed()
            })
        };
        for v in views {
            #[cfg(windows)]
            if v.retry_at.lock().is_some_and(|at| Instant::now() < at) {
                continue;
            }
            if !v.dirty.load(Ordering::Acquire) && !doc_changed(&host, &v) && {
                #[cfg(windows)]
                {
                    !gpu_lost
                }
                #[cfg(not(windows))]
                {
                    true
                }
            } {
                continue;
            }
            v.dirty.store(false, Ordering::Release);
            #[cfg(windows)]
            let errors_before = v.render.lock().as_ref().map_or(0, |r| r.frame_errors)
                + v.recovery_errors.load(Ordering::SeqCst);
            let ok = catch_unwind(AssertUnwindSafe(|| pooled(|| render_one(&host, &v))));
            if ok.is_err() {
                #[cfg(windows)]
                if let Some(gpu) = host.gpu.lock().as_ref() {
                    if gpu.removed() {
                        gpu.lost.store(true, Ordering::SeqCst);
                    }
                }
                record_error(&v, OpStatus::OP_ERR_PANIC);
            }
            #[cfg(windows)]
            {
                let errors_after = v.render.lock().as_ref().map_or(0, |r| r.frame_errors)
                    + v.recovery_errors.load(Ordering::SeqCst);
                if retry_frame(errors_before, errors_after, v.render.lock().is_some()) {
                    *v.retry_at.lock() = Some(Instant::now() + Duration::from_millis(100));
                    v.dirty.store(true, Ordering::Release);
                } else {
                    *v.retry_at.lock() = None;
                }
            }
        }
    }
}

#[cfg(windows)]
fn retry_frame(errors_before: u64, errors_after: u64, render_present: bool) -> bool {
    errors_after > errors_before || !render_present
}

#[cfg(all(test, windows))]
mod retry_tests {
    use super::*;
    #[test]
    fn missing_render_retries_even_when_old_errors_outnumber_new_errors() {
        assert!(super::retry_frame(3, 1, false));
    }

    #[test]
    fn view_info_keeps_counts_while_renderer_is_missing() {
        let view = View {
            doc: 1,
            created: Instant::now(),
            request: Mutex::new(Request {
                width_pt: 1.0,
                height_pt: 1.0,
                scale: 1.0,
                zoom: 1.0,
                origin_x: 0.0,
                origin_y: 0.0,
                flags: 0,
                display_icc: None,
                display_sent: false,
                display_gen: 0,
                set: true,
                background: BACKGROUND,
                overlay: None,
                overlay_dash: 0.0,
            }),
            dirty: true.into(),
            render: Mutex::new(None),
            log: Mutex::new(FrameLog {
                frames: 0,
                times_ms: VecDeque::new(),
                stamps: VecDeque::new(),
            }),
            frame_cv: Condvar::new(),
            recovery_errors: AtomicU64::new(2),
            saved_info: Mutex::new(OpViewInfo {
                configures: 5,
                frame_errors: 3,
                last_error: OpStatus::OP_ERR_INTERNAL as u32,
                table_builds: 4,
                doc_uploads: 7,
                ..OpViewInfo::default()
            }),
            retry_at: Mutex::new(None),
        };
        let mut info = OpViewInfo::default();
        fill_render_info(&view, &mut info);
        assert_eq!(info.configures, 5);
        assert_eq!(info.table_builds, 4);
        assert_eq!(info.doc_uploads, 7);
        assert_eq!(info.frame_errors, 5);
        assert_eq!(info.last_error, OpStatus::OP_ERR_INTERNAL as u32);
    }

    #[test]
    fn wait_frames_stays_busy_while_registered_view_has_no_renderer() {
        let engine = crate::op_engine_new(std::ptr::null(), None, std::ptr::null_mut());
        assert!(!engine.is_null());
        let host = host_for(engine);
        let view = Arc::new(View {
            doc: 1,
            created: Instant::now(),
            request: Mutex::new(Request {
                width_pt: 1.0,
                height_pt: 1.0,
                scale: 1.0,
                zoom: 1.0,
                origin_x: 0.0,
                origin_y: 0.0,
                flags: 0,
                display_icc: None,
                display_sent: false,
                display_gen: 0,
                set: false,
                background: BACKGROUND,
                overlay: None,
                overlay_dash: 0.0,
            }),
            dirty: false.into(),
            render: Mutex::new(None),
            log: Mutex::new(FrameLog {
                frames: 0,
                times_ms: VecDeque::new(),
                stamps: VecDeque::new(),
            }),
            frame_cv: Condvar::new(),
            recovery_errors: AtomicU64::new(0),
            saved_info: Mutex::new(OpViewInfo::default()),
            retry_at: Mutex::new(None),
        });
        host.state.lock().views.insert(1, view);
        assert_eq!(
            op_view_wait_frames(engine, 1, 1, 10, std::ptr::null_mut()),
            OpStatus::OP_ERR_BUSY
        );
        host.state.lock().views.remove(&1);
        assert_eq!(
            op_view_wait_frames(engine, 1, 1, 10, std::ptr::null_mut()),
            OpStatus::OP_ERR_NO_SUCH_DOC
        );
        crate::op_engine_free(engine);
    }
}

#[cfg(any(target_os = "macos", windows))]
fn record_error(v: &View, status: OpStatus) {
    if let Some(r) = v.render.lock().as_mut() {
        r.frame_errors += 1;
        r.last_error = status;
    } else {
        #[cfg(windows)]
        v.recovery_errors.fetch_add(1, Ordering::SeqCst);
    }
}
#[cfg(not(any(target_os = "macos", windows)))]
fn record_error(_v: &View, _status: OpStatus) {}

#[cfg(any(target_os = "macos", windows))]
fn doc_changed(host: &Host, v: &View) -> bool {
    let Ok(snap) = host.engine().core().snapshot(v.doc) else {
        return false;
    };
    let seen = v
        .render
        .lock()
        .as_ref()
        .and_then(|r| r.doc_revision.map(|rev| (rev, r.doc_preview)));
    seen.is_some_and(|key| key != (snap.revision, snap.preview_serial))
}
#[cfg(not(any(target_os = "macos", windows)))]
fn doc_changed(_host: &Host, _v: &View) -> bool {
    false
}

#[cfg(any(target_os = "macos", windows))]
fn render_one(host: &Host, v: &View) {
    use op_gpu::presenter::{CanvasParams, Lut3d, ViewParams};
    let Ok(gpu) = host.gpu() else {
        #[cfg(windows)]
        {
            v.recovery_errors.fetch_add(1, Ordering::SeqCst);
        }
        return;
    };
    #[cfg(all(windows, feature = "test-hooks"))]
    if PANIC_NEXT_FRAME.swap(false, Ordering::SeqCst) {
        panic!("test hook: frame panic without device removal");
    }
    let req = v.request.lock().clone();
    if !req.set {
        return;
    }
    let mut guard = v.render.lock();
    let Some(r) = guard.as_mut() else { return };
    let fail = |r: &mut Render, status: OpStatus, msg: String| {
        #[cfg(windows)]
        {
            if r.last_message.as_deref() != Some(msg.as_str()) {
                eprintln!("op-ffi view: frame failed: {msg}");
            }
            r.last_message = Some(msg);
        }
        #[cfg(not(windows))]
        eprintln!("op-ffi view: frame failed: {msg}");
        r.frame_errors += 1;
        r.last_error = status;
    };
    if req.flags & OP_VIEW_SUSPENDED != 0 {
        r.target.suspend();
        return;
    }
    #[cfg(windows)]
    if let Err(e) = r.target.enter_window_desktop() {
        return fail(r, OpStatus::OP_ERR_INTERNAL, e);
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
    // A preview session (such as Layer Properties) shows its preview pixels.
    if r.doc_revision != Some(snap.revision) || r.doc_preview != snap.preview_serial {
        let shown = |p: &op_engine::Published| p.preview.clone().unwrap_or_else(|| p.state.clone());
        let tiles = r
            .shown
            .as_ref()
            .and_then(|old| op_engine::live_stroke::changed_tiles(&shown(old), &shown(&snap)));
        if let Some(tiles) = tiles {
            for coord in &tiles {
                let (x, y, w, h, rgba) =
                    match op_engine::live_stroke::tile_rgba(&shown(&snap), *coord) {
                        Ok(t) => t,
                        Err(e) => {
                            r.shown = None;
                            return fail(r, OpStatus::OP_ERR_INTERNAL, format!("tile: {e}"));
                        }
                    };
                if let Err(e) = r
                    .renderer
                    .update_document_region(&gpu.queue, x, y, w, h, &rgba)
                {
                    r.shown = None;
                    return fail(r, OpStatus::OP_ERR_INTERNAL, format!("tile upload: {e}"));
                }
            }
            r.region_uploads += 1;
            r.region_tiles += tiles.len() as u64;
        } else {
            let pixels = match snap.pixels(true) {
                Ok(p) => p,
                Err(e) => return fail(r, OpStatus::OP_ERR_INTERNAL, format!("pixels: {e}")),
            };
            let (w, h) = (snap.state.width, snap.state.height);
            if let Err(e) = r
                .renderer
                .set_document(&gpu.device, &gpu.queue, w, h, &pixels)
            {
                r.shown = None;
                return fail(r, OpStatus::OP_ERR_INTERNAL, format!("upload {w}x{h}: {e}"));
            }
            r.doc_uploads += 1;
        }
        r.shown = Some(snap.clone());
        #[cfg(not(windows))]
        {
            r.doc_revision = Some(snap.revision);
            r.doc_preview = snap.preview_serial;
        }
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
                #[cfg(not(windows))]
                return fail(r, OpStatus::OP_ERR_INTERNAL, format!("display table: {e}"));
                #[cfg(windows)]
                {
                    // Shown unconverted; the failure is counted and printed.
                    r.renderer.set_lut(&gpu.device, &gpu.queue, None);
                    r.has_table = false;
                    fail(
                        r,
                        OpStatus::OP_ERR_INTERNAL,
                        format!("display table: {e}; drawing without display conversion"),
                    );
                }
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
            background: req.background,
        },
        checker_px: ((CHECKER_PT * req.scale).round() as u32).max(1),
        pixel_grid: req.flags & OP_VIEW_PIXEL_GRID != 0 && req.zoom >= GRID_MIN_ZOOM - 1e-9,
        use_mips: true,
    };
    let renderer = &mut r.renderer;
    #[cfg(windows)]
    renderer.set_overlay(&gpu.device, req.overlay.clone(), req.overlay_dash as f32, req.scale as f32, width, height);
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
    #[cfg(windows)]
    if let Err(e) = op_gpu::poll_gpu(&gpu.device, "view frame") {
        return fail(r, OpStatus::OP_ERR_INTERNAL, e.to_string());
    }
    #[cfg(windows)]
    {
        r.doc_revision = Some(snap.revision);
        r.doc_preview = snap.preview_serial;
    }
    #[cfg(windows)]
    {
        r.last_message = None;
    }
    #[cfg(not(windows))]
    let _ = gpu.device.poll(wgpu::PollType::wait_indefinitely());
    let stamp = OpFrameStamp {
        time_ns: uptime_ns(),
        revision: r.doc_revision.unwrap_or(0),
        preview_serial: r.doc_preview,
    };
    drop(guard);
    let mut log = v.log.lock();
    log.frames += 1;
    if log.stamps.len() == FRAME_LOG_CAP {
        log.stamps.pop_front();
    }
    log.stamps.push_back(stamp);
    if log.times_ms.len() == FRAME_LOG_CAP {
        log.times_ms.pop_front();
    }
    log.times_ms
        .push_back(v.created.elapsed().as_secs_f64() * 1e3);
    drop(log);
    host.frames_total.fetch_add(1, Ordering::Relaxed);
    v.frame_cv.notify_all();
}
#[cfg(not(any(target_os = "macos", windows)))]
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

#[cfg(all(test, windows))]
#[test]
fn rswop_profile_parses_but_cannot_build_a_display_table() {
    let root = std::env::var_os("WINDIR").expect("Windows directory");
    let profile = std::fs::read(
        std::path::PathBuf::from(root).join("System32/spool/drivers/color/RSWOP.icm"),
    )
    .unwrap();
    op_color::Profile::from_icc(&profile).unwrap();
    assert!(display_table(None, &profile).is_err());
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

/// The document's published pixels changed outside a command (a live stroke):
/// its views draw at once instead of at the next 50 ms idle check.
pub(crate) fn doc_touched(p: *mut OpEngine, doc: u64) {
    let Some(host) = find_host(p) else { return };
    let mut any = false;
    for v in host.state.lock().views.values() {
        if v.doc == doc {
            v.dirty.store(true, Ordering::Release);
            any = true;
        }
    }
    if any {
        host.wake.notify_all();
    }
}

/// CLOCK_UPTIME_RAW in nanoseconds: the clock of `NSEvent.timestamp`.
#[cfg(target_os = "macos")]
pub(crate) fn uptime_ns() -> u64 {
    unsafe extern "C" {
        fn clock_gettime_nsec_np(clock_id: u32) -> u64;
    }
    const CLOCK_UPTIME_RAW: u32 = 8;
    // SAFETY: reads a clock; no pointers involved.
    unsafe { clock_gettime_nsec_np(CLOCK_UPTIME_RAW) }
}
#[cfg(not(target_os = "macos"))]
pub(crate) fn uptime_ns() -> u64 {
    0
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

/// Attaches a view of `doc` to a `CAMetalLayer` on macOS or an HWND on Windows.
/// The caller keeps the HWND alive until detach. Nothing is drawn until the first `op_view_set`.
/// Returns OP_ERR_NO_SUCH_DOC for an unknown document, OP_ERR_BAD_ARGUMENT for a
/// null pointer or an invalid native target, OP_ERR_INTERNAL when no GPU device
/// can be made, and OP_ERR_UNSUPPORTED on other systems. Windows takes an HWND.
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
        pooled(|| attach(p, doc, layer, out_view, false))
    })
}

/// Attaches a Windows WinUI SwapChainPanel. The caller keeps the panel alive
/// until detach and sends its current size through `op_view_set`.
#[cfg(windows)]
#[unsafe(no_mangle)]
pub extern "C" fn op_view_attach_panel(p: *mut OpEngine, doc: u64, panel: *mut c_void, out_view: *mut u64) -> OpStatus {
    guard(|| {
        if out_view.is_null() || panel.is_null() { return OpStatus::OP_ERR_BAD_ARGUMENT; }
        let e = match engine(p) { Ok(e) => e, Err(s) => return s };
        if e.core().snapshot(doc).is_err() { return OpStatus::OP_ERR_NO_SUCH_DOC; }
        pooled(|| attach(p, doc, panel, out_view, true))
    })
}

#[cfg(any(target_os = "macos", windows))]
fn attach(p: *mut OpEngine, doc: u64, layer: *mut c_void, out_view: *mut u64, panel: bool) -> OpStatus {
    let host = host_for(p);
    let gpu = match host.gpu() {
        Ok(g) => g,
        Err(s) => return s,
    };
    #[cfg(windows)]
    let current = host.gpu.lock();
    #[cfg(windows)]
    if !current.as_ref().is_some_and(|now| Arc::ptr_eq(now, &gpu)) {
        drop(current);
        return attach(p, doc, layer, out_view, panel);
    }
    // SAFETY: the caller passes a live Objective-C object; its class is checked.
    #[cfg(target_os = "macos")]
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
    #[cfg(windows)]
    let target = match unsafe {
        if panel { op_gpu::win32::Win32Target::new_panel(&gpu.instance, layer, host.counters.clone()) }
        else { op_gpu::win32::Win32Target::new(&gpu.instance, layer, host.counters.clone()) }
    } {
        Ok(t) => t,
        Err(e) if e == "invalid HWND" || e == "invalid SwapChainPanel" => return OpStatus::OP_ERR_BAD_ARGUMENT,
        Err(e) => {
            eprintln!("op-ffi view: attach failed: {e}");
            return OpStatus::OP_ERR_INTERNAL;
        }
    };
    #[cfg(windows)]
    let display_icc: Arc<[u8]> = Arc::from(target.display_icc());
    #[cfg(windows)]
    let display_problem = (!display_icc.is_empty()).then(|| host
        .engine()
        .core()
        .snapshot(doc)
        .ok()
        .and_then(|snap| display_table(snap.state.profile.as_deref(), &display_icc).err())).flatten();
    #[cfg(windows)]
    if let Some(e) = &display_problem {
        eprintln!(
            "op-ffi view: display table unavailable: {e}; drawing without display conversion"
        );
    }
    #[cfg(target_os = "macos")]
    let format = op_gpu::layer::FORMAT;
    #[cfg(windows)]
    let format = op_gpu::win32::FORMAT;
    let renderer = op_gpu::presenter::ViewRenderer::new(&gpu.device, format);
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
            #[cfg(target_os = "macos")]
            display_icc: None,
            #[cfg(windows)]
            display_icc: (display_problem.is_none() && !display_icc.is_empty()).then_some(display_icc),
            #[cfg(windows)]
            display_sent: false,
            #[cfg(target_os = "macos")]
            display_gen: 0,
            #[cfg(windows)]
            display_gen: u64::from(display_problem.is_none()),
            set: false,
            background: BACKGROUND,
            #[cfg(windows)]
            overlay: None,
            #[cfg(windows)]
            overlay_dash: 0.0,
        }),
        dirty: false.into(),
        render: Mutex::new(Some(Render {
            target,
            renderer,
            doc_revision: None,
            doc_preview: 0,
            doc_profile: None,
            display_gen: 0,
            has_table: false,
            table_builds: 0,
            doc_uploads: 0,
            frame_errors: {
                #[cfg(windows)]
                {
                    u64::from(display_problem.is_some())
                }
                #[cfg(not(windows))]
                {
                    0
                }
            },
            last_error: {
                #[cfg(windows)]
                {
                    if display_problem.is_some() {
                        OpStatus::OP_ERR_INTERNAL
                    } else {
                        OpStatus::OP_OK
                    }
                }
                #[cfg(not(windows))]
                {
                    OpStatus::OP_OK
                }
            },
            #[cfg(windows)]
            last_message: display_problem,
            shown: None,
            region_uploads: 0,
            region_tiles: 0,
        })),
        log: Mutex::new(FrameLog {
            frames: 0,
            times_ms: VecDeque::new(),
            stamps: VecDeque::new(),
        }),
        frame_cv: Condvar::new(),
        #[cfg(windows)]
        recovery_errors: AtomicU64::new(0),
        #[cfg(windows)]
        saved_info: Mutex::new(OpViewInfo::default()),
        #[cfg(windows)]
        retry_at: Mutex::new(None),
    });
    host.state.lock().views.insert(id, view);
    // SAFETY: checked non-null by the caller.
    unsafe { *out_view = id };
    OpStatus::OP_OK
}

#[cfg(not(any(target_os = "macos", windows)))]
fn attach(_p: *mut OpEngine, _doc: u64, _layer: *mut c_void, _out: *mut u64, _panel: bool) -> OpStatus {
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
            #[cfg(windows)]
            if icc.is_some() {
                r.display_sent = true;
            }
        }
        v.dirty.store(true, Ordering::Release);
        host.wake.notify_all();
        OpStatus::OP_OK
    })
}

/// Copies a list of Windows overlay items once. A zero count clears it.
#[cfg(windows)]
#[unsafe(no_mangle)]
pub extern "C" fn op_view_set_overlay(p: *mut OpEngine, view: u64, items: *const OpOverlayItem, count: usize) -> OpStatus {
    guard(|| {
        if let Err(s) = engine(p) { return s; }
        let next = match parse_overlay_items(items,count) { Ok(value)=>value, Err(status)=>return status };
        let Some(host) = find_host(p) else { return OpStatus::OP_ERR_NO_SUCH_DOC; };
        let Some(v) = host.view(view) else { return OpStatus::OP_ERR_NO_SUCH_DOC; };
        v.request.lock().overlay = Some(next);
        v.dirty.store(true, Ordering::Release);
        host.wake.notify_all();
        OpStatus::OP_OK
    })
}

/// Changes only the phase of retained marching ants; no points are copied.
#[cfg(windows)]
#[unsafe(no_mangle)]
pub extern "C" fn op_view_set_overlay_dash(p: *mut OpEngine, view: u64, dash_offset: f64) -> OpStatus {
    guard(|| {
        if let Err(s) = engine(p) { return s; }
        if !dash_offset.is_finite() || !(dash_offset as f32).is_finite() { return OpStatus::OP_ERR_BAD_ARGUMENT; }
        let Some(host) = find_host(p) else { return OpStatus::OP_ERR_NO_SUCH_DOC; };
        let Some(v) = host.view(view) else { return OpStatus::OP_ERR_NO_SUCH_DOC; };
        v.request.lock().overlay_dash = dash_offset;
        v.dirty.store(true, Ordering::Release);
        host.wake.notify_all();
        OpStatus::OP_OK
    })
}

/// The fill for `op_view_set_background`: each component in 0..=1, alpha 1.
fn background(red: f64, green: f64, blue: f64) -> Option<[f32; 4]> {
    let ok = |c: f64| c.is_finite() && (0.0..=1.0).contains(&c);
    (ok(red) && ok(green) && ok(blue)).then_some([red as f32, green as f32, blue as f32, 1.0])
}

/// Sets the color drawn around the document (the canvas surround, M56: the
/// app sends its `stage` appearance token). Components are 0 to 1 in the
/// display's encoding, as the default mid grey is. Until it is called the
/// view uses that grey. A component outside 0..=1 (or NaN) returns
/// OP_ERR_BAD_ARGUMENT and changes nothing; a stale view id returns
/// OP_ERR_NO_SUCH_DOC. Returns at once and asks for a frame.
#[unsafe(no_mangle)]
pub extern "C" fn op_view_set_background(
    p: *mut OpEngine,
    view: u64,
    red: f64,
    green: f64,
    blue: f64,
) -> OpStatus {
    guard(|| {
        if let Err(s) = engine(p) {
            return s;
        }
        let Some(color) = background(red, green, blue) else {
            return OpStatus::OP_ERR_BAD_ARGUMENT;
        };
        let Some(host) = find_host(p) else {
            return OpStatus::OP_ERR_NO_SUCH_DOC;
        };
        let Some(v) = host.view(view) else {
            return OpStatus::OP_ERR_NO_SUCH_DOC;
        };
        {
            let mut r = v.request.lock();
            if r.background == color {
                return OpStatus::OP_OK;
            }
            r.background = color;
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
            background_red: req.background[0],
            background_green: req.background[1],
            background_blue: req.background[2],
            ..Default::default()
        };
        fill_render_info(&v, &mut info);
        // SAFETY: checked non-null.
        unsafe { *out = info };
        OpStatus::OP_OK
    })
}

#[cfg(any(target_os = "macos", windows))]
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
        #[cfg(windows)]
        {
            info.mip_builds = v.saved_info.lock().mip_builds + r.renderer.mip_builds();
        }
        #[cfg(not(windows))]
        {
            info.mip_builds = r.renderer.mip_builds();
        }
        info.doc_uploads = r.doc_uploads;
        info.doc_revision = r.doc_revision.unwrap_or(0);
        info.frame_errors = r.frame_errors;
        info.last_error = r.last_error as u32;
        info.region_uploads = r.region_uploads;
        info.region_tiles = r.region_tiles;
    }
    #[cfg(windows)]
    {
        let saved = *v.saved_info.lock();
        info.configures = info.configures.max(saved.configures);
        info.table_builds = info.table_builds.max(saved.table_builds);
        info.mip_builds = info.mip_builds.max(saved.mip_builds);
        info.doc_uploads = info.doc_uploads.max(saved.doc_uploads);
        info.region_uploads = info.region_uploads.max(saved.region_uploads);
        info.region_tiles = info.region_tiles.max(saved.region_tiles);
        info.frame_errors = info.frame_errors.max(saved.frame_errors);
        if info.last_error == OpStatus::OP_OK as u32 {
            info.last_error = saved.last_error;
        }
        let recovery_errors = v.recovery_errors.load(Ordering::SeqCst);
        info.frame_errors = info.frame_errors.max(saved.frame_errors + recovery_errors);
        if recovery_errors > 0 {
            info.last_error = OpStatus::OP_ERR_INTERNAL as u32;
        }
    }
}
#[cfg(not(any(target_os = "macos", windows)))]
fn fill_render_info(_v: &View, _info: &mut OpViewInfo) {}

/// Test only: destroys the canvas device, so the test host can prove the canvas
/// recovers. This is not `extern "C"`, so it is not in the C header.
#[cfg(all(windows, feature = "test-hooks"))]
#[doc(hidden)]
pub fn view_test_lose_device(p: *mut OpEngine) {
    if let Some(host) = find_host(p)
        && let Some(gpu) = host.gpu.lock().clone()
    {
        gpu.device.destroy();
    }
}

/// Removes the DX12 device only in the WARP test host. DX12 signals monitored
/// fences across devices, so this hook refuses the hardware adapter.
#[cfg(all(windows, feature = "test-hooks"))]
#[doc(hidden)]
pub fn view_test_remove_device(p: *mut OpEngine) -> Result<(), String> {
    use windows::Win32::Graphics::Direct3D12::{ID3D12Device, ID3D12Device5};
    use windows::core::Interface;
    if std::env::var("OP_GPU_ADAPTER").as_deref() != Ok("warp") {
        return Err("RemoveDevice is allowed only with OP_GPU_ADAPTER=warp".into());
    }
    let host = find_host(p).ok_or("no view host")?;
    let gpu = host.gpu.lock().clone().ok_or("no canvas device")?;
    let raw: ID3D12Device = unsafe {
        gpu.device
            .as_hal::<wgpu::hal::api::Dx12>()
            .unwrap()
            .raw_device()
            .clone()
    };
    let d5: ID3D12Device5 = raw.cast().map_err(|e| e.to_string())?;
    unsafe {
        d5.RemoveDevice();
    }
    Ok(())
}

#[cfg(all(windows, feature = "test-hooks"))]
#[doc(hidden)]
pub fn view_test_fail_next_configure() {
    op_gpu::win32::test_fail_next_configure();
}

#[cfg(all(windows, feature = "test-hooks"))]
pub fn view_test_fail_next_recreate() {
    FAIL_NEXT_RECREATE.store(true, Ordering::SeqCst);
}

#[cfg(all(windows, feature = "test-hooks"))]
pub fn view_test_fail_next_surface_recreate() {
    op_gpu::win32::test_fail_next_surface();
}

#[cfg(all(windows, feature = "test-hooks"))]
pub fn view_test_panic_next_frame() {
    PANIC_NEXT_FRAME.store(true, Ordering::SeqCst);
}

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
            #[cfg(windows)]
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
            if !host
                .state
                .lock()
                .views
                .values()
                .any(|live| Arc::ptr_eq(live, &v))
            {
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

/// Moves up to `cap` frame stamps (oldest first) out of the view's log into
/// `out` and writes how many to `out_len`. The log keeps the newest 16384
/// frames until read. Separate from `op_view_take_frame_times`: reading one
/// does not drain the other.
#[unsafe(no_mangle)]
pub extern "C" fn op_view_take_frame_stamps(
    p: *mut OpEngine,
    view: u64,
    out: *mut OpFrameStamp,
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
        let n = cap.min(log.stamps.len());
        for (i, t) in log.stamps.drain(..n).enumerate() {
            // SAFETY: the caller passes `cap` writable stamps; i < n <= cap.
            unsafe { *out.add(i) = t };
        }
        // SAFETY: checked non-null.
        unsafe { *out_len = n };
        OpStatus::OP_OK
    })
}
