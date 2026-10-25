//! GPU dispatch for registered effects.
//!
//! One `GpuContext` per engine starts on the first effect, on the worker side.
//! An effect runs here when its spec names a GPU kernel and the kernel supports
//! the parameters; otherwise the caller uses the CPU twin. The GPU renders the
//! selection bounds plus the effect's halo in chunks inside the device limits.
//! Selection coverage is then applied per pixel exactly as the CPU scheduler
//! does, and only the tiles inside the bounds are written.
//!
//! Supported fallbacks, each named in `doc.info`: no adapter or failed device
//! creation, device loss, `OP_FORCE_CPU=1`, parameters a kernel does not
//! support, and position-dependent kernels on images larger than the limits.
use crate::{Error, Result, RunCtx, bad, internal};
use op_core::doc::{DocState, MaskGrid, SelectionSource, SelectionState};
use op_core::selection::RasterMode;
use op_core::tiles::{TILE_SIZE, TileCoord, TileData};
use op_gpu::{GpuContext, GpuError, NoiseParams};
use op_ops::effect::{self, EffectSpec, GpuKernel, Prepared, Rect};
use parking_lot::Mutex;
use serde_json::{Value, json};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::time::Instant;

/// Output chunk edge when the caller sets none. Gaussian r=200 on a 2048 chunk
/// needs about 2448² × 36 bytes ≈ 216 MB of GPU memory at once.
const DEFAULT_CHUNK: u32 = 2048;
/// The largest per-pixel GPU buffer: the f32 RGBA accumulator, the noise and
/// photo tables, and the Gaussian column sums all use 16 bytes per pixel.
const BYTES_PER_PIXEL: u64 = 16;

/// Engine start-up options.
#[derive(Clone, Debug, Default)]
pub struct EngineOptions {
    /// Render every effect with the CPU twin. `OP_FORCE_CPU=1` sets this.
    pub force_cpu: bool,
    /// Largest output chunk edge for one GPU dispatch. The device limits can
    /// make chunks smaller; tests set this to force several chunks.
    pub max_gpu_chunk: Option<u32>,
    /// Test hook: GPU creation fails with this message.
    #[doc(hidden)]
    pub simulate_gpu_failure: Option<String>,
}
impl EngineOptions {
    /// Options from the environment: `OP_FORCE_CPU=1` turns the GPU off.
    pub fn from_env() -> Self {
        Self {
            force_cpu: Self::force_cpu_from(std::env::var("OP_FORCE_CPU").ok().as_deref()),
            ..Self::default()
        }
    }
    /// True when an `OP_FORCE_CPU` value asks for the CPU path.
    pub fn force_cpu_from(value: Option<&str>) -> bool {
        value.is_some_and(|v| matches!(v.trim(), "1" | "true" | "yes"))
    }
}

/// Device limits that size each dispatch.
#[derive(Clone, Copy, Debug)]
struct Limits {
    max_texture: u32,
    max_buffer: u64,
    max_storage_binding: u64,
    /// Output chunk edge before the halo is added.
    chunk_edge: u32,
}
impl Limits {
    /// The most pixels one dispatch may touch: every per-pixel buffer must fit
    /// one storage binding and one buffer.
    fn max_pixels(&self) -> u64 {
        self.max_buffer.min(self.max_storage_binding) / BYTES_PER_PIXEL
    }
    fn fits(&self, width: u32, height: u32) -> bool {
        width <= self.max_texture
            && height <= self.max_texture
            && u64::from(width) * u64::from(height) <= self.max_pixels()
    }
    /// Output chunk edge for a halo, or None when the halo alone fills a dispatch.
    fn chunk_for(&self, halo: u32) -> Option<u32> {
        let square = (self.max_pixels() as f64).sqrt().floor() as u64;
        let input_edge = u64::from(self.max_texture).min(square);
        let room = input_edge.checked_sub(2 * u64::from(halo))?;
        let edge = room.min(u64::from(self.chunk_edge)) as u32;
        (edge >= 16).then_some(edge)
    }
}

struct Device {
    ctx: GpuContext,
    limits: Limits,
}
enum Slot {
    Untried,
    Ready(Box<Device>),
    Off(String),
}

#[derive(Default)]
struct Stats {
    /// Effect runs that took the GPU path, including cancelled ones.
    gpu_started: u64,
    /// Effect runs the GPU path finished.
    gpu_runs: u64,
    gpu_cancelled: u64,
    /// Effect runs with a GPU kernel that used the CPU twin, for any reason.
    cpu_runs: u64,
    gpu_dispatches: u64,
    last_cpu_reason: Option<String>,
    last_chunks: u64,
    /// Milliseconds of the last GPU run: tile reads, GPU work, blend and writes.
    last_ms: [f64; 3],
}

