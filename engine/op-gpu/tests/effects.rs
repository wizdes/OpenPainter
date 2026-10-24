use op_gpu::GpuContext;
use op_ops::effect::{self, GpuKernel};

#[test]
fn registered_effect_kernels_are_byte_exact_and_dispatch() {
    let mut gpu = GpuContext::new().expect("GPU adapter");
    #[cfg(target_os = "macos")]
    assert_eq!(format!("{:?}", gpu.info().backend), "Metal");
    let (width, height) = (257, 19);
    let mut seed = 0x5182_9137u32;
    let bytes: Vec<u8> = (0..width * height * 4)
        .map(|_| {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            (seed >> 24) as u8
        })
        .collect();
    for spec in effect::registered()
        .into_iter()
        .filter(|spec| spec.gpu.is_some())
    {
        let kernel = spec.gpu.expect("registered effect needs GPU kernel");
        let before = gpu.dispatch_count();
        let actual = gpu.effect_kernel(kernel, width, height, &bytes).unwrap();
        if kernel == GpuKernel::Gaussian {
            assert_eq!(
                gpu.dispatch_count() - before,
                2,
                "{} dispatch count",
                spec.id
            );
        } else {
            assert_eq!(
                gpu.dispatch_count() - before,
                1,
                "{} dispatch count",
                spec.id
            );
        }
        let mut expected = bytes.clone();
        for pixel in expected.chunks_exact_mut(4) {
            match kernel {
                GpuKernel::InvertColors => pixel[..3].iter_mut().for_each(|v| *v = 255 - *v),
                GpuKernel::InvertAlpha => pixel[3] = 255 - pixel[3],
                GpuKernel::Gaussian => break,
                GpuKernel::Fragment
                | GpuKernel::Median
                | GpuKernel::Motion
                | GpuKernel::Radial
                | GpuKernel::Zoom => break,
            }
        }
        if !matches!(kernel, GpuKernel::InvertColors | GpuKernel::InvertAlpha) {
            expected = op_ops::effect::reference_rgba8(spec.id, width, height, &bytes).unwrap();
        }
        if !matches!(kernel, GpuKernel::InvertColors | GpuKernel::InvertAlpha) {
            let tolerance = if matches!(kernel, GpuKernel::Gaussian | GpuKernel::Motion) {
                1
            } else {
                0
            };
            for (a, b) in actual.iter().zip(&expected) {
                assert!(a.abs_diff(*b) <= tolerance, "{}: {} vs {}", spec.id, a, b);
            }
        } else {
            assert_eq!(actual, expected, "{}", spec.id);
        }
    }
}

#[test]
fn gaussian_radius_50_gpu_matches_cpu_within_one_level() {
    let mut gpu = GpuContext::new().expect("GPU adapter");
    #[cfg(target_os = "macos")]
    assert_eq!(format!("{:?}", gpu.info().backend), "Metal");
    let (width, height) = (65, 67);
    let mut seed = 0x8472_1293u32;
    let bytes: Vec<u8> = (0..width * height * 4)
        .map(|_| {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            (seed >> 24) as u8
        })
        .collect();
    let expected =
        op_ops::effects::gaussian_blur::reference_rgba8_with_radius(width, height, &bytes, 50);
    let before = gpu.dispatch_count();
    let (actual, timing) = gpu.gaussian_timed(width, height, &bytes, 50).unwrap();
    assert_eq!(gpu.dispatch_count() - before, 2);
    for (index, (a, b)) in actual.iter().zip(&expected).enumerate() {
        assert!(a.abs_diff(*b) <= 1, "byte {index}: {a} vs {b}");
    }
    println!(
        "Gaussian radius 50 small-image full {:.3} ms kernel {:.3} ms",
        timing.full_ms, timing.kernel_ms
    );
}

#[test]
#[ignore = "manual 24 MP Metal performance gate"]
fn gaussian_radius_50_24mp_metal_timing() {
    let mut gpu = GpuContext::new().expect("GPU adapter");
    assert_eq!(format!("{:?}", gpu.info().backend), "Metal");
    let (width, height) = (6000, 4000);
    let mut bytes = vec![0u8; width as usize * height as usize * 4];
    for y in 0..height {
        for x in 0..width {
            let i = (y as usize * width as usize + x as usize) * 4;
            bytes[i..i + 4].copy_from_slice(&[
                (x % 256) as u8,
                (y % 256) as u8,
                ((x + y) % 256) as u8,
                ((x * 3 + y) % 256) as u8,
            ]);
        }
    }
    let (_, warm) = gpu.gaussian_timed(1, 1, &[7, 13, 29, 127], 50).unwrap();
    println!("warm-up full {:.3} ms", warm.full_ms);
    let (output, timing) = gpu.gaussian_timed(width, height, &bytes, 50).unwrap();
    assert_eq!(output.len(), bytes.len());
    println!(
        "Gaussian Metal 6000x4000 radius=50 full {:.3} ms upload {:.3} ms kernel dispatch+wait {:.3} ms readback {:.3} ms",
        timing.full_ms, timing.upload_ms, timing.kernel_ms, timing.readback_ms
    );
    assert!(
        timing.full_ms < 1000.0,
        "24 MP Gaussian full operation {:.3} ms exceeds 1 s",
        timing.full_ms
    );
}

#[test]
fn gaussian_gpu_radius_endpoints_match_cpu() {
    let mut gpu = GpuContext::new().expect("GPU adapter");
    let (width, height) = (9, 9);
    let bytes: Vec<u8> = (0..width * height)
        .flat_map(|i| {
            [
                ((i * 13) % 256) as u8,
                ((i * 31) % 256) as u8,
                ((i * 47) % 256) as u8,
                ((i * 17) % 256) as u8,
            ]
        })
        .collect();
    let (identity, _) = gpu.gaussian_timed(width, height, &bytes, 0).unwrap();
    assert_eq!(identity, bytes);
    let (actual, _) = gpu.gaussian_timed(width, height, &bytes, 200).unwrap();
    let expected =
        op_ops::effects::gaussian_blur::reference_rgba8_with_radius(width, height, &bytes, 200);
    for (a, b) in actual.iter().zip(&expected) {
        assert!(a.abs_diff(*b) <= 1, "{a} vs {b}");
    }
}
