#![cfg(windows)]

use op_gpu::GpuContext;
use op_ops::effect::GpuKernel;

#[test]
fn warp_sepia_zero_strength_preserves_integer_gray() {
    let mut gpu = GpuContext::new().unwrap();
    assert_eq!(gpu.info().backend, wgpu::Backend::Dx12);
    let source = [30u8, 100, 220, 0];
    let gray = ((7471u32 * 220 + 38470 * 100 + 19595 * 30) >> 16) as u8;
    let actual = gpu
        .effect_kernel_with_params(GpuKernel::Sepia, 1, 1, &source, [0, 0, 0])
        .unwrap();
    assert_eq!(actual, [gray, gray, gray, 0]);
}

#[test]
fn twist_matches_cpu_at_first_differing_pixel() {
    let mut gpu = GpuContext::new().unwrap();
    assert_eq!(gpu.info().backend, wgpu::Backend::Dx12);
    let (width, height) = (257u32, 19u32);
    let mut seed = 0x5182_9137u32;
    let source: Vec<u8> = (0..width * height * 4)
        .map(|_| {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            (seed >> 24) as u8
        })
        .collect();
    let actual = gpu
        .effect_kernel(GpuKernel::Twist, width, height, &source)
        .unwrap();
    let expected =
        op_ops::effect::reference_rgba8("effect.distort.twist", width, height, &source).unwrap();
    if let Some(i) = actual.iter().zip(&expected).position(|(a, b)| a != b) {
        let pixel = i / 4;
        let x = (pixel % width as usize) as f64;
        let y = (pixel / width as usize) as f64;
        let cx = width as f64 * 0.5;
        let cy = height as f64 * 0.5;
        let u = x - cx - 0.1;
        let v = y - cy - 0.3;
        let r = u.hypot(v);
        let t = (1.0 - r * 2.0 / height as f64).max(0.0);
        let cpu_angle = v.atan2(u) - 9.0 * t * t * t;
        let gpu_angle_estimate = (v as f32).atan2(u as f32) - 9.0_f32 * (t as f32).powi(3);
        eprintln!(
            "first twist mismatch at ({x},{y}) channel {} input {:?} GPU {:?} CPU {:?} angle CPU {cpu_angle:.12} f32 {gpu_angle_estimate:.12}",
            i % 4,
            &source[pixel * 4..pixel * 4 + 4],
            &actual[pixel * 4..pixel * 4 + 4],
            &expected[pixel * 4..pixel * 4 + 4]
        );
    }
    assert!(
        actual == expected,
        "Twist differs from CPU; see first pixel above"
    );
}

#[test]
fn four_warp_devices_create_and_run_together() {
    if std::env::var("OP_GPU_ADAPTER").as_deref() != Ok("warp") {
        return;
    }
    use std::sync::{Arc, Barrier};
    for _round in 0..8 {
        let barrier = Arc::new(Barrier::new(4));
        std::thread::scope(|scope| {
            let handles: Vec<_> = (0..4)
                .map(|_| {
                    let barrier = barrier.clone();
                    scope.spawn(move || {
                        barrier.wait();
                        let mut gpu = GpuContext::new().unwrap();
                        assert_eq!(gpu.info().backend, wgpu::Backend::Dx12);
                        assert!(
                            gpu.info()
                                .name
                                .to_ascii_lowercase()
                                .contains("basic render")
                        );
                        let result = gpu
                            .effect_kernel_with_params(
                                GpuKernel::Sepia,
                                1,
                                1,
                                &[30, 100, 220, 0],
                                [0, 0, 0],
                            )
                            .unwrap();
                        assert_eq!(result, [92, 92, 92, 0]);
                    })
                })
                .collect();
            for handle in handles {
                handle.join().unwrap();
            }
        });
    }
}

#[test]
fn warp_lock_survives_a_panicking_worker() {
    if std::env::var("OP_GPU_ADAPTER").as_deref() != Ok("warp") {
        return;
    }
    assert!(
        std::thread::spawn(|| {
            let _guard = op_gpu::windows_warp_guard().unwrap();
            panic!("deliberate WARP worker panic");
        })
        .join()
        .is_err()
    );
    let result = std::thread::spawn(|| {
        let mut gpu = GpuContext::new().unwrap();
        gpu.effect_kernel_with_params(GpuKernel::Sepia, 1, 1, &[30, 100, 220, 0], [0, 0, 0])
            .unwrap()
    })
    .join()
    .unwrap();
    assert_eq!(result, [92, 92, 92, 0]);
}

#[test]
fn gpu_wait_timeout_names_the_operation() {
    let error = op_gpu::GpuError::WaitTimeout("readback mapping");
    assert_eq!(
        error.to_string(),
        "GPU wait exceeded its deadline during readback mapping"
    );
}

#[test]
fn expensive_morphology_is_refused_before_gpu_dispatch() {
    let mut gpu = GpuContext::new().unwrap();
    let before = gpu.dispatch_count();
    let pixels = vec![0u8; 512 * 512 * 4];
    let result =
        gpu.effect_kernel_with_params(GpuKernel::Morphology, 512, 512, &pixels, [101, 101, 0]);
    assert!(matches!(
        result,
        Err(op_gpu::GpuError::JobTooLong("Morphology"))
    ));
    assert_eq!(gpu.dispatch_count(), before);
}