/// The engine's one GPU, shared by every document and worker.
pub(crate) struct GpuShared {
    options: EngineOptions,
    slot: Mutex<Slot>,
    stats: Mutex<Stats>,
}
impl GpuShared {
    pub(crate) fn new(options: EngineOptions) -> Arc<Self> {
        let slot = if options.force_cpu {
            Slot::Off("forced off by OP_FORCE_CPU=1 or the force_cpu engine option".into())
        } else {
            Slot::Untried
        };
        Arc::new(Self {
            options,
            slot: Mutex::new(slot),
            stats: Mutex::new(Stats::default()),
        })
    }
    /// Fields merged into `doc.info`.
    pub(crate) fn info(&self) -> Value {
        let (gpu, reason, device) = match &*self.slot.lock() {
            Slot::Untried => (
                false,
                Some("not started yet; the GPU starts on the first effect".to_owned()),
                Value::Null,
            ),
            Slot::Off(reason) => (false, Some(reason.clone()), Value::Null),
            Slot::Ready(device) => {
                let info = device.ctx.info();
                let l = device.limits;
                (
                    true,
                    None,
                    json!({
                        "adapter": info.name,
                        "backend": format!("{:?}", info.backend),
                        "driver": info.driver,
                        "max_texture_dimension_2d": l.max_texture,
                        "max_buffer_size": l.max_buffer,
                        "max_storage_buffer_binding_size": l.max_storage_binding,
                        "max_dispatch_pixels": l.max_pixels(),
                        "chunk_edge": l.chunk_edge,
                    }),
                )
            }
        };
        let s = self.stats.lock();
        json!({
            "gpu": gpu,
            "gpu_reason": reason,
            "gpu_device": device,
            "gpu_effects": {
                "gpu_started": s.gpu_started,
                "gpu_runs": s.gpu_runs,
                "gpu_cancelled": s.gpu_cancelled,
                "cpu_runs": s.cpu_runs,
                "gpu_dispatches": s.gpu_dispatches,
                "last_cpu_reason": s.last_cpu_reason,
                "last_chunks": s.last_chunks,
                "last_read_ms": s.last_ms[0],
                "last_gpu_ms": s.last_ms[1],
                "last_compose_ms": s.last_ms[2],
            },
        })
    }
    fn note_cpu(&self, reason: String) {
        let mut s = self.stats.lock();
        s.cpu_runs += 1;
        s.last_cpu_reason = Some(reason);
    }
    /// Starts the GPU on first use. Returns the reason when there is none.
    fn ensure(&self, slot: &mut Slot) -> std::result::Result<(), String> {
        if matches!(slot, Slot::Untried) {
            let created = match &self.options.simulate_gpu_failure {
                Some(message) => Err(format!("GPU creation failed: {message}")),
                None => catch_unwind(GpuContext::new)
                    .map_err(|_| "GPU creation panicked".to_owned())
                    .and_then(|r| r.map_err(|e| format!("GPU creation failed: {e}"))),
            };
            *slot = match created {
                Ok(ctx) => {
                    let l = &ctx.info().limits;
                    let limits = Limits {
                        max_texture: l.max_texture_dimension_2d,
                        max_buffer: l.max_buffer_size,
                        max_storage_binding: u64::from(l.max_storage_buffer_binding_size),
                        chunk_edge: self
                            .options
                            .max_gpu_chunk
                            .unwrap_or(DEFAULT_CHUNK)
                            .clamp(16, l.max_texture_dimension_2d),
                    };
                    Slot::Ready(Box::new(Device { ctx, limits }))
                }
                Err(reason) => {
                    eprintln!("op-engine: no GPU for effects, using the CPU path: {reason}");
                    Slot::Off(reason)
                }
            };
        }
        match slot {
            Slot::Off(reason) => Err(reason.clone()),
            _ => Ok(()),
        }
    }
    /// Turns the GPU off for the rest of the engine's life and logs it once.
    fn disable(&self, slot: &mut Slot, reason: String) {
        eprintln!("op-engine: GPU turned off, using the CPU path from now on: {reason}");
        *slot = Slot::Off(reason);
    }
}

/// How one effect maps onto an op-gpu entry point.
#[derive(Clone, Copy, Debug)]
enum Call {
    Params(GpuKernel, [i32; 3]),
    Lut(GpuKernel, bool),
    Gaussian(u32),
    Glow(u32, i32, i32),
    Soften(u32, i32, i32),
    Noise(NoiseParams),
    Clouds(u8),
}
#[derive(Clone, Copy, Debug)]
struct Plan {
    call: Call,
    /// Pixels the kernel reads beyond each output pixel.
    halo: u32,
    /// The kernel uses the image size or absolute positions, so one dispatch
    /// must cover the whole image.
    whole_image: bool,
    /// The kernel needs `spec.prepare` (the channel lookup tables).
    prepare: bool,
}
impl Plan {
    fn new(call: Call, halo: u32) -> Self {
        Self {
            call,
            halo,
            whole_image: false,
            prepare: false,
        }
    }
    fn whole(call: Call) -> Self {
        Self {
            whole_image: true,
            ..Self::new(call, 0)
        }
    }
}

