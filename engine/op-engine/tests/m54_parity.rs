//! M54 parity and performance sweep.
//!
//! Fast checks (normal test runs): every registered effect is listed as GPU or
//! CPU-only with a reason, GPU effects run on the GPU at their defaults, tiled
//! renders equal a whole-image render, legacy target rows hold on the small
//! oracle fixtures, engine preview equals the committed output, and a mixed
//! 10-effect chain stays within 2/255 between CPU and GPU.
//!
//! Long runs (ignored, run them in release):
//! - `cargo test --release -p op-engine --test m54_parity m54_write_parity_matrix -- --ignored --test-threads=1`
//!   writes docs/reports/parity-matrix.md.
//! - `cargo test --release -p op-engine --test m54_parity m54_write_perf_table -- --ignored --test-threads=1`
//!   writes docs/reports/perf-table.md.
//!
//! Set OP_GPU_TESTS=0 to skip the parts that need a GPU adapter.
use op_core::doc::DocState;
use op_core::tiles::{TILE_BYTES, TILE_SIZE, TileCoord, TileData, TileStore};
use op_engine::{Engine, EngineOptions, Event, EventKind, JobTicket, Op};
use op_ops::effect::{self, BorderRule, EffectSpec, GpuKernel, ParamKind, Rect};
use serde_json::{Value, json};
use std::alloc::{GlobalAlloc, Layout, System};
use std::collections::{BTreeMap, HashSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Counts heap bytes so each matrix cell can report its peak CPU-side memory.
/// GPU memory is not visible here.
struct Counting;
static CURRENT: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(layout) };
        if !p.is_null() {
            let now = CURRENT.fetch_add(layout.size(), Ordering::Relaxed) + layout.size();
            PEAK.fetch_max(now, Ordering::Relaxed);
        }
        p
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) };
        CURRENT.fetch_sub(layout.size(), Ordering::Relaxed);
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let p = unsafe { System.realloc(ptr, layout, new_size) };
        if !p.is_null() {
            if new_size > layout.size() {
                let now = CURRENT.fetch_add(new_size - layout.size(), Ordering::Relaxed)
                    + (new_size - layout.size());
                PEAK.fetch_max(now, Ordering::Relaxed);
            } else {
                CURRENT.fetch_sub(layout.size() - new_size, Ordering::Relaxed);
            }
        }
        p
    }
}
#[global_allocator]
static ALLOC: Counting = Counting;
/// Starts a peak window; returns the bytes in use at the start.
fn peak_start() -> usize {
    let now = CURRENT.load(Ordering::Relaxed);
    PEAK.store(now, Ordering::Relaxed);
    now
}
fn peak_mb(start: usize) -> f64 {
    PEAK.load(Ordering::Relaxed).saturating_sub(start) as f64 / (1024.0 * 1024.0)
}

fn gpu_tests_enabled() -> bool {
    if std::env::var("OP_GPU_TESTS").as_deref() == Ok("0") {
        println!("SKIP OP_GPU_TESTS=0");
        return false;
    }
    true
}
fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../")
}

/// Effects with no GPU kernel. Each one runs on the CPU on purpose; the matrix
/// prints this list, and a new effect without a kernel must be added here.
const CPU_ONLY: [&str; 13] = [
    "effect.adjust.auto_level",
    "effect.adjust.hue_saturation",
    "effect.render.julia_fractal",
    "effect.render.mandelbrot_fractal",
    "effect.color.quantize",
    "effect.distort.crystalize",
    "effect.distort.dents",
    "effect.distort.frosted_glass",
    "effect.distort.polar_inversion",
    "effect.distort.tile_reflection",
    "effect.object.drop_shadow",
    "effect.fill.content_aware",
    "effect.layers.rotate_zoom",
];
fn cpu_only_reason(spec: &EffectSpec) -> String {
    format!(
        "CPU only: op-ops registers no GPU kernel (passes {:?})",
        spec.passes
    )
}

/// CPU vs GPU tolerance per kernel. Same table as tests/gpu_effects.rs.
fn kernel_tolerance(kernel: GpuKernel) -> u8 {
    u8::from(matches!(
        kernel,
        GpuKernel::Bulge
            | GpuKernel::Twist
            | GpuKernel::Gaussian
            | GpuKernel::Motion
            | GpuKernel::EdgeDetect
            | GpuKernel::Emboss
            | GpuKernel::Relief
            | GpuKernel::Vignette
            | GpuKernel::ReduceNoise
            | GpuKernel::Glow
            | GpuKernel::SoftenPortrait
            | GpuKernel::Sepia
            | GpuKernel::Bokeh
            | GpuKernel::Sketch
            | GpuKernel::Square
            | GpuKernel::Surface
            | GpuKernel::Unfocus
    ))
}

// ---------------------------------------------------------------- inputs

#[derive(Clone)]
struct Fixture {
    name: String,
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}
const ORACLE_FIXTURES: [&str; 5] = [
    "impulse_64",
    "partial_alpha_64",
    "alpha_edge_64",
    "plasma_512x384",
    "plasma_alpha_512x384",
];
fn oracle_fixture(name: &str) -> Fixture {
    let png = op_formats::png::decode_png(
        &std::fs::read(root().join(format!("tests/fixtures/{name}.png"))).unwrap(),
    )
    .unwrap();
    Fixture {
        name: name.into(),
        width: png.width,
        height: png.height,
        pixels: png.pixels,
    }
}
/// Gradients plus noise with clear, opaque and partial alpha bands.
fn synthetic(width: u32, height: u32) -> Vec<u8> {
    let mut seed = 0x2468_ace1u32;
    let mut out = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            let noise = (seed >> 24) as u8;
            let alpha = match (x / 37 + y / 23) % 5 {
                0 => 0,
                1 => 255,
                _ => noise | 0x40,
            };
            out.extend_from_slice(&[
                (x * 255 / width) as u8 ^ (noise >> 3),
                (y * 255 / height) as u8,
                noise,
                alpha,
            ]);
        }
    }
    out
}
fn multi_tile(width: u32, height: u32) -> Fixture {
    Fixture {
        name: format!("multi_tile_{width}x{height}"),
        width,
        height,
        pixels: synthetic(width, height),
    }
}
fn all_fixtures() -> Vec<Fixture> {
    let mut out: Vec<_> = ORACLE_FIXTURES.iter().map(|n| oracle_fixture(n)).collect();
    out.push(multi_tile(600, 400));
    out
}

fn doc_from(f: &Fixture) -> DocState {
    let store = Arc::new(TileStore::new(std::env::temp_dir(), 512 * 1024 * 1024).unwrap());
    let mut doc = DocState::new(store, f.width, f.height, 96.0).unwrap();
    for coord in effect::tile_coords(Rect::new(0, 0, f.width, f.height)) {
        let mut tile = vec![0; TILE_BYTES];
        for py in 0..TILE_SIZE {
            let y = coord.y as usize * TILE_SIZE + py;
            if y >= f.height as usize {
                break;
            }
            let x0 = coord.x as usize * TILE_SIZE;
            let n = (f.width as usize - x0).min(TILE_SIZE);
            let src = (y * f.width as usize + x0) * 4;
            tile[py * TILE_SIZE * 4..][..n * 4].copy_from_slice(&f.pixels[src..src + n * 4]);
        }
        doc = doc
            .write_tile(0, coord, TileData::try_from(tile).unwrap())
            .unwrap();
    }
    doc
}
fn doc_pixels(doc: &DocState) -> Vec<u8> {
    let (w, h) = (doc.width as usize, doc.height as usize);
    let mut out = vec![0; w * h * 4];
    for ty in 0..h.div_ceil(TILE_SIZE) {
        for tx in 0..w.div_ceil(TILE_SIZE) {
            let coord = TileCoord {
                x: tx as i32,
                y: ty as i32,
            };
            let tile = doc.read_tile(doc.active_layer, coord).unwrap();
            let n = (w - tx * TILE_SIZE).min(TILE_SIZE) * 4;
            for y in 0..(h - ty * TILE_SIZE).min(TILE_SIZE) {
                let dst = ((ty * TILE_SIZE + y) * w + tx * TILE_SIZE) * 4;
                out[dst..dst + n].copy_from_slice(&tile[y * TILE_SIZE * 4..][..n]);
            }
        }
    }
    out
}
/// The CPU path: op-ops tiles, rendered in parallel.
fn cpu_render(spec: &EffectSpec, f: &Fixture, params: &Value) -> Result<Vec<u8>, String> {
    let doc = doc_from(f);
    let rect = Rect::new(0, 0, f.width, f.height);
    let out = effect::render_tiles(
        &doc,
        spec,
        rect,
        &effect::tile_coords(rect),
        true,
        Some(params.clone()),
    )
    .map_err(|e| e.to_string())?;
    Ok(doc_pixels(&out))
}