#[test]
fn large_median_is_refused_before_gpu_dispatch() {
    assert_eq!(
        op_gpu::windows_kernel_samples(GpuKernel::Median, [200, 50, 0]),
        Some(8 * 401 * 401)
    );
    assert!(op_gpu::windows_job_too_long(GpuKernel::Median, 128, 128, [200, 50, 0]).is_some());
    assert!(op_gpu::windows_job_too_long(GpuKernel::Median, 40, 30, [200, 50, 0]).is_none());
}

#[test]
fn ordinary_outline_is_allowed_on_a_four_megapixel_chunk() {
    assert_eq!(
        op_gpu::windows_kernel_samples(GpuKernel::Outline, [3, 50, 0]),
        Some(49)
    );
    assert!(op_gpu::windows_job_too_long(GpuKernel::Outline, 2048, 2048, [3, 50, 0]).is_none());
}

#[test]
fn every_neighborhood_kernel_has_a_sample_estimate() {
    for kernel in [
        GpuKernel::Gaussian,
        GpuKernel::Median,
        GpuKernel::Motion,
        GpuKernel::Radial,
        GpuKernel::Zoom,
        GpuKernel::Bokeh,
        GpuKernel::Sketch,
        GpuKernel::Square,
        GpuKernel::Surface,
        GpuKernel::Unfocus,
        GpuKernel::EdgeDetect,
        GpuKernel::Emboss,
        GpuKernel::Relief,
        GpuKernel::OilPainting,
        GpuKernel::Outline,
        GpuKernel::InkSketch,
        GpuKernel::PencilSketch,
        GpuKernel::Pixelate,
        GpuKernel::Morphology,
        GpuKernel::Sharpen,
        GpuKernel::ReduceNoise,
    ] {
        assert!(
            op_gpu::windows_kernel_samples(kernel, [3, 3, 0]).is_some(),
            "{kernel:?}"
        );
    }
}

#[test]
fn measure_short_neighborhood_jobs() {
    let mut gpu = GpuContext::new().unwrap();
    for (kernel, values, name) in [
        (GpuKernel::Outline, [3, 50, 0], "Outline"),
        (GpuKernel::Median, [5, 50, 0], "Median"),
        (GpuKernel::Unfocus, [5, 0, 0], "Unfocus"),
    ] {
        let (width, height) = if name == "Outline" {
            (512u32, 512u32)
        } else {
            (128u32, 128u32)
        };
        let pixels = vec![127u8; (width * height * 4) as usize];
        gpu.effect_kernel_with_params(kernel, width, height, &pixels, values)
            .unwrap();
        let start = std::time::Instant::now();
        gpu.effect_kernel_with_params(kernel, width, height, &pixels, values)
            .unwrap();
        let samples = op_gpu::windows_kernel_samples(kernel, values).unwrap()
            * u64::from(width)
            * u64::from(height);
        eprintln!(
            "throughput {name}: {samples} estimated loop iterations in {:.3} ms",
            start.elapsed().as_secs_f64() * 1000.0
        );
    }
}

/// One Vulkan-path process spun for nine minutes on 2026-11-02. One stack
/// sample put a thread in nvoglv64.dll DllMain during LdrShutdownThread,
/// looping over SwitchToThread and GetExitCodeThread. Another thread was in
/// LdrInitializeThunk with OPENGL32.dll on its stack. This fits the W1 hang,
/// but that hang was never sampled. DX12 must keep nvoglv64.dll and
/// vulkan-1.dll unloaded.
#[test]
fn the_opengl_and_vulkan_drivers_are_not_loaded() {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetModuleHandleW(name: *const u16) -> *mut std::ffi::c_void;
    }
    let gpu = GpuContext::new().unwrap();
    assert_eq!(gpu.info().backend, wgpu::Backend::Dx12);
    // opengl32.dll itself loads with the program; it does not load the driver.
    for name in ["nvoglv64.dll", "vulkan-1.dll"] {
        let wide: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
        let module = unsafe { GetModuleHandleW(wide.as_ptr()) };
        assert!(module.is_null(), "{name} is loaded");
    }
}

/// Twist's coordinates go to the GPU in 64 MB chunks.
#[test]
fn twist_matches_cpu_across_more_than_one_chunk() {
    let mut gpu = GpuContext::new().unwrap();
    let width = 1024u32;
    let height = 1024 + 16;
    let mut seed = 0x1234_5678u32;
    let source: Vec<u8> = (0..u64::from(width) * u64::from(height) * 4)
        .map(|_| {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            (seed >> 24) as u8
        })
        .collect();
    let actual = gpu
        .effect_kernel(GpuKernel::Twist, width, height, &source)
        .unwrap();
    let expected =
        op_ops::effect::reference_rgba8("effect.distort.twist", width, height, &source).unwrap();
    assert!(actual == expected, "Twist differs across a chunk boundary");
}

#[test]
fn warp_default_twist_completes_at_3000_by_2700() {
    if std::env::var("OP_GPU_ADAPTER").as_deref() != Ok("warp") {
        return;
    }
    let mut gpu = GpuContext::new().unwrap();
    let (width, height) = (3000u32, 2700u32);
    let source = vec![128u8; (width * height * 4) as usize];
    let start = std::time::Instant::now();
    let result = gpu
        .effect_kernel(GpuKernel::Twist, width, height, &source)
        .unwrap();
    assert_eq!(result.len(), source.len());
    eprintln!(
        "WARP Twist 3000x2700: {:.3} s",
        start.elapsed().as_secs_f64()
    );
}