fn int(p: &Value, key: &str) -> i32 {
    p[key].as_i64().unwrap_or_default() as i32
}
fn float(p: &Value, key: &str) -> f64 {
    p[key].as_f64().unwrap_or(f64::NAN)
}
fn require(ok: bool, detail: &str) -> std::result::Result<(), String> {
    if ok { Ok(()) } else { Err(detail.into()) }
}
/// A float the kernel takes as a whole number.
fn whole_number(v: f64, key: &str) -> std::result::Result<i32, String> {
    require(v.fract() == 0.0, &format!("{key} must be a whole number"))?;
    Ok(v as i32)
}
/// A float the kernel takes in thousandths.
fn thousandths(v: f64, key: &str) -> std::result::Result<i32, String> {
    let t = (v * 1000.0).round();
    require(
        (t / 1000.0 - v).abs() < 1e-9,
        &format!("{key} needs more than 3 decimals"),
    )?;
    Ok(t as i32)
}
fn is_default(spec: &EffectSpec, p: &Value, keys: &[&str]) -> bool {
    let defaults = spec.default_params();
    keys.iter().all(|key| p[*key] == defaults[*key])
}
fn defaults_only(spec: &EffectSpec, p: &Value, keys: &[&str]) -> std::result::Result<(), String> {
    require(
        is_default(spec, p, keys),
        &format!("the kernel has fixed {}", keys.join(", ")),
    )
}

