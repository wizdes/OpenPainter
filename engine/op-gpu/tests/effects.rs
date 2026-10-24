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
    for spec in effect::registered() {
        let Some(kernel) = spec.gpu else { continue };
        let before = gpu.dispatch_count();
        let actual = gpu.effect_kernel(kernel, width, height, &bytes).unwrap();
        assert_eq!(
            gpu.dispatch_count() - before,
            1,
            "{} dispatch count",
            spec.id
        );
        let mut expected = bytes.clone();
        for pixel in expected.chunks_exact_mut(4) {
            match kernel {
                GpuKernel::InvertColors => pixel[..3].iter_mut().for_each(|v| *v = 255 - *v),
                GpuKernel::InvertAlpha => pixel[3] = 255 - pixel[3],
                GpuKernel::BlackAndWhite => {
                    let value = ((7471 * u32::from(pixel[2])
                        + 38470 * u32::from(pixel[1])
                        + 19595 * u32::from(pixel[0]))
                        >> 16) as u8;
                    pixel[..3].fill(value);
                }
                GpuKernel::BrightnessContrast | GpuKernel::TemperatureTint => {}
                GpuKernel::Posterize => {
                    for channel in &mut pixel[..3] {
                        let value = u32::from(*channel);
                        let index = if value == 0 {
                            0
                        } else {
                            (value * 16 - 1) / 255
                        };
                        *channel = (255 * index / 15) as u8;
                    }
                }
                GpuKernel::Sepia => {
                    let gray = ((7471 * u32::from(pixel[2])
                        + 38470 * u32::from(pixel[1])
                        + 19595 * u32::from(pixel[0]))
                        >> 16) as u8;
                    pixel[0] = (255.0 * (f64::from(gray) / 255.0).powf(0.8)) as u8;
                    pixel[1] = gray;
                    pixel[2] = (255.0 * (f64::from(gray) / 255.0).powf(1.2)) as u8;
                }
            }
        }
        if kernel == GpuKernel::Sepia {
            assert!(
                actual
                    .iter()
                    .zip(&expected)
                    .all(|(a, b)| (i16::from(*a) - i16::from(*b)).abs() <= 1),
                "{}",
                spec.id
            );
        } else {
            assert_eq!(actual, expected, "{}", spec.id);
        }
    }
}

#[test]
fn configurable_adjustment_endpoints_run_on_metal() {
    let mut gpu = GpuContext::new().expect("GPU adapter");
    #[cfg(target_os = "macos")]
    assert_eq!(format!("{:?}", gpu.info().backend), "Metal");
    let source = [30u8, 100, 220, 0];
    let cases = [
        (
            GpuKernel::BrightnessContrast,
            [100, 0, 0],
            [130, 200, 255, 0],
        ),
        (GpuKernel::Posterize, [2, 2, 2], [0, 0, 255, 0]),
        (GpuKernel::TemperatureTint, [100, -100, 0], [30, 250, 70, 0]),
    ];
    for (kernel, params, expected) in cases {
        let before = gpu.dispatch_count();
        let actual = gpu
            .effect_kernel_with_params(kernel, 1, 1, &source, params)
            .unwrap();
        assert_eq!(gpu.dispatch_count() - before, 1);
        assert_eq!(actual, expected, "{kernel:?}");
    }
    let gray = ((7471u32 * 220 + 38470 * 100 + 19595 * 30) >> 16) as u8;
    let actual = gpu
        .effect_kernel_with_params(GpuKernel::Sepia, 1, 1, &source, [0, 0, 0])
        .unwrap();
    assert_eq!(actual, [gray, gray, gray, 0]);
}

#[test]
fn invalid_posterize_level_does_not_dispatch() {
    let mut gpu = GpuContext::new().expect("GPU adapter");
    let before = gpu.dispatch_count();
    let result =
        gpu.effect_kernel_with_params(GpuKernel::Posterize, 1, 1, &[17, 29, 41, 0], [0, 16, 16]);
    assert!(matches!(result, Err(op_gpu::GpuError::InvalidImage)));
    assert_eq!(gpu.dispatch_count(), before);
}

#[test]
fn configurable_adjustments_match_cpu_on_random_pixels() {
    let mut gpu = GpuContext::new().expect("GPU adapter");
    #[cfg(target_os = "macos")]
    assert_eq!(format!("{:?}", gpu.info().backend), "Metal");
    let (width, height) = (73, 17);
    let mut seed = 0x7182_9147u32;
    let source: Vec<u8> = (0..width * height * 4)
        .map(|_| {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            (seed >> 24) as u8
        })
        .collect();
    type Case = (&'static str, GpuKernel, [(&'static str, i64); 3], [i32; 3]);
    let cases: [Case; 6] = [
        (
            "effect.adjust.brightness_contrast",
            GpuKernel::BrightnessContrast,
            [("Brightness", -75), ("Contrast", 50), ("Contrast", 50)],
            [-75, 50, 0],
        ),
        (
            "effect.adjust.brightness_contrast",
            GpuKernel::BrightnessContrast,
            [("Brightness", 100), ("Contrast", -100), ("Contrast", -100)],
            [100, -100, 0],
        ),
        (
            "effect.adjust.posterize",
            GpuKernel::Posterize,
            [("RedLevels", 2), ("GreenLevels", 32), ("BlueLevels", 64)],
            [2, 32, 64],
        ),
        (
            "effect.adjust.sepia",
            GpuKernel::Sepia,
            [("Intensity", 0), ("Intensity", 0), ("Intensity", 0)],
            [0, 0, 0],
        ),
        (
            "effect.adjust.sepia",
            GpuKernel::Sepia,
            [("Intensity", 100), ("Intensity", 100), ("Intensity", 100)],
            [100, 0, 0],
        ),
        (
            "effect.adjust.temperature_tint",
            GpuKernel::TemperatureTint,
            [("Temperature", -100), ("Tint", 75), ("Tint", 75)],
            [-100, 75, 0],
        ),
    ];
    for (id, kernel, fields, values) in cases {
        let spec = effect::find(id).unwrap();
        let mut params = spec.default_params();
        for (key, value) in fields {
            params
                .as_object_mut()
                .unwrap()
                .insert(key.into(), value.into());
        }
        if kernel == GpuKernel::Posterize {
            params
                .as_object_mut()
                .unwrap()
                .insert("LinkLevels".into(), false.into());
        }
        let expected: Vec<u8> = source
            .chunks_exact(4)
            .flat_map(|pixel| {
                let original = [pixel[0], pixel[1], pixel[2], pixel[3]];
                (spec.render_cpu)(
                    &|_, _| original,
                    0,
                    0,
                    &effect::Prepared::default(),
                    &params,
                )
            })
            .collect();
        let before = gpu.dispatch_count();
        let actual = gpu
            .effect_kernel_with_params(kernel, width, height, &source, values)
            .unwrap();
        assert_eq!(gpu.dispatch_count() - before, 1);
        let tolerance = if kernel == GpuKernel::Sepia { 1i16 } else { 0 };
        for (index, (got, want)) in actual.iter().zip(expected.iter()).enumerate() {
            assert!(
                (i16::from(*got) - i16::from(*want)).abs() <= tolerance,
                "{id} byte {index}: {got} vs {want}"
            );
        }
    }
}
