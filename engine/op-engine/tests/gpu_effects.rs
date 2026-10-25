//! GPU effect dispatch: the engine with the GPU on must give the CPU path's bytes
//! (within each kernel's parity tolerance), fall back by name, and cancel cleanly.
//! Set OP_GPU_TESTS=0 to skip the tests that need a GPU adapter.
use op_core::tiles::{TILE_SIZE, TileCoord};
use op_engine::{Engine, EngineOptions, Event, EventKind, JobTicket, Op};
use op_ops::effect::{self, GpuKernel};
use serde_json::{Value, json};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// 520x300 is 3x2 tiles with partial right and bottom tiles.
const W: u32 = 520;
const H: u32 = 300;

fn gpu_tests_enabled() -> bool {
    if std::env::var("OP_GPU_TESTS").as_deref() == Ok("0") {
        println!("SKIP OP_GPU_TESTS=0");
        return false;
    }
    true
}
fn image(width: u32, height: u32) -> Vec<u8> {
    let mut seed = 0x2468_ace1u32;
    let mut out = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            let noise = (seed >> 24) as u8;
            // Smooth gradients plus noise, with fully clear and fully opaque areas.
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
fn options(force_cpu: bool) -> EngineOptions {
    EngineOptions {
        force_cpu,
        // Small chunks so every neighborhood kernel needs several dispatches.
        max_gpu_chunk: Some(192),
        ..EngineOptions::default()
    }
}
fn info(e: &Engine, id: u64) -> Value {
    serde_json::from_slice(&e.query(id, "doc.info", json!({})).unwrap()).unwrap()
}
/// The active layer's raw bytes, including color under zero alpha
/// (`doc.pixels` composites, which hides that color).
fn pixels(e: &Engine, id: u64) -> Vec<u8> {
    let state = e.actor_probe(id).unwrap().0;
    let (w, h) = (state.width as usize, state.height as usize);
    let mut out = vec![0; w * h * 4];
    for ty in 0..h.div_ceil(TILE_SIZE) {
        for tx in 0..w.div_ceil(TILE_SIZE) {
            let coord = TileCoord {
                x: tx as i32,
                y: ty as i32,
            };
            let tile = state.read_tile(state.active_layer, coord).unwrap();
            let n = (w - tx * TILE_SIZE).min(TILE_SIZE) * 4;
            for y in 0..(h - ty * TILE_SIZE).min(TILE_SIZE) {
                let dst = ((ty * TILE_SIZE + y) * w + tx * TILE_SIZE) * 4;
                out[dst..dst + n].copy_from_slice(&tile[y * TILE_SIZE * 4..][..n]);
            }
        }
    }
    out
}
fn same(a: &[u8], b: &[u8]) -> bool {
    a == b
}
fn doc_hash(e: &Engine, id: u64) -> u64 {
    let mut h = DefaultHasher::new();
    pixels(e, id).hash(&mut h);
    e.query(id, "doc.pixels", json!({})).unwrap().hash(&mut h);
    e.snapshot(id).unwrap().revision.hash(&mut h);
    info(e, id)["history_labels"].to_string().hash(&mut h);
    h.finish()
}
fn gpu_runs(e: &Engine, id: u64) -> u64 {
    info(e, id)["gpu_effects"]["gpu_runs"].as_u64().unwrap()
}
fn cpu_runs(e: &Engine, id: u64) -> u64 {
    info(e, id)["gpu_effects"]["cpu_runs"].as_u64().unwrap()
}
/// The CPU vs GPU tolerance from op-gpu's parity tests (op-gpu/tests/effects.rs),
/// plus Bulge and Twist: their f32 warp math rounds 1 away from the CPU at a few
/// pixels of larger images, which the parity runner's ±1 rule allows (see
/// `warp_kernels_round_like_the_direct_kernel`).
fn tolerance(kernel: GpuKernel) -> u8 {
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
/// Non-default parameters for kernels that take parameters.
fn alternate_params(id: &str) -> Option<Value> {
    Some(match id {
        "effect.adjust.brightness_contrast" => json!({"Brightness":-30,"Contrast":40}),
        "effect.adjust.temperature_tint" => json!({"Temperature":35,"Tint":-20}),
        "effect.adjust.highlights_shadows" => json!({"Highlights":-40,"Shadows":60}),
        "effect.adjust.posterize" => {
            json!({"RedLevels":5,"GreenLevels":9,"BlueLevels":3,"LinkLevels":false})
        }
        "effect.adjust.sepia" => json!({"Intensity":80}),
        "effect.adjust.curves" => json!({
            "ColorTransferMode":"Rgb",
            "ControlPoints":[[[0,0],[90,140],[255,255]],[[0,30],[255,200]],[[0,0],[200,90],[255,255]]]
        }),
        "effect.adjust.levels" => json!({
            "InputBlack":[10,20,5],"InputWhite":[240,250,200],"Gamma":[0.7,1.0,1.6],
            "OutputBlack":[0,15,0],"OutputWhite":[255,230,250]
        }),
        "effect.adjust.exposure" => json!({"Exposure":0.7}),
        "effect.blur.gaussian" => json!({"radius":13}),
        "effect.stylize.edge_detect" => json!({"angle":-60.0}),
        "effect.stylize.emboss" => json!({"angle":30.0}),
        "effect.stylize.relief" => json!({"angle":120.0}),
        "effect.artistic.oil_painting" => json!({"brush_size":5,"coarseness":120}),
        "effect.stylize.outline" => json!({"thickness":6,"intensity":20,"quality":"high"}),
        "effect.artistic.ink_sketch" => json!({"ink_outline":30,"coloring":70}),
        "effect.artistic.pencil_sketch" => json!({"pencil_tip_size":5,"range":-8}),
        "effect.distort.pixelate" => json!({"cell_size":7}),
        "effect.photo.red_eye_removal" => json!({"tolerance":20,"saturation":40}),
        "effect.photo.vignette" => {
            json!({"center_x":0.25,"center_y":-0.4,"radius":30,"strength":80})
        }
        "effect.photo.sharpen" => json!({"amount":6}),
        "effect.noise.reduce" => json!({"radius":4,"strength":0.75}),
        "effect.noise.add" => {
            json!({"intensity":30,"saturation":250,"coverage":60.0,"seed":12345})
        }
        "effect.photo.glow" => json!({"radius":9,"brightness":-20,"contrast":30}),
        "effect.photo.soften_portrait" => json!({"softness":3,"lighting":7,"warmth":15}),
        _ => return None,
    })
}
fn assert_close(id: &str, label: &str, tol: u8, expected: &[u8], actual: &[u8]) {
    assert_eq!(expected.len(), actual.len(), "{id} {label} length");
    let mut worst = 0;
    let mut first = None;
    for (i, (a, b)) in expected.iter().zip(actual).enumerate() {
        let d = a.abs_diff(*b);
        if d > worst {
            worst = d;
        }
        if d > tol && first.is_none() {
            first = Some(i);
        }
    }
    if let Some(i) = first {
        let p = i / 4;
        let (x, y) = (p as u32 % W, p as u32 / W);
        panic!(
            "{id} {label}: worst delta {worst} > {tol} first at ({x},{y}) cpu={:?} gpu={:?}",
            &expected[p * 4..p * 4 + 4],
            &actual[p * 4..p * 4 + 4]
        );
    }
}

#[test]
fn every_gpu_effect_matches_cpu_with_geometry_and_soft_selections() {
    if !gpu_tests_enabled() {
        return;
    }
    let source = image(W, H);
    let gpu = Engine::with_options(None, options(false)).unwrap();
    let cpu = Engine::with_options(None, options(true)).unwrap();
    let mut checked = 0;
    for shape in ["ellipse", "soft"] {
        let g = gpu.import_pixels(W, H, &source, None).unwrap();
        let c = cpu.import_pixels(W, H, &source, None).unwrap();
        for (e, id) in [(&gpu, g), (&cpu, c)] {
            e.dispatch(id, "test.set_selection_shape", json!({"shape":shape}))
                .unwrap();
        }
        for spec in effect::registered() {
            let Some(kernel) = spec.gpu else { continue };
            let mut sets = vec![("defaults", json!({}))];
            if let Some(alt) = alternate_params(spec.id) {
                sets.push(("alternate", alt));
            }
            for (label, params) in sets {
                let label = format!("{shape} {label}");
                let before = gpu_runs(&gpu, g);
                let start = Instant::now();
                gpu.dispatch(g, spec.id, params.clone())
                    .unwrap_or_else(|e| panic!("{} {label} gpu: {e}", spec.id));
                let gpu_ms = start.elapsed().as_millis();
                let start = Instant::now();
                cpu.dispatch(c, spec.id, params.clone())
                    .unwrap_or_else(|e| panic!("{} {label} cpu: {e}", spec.id));
                let cpu_ms = start.elapsed().as_millis();
                let gi = info(&gpu, g);
                assert_eq!(gi["gpu"], true, "{} {label}: {gi}", spec.id);
                assert_eq!(
                    gpu_runs(&gpu, g),
                    before + 1,
                    "{} {label} did not run on the GPU: {}",
                    spec.id,
                    gi["gpu_effects"]
                );
                assert_eq!(gpu_runs(&cpu, c), 0, "forced-CPU engine used the GPU path");
                assert_close(
                    spec.id,
                    &label,
                    tolerance(kernel),
                    &pixels(&cpu, c),
                    &pixels(&gpu, g),
                );
                // The source pixels must not drift between effects.
                for (e, id) in [(&gpu, g), (&cpu, c)] {
                    e.dispatch(id, "edit.undo", json!({})).unwrap();
                }
                println!("{} {label}: gpu {gpu_ms} ms, cpu {cpu_ms} ms", spec.id);
                checked += 1;
            }
        }
    }
    // 34 kernels today (another builder may add more); 24 get a second parameter set.
    assert!(
        checked >= 2 * (34 + 24),
        "only {checked} effect runs checked"
    );
}

#[test]
fn whole_image_without_selection_matches_cpu() {
    if !gpu_tests_enabled() {
        return;
    }
    let source = image(W, H);
    let gpu = Engine::with_options(None, options(false)).unwrap();
    let cpu = Engine::with_options(None, options(true)).unwrap();
    let g = gpu.import_pixels(W, H, &source, None).unwrap();
    let c = cpu.import_pixels(W, H, &source, None).unwrap();
    for (id, params) in [
        ("effect.blur.gaussian", json!({"radius":40})),
        ("effect.distort.twist", json!({})),
        ("effect.adjust.invert_colors", json!({})),
    ] {
        gpu.dispatch(g, id, params.clone()).unwrap();
        cpu.dispatch(c, id, params).unwrap();
        let kernel = effect::find(id).unwrap().gpu.unwrap();
        assert_close(
            id,
            "none",
            tolerance(kernel),
            &pixels(&cpu, c),
            &pixels(&gpu, g),
        );
        for (e, id) in [(&gpu, g), (&cpu, c)] {
            e.dispatch(id, "edit.undo", json!({})).unwrap();
        }
    }
    assert_eq!(gpu_runs(&gpu, g), 3);
}

/// The ±1 Bulge and Twist differences come from the kernels, not from dispatch:
/// with no selection the engine's GPU bytes equal a direct op-gpu call.
#[test]
fn warp_kernels_round_like_the_direct_kernel() {
    if !gpu_tests_enabled() {
        return;
    }
    let source = image(W, H);
    let gpu = Engine::with_options(None, options(false)).unwrap();
    let g = gpu.import_pixels(W, H, &source, None).unwrap();
    let mut direct = op_gpu::GpuContext::new().expect("GPU adapter");
    for (id, kernel) in [
        ("effect.distort.bulge", GpuKernel::Bulge),
        ("effect.distort.twist", GpuKernel::Twist),
    ] {
        gpu.dispatch(g, id, json!({})).unwrap();
        let engine = pixels(&gpu, g);
        gpu.dispatch(g, "edit.undo", json!({})).unwrap();
        let kernel_out = direct.effect_kernel(kernel, W, H, &source).unwrap();
        assert!(
            same(&engine, &kernel_out),
            "{id}: engine GPU bytes differ from direct kernel bytes"
        );
        let reference = effect::reference_rgba8(id, W, H, &source).unwrap();
        let worst = reference
            .iter()
            .zip(&kernel_out)
            .map(|(a, b)| a.abs_diff(*b))
            .max()
            .unwrap();
        let count = reference
            .iter()
            .zip(&kernel_out)
            .filter(|(a, b)| a != b)
            .count();
        println!("{id}: direct kernel vs CPU reference worst {worst}, {count} bytes differ");
        assert!(worst <= 1, "{id} worst {worst}");
    }
    // Non-default Bulge amounts drift further, so the engine keeps Bulge on the
    // CPU for them (see the plan in gpu_effects.rs).
    let spec = effect::find("effect.distort.bulge").unwrap();
    let mut params = spec.default_params();
    params["amount"] = json!(-80);
    let prepared = op_ops::effect::Prepared {
        width: W as i32,
        height: H as i32,
        ..Default::default()
    };
    let sample = |x: i32, y: i32| -> [u8; 4] {
        if x < 0 || y < 0 || x >= W as i32 || y >= H as i32 {
            return [0; 4];
        }
        let i = (y as usize * W as usize + x as usize) * 4;
        source[i..i + 4].try_into().unwrap()
    };
    let mut reference = Vec::new();
    for y in 0..H as i32 {
        for x in 0..W as i32 {
            reference.extend((spec.render_cpu)(&sample, x, y, &prepared, &params));
        }
    }
    let direct = direct
        .effect_kernel_with_params(GpuKernel::Bulge, W, H, &source, [-80, 0, 0])
        .unwrap();
    let worst = reference
        .iter()
        .zip(&direct)
        .map(|(a, b)| a.abs_diff(*b))
        .max()
        .unwrap();
    println!("bulge amount -80: direct kernel vs CPU worst {worst}");
}

#[test]
fn kernel_rejecting_parameters_falls_back_to_cpu_by_name() {
    if !gpu_tests_enabled() {
        return;
    }
    let source = image(W, H);
    let gpu = Engine::with_options(None, options(false)).unwrap();
    let cpu = Engine::with_options(None, options(true)).unwrap();
    let g = gpu.import_pixels(W, H, &source, None).unwrap();
    let c = cpu.import_pixels(W, H, &source, None).unwrap();
    // The GPU Gaussian has no gamma stage, the GPU Outline has no fast mode, and
    // the GPU Bulge drifts past ±1 away from its default amount.
    for (id, params) in [
        ("effect.blur.gaussian", json!({"radius":3,"gamma":"srgb"})),
        ("effect.stylize.outline", json!({"quality":"fast"})),
        ("effect.distort.bulge", json!({"amount":-80})),
    ] {
        let before = cpu_runs(&gpu, g);
        gpu.dispatch(g, id, params.clone()).unwrap();
        cpu.dispatch(c, id, params).unwrap();
        let gi = info(&gpu, g);
        assert_eq!(cpu_runs(&gpu, g), before + 1, "{id}: {gi}");
        let reason = gi["gpu_effects"]["last_cpu_reason"].as_str().unwrap();
        assert!(reason.contains(id), "{reason}");
        assert!(reason.contains("parameters"), "{reason}");
        assert!(same(&pixels(&cpu, c), &pixels(&gpu, g)), "{id}");
    }
    assert_eq!(gpu_runs(&gpu, g), 0);
    // A rejected plan does not start or turn off the GPU; the next effect uses it.
    gpu.dispatch(g, "effect.adjust.sepia", json!({})).unwrap();
    assert_eq!(gpu_runs(&gpu, g), 1);
    assert_eq!(info(&gpu, g)["gpu"], true);
}

#[test]
fn gpu_creation_failure_falls_back_with_reason_in_doc_info() {
    let source = image(W, H);
    let failing = Engine::with_options(
        None,
        EngineOptions {
            simulate_gpu_failure: Some("simulated adapter failure".into()),
            ..options(false)
        },
    )
    .unwrap();
    let cpu = Engine::with_options(None, options(true)).unwrap();
    let f = failing.import_pixels(W, H, &source, None).unwrap();
    let c = cpu.import_pixels(W, H, &source, None).unwrap();
    // Nothing starts the GPU before the first effect.
    let before = info(&failing, f);
    assert_eq!(before["gpu"], false);
    assert!(
        before["gpu_reason"]
            .as_str()
            .unwrap()
            .contains("not started"),
        "{before}"
    );
    for e in [&failing, &cpu] {
        let id = if std::ptr::eq(e, &failing) { f } else { c };
        e.dispatch(id, "effect.blur.gaussian", json!({"radius":4}))
            .unwrap();
        e.dispatch(id, "effect.adjust.sepia", json!({})).unwrap();
    }
    let after = info(&failing, f);
    assert_eq!(after["gpu"], false, "{after}");
    let reason = after["gpu_reason"].as_str().unwrap();
    assert!(reason.contains("simulated adapter failure"), "{reason}");
    assert_eq!(after["gpu_effects"]["gpu_runs"], 0);
    assert_eq!(after["gpu_effects"]["cpu_runs"], 2);
    assert!(same(&pixels(&cpu, c), &pixels(&failing, f)));
}

#[test]
fn forced_cpu_reports_gpu_off_and_env_switch_parses() {
    let cpu = Engine::with_options(None, options(true)).unwrap();
    let c = cpu.import_pixels(4, 4, &[9; 64], None).unwrap();
    cpu.dispatch(c, "effect.adjust.invert_colors", json!({}))
        .unwrap();
    let i = info(&cpu, c);
    assert_eq!(i["gpu"], false);
    assert!(
        i["gpu_reason"].as_str().unwrap().contains("OP_FORCE_CPU"),
        "{i}"
    );
    assert!(EngineOptions::force_cpu_from(Some("1")));
    assert!(!EngineOptions::force_cpu_from(Some("0")));
    assert!(!EngineOptions::force_cpu_from(None));
}

fn engine_with_events(options: EngineOptions) -> (Engine, Receiver<Event>) {
    let (tx, rx) = channel();
    let tx = Mutex::new(tx);
    let e = Engine::with_options(
        Some(Arc::new(move |ev| {
            let _ = tx.lock().unwrap().send(ev);
        })),
        options,
    )
    .unwrap();
    (e, rx)
}
fn wait_job(rx: &Receiver<Event>, ticket: u64) -> Event {
    let end = Instant::now() + Duration::from_secs(60);
    loop {
        let left = end.saturating_duration_since(Instant::now());
        let ev = rx.recv_timeout(left).expect("JobFinished");
        if matches!(ev.kind, EventKind::JobFinished) && ev.ticket == ticket {
            return ev;
        }
    }
}

#[test]
fn cancel_during_gpu_effect_leaves_document_unchanged() {
    if !gpu_tests_enabled() {
        return;
    }
    let (e, rx) = engine_with_events(EngineOptions {
        max_gpu_chunk: Some(64),
        ..EngineOptions::default()
    });
    let (w, h) = (1024, 768);
    let doc = e.import_pixels(w, h, &image(w, h), None).unwrap();
    // Warm the GPU so the cancelled run is a GPU run, not device creation.
    e.dispatch(doc, "effect.adjust.invert_colors", json!({}))
        .unwrap();
    e.dispatch(doc, "edit.undo", json!({})).unwrap();
    let before = doc_hash(&e, doc);
    let runs_before = info(&e, doc)["gpu_effects"]["gpu_started"]
        .as_u64()
        .unwrap();

    // A ticket cancelled before the job starts never runs.
    let early = JobTicket::new(41);
    early.cancel();
    e.submit(
        doc,
        Op::Command("effect.blur.gaussian".into(), json!({"radius":30})),
        early,
    )
    .unwrap();
    assert_eq!(wait_job(&rx, 41).json["status"], "cancelled");

    // Cancel at the first progress report of a running GPU job.
    let ticket = JobTicket::new(42);
    e.submit(
        doc,
        Op::Command("effect.blur.gaussian".into(), json!({"radius":30})),
        ticket.clone(),
    )
    .unwrap();
    let finished = loop {
        let ev = rx.recv_timeout(Duration::from_secs(60)).unwrap();
        if ev.ticket == 42 && matches!(ev.kind, EventKind::Progress) {
            ticket.cancel();
        }
        if ev.ticket == 42 && matches!(ev.kind, EventKind::JobFinished) {
            break ev;
        }
    };
    assert_eq!(finished.json["status"], "cancelled", "{}", finished.json);
    assert_eq!(doc_hash(&e, doc), before);
    let gi = info(&e, doc);
    assert_eq!(
        gi["gpu_effects"]["gpu_started"].as_u64().unwrap(),
        runs_before + 1,
        "the cancelled job ran on the GPU path: {gi}"
    );
    assert!(
        gi["gpu_effects"]["gpu_cancelled"].as_u64().unwrap() >= 1,
        "{gi}"
    );
}

#[test]
fn effect_preview_and_commit_use_the_gpu_and_match_cpu() {
    if !gpu_tests_enabled() {
        return;
    }
    let source = image(W, H);
    let (gpu, grx) = engine_with_events(options(false));
    let (cpu, crx) = engine_with_events(options(true));
    let g = gpu.import_pixels(W, H, &source, None).unwrap();
    let c = cpu.import_pixels(W, H, &source, None).unwrap();
    for (e, rx, id) in [(&gpu, &grx, g), (&cpu, &crx, c)] {
        e.dispatch(id, "test.set_selection_shape", json!({"shape":"soft"}))
            .unwrap();
        let begin = Op::PreviewBegin("effect.photo.sharpen".into(), json!({"amount":3}));
        e.submit(id, begin, JobTicket::new(51)).unwrap();
        assert_eq!(wait_job(rx, 51).json["status"], "ok");
        e.submit(
            id,
            Op::PreviewUpdate(json!({"amount":5})),
            JobTicket::new(52),
        )
        .unwrap();
        assert_eq!(wait_job(rx, 52).json["status"], "ok");
    }
    let gp = gpu.snapshot(g).unwrap();
    let cp = cpu.snapshot(c).unwrap();
    assert!(gp.preview.is_some() && cp.preview.is_some());
    assert!(
        same(&gp.pixels(true).unwrap(), &cp.pixels(true).unwrap()),
        "preview bytes"
    );
    assert_ne!(gp.pixels(true).unwrap(), gp.pixels(false).unwrap());
    assert_eq!(gpu_runs(&gpu, g), 2, "both preview renders ran on the GPU");
    gpu.preview_commit(g).unwrap();
    cpu.preview_commit(c).unwrap();
    assert_eq!(gpu_runs(&gpu, g), 3, "commit ran on the GPU");
    assert!(same(&pixels(&gpu, g), &pixels(&cpu, c)), "commit bytes");
    assert_eq!(info(&gpu, g)["history_labels"].as_array().unwrap().len(), 3);
}

#[test]
fn m54_ten_effect_chain_stays_within_two_channels() {
    if !gpu_tests_enabled() {
        return;
    }
    let source = image(64, 64);
    let gpu = Engine::with_options(None, options(false)).unwrap();
    let cpu = Engine::with_options(None, options(true)).unwrap();
    let g = gpu.import_pixels(64, 64, &source, None).unwrap();
    let c = cpu.import_pixels(64, 64, &source, None).unwrap();
    let chain = [
        "effect.adjust.invert_colors",
        "effect.adjust.brightness_contrast",
        "effect.adjust.sepia",
        "effect.adjust.temperature_tint",
        "effect.adjust.posterize",
        "effect.adjust.invert_alpha",
        "effect.adjust.black_and_white",
        "effect.adjust.exposure",
        "effect.adjust.highlights_shadows",
        "effect.photo.red_eye_removal",
    ];
    for effect_id in chain {
        let before = gpu_runs(&gpu, g);
        gpu.dispatch(g, effect_id, json!({})).unwrap();
        cpu.dispatch(c, effect_id, json!({})).unwrap();
        assert_eq!(gpu_runs(&gpu, g), before + 1, "{effect_id} fell back");
    }
    let max_error = pixels(&gpu, g)
        .iter()
        .zip(pixels(&cpu, c))
        .map(|(a, b)| a.abs_diff(b))
        .max()
        .unwrap();
    println!("M54 10-effect engine chain: max channel error {max_error}/255");
    assert!(max_error <= 2, "10-effect chain error {max_error}/255");
}

/// The matrix is written by tests/m54_parity.rs (`m54_write_parity_matrix`).
#[test]
fn m54_matrix_lists_every_registered_effect() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/reports/parity-matrix.md");
    let matrix = std::fs::read_to_string(path).expect("run the ignored M54 report test first");
    let mut checked = 0;
    for spec in effect::registered() {
        assert!(
            matrix.contains(&format!("| {} |", spec.id)),
            "{} missing from matrix",
            spec.id
        );
        checked += 1;
    }
    assert_eq!(checked, 55);
}

#[test]
fn doc_info_reports_gpu_limits_and_chunk_size() {
    if !gpu_tests_enabled() {
        return;
    }
    let gpu = Engine::with_options(None, options(false)).unwrap();
    let g = gpu.import_pixels(W, H, &image(W, H), None).unwrap();
    gpu.dispatch(g, "effect.blur.gaussian", json!({"radius":20}))
        .unwrap();
    let i = info(&gpu, g);
    assert_eq!(i["gpu"], true, "{i}");
    assert!(i["gpu_reason"].is_null(), "{i}");
    let d = &i["gpu_device"];
    assert!(d["adapter"].as_str().is_some_and(|s| !s.is_empty()), "{d}");
    assert!(d["max_texture_dimension_2d"].as_u64().unwrap() >= 2048);
    assert!(d["max_buffer_size"].as_u64().unwrap() > 0);
    assert!(d["max_storage_buffer_binding_size"].as_u64().unwrap() > 0);
    assert_eq!(d["chunk_edge"], 192);
    // 520x300 with a 20 px halo in 192 px chunks: 3 columns x 2 rows.
    assert_eq!(i["gpu_effects"]["last_chunks"], 6, "{i}");
}