/// Maps validated parameters onto a kernel, or says why the kernel cannot run them.
fn plan(spec: &EffectSpec, kernel: GpuKernel, p: &Value) -> std::result::Result<Plan, String> {
    let params = |values: [i32; 3]| Call::Params(kernel, values);
    Ok(match spec.id {
        "effect.adjust.invert_colors"
        | "effect.adjust.invert_alpha"
        | "effect.adjust.black_and_white" => Plan::new(params([0; 3]), 0),
        "effect.adjust.brightness_contrast" => {
            Plan::new(params([int(p, "Brightness"), int(p, "Contrast"), 0]), 0)
        }
        "effect.adjust.temperature_tint" => {
            Plan::new(params([int(p, "Temperature"), int(p, "Tint"), 0]), 0)
        }
        "effect.adjust.highlights_shadows" => {
            Plan::new(params([int(p, "Highlights"), int(p, "Shadows"), 0]), 0)
        }
        "effect.adjust.posterize" => {
            let red = int(p, "RedLevels");
            let values = if p["LinkLevels"] == true {
                [red; 3]
            } else {
                [red, int(p, "GreenLevels"), int(p, "BlueLevels")]
            };
            Plan::new(params(values), 0)
        }
        "effect.adjust.sepia" => Plan::new(params([int(p, "Intensity"), 0, 0]), 0),
        "effect.adjust.curves" => Plan {
            prepare: true,
            ..Plan::new(Call::Lut(kernel, p["ColorTransferMode"] != "Rgb"), 0)
        },
        "effect.adjust.levels" | "effect.adjust.exposure" => Plan {
            prepare: true,
            ..Plan::new(Call::Lut(kernel, false), 0)
        },
        "effect.blur.gaussian" => {
            // Gamma power 1 and full quality are the legacy integer Gaussian.
            require(
                p["gamma"] == "linear" && p["linear_light"] == true,
                "the kernel has no gamma stage",
            )?;
            require(
                float(p, "gamma_boost") == 0.0,
                "the kernel has no gamma boost",
            )?;
            require(int(p, "quality") == 4, "the kernel samples every offset")?;
            let radius = int(p, "radius") as u32;
            Plan::new(Call::Gaussian(radius), radius)
        }
        "effect.blur.fragment" => {
            defaults_only(spec, p, &["fragments", "distance", "rotation"])?;
            Plan::new(params([0; 3]), 8)
        }
        "effect.blur.median" => {
            let radius = int(p, "radius");
            Plan::new(params([radius, int(p, "percentile"), 0]), radius as u32)
        }
        "effect.blur.motion" => {
            defaults_only(spec, p, &["angle", "centered", "distance"])?;
            Plan::new(params([0; 3]), 11)
        }
        "effect.blur.radial" => {
            defaults_only(spec, p, &["angle", "offset_x", "offset_y", "quality"])?;
            Plan::whole(params([0; 3]))
        }
        "effect.blur.zoom" => {
            defaults_only(spec, p, &["amount", "offset_x", "offset_y"])?;
            Plan::whole(params([0; 3]))
        }
        "effect.stylize.edge_detect" => {
            defaults_only(
                spec,
                p,
                &["strength", "blurring", "algorithm", "overlay_edges"],
            )?;
            Plan::new(params([whole_number(float(p, "angle"), "angle")?, 0, 0]), 1)
        }
        "effect.stylize.emboss" | "effect.stylize.relief" => {
            Plan::new(params([whole_number(float(p, "angle"), "angle")?, 0, 0]), 1)
        }
        "effect.artistic.oil_painting" => {
            let radius = int(p, "brush_size");
            Plan::new(params([radius, int(p, "coarseness"), 0]), radius as u32)
        }
        "effect.stylize.outline" => {
            let radius = match p["quality"].as_str() {
                Some("standard") => int(p, "thickness"),
                Some("high") => int(p, "thickness") + 1,
                _ => return Err("the kernel has no fast quality".into()),
            };
            require(radius <= 200, "the kernel's radius is at most 200")?;
            Plan::new(params([radius, int(p, "intensity"), 0]), radius as u32)
        }
        "effect.artistic.ink_sketch" => {
            Plan::new(params([int(p, "ink_outline"), int(p, "coloring"), 0]), 6)
        }
        "effect.artistic.pencil_sketch" => {
            // legacy_gaussian keeps 64-bit row sums, so every tip size fits.
            let radius = int(p, "pencil_tip_size");
            Plan::new(params([radius, int(p, "range"), 0]), radius as u32)
        }
        "effect.distort.pixelate" => {
            require(
                p["scale_down"] == "legacy" && p["scale_up"] == "nearest",
                "the kernel has only legacy scale down and nearest scale up",
            )?;
            Plan::whole(params([int(p, "cell_size"), 0, 0]))
        }
        "effect.distort.bulge" => {
            defaults_only(
                spec,
                p,
                &["center_x", "center_y", "edge_behavior", "quality"],
            )?;
            // The kernel now rounds each f32 step as the CPU does and gives the
            // CPU's bytes at every amount (op-gpu tests/effects.rs), but
            // tests/gpu_effects.rs still pins the CPU route away from the
            // default amount; lifting it needs that test changed first.
            require(
                is_default(spec, p, &["amount"]),
                "the engine routes Bulge to the CPU away from the default amount",
            )?;
            Plan::whole(params([int(p, "amount"), 0, 0]))
        }
        "effect.distort.twist" => {
            defaults_only(
                spec,
                p,
                &["amount", "size", "center_x", "center_y", "quality"],
            )?;
            Plan::whole(params([0; 3]))
        }
        "effect.photo.red_eye_removal" => {
            Plan::new(params([int(p, "tolerance"), int(p, "saturation"), 0]), 0)
        }
        "effect.photo.vignette" => {
            let cx = thousandths(float(p, "center_x"), "center_x")?;
            let cy = thousandths(float(p, "center_y"), "center_y")?;
            let packed = (int(p, "radius") << 16) | int(p, "strength");
            Plan::whole(params([cx, cy, packed]))
        }
        "effect.photo.sharpen" => {
            let radius = int(p, "amount");
            Plan::new(params([radius, 0, 0]), radius as u32)
        }
        "effect.noise.reduce" => {
            let radius = int(p, "radius");
            let strength = thousandths(float(p, "strength"), "strength")?;
            Plan::new(params([radius, strength, 0]), radius as u32)
        }
        "effect.noise.add" => {
            let seed = p["seed"].as_i64().unwrap_or(-1);
            require(
                (0..=i64::from(i32::MAX)).contains(&seed),
                "seed is out of range",
            )?;
            Plan::whole(Call::Noise(NoiseParams {
                intensity: int(p, "intensity"),
                saturation: int(p, "saturation"),
                coverage: float(p, "coverage"),
                seed: seed as i32,
            }))
        }
        "effect.photo.glow" => {
            let radius = int(p, "radius") as u32;
            let call = Call::Glow(radius, int(p, "brightness"), int(p, "contrast"));
            Plan::new(call, radius)
        }
        "effect.photo.soften_portrait" => {
            let softness = int(p, "softness") as u32;
            let call = Call::Soften(softness, int(p, "lighting"), int(p, "warmth"));
            Plan::new(call, softness * 3)
        }
        "effect.blur.bokeh" => {
            let radius = int(p, "radius");
            let call = params([radius, int(p, "gamma_boost"), int(p, "quality")]);
            Plan::new(call, radius as u32)
        }
        "effect.blur.sketch" => {
            let radius = int(p, "radius");
            let call = params([radius, int(p, "percentile"), int(p, "smoothness")]);
            Plan::new(call, radius as u32)
        }
        "effect.blur.square" => {
            let radius = int(p, "radius");
            Plan::new(params([radius, int(p, "gamma_boost"), 0]), radius as u32)
        }
        "effect.blur.surface" => {
            let radius = int(p, "radius");
            Plan::new(params([radius, int(p, "threshold"), 0]), radius as u32)
        }
        "effect.blur.unfocus" => {
            let radius = int(p, "radius");
            Plan::new(params([radius, 0, 0]), radius as u32)
        }
        "effect.distort.morphology" => {
            let width = int(p, "width");
            let height = if p["linked"] == true {
                width
            } else {
                int(p, "height")
            };
            let dilate = i32::from(p["mode"] == "dilate");
            // The CPU twin declares the whole image as its input.
            Plan::whole(params([width, height, dilate]))
        }
        "effect.render.turbulence" => {
            // Same packing as op-gpu's effect_turbulence: octaves, period, size,
            // noise kind and blend in one word; the noise uses absolute positions.
            let packed = int(p, "octaves")
                | (int(p, "period") << 4)
                | (int(p, "size") << 14)
                | (i32::from(p["noise"] == "turbulence") << 21)
                | (i32::from(p["blend"] == true) << 22);
            Plan::whole(params([packed, int(p, "seed"), 0]))
        }
        "effect.render.clouds" => {
            // op-gpu's Clouds path writes the default palette over a CPU-made field.
            defaults_only(
                spec,
                p,
                &[
                    "scale",
                    "roughness",
                    "blend_mode",
                    "color_1",
                    "color_1_alpha",
                    "color_2",
                    "color_2_alpha",
                ],
            )?;
            let seed = int(p, "seed");
            require(
                (0..=255).contains(&seed),
                "the GPU Clouds path takes seeds 0 to 255",
            )?;
            Plan::whole(Call::Clouds(seed as u8))
        }
        _ => return Err("the engine has no GPU mapping for this effect yet".into()),
    })
}