/// Max channel error, channels that differ, channels beyond `tol`.
fn compare(a: &[u8], b: &[u8], tol: u8) -> (u8, usize, usize) {
    assert_eq!(a.len(), b.len());
    let mut worst = 0;
    let mut differ = 0;
    let mut over = 0;
    for (x, y) in a.iter().zip(b) {
        let d = x.abs_diff(*y);
        worst = worst.max(d);
        differ += usize::from(d != 0);
        over += usize::from(d > tol);
    }
    (worst, differ, over)
}

// ---------------------------------------------------------------- targets

struct Target {
    target: String,
    tolerance_text: String,
    /// Allowed channel error, when the row states one in /255 steps.
    tolerance: Option<u8>,
    /// File-name prefix of the legacy captures, when the row has one.
    capture_prefix: Option<String>,
}
fn targets() -> BTreeMap<String, Target> {
    let text = std::fs::read_to_string(root().join("tests/oracle/targets.toml")).unwrap();
    let mut out = BTreeMap::new();
    for block in text.split("[[effect]]").skip(1) {
        let block = block.split("[[").next().unwrap();
        let field = |key: &str| {
            block
                .lines()
                .find_map(|line| line.trim().strip_prefix(&format!("{key} = \"")))
                .and_then(|tail| tail.rsplit_once('"').map(|(v, _)| v.to_owned()))
                .unwrap_or_default()
        };
        let tolerance_text = field("tolerance");
        let tolerance = if tolerance_text == "exact" {
            Some(0)
        } else if tolerance_text.contains("1/255") {
            Some(1)
        } else {
            None
        };
        let reference = field("reference");
        let capture_prefix = reference
            .strip_prefix("tests/oracle/legacy336/effects/")
            .and_then(|r| r.strip_suffix("*.png"))
            .map(str::to_owned);
        out.insert(
            field("id"),
            Target {
                target: field("target"),
                tolerance_text,
                tolerance,
                capture_prefix,
            },
        );
    }
    out
}

/// One legacy capture: input fixture, output path, translated parameters.
struct Capture {
    fixture: String,
    output: PathBuf,
    params: Value,
    status: String,
}
fn manifest() -> Vec<Value> {
    serde_json::from_slice::<Value>(
        &std::fs::read(root().join("tests/oracle/legacy336/effects/manifest.json")).unwrap(),
    )
    .unwrap()
    .as_array()
    .unwrap()
    .clone()
}
/// Maps legacy 3.36 parameter names onto the registered schema, as the
/// per-milestone oracle tests do.
fn translate(id: &str, legacy: &Value) -> Value {
    let mut out = serde_json::Map::new();
    for (key, value) in legacy.as_object().unwrap() {
        let (key, value) = match (id, key.as_str()) {
            ("effect.adjust.brightness_contrast", "brightness") => ("Brightness", value.clone()),
            ("effect.adjust.brightness_contrast", "contrast") => ("Contrast", value.clone()),
            ("effect.adjust.posterize", "red") => ("RedLevels", value.clone()),
            ("effect.adjust.posterize", "green") => ("GreenLevels", value.clone()),
            ("effect.adjust.posterize", "blue") => ("BlueLevels", value.clone()),
            ("effect.adjust.hue_saturation", "hue") => ("Hue", value.clone()),
            ("effect.adjust.hue_saturation", "saturation") => ("Saturation", value.clone()),
            ("effect.adjust.hue_saturation", "lightness") => ("Lightness", value.clone()),
            ("effect.adjust.levels", "gamma") => ("Gamma", json!([value, value, value])),
            ("effect.adjust.curves", "mode") => ("ColorTransferMode", value.clone()),
            ("effect.adjust.curves", "points") => ("ControlPoints", value.clone()),
            ("effect.render.clouds", "power") => ("roughness", value.clone()),
            _ => (key.as_str(), value.clone()),
        };
        out.insert(key.into(), value);
    }
    if id == "effect.adjust.posterize" {
        out.insert("LinkLevels".into(), json!(false));
    }
    Value::Object(out)
}
fn captures(spec: &EffectSpec, target: &Target, manifest: &[Value]) -> Vec<Capture> {
    let Some(prefix) = &target.capture_prefix else {
        return Vec::new();
    };
    manifest
        .iter()
        .filter(|e| {
            e["status"] != "not-run"
                && e["output"]
                    .as_str()
                    .and_then(|o| o.rsplit('/').next())
                    .is_some_and(|file| file.starts_with(prefix.as_str()))
        })
        .filter_map(|e| {
            let output = root().join(e["output"].as_str()?);
            let fixture = e["input"]
                .as_str()?
                .rsplit('/')
                .next()?
                .strip_suffix(".png")?
                .to_owned();
            let params = spec
                .schema
                .validate(translate(spec.id, &e["parameters"]))
                .unwrap_or_else(|err| panic!("{} legacy {}: {err}", spec.id, e["parameters"]));
            output.exists().then(|| Capture {
                fixture,
                output,
                params,
                status: e["status"].as_str().unwrap_or_default().to_owned(),
            })
        })
        .collect()
}

// ---------------------------------------------------------------- parameter sets