/// Selection coverage, read per pixel as the CPU scheduler reads it.
enum Coverage {
    All,
    Mask(Arc<MaskGrid>),
}
impl Coverage {
    fn at(&self, x: u32, y: u32) -> u8 {
        match self {
            Coverage::All => 255,
            Coverage::Mask(grid) if x < grid.width && y < grid.height => {
                grid.bytes[y as usize * grid.width as usize + x as usize]
            }
            Coverage::Mask(_) => 0,
        }
    }
}
/// The coverage and the smallest rectangle holding every covered pixel, or
/// None when nothing is covered. Err means the mask is malformed; the CPU path
/// then reports the error.
fn selection(doc: &DocState) -> std::result::Result<Option<(Coverage, Rect)>, String> {
    let grid = match &doc.selection {
        SelectionState::None => {
            return Ok(Some((
                Coverage::All,
                Rect::new(0, 0, doc.width, doc.height),
            )));
        }
        SelectionState::Active(SelectionSource::Geometry(_)) => {
            doc.rasterize_selection(RasterMode::Antialiased)
        }
        SelectionState::Active(SelectionSource::Coverage(mask)) => mask.clone(),
    };
    if grid.bytes.len() != grid.width as usize * grid.height as usize {
        return Err("the selection mask has the wrong byte count".into());
    }
    let (w, h) = (grid.width.min(doc.width), grid.height.min(doc.height));
    let (mut left, mut top, mut right, mut bottom) = (u32::MAX, u32::MAX, 0, 0);
    for y in 0..h {
        let row = &grid.bytes[y as usize * grid.width as usize..][..w as usize];
        let Some(first) = row.iter().position(|&c| c != 0) else {
            continue;
        };
        let last = row.iter().rposition(|&c| c != 0).unwrap();
        left = left.min(first as u32);
        right = right.max(last as u32 + 1);
        top = top.min(y);
        bottom = y + 1;
    }
    if left == u32::MAX {
        return Ok(None);
    }
    Ok(Some((
        Coverage::Mask(grid),
        Rect::new(left as i32, top as i32, right - left, bottom - top),
    )))
}

/// Copies an in-image rectangle of the active layer into tight RGBA rows.
fn read_region(doc: &DocState, r: Rect) -> Result<Vec<u8>> {
    let (rw, rh) = (r.width as usize, r.height as usize);
    let mut out = vec![0; rw * rh * 4];
    for coord in effect::tile_coords(r) {
        let tile = doc.read_tile(doc.active_layer, coord).map_err(internal)?;
        let (tx, ty) = (coord.x * TILE_SIZE as i32, coord.y * TILE_SIZE as i32);
        let x0 = tx.max(r.x);
        let x1 = (tx + TILE_SIZE as i32).min(r.x + r.width as i32);
        let y0 = ty.max(r.y);
        let y1 = (ty + TILE_SIZE as i32).min(r.y + r.height as i32);
        let n = (x1 - x0) as usize * 4;
        for y in y0..y1 {
            let src = ((y - ty) as usize * TILE_SIZE + (x0 - tx) as usize) * 4;
            let dst = ((y - r.y) as usize * rw + (x0 - r.x) as usize) * 4;
            out[dst..dst + n].copy_from_slice(&tile[src..src + n]);
        }
    }
    Ok(out)
}

fn run_call(
    gpu: &mut GpuContext,
    call: Call,
    width: u32,
    height: u32,
    bytes: &[u8],
    prepared: &Prepared,
) -> op_gpu::Result<Vec<u8>> {
    match call {
        Call::Params(kernel, values) => {
            gpu.effect_kernel_with_params(kernel, width, height, bytes, values)
        }
        Call::Lut(kernel, luminosity) => {
            let identity = std::array::from_fn(|_| std::array::from_fn(|v| v as u8));
            let lut = prepared.channel_lut.as_deref().unwrap_or(&identity);
            gpu.effect_kernel_with_lut(kernel, width, height, bytes, lut, luminosity)
        }
        Call::Gaussian(radius) => gpu.gaussian_cached(width, height, bytes, radius),
        Call::Glow(radius, brightness, contrast) => {
            gpu.effect_photo_glow(width, height, bytes, radius, brightness, contrast)
        }
        Call::Soften(softness, lighting, warmth) => {
            gpu.effect_photo_soften(width, height, bytes, softness, lighting, warmth)
        }
        Call::Noise(params) => gpu.effect_add_noise(width, height, bytes, params),
        Call::Clouds(seed) => gpu.effect_clouds_seeded(width, height, bytes, seed),
    }
}

/// One GPU dispatch: an output rectangle and the input it reads.
struct Chunk {
    output: Rect,
    input: Rect,
}
fn chunks(bounds: Rect, doc: &DocState, plan: &Plan, limits: &Limits) -> Option<Vec<Chunk>> {
    if plan.whole_image {
        let whole = Rect::new(0, 0, doc.width, doc.height);
        return limits.fits(doc.width, doc.height).then(|| {
            vec![Chunk {
                output: bounds,
                input: whole,
            }]
        });
    }
    let edge = limits.chunk_for(plan.halo)? as i64;
    let halo = i64::from(plan.halo);
    let (bx, by) = (i64::from(bounds.x), i64::from(bounds.y));
    let (bw, bh) = (i64::from(bounds.width), i64::from(bounds.height));
    let mut out = Vec::new();
    let mut y = by;
    while y < by + bh {
        let h = edge.min(by + bh - y);
        let mut x = bx;
        while x < bx + bw {
            let w = edge.min(bx + bw - x);
            let ix = (x - halo).max(0);
            let iy = (y - halo).max(0);
            let ix1 = (x + w + halo).min(i64::from(doc.width));
            let iy1 = (y + h + halo).min(i64::from(doc.height));
            out.push(Chunk {
                output: Rect::new(x as i32, y as i32, w as u32, h as u32),
                input: Rect::new(ix as i32, iy as i32, (ix1 - ix) as u32, (iy1 - iy) as u32),
            });
            x += w;
        }
        y += h;
    }
    Some(out)
}

/// What the GPU path decided.
enum Outcome {
    Done(DocState),
    /// Use the CPU twin; the reason goes to `doc.info`.
    Cpu(String),
}

/// Renders a registered effect on the GPU when it can. `Ok(None)` means the
/// caller must use the CPU path (the reason is already recorded).
pub(crate) fn try_render(
    ctx: &RunCtx,
    doc: &DocState,
    spec: &EffectSpec,
    params: &Value,
) -> Result<Option<DocState>> {
    let Some(kernel) = spec.gpu else {
        return Ok(None);
    };
    let shared = ctx.gpu.clone();
    match render(ctx, &shared, doc, spec, kernel, params) {
        Ok(Outcome::Done(state)) => {
            shared.stats.lock().gpu_runs += 1;
            Ok(Some(state))
        }
        Ok(Outcome::Cpu(reason)) => {
            shared.note_cpu(format!("{}: {reason}; used the CPU", spec.id));
            Ok(None)
        }
        Err(Error::Cancelled) => {
            shared.stats.lock().gpu_cancelled += 1;
            Err(Error::Cancelled)
        }
        Err(e) => Err(e),
    }
}