struct Case {
    label: String,
    params: Value,
}
/// Window sizes above this run only on the 64 × 64 inputs in the matrix: a
/// radius-200 Bokeh on 600 × 400 needs minutes of CPU per cell.
const LARGE_WINDOW: i64 = 50;
fn large_window(spec: &EffectSpec, case: &Case) -> bool {
    let keys: &[&str] = if spec.id == "effect.photo.sharpen" {
        &["amount"]
    } else {
        &[
            "radius",
            "softness",
            "brush_size",
            "thickness",
            "pencil_tip_size",
        ]
    };
    case.label != "default"
        && !case.label.starts_with("legacy")
        && keys
            .iter()
            .any(|k| case.params[*k].as_i64().is_some_and(|v| v > LARGE_WINDOW))
}
fn short(v: &Value) -> String {
    match v {
        Value::Number(n) => n.to_string(),
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}
/// Default, each parameter at min and max, values one step inside each end
/// (clamp and division edges: posterize 2 levels, contrast ±100, radius 0/1),
/// the middle and a half step (rounding), every choice and flag, and every
/// legacy capture's parameters.
fn cases(spec: &EffectSpec, captures: &[Capture]) -> Vec<Case> {
    let defaults = spec.default_params();
    let mut out = vec![Case {
        label: "default".into(),
        params: defaults.clone(),
    }];
    let add = |out: &mut Vec<Case>, label: String, params: Value| {
        if let Ok(valid) = spec.schema.validate(params)
            && !out.iter().any(|c| c.params == valid)
        {
            out.push(Case {
                label,
                params: valid,
            });
        }
    };
    for c in captures {
        let changed: Vec<_> = c
            .params
            .as_object()
            .unwrap()
            .iter()
            .filter(|(k, v)| defaults[k.as_str()] != **v)
            .map(|(k, v)| format!("{k}={}", short(v)))
            .collect();
        if !changed.is_empty() {
            add(
                &mut out,
                format!("legacy {}", changed.join(",")),
                c.params.clone(),
            );
        }
    }
    for field in &spec.schema.fields {
        let set = |out: &mut Vec<Case>, name: &str, value: Value| {
            let mut params = defaults.clone();
            params[field.key] = value.clone();
            add(
                out,
                format!("{}={} ({name})", field.key, short(&value)),
                params,
            );
        };
        match &field.kind {
            ParamKind::Int => {
                if let (Some(min), Some(max)) = (field.min, field.max) {
                    let (min, max) = (min as i64, max as i64);
                    set(&mut out, "min", json!(min));
                    set(&mut out, "min+1", json!(min + 1));
                    set(&mut out, "mid", json!((min + max) / 2));
                    set(&mut out, "max-1", json!(max - 1));
                    set(&mut out, "max", json!(max));
                }
            }
            ParamKind::Float => {
                if let (Some(min), Some(max)) = (field.min, field.max) {
                    set(&mut out, "min", json!(min));
                    set(&mut out, "max", json!(max));
                    let mid = ((min + max) / 2.0).floor();
                    set(&mut out, "mid", json!(mid));
                    set(&mut out, "half step", json!(mid + 0.5));
                }
            }
            ParamKind::Bool => {
                let flipped = !defaults[field.key].as_bool().unwrap_or(false);
                set(&mut out, "flag", json!(flipped));
            }
            ParamKind::Choice(options) => {
                for option in *options {
                    set(&mut out, "choice", json!(option));
                }
            }
            ParamKind::RgbGamma => {
                set(&mut out, "min", json!([0.1, 0.1, 0.1]));
                set(&mut out, "max", json!([10.0, 10.0, 10.0]));
            }
            ParamKind::String | ParamKind::Curve | ParamKind::RgbBytes => {}
        }
    }
    out
}
/// The working space of a case. Gaussian is the one effect with more than one
/// (its gamma choice and linear-light flag); every other effect declares one.
fn space(spec: &EffectSpec, params: &Value) -> String {
    if spec.id == "effect.blur.gaussian" {
        format!(
            "gamma {}, linear light {}",
            short(&params["gamma"]),
            params["linear_light"]
        )
    } else {
        format!("{:?} (only)", spec.contract.working_space)
    }
}

// ---------------------------------------------------------------- engine helpers

fn options(force_cpu: bool, chunk: Option<u32>) -> EngineOptions {
    EngineOptions {
        force_cpu,
        max_gpu_chunk: chunk,
        ..EngineOptions::default()
    }
}
fn info(e: &Engine, id: u64) -> Value {
    serde_json::from_slice(&e.query(id, "doc.info", json!({})).unwrap()).unwrap()
}
fn gpu_runs(e: &Engine, id: u64) -> u64 {
    info(e, id)["gpu_effects"]["gpu_runs"].as_u64().unwrap()
}
fn engine_pixels(e: &Engine, id: u64) -> Vec<u8> {
    doc_pixels(&e.actor_probe(id).unwrap().0)
}
/// One engine run. Returns the bytes, the time, and the CPU reason when the
/// engine fell back. The document is restored with undo.
fn engine_run(
    e: &Engine,
    doc: u64,
    id: &str,
    params: &Value,
) -> Result<(Vec<u8>, f64, Option<String>), String> {
    let before = gpu_runs(e, doc);
    let start = Instant::now();
    e.dispatch(doc, id, params.clone())
        .map_err(|err| err.to_string())?;
    let ms = start.elapsed().as_secs_f64() * 1000.0;
    let gi = info(e, doc);
    let fell_back = (gi["gpu_effects"]["gpu_runs"].as_u64().unwrap() == before).then(|| {
        gi["gpu_effects"]["last_cpu_reason"]
            .as_str()
            .or(gi["gpu_reason"].as_str())
            .unwrap_or("no reason recorded")
            .to_owned()
    });
    let pixels = engine_pixels(e, doc);
    e.dispatch(doc, "edit.undo", json!({}))
        .map_err(|err| err.to_string())?;
    Ok((pixels, ms, fell_back))
}

// ---------------------------------------------------------------- fast checks

#[test]
fn m54_cpu_only_list_matches_registry_and_gpu_effects_run_on_gpu_at_defaults() {
    let registry = effect::registered();
    let without: Vec<_> = registry
        .iter()
        .filter(|s| s.gpu.is_none())
        .map(|s| s.id)
        .collect();
    let mut listed = CPU_ONLY.to_vec();
    listed.sort_unstable();
    let mut found = without.clone();
    found.sort_unstable();
    assert_eq!(
        found, listed,
        "CPU_ONLY must list exactly the effects with no kernel"
    );
    let targets = targets();
    for spec in &registry {
        assert!(
            targets.contains_key(spec.id),
            "{} has no target row",
            spec.id
        );
    }
    if !gpu_tests_enabled() {
        return;
    }
    let f = multi_tile(280, 270);
    let gpu = Engine::with_options(None, options(false, Some(192))).unwrap();
    let g = gpu
        .import_pixels(f.width, f.height, &f.pixels, None)
        .unwrap();
    let mut failures = Vec::new();
    for spec in &registry {
        let Some(kernel) = spec.gpu else { continue };
        let params = spec.default_params();
        let (bytes, _, fell_back) = engine_run(&gpu, g, spec.id, &params).unwrap();
        if let Some(reason) = fell_back {
            failures.push(format!("{} fell back at defaults: {reason}", spec.id));
            continue;
        }
        let cpu = cpu_render(spec, &f, &params).unwrap();
        let (worst, _, over) = compare(&cpu, &bytes, kernel_tolerance(kernel));
        if over > 0 {
            failures.push(format!("{} CPU-GPU worst {worst}", spec.id));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Whole-image render: the effect's per-pixel function over one buffer with
/// the effect's border rule, no tiles.
fn whole_image(spec: &EffectSpec, f: &Fixture, params: &Value) -> Vec<u8> {
    let doc = doc_from(f);
    let params = spec.schema.validate(params.clone()).unwrap();
    let prepared = (spec.prepare)(&doc, &doc.selection, &params).unwrap();
    let (w, h) = (i64::from(f.width), i64::from(f.height));
    let border = |v: i32, len: i64| -> Option<i64> {
        let v = i64::from(v);
        match spec.contract.border {
            BorderRule::Transparent => (0..len).contains(&v).then_some(v),
            BorderRule::Clamp => Some(v.clamp(0, len - 1)),
            BorderRule::Wrap => Some(v.rem_euclid(len)),
            BorderRule::Mirror if len == 1 => Some(0),
            BorderRule::Mirror => {
                let p = v.rem_euclid(2 * (len - 1));
                Some(if p < len { p } else { 2 * (len - 1) - p })
            }
        }
    };
    let sample = |x: i32, y: i32| -> [u8; 4] {
        match (border(x, w), border(y, h)) {
            (Some(x), Some(y)) => {
                let i = ((y * w + x) * 4) as usize;
                f.pixels[i..i + 4].try_into().unwrap()
            }
            _ => [0; 4],
        }
    };
    let mut out = Vec::with_capacity(f.pixels.len());
    for y in 0..h as i32 {
        for x in 0..w as i32 {
            out.extend((spec.render_cpu)(&sample, x, y, &prepared, &params));
        }
    }
    out
}

#[test]
fn m54_tiled_render_equals_whole_image_for_every_effect() {
    // 2 x 2 tiles with partial right and bottom tiles.
    let f = multi_tile(280, 270);
    let mut checked = 0;
    for spec in effect::registered() {
        if spec.id == "effect.fill.content_aware" {
            // Needs a selected hole; content_aware.rs checks its tiles.
            continue;
        }
        let params = spec.default_params();
        let tiled = cpu_render(&spec, &f, &params).unwrap();
        let whole = whole_image(&spec, &f, &params);
        let (worst, differ, _) = compare(&whole, &tiled, 0);
        assert_eq!(
            differ, 0,
            "{}: tiles differ from whole image, worst {worst}",
            spec.id
        );
        checked += 1;
    }
    assert_eq!(checked, 54);
}

#[test]
fn m54_legacy_rows_hold_on_small_fixtures_through_the_tiled_path() {
    let targets = targets();
    let manifest = manifest();
    let small: HashSet<_> = ["impulse_64", "partial_alpha_64", "alpha_edge_64"]
        .into_iter()
        .collect();
    let fixtures: BTreeMap<_, _> = small
        .iter()
        .map(|n| (n.to_string(), oracle_fixture(n)))
        .collect();
    let mut failures = Vec::new();
    let mut checked = 0;
    for spec in effect::registered() {
        let target = &targets[spec.id];
        if target.target != "legacy-3.36" {
            continue;
        }
        let tol = target.tolerance.expect("legacy rows state a tolerance");
        for c in captures(&spec, target, &manifest) {
            let Some(f) = fixtures.get(&c.fixture) else {
                continue;
            };
            let expected = op_formats::png::decode_png(&std::fs::read(&c.output).unwrap())
                .unwrap()
                .pixels;
            let actual = cpu_render(&spec, f, &c.params).unwrap();
            let (worst, _, over) = compare(&expected, &actual, tol);
            if over > 0 {
                failures.push(format!(
                    "{} {}: {over} channels over {tol}, worst {worst}",
                    spec.id,
                    c.output.display()
                ));
            }
            checked += 1;
        }
    }
    println!("M54 legacy small-fixture captures checked: {checked}");
    assert!(checked >= 300, "only {checked} captures");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

fn engine_with_events(o: EngineOptions) -> (Engine, Receiver<Event>) {
    let (tx, rx) = channel();
    let tx = Mutex::new(tx);
    let e = Engine::with_options(
        Some(Arc::new(move |ev| {
            let _ = tx.lock().unwrap().send(ev);
        })),
        o,
    )
    .unwrap();
    (e, rx)
}
fn wait_job(rx: &Receiver<Event>, ticket: u64) -> Event {
    let end = Instant::now() + Duration::from_secs(120);
    loop {
        let left = end.saturating_duration_since(Instant::now());
        let ev = rx.recv_timeout(left).expect("JobFinished");
        if matches!(ev.kind, EventKind::JobFinished) && ev.ticket == ticket {
            return ev;
        }
    }
}

#[test]
fn m54_preview_equals_commit_for_every_effect() {
    let f = multi_tile(280, 270);
    let mut engines = vec![("cpu", engine_with_events(options(true, None)))];
    if gpu_tests_enabled() {
        engines.push(("gpu", engine_with_events(options(false, Some(192)))));
    }
    let mut failures = Vec::new();
    let mut ticket = 100;
    for (name, (e, rx)) in &engines {
        for spec in effect::registered() {
            let doc = e.import_pixels(f.width, f.height, &f.pixels, None).unwrap();
            let again = e.import_pixels(f.width, f.height, &f.pixels, None).unwrap();
            if spec.id == "effect.fill.content_aware" {
                for d in [doc, again] {
                    e.dispatch(d, "test.set_selection_shape", json!({"shape":"ellipse"}))
                        .unwrap();
                }
            }
            let params = spec.default_params();
            ticket += 1;
            e.submit(
                doc,
                Op::PreviewBegin(spec.id.into(), params.clone()),
                JobTicket::new(ticket),
            )
            .unwrap();
            let done = wait_job(rx, ticket);
            assert_eq!(
                done.json["status"], "ok",
                "{name} {}: {}",
                spec.id, done.json
            );
            let snap = e.snapshot(doc).unwrap();
            assert!(snap.preview.is_some(), "{name} {}: no preview", spec.id);
            let preview = snap.pixels(true).unwrap();
            e.preview_commit(doc).unwrap();
            let committed = e.snapshot(doc).unwrap().pixels(false).unwrap();
            if preview != committed {
                let (worst, differ, _) = compare(&preview, &committed, 0);
                failures.push(format!(
                    "{name} {}: preview vs commit {differ} channels, worst {worst}",
                    spec.id
                ));
            }
            // The same command dispatched directly gives the same bytes again.
            e.dispatch(again, spec.id, params).unwrap();
            if e.snapshot(again).unwrap().pixels(false).unwrap() != committed {
                failures.push(format!("{name} {}: second run differs", spec.id));
            }
            e.close(doc).unwrap();
            e.close(again).unwrap();
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Neighborhood and pointwise kernels with ±1 tolerances, in a fixed order, on
/// a multi-tile image split into several GPU chunks. Effects that rank or
/// threshold local values (Reduce Noise, Median, Surface) are left out: they
/// turn a 1/255 input difference into a much larger one (see RANKING_CHAIN).
const MIXED_CHAIN: [(&str, &str); 10] = [
    ("effect.blur.gaussian", r#"{"radius":4}"#),
    (
        "effect.adjust.brightness_contrast",
        r#"{"Brightness":12,"Contrast":25}"#,
    ),
    ("effect.blur.motion", "{}"),
    ("effect.adjust.sepia", "{}"),
    ("effect.photo.sharpen", "{}"),
    ("effect.stylize.emboss", "{}"),
    ("effect.blur.unfocus", "{}"),
    ("effect.photo.vignette", "{}"),
    ("effect.blur.square", "{}"),
    ("effect.blur.bokeh", "{}"),
];
/// MIXED_CHAIN ending in Glow. Reported in the matrix, not held to 2/255: on
/// equal input Glow matches the CPU exactly, but its contrast gain magnifies
/// the earlier ±1 differences.
const GAIN_CHAIN: [(&str, &str); 10] = [
    MIXED_CHAIN[0],
    MIXED_CHAIN[1],
    MIXED_CHAIN[2],
    MIXED_CHAIN[3],
    MIXED_CHAIN[4],
    MIXED_CHAIN[5],
    MIXED_CHAIN[6],
    MIXED_CHAIN[7],
    MIXED_CHAIN[8],
    ("effect.photo.glow", "{}"),
];
/// GAIN_CHAIN with Reduce Noise in place of Square Blur. Reported only: on
/// equal input Reduce Noise matches the CPU exactly, but its local ranking
/// magnifies the earlier ±1 differences.
const RANKING_CHAIN: [(&str, &str); 10] = [
    MIXED_CHAIN[0],
    MIXED_CHAIN[1],
    MIXED_CHAIN[2],
    MIXED_CHAIN[3],
    MIXED_CHAIN[4],
    MIXED_CHAIN[5],
    MIXED_CHAIN[6],
    MIXED_CHAIN[7],
    ("effect.noise.reduce", "{}"),
    ("effect.photo.glow", "{}"),
];
/// Runs a chain on a CPU and a GPU engine; returns the worst error after each step.
fn run_chain(chain: &[(&str, &str)], f: &Fixture) -> Vec<(String, u8, usize)> {
    let gpu = Engine::with_options(None, options(false, Some(192))).unwrap();
    let cpu = Engine::with_options(None, options(true, None)).unwrap();
    let g = gpu
        .import_pixels(f.width, f.height, &f.pixels, None)
        .unwrap();
    let c = cpu
        .import_pixels(f.width, f.height, &f.pixels, None)
        .unwrap();
    let mut out = Vec::new();
    for (id, params) in chain {
        let params: Value = serde_json::from_str(params).unwrap();
        let before = gpu_runs(&gpu, g);
        gpu.dispatch(g, id, params.clone()).unwrap();
        cpu.dispatch(c, id, params).unwrap();
        assert_eq!(
            gpu_runs(&gpu, g),
            before + 1,
            "{id} fell back: {}",
            info(&gpu, g)
        );
        let (worst, differ, _) = compare(&engine_pixels(&cpu, c), &engine_pixels(&gpu, g), 0);
        out.push((id.to_string(), worst, differ));
    }
    out
}

#[test]
fn m54_mixed_ten_effect_chain_stays_within_two_channels() {
    if !gpu_tests_enabled() {
        return;
    }
    let steps = run_chain(&MIXED_CHAIN, &multi_tile(600, 400));
    for (id, worst, _) in &steps {
        println!("M54 mixed chain after {id}: max channel error {worst}/255");
    }
    let (_, worst, differ) = steps.last().unwrap();
    println!("M54 mixed 10-effect chain 600x400: max {worst}/255, {differ} channels differ");
    assert!(*worst <= 2, "mixed chain error {worst}/255");
}

#[test]
fn m54_matrix_covers_every_effect_backend_and_fixture() {
    let matrix = std::fs::read_to_string(root().join("docs/reports/parity-matrix.md"))
        .expect("run the ignored m54_write_parity_matrix test first");
    for spec in effect::registered() {
        for fixture in ORACLE_FIXTURES
            .iter()
            .map(|s| s.to_string())
            .chain(["multi_tile_600x400".to_string()])
        {
            for backend in ["CPU", "engine GPU", "op-gpu"] {
                let row = format!("| {} | {backend} | {fixture} | default |", spec.id);
                assert!(matrix.contains(&row), "matrix lacks {row}");
            }
        }
        if spec.gpu.is_none() {
            assert!(
                matrix.contains(&format!(
                    "| {} | engine GPU | impulse_64 | default |",
                    spec.id
                )) && matrix.contains(&cpu_only_reason(&spec)),
                "{} needs its CPU-only reason in the matrix",
                spec.id
            );
        }
    }
}

// ---------------------------------------------------------------- long runs

struct Row {
    effect: String,
    backend: &'static str,
    fixture: String,
    case: String,
    space: String,
    reference: String,
    ref_max: String,
    ref_mismatch: String,
    cpu_gpu: String,
    ms: String,
    peak: String,
    pipeline: &'static str,
    result: String,
}
impl Row {
    fn line(&self) -> String {
        format!(
            "| {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} |\n",
            self.effect,
            self.backend,
            self.fixture,
            self.case,
            self.space,
            self.reference,
            self.ref_max,
            self.ref_mismatch,
            self.cpu_gpu,
            self.ms,
            self.peak,
            self.pipeline,
            self.result
        )
    }
}
#[derive(Default)]
struct Summary {
    cells: usize,
    ref_checked: usize,
    ref_worst_cpu: u8,
    ref_worst_gpu: Option<u8>,
    cpu_gpu_worst: Option<u8>,
    gpu_runs: usize,
    fallbacks: BTreeMap<String, usize>,
    failures: usize,
    skipped: usize,
}

fn machine() -> String {
    let run = |cmd: &str, args: &[&str]| {
        std::process::Command::new(cmd)
            .args(args)
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
            .unwrap_or_else(|e| format!("unavailable ({e})"))
    };
    format!(
        "{}, {} cores, {} GB RAM, macOS {}",
        run("sysctl", &["-n", "machdep.cpu.brand_string"]),
        run("sysctl", &["-n", "hw.ncpu"]),
        run("sysctl", &["-n", "hw.memsize"])
            .parse::<u64>()
            .map(|b| (b >> 30).to_string())
            .unwrap_or_default(),
        run("sw_vers", &["-productVersion"]),
    )
}
fn load() -> String {
    std::process::Command::new("sysctl")
        .args(["-n", "vm.loadavg"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        .unwrap_or_else(|e| format!("unavailable ({e})"))
}

/// Writes docs/reports/parity-matrix.md, then fails if any cell broke its target
/// row, its kernel tolerance, or fell back at defaults.
/// `OP_M54_ONLY=id1,id2` limits the run to some effects (the report then says so).
#[test]
#[ignore = "long M54 sweep; run in release"]
fn m54_write_parity_matrix() {
    assert!(gpu_tests_enabled(), "the matrix needs a GPU adapter");
    let only: Option<HashSet<String>> = std::env::var("OP_M54_ONLY")
        .ok()
        .map(|v| v.split(',').map(str::to_owned).collect());
    let started = Instant::now();
    let load_before = load();
    let targets = targets();
    let manifest = manifest();
    let fixtures = all_fixtures();
    let gpu = Engine::with_options(None, options(false, Some(192))).unwrap();
    let engine_docs: Vec<u64> = fixtures
        .iter()
        .map(|f| {
            gpu.import_pixels(f.width, f.height, &f.pixels, None)
                .unwrap()
        })
        .collect();
    let mut direct = op_gpu::GpuContext::new().expect("GPU adapter");
    let adapter = direct.info().clone();
    let mut warm: HashSet<(&'static str, String)> = HashSet::new();
    let mut rows = String::new();
    let mut summaries: BTreeMap<String, Summary> = BTreeMap::new();
    let mut failures = Vec::new();
    for spec in effect::registered() {
        if only.as_ref().is_some_and(|o| !o.contains(spec.id)) {
            continue;
        }
        let effect_start = Instant::now();
        let target = &targets[spec.id];
        let caps = captures(&spec, target, &manifest);
        let sum = summaries.entry(spec.id.to_owned()).or_default();
        let effect_cases = if spec.id == "effect.fill.content_aware" {
            vec![Case {
                label: "default".into(),
                params: spec.default_params(),
            }]
        } else {
            cases(&spec, &caps)
        };
        for case in &effect_cases {
            let space = space(&spec, &case.params);
            for (fi, f) in fixtures.iter().enumerate() {
                let capture = caps
                    .iter()
                    .find(|c| c.fixture == f.name && c.params == case.params);
                if capture.is_none() && f.width > 64 && large_window(&spec, case) {
                    for backend in ["CPU", "engine GPU"] {
                        rows.push_str(
                            &Row {
                                effect: spec.id.into(),
                                backend,
                                fixture: f.name.clone(),
                                case: case.label.clone(),
                                space: space.clone(),
                                reference: format!(
                                    "not run: window over {LARGE_WINDOW} px on a large input (time); the 64 × 64 inputs run this set"
                                ),
                                ref_max: "n/a".into(),
                                ref_mismatch: "n/a".into(),
                                cpu_gpu: "n/a".into(),
                                ms: "n/a".into(),
                                peak: "n/a".into(),
                                pipeline: "n/a",
                                result: "not run".into(),
                            }
                            .line(),
                        );
                        sum.cells += 1;
                        sum.skipped += 1;
                    }
                    continue;
                }
                let expected = capture.map(|c| {
                    op_formats::png::decode_png(&std::fs::read(&c.output).unwrap())
                        .unwrap()
                        .pixels
                });
                let no_reference = if target.target == "own" {
                    format!(
                        "own target ({}); CPU twin is the reference",
                        target.tolerance_text
                    )
                } else if target.capture_prefix.is_none() || caps.is_empty() {
                    "legacy capture not run (manifest status not-run)".to_owned()
                } else if f.name.starts_with("multi_tile") {
                    "no legacy capture for this synthetic input".to_owned()
                } else {
                    "no legacy capture at these parameters".to_owned()
                };
                let reference_of = |bytes: &[u8]| -> (String, String, String, Option<u8>) {
                    match (&expected, capture) {
                        (Some(exp), Some(c)) => {
                            let (worst, differ, _) = compare(exp, bytes, 0);
                            (
                                format!(
                                    "legacy-3.36 {} ({})",
                                    c.output.file_name().unwrap().to_string_lossy(),
                                    c.status
                                ),
                                worst.to_string(),
                                differ.to_string(),
                                Some(worst),
                            )
                        }
                        _ => (no_reference.clone(), "n/a".into(), "n/a".into(), None),
                    }
                };
                // CPU through op-ops.
                if spec.id == "effect.fill.content_aware" {
                    for backend in ["CPU", "engine GPU", "op-gpu"] {
                        rows.push_str(
                            &Row {
                                effect: spec.id.into(),
                                backend,
                                fixture: f.name.clone(),
                                case: case.label.clone(),
                                space: space.clone(),
                                reference: format!(
                                    "not run: needs a selected hole; content_aware.rs checks PSNR and SSIM; {}",
                                    if backend == "CPU" { "".into() } else { cpu_only_reason(&spec) }
                                ),
                                ref_max: "n/a".into(),
                                ref_mismatch: "n/a".into(),
                                cpu_gpu: "n/a".into(),
                                ms: "n/a".into(),
                                peak: "n/a".into(),
                                pipeline: "n/a",
                                result: "not run".into(),
                            }
                            .line(),
                        );
                        sum.cells += 1;
                    }
                    continue;
                }
                let base = peak_start();
                let start = Instant::now();
                let cpu = match cpu_render(&spec, f, &case.params) {
                    Ok(bytes) => bytes,
                    // A generated set the effect itself rejects (Curves in Rgb
                    // mode with one channel of points) is listed, not failed.
                    Err(err) if err.starts_with("invalid parameters") => {
                        rows.push_str(&format!(
                            "| {} | CPU | {} | {} | {space} | not run: the effect rejects this set ({err}) | n/a | n/a | n/a | n/a | n/a | n/a | rejected |\n",
                            spec.id, f.name, case.label
                        ));
                        sum.cells += 1;
                        continue;
                    }
                    Err(err) => {
                        failures.push(format!(
                            "{} {} {}: CPU error {err}",
                            spec.id, f.name, case.label
                        ));
                        continue;
                    }
                };
                let cpu_ms = start.elapsed().as_secs_f64() * 1000.0;
                let cpu_peak = peak_mb(base);
                let (reference, ref_max, ref_mismatch, worst) = reference_of(&cpu);
                let mut result = "ok".to_owned();
                if let Some(worst) = worst {
                    sum.ref_checked += 1;
                    sum.ref_worst_cpu = sum.ref_worst_cpu.max(worst);
                    let tol = target.tolerance.unwrap_or(0);
                    if worst > tol {
                        result = format!("FAIL: over row tolerance {tol}");
                        failures.push(format!(
                            "{} {} {}: CPU vs legacy worst {worst} > {tol}",
                            spec.id, f.name, case.label
                        ));
                        sum.failures += 1;
                    }
                }
                rows.push_str(
                    &Row {
                        effect: spec.id.into(),
                        backend: "CPU",
                        fixture: f.name.clone(),
                        case: case.label.clone(),
                        space: space.clone(),
                        reference,
                        ref_max,
                        ref_mismatch,
                        cpu_gpu: "n/a".into(),
                        ms: format!("{cpu_ms:.2}"),
                        peak: format!("{cpu_peak:.1}"),
                        pipeline: "n/a",
                        result,
                    }
                    .line(),
                );
                sum.cells += 1;
                let Some(kernel) = spec.gpu else {
                    for backend in ["engine GPU", "op-gpu"] {
                        rows.push_str(
                            &Row {
                                effect: spec.id.into(),
                                backend,
                                fixture: f.name.clone(),
                                case: case.label.clone(),
                                space: space.clone(),
                                reference: cpu_only_reason(&spec),
                                ref_max: "n/a".into(),
                                ref_mismatch: "n/a".into(),
                                cpu_gpu: "n/a".into(),
                                ms: "n/a".into(),
                                peak: "n/a".into(),
                                pipeline: "n/a",
                                result: "CPU only".into(),
                            }
                            .line(),
                        );
                        sum.cells += 1;
                    }
                    continue;
                };
                let tol = kernel_tolerance(kernel);
                // GPU through the engine.
                let pipeline = if warm.insert(("engine", format!("{kernel:?}"))) {
                    "cold"
                } else {
                    "warm"
                };
                let base = peak_start();
                match engine_run(&gpu, engine_docs[fi], spec.id, &case.params) {
                    Ok((bytes, ms, None)) => {
                        let mb = peak_mb(base);
                        let (cg, _, over) = compare(&cpu, &bytes, tol);
                        let (reference, ref_max, ref_mismatch, rw) = reference_of(&bytes);
                        sum.gpu_runs += 1;
                        sum.cpu_gpu_worst = Some(sum.cpu_gpu_worst.unwrap_or(0).max(cg));
                        if let Some(rw) = rw {
                            sum.ref_worst_gpu = Some(sum.ref_worst_gpu.unwrap_or(0).max(rw));
                        }
                        let result = if over > 0 {
                            failures.push(format!(
                                "{} {} {}: engine GPU vs CPU worst {cg} > {tol} ({over} channels)",
                                spec.id, f.name, case.label
                            ));
                            sum.failures += 1;
                            format!("FAIL: over kernel tolerance {tol}")
                        } else {
                            "ok".to_owned()
                        };
                        rows.push_str(
                            &Row {
                                effect: spec.id.into(),
                                backend: "engine GPU",
                                fixture: f.name.clone(),
                                case: case.label.clone(),
                                space: space.clone(),
                                reference,
                                ref_max,
                                ref_mismatch,
                                cpu_gpu: format!("{cg} (tol {tol})"),
                                ms: format!("{ms:.2}"),
                                peak: format!("{mb:.1}"),
                                pipeline,
                                result,
                            }
                            .line(),
                        );
                    }
                    Ok((bytes, ms, Some(reason))) => {
                        let same = bytes == cpu;
                        *sum.fallbacks.entry(reason.clone()).or_default() += 1;
                        let result = if case.label == "default" {
                            failures.push(format!(
                                "{} {}: fell back at defaults: {reason}",
                                spec.id, f.name
                            ));
                            sum.failures += 1;
                            "FAIL: fell back at defaults".to_owned()
                        } else if !same {
                            failures.push(format!(
                                "{} {} {}: CPU fallback bytes differ from op-ops",
                                spec.id, f.name, case.label
                            ));
                            sum.failures += 1;
                            "FAIL: fallback differs from CPU".to_owned()
                        } else {
                            "listed fallback".to_owned()
                        };
                        rows.push_str(
                            &Row {
                                effect: spec.id.into(),
                                backend: "engine GPU",
                                fixture: f.name.clone(),
                                case: case.label.clone(),
                                space: space.clone(),
                                reference: format!("engine used the CPU: {reason}"),
                                ref_max: "n/a".into(),
                                ref_mismatch: "n/a".into(),
                                cpu_gpu: if same {
                                    "0 (CPU twin)".into()
                                } else {
                                    "differs".into()
                                },
                                ms: format!("{ms:.2}"),
                                peak: "n/a".into(),
                                pipeline: "n/a",
                                result,
                            }
                            .line(),
                        );
                    }
                    Err(err) => {
                        failures.push(format!(
                            "{} {} {}: engine error {err}",
                            spec.id, f.name, case.label
                        ));
                        sum.failures += 1;
                    }
                }
                sum.cells += 1;
                // GPU through op-gpu's default entry point.
                if case.label == "default" {
                    let pipeline = if warm.insert(("op-gpu", format!("{kernel:?}"))) {
                        "cold"
                    } else {
                        "warm"
                    };
                    let base = peak_start();
                    let before = direct.dispatch_count();
                    let start = Instant::now();
                    let out = direct.effect_kernel(kernel, f.width, f.height, &f.pixels);
                    let ms = start.elapsed().as_secs_f64() * 1000.0;
                    let mb = peak_mb(base);
                    let row = match out {
                        Ok(bytes) if direct.dispatch_count() > before => {
                            let (cg, _, over) = compare(&cpu, &bytes, tol);
                            let (reference, ref_max, ref_mismatch, _) = reference_of(&bytes);
                            let result = if over > 0 {
                                failures.push(format!(
                                    "{} {}: op-gpu vs CPU worst {cg} > {tol}",
                                    spec.id, f.name
                                ));
                                sum.failures += 1;
                                format!("FAIL: over kernel tolerance {tol}")
                            } else {
                                "ok".to_owned()
                            };
                            Row {
                                effect: spec.id.into(),
                                backend: "op-gpu",
                                fixture: f.name.clone(),
                                case: case.label.clone(),
                                space: space.clone(),
                                reference,
                                ref_max,
                                ref_mismatch,
                                cpu_gpu: format!("{cg} (tol {tol})"),
                                ms: format!("{ms:.2}"),
                                peak: format!("{mb:.1}"),
                                pipeline,
                                result,
                            }
                        }
                        other => {
                            failures.push(format!(
                                "{} {}: op-gpu did not run: {:?}",
                                spec.id,
                                f.name,
                                other.err()
                            ));
                            sum.failures += 1;
                            continue;
                        }
                    };
                    rows.push_str(&row.line());
                    sum.cells += 1;
                }
            }
            if case.label != "default" && spec.gpu.is_some() {
                // One line per parameter set instead of one per fixture.
                rows.push_str(&format!(
                    "| {} | op-gpu | all | {} | {space} | not run: op-gpu's test entry point takes kernel defaults; the engine GPU rows run this set through the same op-gpu calls | n/a | n/a | n/a | n/a | n/a | n/a | not run |\n",
                    spec.id, case.label
                ));
            }
        }
        println!(
            "M54 matrix {}: {} cases, {:.1} s",
            spec.id,
            effect_cases.len(),
            effect_start.elapsed().as_secs_f64()
        );
    }
    let elapsed = started.elapsed().as_secs_f64();
    let mut report = String::new();
    writeln!(report, "# M54 parity matrix\n").unwrap();
    writeln!(
        report,
        "Written by `cargo test --release -p op-engine --test m54_parity m54_write_parity_matrix -- --ignored --test-threads=1` \
         (engine/op-engine/tests/m54_parity.rs). Do not edit by hand.\n"
    )
    .unwrap();
    if let Some(only) = &only {
        let mut only: Vec<_> = only.iter().cloned().collect();
        only.sort();
        writeln!(
            report,
            "**Partial run:** OP_M54_ONLY limited this run to {}.\n",
            only.join(", ")
        )
        .unwrap();
    }
    writeln!(
        report,
        "- Machine: {}. GPU adapter: {} ({:?}).\n- Load average before: {load_before}; after: {}. Run time {elapsed:.0} s.\n\
         - Fixtures: the five oracle inputs (tests/fixtures) and a synthetic 600 × 400 image (3 × 2 tiles, partial edge tiles).\n\
         - Backends: CPU is op-ops' parallel tile scheduler. Engine GPU is `Engine::dispatch` with the GPU on and 192 px chunks, read back from the layer tiles. op-gpu is `GpuContext::effect_kernel` on the whole image; that entry point takes kernel defaults, so it runs the default set only.\n\
         - Parameter sets: default; every legacy capture's parameters; each numeric parameter at min, min+1, mid, max-1 and max; floats at min, mid, mid+0.5 and max; each flag flipped; every choice. Sets with a window (radius, softness, brush, thickness, pencil tip, sharpen amount) over {LARGE_WINDOW} px that have no legacy capture run on the three 64 × 64 inputs only; their large-input cells say not run.\n\
         - Working space: every effect declares one (Encoded) except Gaussian, whose gamma choice and linear-light flag are listed in the Space column.\n\
         - Reference: legacy-3.36 rows compare against the capture PNG with the same input and parameters. Own rows have no external capture, so the CPU twin is the reference and the GPU rows report CPU-GPU error.\n\
         - Max reference error is the largest channel difference; mismatches count channels that differ at all. A CPU cell fails when its error is over the row tolerance. A GPU cell fails when its CPU-GPU error is over the kernel tolerance (tests/gpu_effects.rs). GPU rows also report their own error against the capture.\n\
         - Time is wall-clock milliseconds for one render including tile reads and writes (and, for the engine, GPU upload, dispatch and readback). Peak memory is the peak of new CPU heap bytes during the cell, in MB, from a counting allocator; GPU memory is not measured. Pipeline is cold for the first run of a kernel on that GPU context and warm after.\n",
        machine(),
        adapter.name,
        adapter.backend,
        load(),
    )
    .unwrap();
    writeln!(report, "## CPU-only effects\n").unwrap();
    for id in CPU_ONLY {
        let spec = effect::find(id).unwrap();
        writeln!(report, "- `{id}`: {}", cpu_only_reason(&spec)).unwrap();
    }
    writeln!(
        report,
        "\n## Long chains\n\nCPU engine vs GPU engine on the 600 × 400 image, 192 px GPU chunks. \
         Each step shows the worst channel error after that effect.\n"
    )
    .unwrap();
    for (name, chain) in [
        (
            "Mixed chain (held to 2/255 by m54_mixed_ten_effect_chain_stays_within_two_channels)",
            &MIXED_CHAIN,
        ),
        (
            "Gain chain (Glow in place of Bokeh; reported only)",
            &GAIN_CHAIN,
        ),
        (
            "Ranking chain (Reduce Noise and Glow in place of Square Blur and Bokeh; reported only)",
            &RANKING_CHAIN,
        ),
    ] {
        let steps = run_chain(chain, &multi_tile(600, 400));
        let (_, worst, differ) = steps.last().unwrap();
        writeln!(
            report,
            "- {name}: final {worst}/255, {differ} channels differ. Steps: {}.",
            steps
                .iter()
                .map(|(id, w, _)| format!("{id} {w}"))
                .collect::<Vec<_>>()
                .join(" → ")
        )
        .unwrap();
    }
    writeln!(report, "\n## Summary per effect\n").unwrap();
    writeln!(report, "| Effect | Target | Tolerance | Kernel | Cells | Legacy cells checked | Worst CPU vs legacy | Worst engine GPU vs legacy | Worst CPU-GPU | Engine GPU runs | Engine CPU fallbacks (reason: count) | Cells not run (large window) | Failures |").unwrap();
    writeln!(
        report,
        "| --- | --- | --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: | ---: |"
    )
    .unwrap();
    for spec in effect::registered() {
        let Some(s) = summaries.get(spec.id) else {
            continue;
        };
        let t = &targets[spec.id];
        let opt = |v: Option<u8>| v.map_or("n/a".to_owned(), |v| v.to_string());
        writeln!(
            report,
            "| {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} |",
            spec.id,
            t.target,
            t.tolerance_text,
            spec.gpu.map_or("none".to_owned(), |k| format!(
                "{k:?} (tol {})",
                kernel_tolerance(k)
            )),
            s.cells,
            s.ref_checked,
            if s.ref_checked > 0 {
                s.ref_worst_cpu.to_string()
            } else {
                "n/a".into()
            },
            opt(s.ref_worst_gpu),
            opt(s.cpu_gpu_worst),
            s.gpu_runs,
            if s.fallbacks.is_empty() {
                "none".to_owned()
            } else {
                s.fallbacks
                    .iter()
                    .map(|(r, n)| format!("{}: {n}", r.replace('|', "/")))
                    .collect::<Vec<_>>()
                    .join("; ")
            },
            s.skipped,
            s.failures
        )
        .unwrap();
    }
    writeln!(report, "\n## Failures\n").unwrap();
    if failures.is_empty() {
        writeln!(report, "None.").unwrap();
    } else {
        for f in &failures {
            writeln!(report, "- {}", f.replace('|', "/")).unwrap();
        }
    }
    writeln!(report, "\n## Cells\n").unwrap();
    writeln!(report, "| Effect | Backend | Fixture | Parameters | Space | Reference | Max reference error | Reference mismatches | CPU-GPU max | Time (ms) | Peak memory (MB) | Pipeline | Result |").unwrap();
    writeln!(
        report,
        "| --- | --- | --- | --- | --- | --- | ---: | ---: | --- | ---: | ---: | --- | --- |"
    )
    .unwrap();
    report.push_str(&rows);
    std::fs::write(root().join("docs/reports/parity-matrix.md"), report).unwrap();
    println!("M54 matrix: {} failures, {elapsed:.0} s", failures.len());
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Named parameter sets for the performance table. "default" for every effect,
/// plus a heavy set for the neighborhood effects whose cost grows with radius.
fn perf_sets(spec: &EffectSpec) -> Vec<(&'static str, Value)> {
    let mut out = vec![("default", json!({}))];
    let heavy = match spec.id {
        "effect.blur.gaussian" => json!({"radius":50}),
        "effect.blur.surface" => json!({"radius":30}),
        "effect.blur.unfocus" => json!({"radius":30}),
        "effect.photo.glow" => json!({"radius":20}),
        "effect.noise.reduce" => json!({"radius":30}),
        _ => return out,
    };
    out.push(("large radius", heavy));
    out
}

/// Writes docs/reports/perf-table.md: median of 3 engine runs per backend at
/// 1, 12 and 24 MP. A first run over `OP_M54_CAP_S` seconds (default 30) is
/// recorded alone instead of repeating it.
#[test]
#[ignore = "release performance table"]
fn m54_write_perf_table() {
    let gpu_on = gpu_tests_enabled();
    let cap: f64 = std::env::var("OP_M54_CAP_S")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(30.0);
    let only: Option<HashSet<String>> = std::env::var("OP_M54_ONLY")
        .ok()
        .map(|v| v.split(',').map(str::to_owned).collect());
    let started = Instant::now();
    let load_before = load();
    let sizes = [
        ("1 MP", 1000u32, 1000u32),
        ("12 MP", 4000, 3000),
        ("24 MP", 6000, 4000),
    ];
    let cpu = Engine::with_options(None, options(true, None)).unwrap();
    let gpu = Engine::with_options(None, options(false, None)).unwrap();
    let mut table = String::new();
    // CPU milliseconds per megapixel from the first size, to skip CPU runs that
    // would pass the cap at larger sizes.
    let mut one_mp: std::collections::HashMap<(&str, &str), f64> = Default::default();
    for (size, w, h) in sizes {
        let pixels = synthetic(w, h);
        let c = cpu.import_pixels(w, h, &pixels, None).unwrap();
        let g = gpu_on.then(|| gpu.import_pixels(w, h, &pixels, None).unwrap());
        drop(pixels);
        for spec in effect::registered() {
            if only.as_ref().is_some_and(|o| !o.contains(spec.id)) {
                continue;
            }
            if spec.id == "effect.fill.content_aware" {
                writeln!(table, "| {} | default | {size} ({w} × {h}) | not run: needs a selected hole | CPU only | n/a |", spec.id).unwrap();
                continue;
            }
            for (set, params) in perf_sets(&spec) {
                let time = |e: &Engine, doc: u64| -> Result<(String, f64), String> {
                    let mut runs = Vec::new();
                    for _ in 0..3 {
                        let start = Instant::now();
                        e.dispatch(doc, spec.id, params.clone())
                            .map_err(|err| err.to_string())?;
                        runs.push(start.elapsed().as_secs_f64() * 1000.0);
                        e.dispatch(doc, "edit.undo", json!({}))
                            .map_err(|err| err.to_string())?;
                        if runs[0] > cap * 1000.0 {
                            break;
                        }
                    }
                    runs.sort_by(f64::total_cmp);
                    Ok(if runs.len() == 3 {
                        (format!("{:.0} ms", runs[1]), runs[1])
                    } else {
                        (
                            format!("{:.0} ms (1 run; over {cap:.0} s cap)", runs[0]),
                            runs[0],
                        )
                    })
                };
                let key = (spec.id, set);
                let megapixels = f64::from(w) * f64::from(h) / 1e6;
                let cpu_cell = match one_mp.get(&key) {
                    Some(&ms) if ms * megapixels > cap * 1000.0 => format!(
                        "not run: 1 MP took {ms:.0} ms, so this size would pass the {cap:.0} s cap"
                    ),
                    _ => match time(&cpu, c) {
                        Ok((t, ms)) => {
                            one_mp.entry(key).or_insert(ms / megapixels);
                            t
                        }
                        Err(e) => format!("error: {e}"),
                    },
                };
                let gpu_cell = match (spec.gpu, g) {
                    (None, _) => "CPU only".to_owned(),
                    (Some(_), None) => "not run: OP_GPU_TESTS=0".to_owned(),
                    (Some(_), Some(g)) => {
                        // Warm the pipeline so the median is warm.
                        let before = gpu_runs(&gpu, g);
                        let warm = engine_run(&gpu, g, spec.id, &params);
                        match warm {
                            Ok((_, _, Some(reason))) => format!("fell back to the CPU: {reason}"),
                            Err(e) => format!("error: {e}"),
                            Ok(_) => {
                                let t = time(&gpu, g)
                                    .map_or_else(|e| format!("error: {e}"), |(t, _)| t);
                                assert!(
                                    gpu_runs(&gpu, g) > before,
                                    "{} did not run on the GPU",
                                    spec.id
                                );
                                t
                            }
                        }
                    }
                };
                let params_text = spec
                    .schema
                    .validate(params.clone())
                    .map(|p| {
                        if set == "default" {
                            "defaults".to_owned()
                        } else {
                            params
                                .as_object()
                                .unwrap()
                                .iter()
                                .map(|(k, _)| format!("{k}={}", short(&p[k.as_str()])))
                                .collect::<Vec<_>>()
                                .join(", ")
                        }
                    })
                    .unwrap_or_default();
                let line = format!(
                    "| {} | {set} ({params_text}) | {size} ({w} × {h}) | {cpu_cell} | {gpu_cell} | {:.0} s |",
                    spec.id,
                    started.elapsed().as_secs_f64()
                );
                println!("{line}");
                writeln!(table, "{line}").unwrap();
            }
        }
        cpu.close(c).unwrap();
        if let Some(g) = g {
            gpu.close(g).unwrap();
        }
    }
    let mut report = String::new();
    writeln!(report, "# M54 performance table\n").unwrap();
    writeln!(
        report,
        "Written by `cargo test --release -p op-engine --test m54_parity m54_write_perf_table -- --ignored --test-threads=1` \
         (engine/op-engine/tests/m54_parity.rs). Do not edit by hand.\n\n\
         - Build: release (`--release`, the workspace's release profile).\n\
         - Machine: {}.\n\
         - Load average before: {load_before}; after: {}. Other builders shared this Mac during the run, so treat times as upper bounds. Total run time {:.0} s.\n\
         - Path: both columns are the engine (`Engine::dispatch` on the whole layer, no selection), timed from dispatch to finished document, including tile reads, upload, dispatch, readback and tile writes. CPU is an engine with `force_cpu`; GPU uses the default 2048 px chunks. The GPU column warms the kernel once before timing.\n\
         - Input: the synthetic gradient-and-noise image with clear, opaque and partial alpha bands, at 1000 × 1000, 4000 × 3000 and 6000 × 4000.\n\
         - Each time is the median of 3 runs. A run over the {cap:.0} s cap is reported alone. A CPU cell at 12 or 24 MP is not run when the 1 MP time, scaled by pixel count, passes the cap; the cell says so and gives the 1 MP time (no estimate is printed as a result). The last column is the time since the run started, to show when each row ran.\n",
        machine(),
        load(),
        started.elapsed().as_secs_f64()
    )
    .unwrap();
    if let Some(only) = &only {
        let mut only: Vec<_> = only.iter().cloned().collect();
        only.sort();
        writeln!(
            report,
            "**Partial run:** OP_M54_ONLY limited this run to {}.\n",
            only.join(", ")
        )
        .unwrap();
    }
    writeln!(
        report,
        "| Effect | Parameter set | Size | CPU median of 3 | Engine GPU median of 3 | Elapsed |"
    )
    .unwrap();
    writeln!(report, "| --- | --- | --- | ---: | ---: | ---: |").unwrap();
    report.push_str(&table);
    std::fs::write(root().join("docs/reports/perf-table.md"), report).unwrap();
}

/// M54 regression: the GPU Pencil Sketch blur sums in 32-bit integers, which
/// overflow from tip size 13 (255 × 256 × 14⁴ > 2³¹). The engine must give the
/// CPU's bytes there (the matrix found 255/255 errors at tip sizes 19 and 20).
#[test]
fn m54_pencil_sketch_large_tips_match_cpu_through_the_engine() {
    if !gpu_tests_enabled() {
        return;
    }
    let f = oracle_fixture("impulse_64");
    let gpu = Engine::with_options(None, options(false, Some(192))).unwrap();
    let g = gpu
        .import_pixels(f.width, f.height, &f.pixels, None)
        .unwrap();
    let spec = effect::find("effect.artistic.pencil_sketch").unwrap();
    for tip in [12, 13, 20] {
        let mut params = spec.default_params();
        params["pencil_tip_size"] = json!(tip);
        let (bytes, _, fell_back) = engine_run(&gpu, g, spec.id, &params).unwrap();
        let cpu = cpu_render(&spec, &f, &params).unwrap();
        let (worst, _, _) = compare(&cpu, &bytes, 0);
        assert_eq!(
            worst, 0,
            "tip {tip}: worst {worst}, fell back {fell_back:?}"
        );
        if tip == 12 {
            assert!(fell_back.is_none(), "tip 12 fits the kernel's sums");
        }
    }
}

/// One matrix cell on the 600 × 400 input through the engine (192 px chunks):
/// the worst CPU-GPU channel error, where it is, and the CPU reason when the
/// engine fell back.
fn cell_600x400(id: &str, set: &[(&str, Value)]) -> (u8, String, Option<String>) {
    let f = multi_tile(600, 400);
    let spec = effect::find(id).unwrap();
    let mut params = spec.default_params();
    for (key, value) in set {
        params[*key] = value.clone();
    }
    let gpu = Engine::with_options(None, options(false, Some(192))).unwrap();
    let g = gpu
        .import_pixels(f.width, f.height, &f.pixels, None)
        .unwrap();
    let (bytes, _, fell_back) = engine_run(&gpu, g, spec.id, &params).unwrap();
    let cpu = cpu_render(&spec, &f, &params).unwrap();
    let (i, worst) = cpu
        .iter()
        .zip(&bytes)
        .map(|(a, b)| a.abs_diff(*b))
        .enumerate()
        .max_by_key(|(_, d)| *d)
        .unwrap();
    let p = i / 4;
    let at = format!(
        "({},{}) channel {}: cpu {:?} gpu {:?}",
        p % 600,
        p / 600,
        i % 4,
        &cpu[p * 4..p * 4 + 4],
        &bytes[p * 4..p * 4 + 4]
    );
    (worst, at, fell_back)
}

/// M54 fix regression: the six failing matrix cells. Each one must be within
/// its kernel tolerance, whether the GPU ran it or the engine named a reason
/// and used the CPU.
#[test]
fn m54fix_failing_matrix_cells_hold_their_kernel_tolerance() {
    if !gpu_tests_enabled() {
        return;
    }
    type Cell = (&'static str, Vec<(&'static str, Value)>, u8);
    let cells: [Cell; 5] = [
        ("effect.distort.bulge", vec![], 1),
        ("effect.photo.glow", vec![("radius", json!(20))], 1),
        (
            "effect.photo.soften_portrait",
            vec![("warmth", json!(19))],
            1,
        ),
        (
            "effect.photo.red_eye_removal",
            vec![("saturation", json!(50))],
            0,
        ),
        (
            "effect.photo.red_eye_removal",
            vec![("saturation", json!(100))],
            0,
        ),
    ];
    let mut failures = Vec::new();
    for (id, set, tol) in cells {
        let (worst, at, fell_back) = cell_600x400(id, &set);
        if worst > tol {
            failures.push(format!(
                "{id} {set:?}: worst {worst} > {tol} at {at}; fell back {fell_back:?}"
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// M54 fix regression: Red Eye is exact on the GPU for every saturation step
/// and stays on the GPU. The CPU truncates an f64 product, which sits just
/// under a whole number for some colours (0,72,24 at saturation 100 gives 44,
/// not 45).
#[test]
fn m54fix_red_eye_is_exact_on_every_colour_and_saturation() {
    if !gpu_tests_enabled() {
        return;
    }
    // Every red-heavy colour that passes the tolerance 0 test, 4096 per row.
    let mut pixels = Vec::new();
    for r in (0..256u32).step_by(3) {
        for g in 0..256u32 {
            for b in (0..256u32).step_by(5) {
                if r > g.max(b) {
                    pixels.extend_from_slice(&[r as u8, g as u8, b as u8, 255]);
                }
            }
        }
    }
    let width = 4096u32;
    pixels.resize(
        pixels.len().div_ceil(width as usize * 4) * width as usize * 4,
        0,
    );
    let height = (pixels.len() / 4) as u32 / width;
    let f = Fixture {
        name: "red_eye_colours".into(),
        width,
        height,
        pixels,
    };
    let spec = effect::find("effect.photo.red_eye_removal").unwrap();
    let gpu = Engine::with_options(None, options(false, None)).unwrap();
    let g = gpu
        .import_pixels(f.width, f.height, &f.pixels, None)
        .unwrap();
    let mut failures = Vec::new();
    for saturation in [0, 1, 7, 25, 50, 64, 80, 99, 100] {
        let mut params = spec.default_params();
        params["tolerance"] = json!(0);
        params["saturation"] = json!(saturation);
        let (bytes, _, fell_back) = engine_run(&gpu, g, spec.id, &params).unwrap();
        assert!(fell_back.is_none(), "Red Eye fell back: {fell_back:?}");
        let cpu = cpu_render(&spec, &f, &params).unwrap();
        let (worst, differ, _) = compare(&cpu, &bytes, 0);
        if worst > 0 {
            failures.push(format!(
                "saturation {saturation}: worst {worst} on {differ} channels"
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// M54 fix regression: the Pencil Sketch blur now keeps 64-bit row sums, so
/// tips above 12 stay on the GPU and give the CPU's bytes.
#[test]
fn m54fix_pencil_sketch_large_tips_stay_on_the_gpu() {
    if !gpu_tests_enabled() {
        return;
    }
    let f = multi_tile(280, 270);
    let gpu = Engine::with_options(None, options(false, Some(192))).unwrap();
    let g = gpu
        .import_pixels(f.width, f.height, &f.pixels, None)
        .unwrap();
    let spec = effect::find("effect.artistic.pencil_sketch").unwrap();
    for (tip, range) in [(13, 0), (16, -20), (20, 20)] {
        let mut params = spec.default_params();
        params["pencil_tip_size"] = json!(tip);
        params["range"] = json!(range);
        let (bytes, _, fell_back) = engine_run(&gpu, g, spec.id, &params).unwrap();
        assert!(fell_back.is_none(), "tip {tip} fell back: {fell_back:?}");
        let cpu = cpu_render(&spec, &f, &params).unwrap();
        let (worst, differ, _) = compare(&cpu, &bytes, 0);
        assert_eq!(
            worst, 0,
            "tip {tip} range {range}: {differ} channels differ"
        );
    }
}