fn render(
    ctx: &RunCtx,
    shared: &GpuShared,
    doc: &DocState,
    spec: &EffectSpec,
    kernel: GpuKernel,
    params: &Value,
) -> Result<Outcome> {
    // Invalid parameters: the CPU path reports the same error it always did.
    let Ok(params) = spec.schema.validate(params.clone()) else {
        return Ok(Outcome::Cpu("parameters failed validation".into()));
    };
    let plan = match plan(spec, kernel, &params) {
        Ok(plan) => plan,
        Err(detail) => {
            return Ok(Outcome::Cpu(format!(
                "the GPU kernel does not support these parameters ({detail})"
            )));
        }
    };
    let (coverage, bounds) = match selection(doc) {
        Ok(Some(found)) => found,
        // An active empty selection changes nothing.
        Ok(None) => return Ok(Outcome::Done(doc.clone())),
        Err(detail) => return Ok(Outcome::Cpu(detail)),
    };
    let mut slot = shared.slot.lock();
    if let Err(reason) = shared.ensure(&mut slot) {
        return Ok(Outcome::Cpu(format!("no GPU ({reason})")));
    }
    let Slot::Ready(device) = &mut *slot else {
        unreachable!("ensure returned Ok without a device");
    };
    let Some(work) = chunks(bounds, doc, &plan, &device.limits) else {
        return Ok(Outcome::Cpu(format!(
            "the {}x{} image or the {} px halo exceeds the GPU limits",
            doc.width, doc.height, plan.halo
        )));
    };
    let prepared = if plan.prepare {
        match (spec.prepare)(doc, &doc.selection, &params) {
            Ok(prepared) => prepared,
            Err(e) => return Ok(Outcome::Cpu(format!("prepare failed ({e})"))),
        }
    } else {
        Prepared::default()
    };
    shared.stats.lock().gpu_started += 1;
    let (rw, rh) = (bounds.width as usize, bounds.height as usize);
    let mut effected = vec![0u8; rw * rh * 4];
    let mut ms = [0.0; 3];
    let elapsed = |start: Instant| start.elapsed().as_secs_f64() * 1000.0;
    for chunk in &work {
        ctx.check()?;
        let start = Instant::now();
        let input = read_region(doc, chunk.input)?;
        ms[0] += elapsed(start);
        let start = Instant::now();
        let (iw, ih) = (chunk.input.width, chunk.input.height);
        let before = device.ctx.dispatch_count();
        let result = catch_unwind(AssertUnwindSafe(|| {
            run_call(&mut device.ctx, plan.call, iw, ih, &input, &prepared)
        }));
        ms[1] += elapsed(start);
        let dispatched = device.ctx.dispatch_count() - before;
        shared.stats.lock().gpu_dispatches += dispatched;
        let pixels = match result {
            Ok(Ok(pixels)) => pixels,
            Ok(Err(GpuError::DeviceLost)) => {
                shared.disable(&mut slot, "the GPU device was lost".into());
                return Ok(Outcome::Cpu("the GPU device was lost".into()));
            }
            Ok(Err(GpuError::InvalidImage)) => {
                return Ok(Outcome::Cpu(
                    "the GPU kernel rejected the parameters or the image size".into(),
                ));
            }
            Ok(Err(e)) => return Ok(Outcome::Cpu(format!("GPU error ({e})"))),
            Err(_) => {
                shared.disable(&mut slot, "GPU work panicked".into());
                return Ok(Outcome::Cpu("GPU work panicked".into()));
            }
        };
        ctx.check()?;
        if pixels.len() != iw as usize * ih as usize * 4 {
            return Err(internal("GPU returned the wrong byte count"));
        }
        let o = chunk.output;
        let n = o.width as usize * 4;
        for y in 0..o.height as i32 {
            let src = ((o.y + y - chunk.input.y) as usize * iw as usize
                + (o.x - chunk.input.x) as usize)
                * 4;
            let dst = ((o.y + y - bounds.y) as usize * rw + (o.x - bounds.x) as usize) * 4;
            effected[dst..dst + n].copy_from_slice(&pixels[src..src + n]);
        }
    }
    drop(slot);
    let start = Instant::now();
    let next = compose(ctx, doc, bounds, &coverage, &effected)?;
    ms[2] = elapsed(start);
    let mut stats = shared.stats.lock();
    stats.last_chunks = work.len() as u64;
    stats.last_ms = ms;
    Ok(Outcome::Done(next))
}

/// Blends the effected rectangle into each tile by selection coverage, with the
/// CPU scheduler's rounding, and writes the tiles. Checks cancel per tile.
fn compose(
    ctx: &RunCtx,
    doc: &DocState,
    bounds: Rect,
    coverage: &Coverage,
    effected: &[u8],
) -> Result<DocState> {
    let coords: Vec<TileCoord> = effect::tile_coords(bounds);
    let rw = bounds.width as usize;
    let mut next = doc.clone();
    for (done, &coord) in coords.iter().enumerate() {
        ctx.check()?;
        let mut tile = doc.read_tile(doc.active_layer, coord).map_err(internal)?;
        let (tx, ty) = (coord.x * TILE_SIZE as i32, coord.y * TILE_SIZE as i32);
        let x0 = tx.max(bounds.x);
        let x1 = (tx + TILE_SIZE as i32).min(bounds.x + bounds.width as i32);
        let y0 = ty.max(bounds.y);
        let y1 = (ty + TILE_SIZE as i32).min(bounds.y + bounds.height as i32);
        for y in y0..y1 {
            if matches!(coverage, Coverage::All) {
                // Full coverage: coverage_lerp(o, e, 255) == e, so copy the row.
                let i = ((y - ty) as usize * TILE_SIZE + (x0 - tx) as usize) * 4;
                let e = ((y - bounds.y) as usize * rw + (x0 - bounds.x) as usize) * 4;
                let n = (x1 - x0) as usize * 4;
                tile[i..i + n].copy_from_slice(&effected[e..e + n]);
                continue;
            }
            for x in x0..x1 {
                let c = coverage.at(x as u32, y as u32);
                if c == 0 {
                    continue;
                }
                let i = ((y - ty) as usize * TILE_SIZE + (x - tx) as usize) * 4;
                let e = ((y - bounds.y) as usize * rw + (x - bounds.x) as usize) * 4;
                for channel in 0..4 {
                    tile[i + channel] =
                        effect::coverage_lerp(tile[i + channel], effected[e + channel], c);
                }
            }
        }
        next = next
            .write_tile(
                doc.active_layer,
                coord,
                TileData::try_from(tile).map_err(internal)?,
            )
            .map_err(bad)?;
        ctx.progress(done + 1, coords.len());
    }
    ctx.check()?;
    Ok(next)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn limits(max_texture: u32, max_bytes: u64, chunk_edge: u32) -> Limits {
        Limits {
            max_texture,
            max_buffer: max_bytes,
            max_storage_binding: max_bytes,
            chunk_edge,
        }
    }

    #[test]
    fn chunks_respect_texture_and_buffer_limits() {
        // 4096 px textures, 64 MB buffers (4M pixels at 16 B), 2048 px chunks.
        let l = limits(4096, 64 << 20, 2048);
        assert_eq!(l.chunk_for(0), Some(2048));
        // sqrt(4M) = 2048 input pixels per edge, so a 50 px halo leaves 1948.
        assert_eq!(l.chunk_for(50), Some(1948));
        assert_eq!(l.chunk_for(200), Some(1648));
        assert_eq!(l.chunk_for(1100), None);
        assert!(l.fits(2048, 2048));
        assert!(!l.fits(4097, 10));
        assert!(!l.fits(4096, 4096));
    }

    #[test]
    fn full_coverage_lerp_is_a_copy() {
        for original in 0..=255 {
            for effected in 0..=255 {
                assert_eq!(effect::coverage_lerp(original, effected, 255), effected);
            }
        }
    }

    #[test]
    fn force_cpu_values() {
        assert!(EngineOptions::force_cpu_from(Some(" 1 ")));
        assert!(EngineOptions::force_cpu_from(Some("true")));
        assert!(!EngineOptions::force_cpu_from(Some("")));
    }

    #[test]
    fn every_registered_gpu_effect_has_a_mapping_for_its_defaults() {
        for spec in effect::registered() {
            let Some(kernel) = spec.gpu else { continue };
            let p = spec.default_params();
            let plan = plan(&spec, kernel, &p);
            assert!(plan.is_ok(), "{}: {:?}", spec.id, plan.err());
        }
    }

    #[test]
    fn plan_halo_covers_the_cpu_required_input() {
        let store = Arc::new(
            op_core::tiles::TileStore::new(std::env::temp_dir(), 8 * 1024 * 1024).unwrap(),
        );
        let doc = DocState::new(store, 64, 48, 96.0).unwrap();
        for spec in effect::registered() {
            let Some(kernel) = spec.gpu else { continue };
            let p = spec.default_params();
            let plan = plan(&spec, kernel, &p).unwrap();
            if plan.whole_image {
                continue;
            }
            let prepared = (spec.prepare)(&doc, &doc.selection, &p).unwrap();
            let output = Rect::new(10, 10, 4, 4);
            let input = (spec.required_input)(output, &prepared);
            let halo = (output.x - input.x).max(0) as u32;
            assert!(plan.halo >= halo, "{}: {} < {halo}", spec.id, plan.halo);
        }
    }
}
