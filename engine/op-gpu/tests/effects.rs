use op_gpu::{GpuContext, NoiseParams};
use op_ops::effect::{self, GpuKernel};

#[test]
fn m48_gpu_parameter_endpoints_match_cpu_on_metal() {
    let mut gpu = GpuContext::new().expect("GPU adapter");
    #[cfg(target_os = "macos")]
    assert_eq!(format!("{:?}", gpu.info().backend), "Metal");
    let (width, height) = (5u32, 3u32);
    let bytes: Vec<u8> = (0..width * height)
        .flat_map(|i| {
            [
                (i * 17) as u8,
                (255 - i * 9) as u8,
                (i * 13) as u8,
                if i % 3 == 0 {
                    0
                } else if i % 3 == 1 {
                    113
                } else {
                    255
                },
            ]
        })
        .collect();
    for (id, values, params) in [
        (
            "effect.blur.bokeh",
            [200, 100, 10],
            serde_json::json!({"radius":200,"gamma_boost":100,"quality":10}),
        ),
        (
            "effect.blur.sketch",
            [200, 100, 10],
            serde_json::json!({"radius":200,"percentile":100,"smoothness":10}),
        ),
        (
            "effect.blur.square",
            [200, -100, 0],
            serde_json::json!({"radius":200,"gamma_boost":-100}),
        ),
        (
            "effect.blur.surface",
            [100, 100, 0],
            serde_json::json!({"radius":100,"threshold":100}),
        ),
        (
            "effect.blur.unfocus",
            [200, 0, 0],
            serde_json::json!({"radius":200}),
        ),
    ] {
        let spec = effect::find(id).unwrap();
        let kernel = spec.gpu.unwrap();
        let before = gpu.dispatch_count();
        let actual = gpu
            .effect_kernel_with_params(kernel, width, height, &bytes, values)
            .unwrap();
        assert!(gpu.dispatch_count() > before, "{id} dispatched");
        let prep = effect::Prepared {
            width: width as i32,
            height: height as i32,
            ..Default::default()
        };
        let sample = |x: i32, y: i32| {
            let i = (y as usize * width as usize + x as usize) * 4;
            bytes[i..i + 4].try_into().unwrap()
        };
        for y in 0..height as i32 {
            for x in 0..width as i32 {
                let i = (y as usize * width as usize + x as usize) * 4;
                let expected = (spec.render_cpu)(&sample, x, y, &prep, &params);
                for channel in 0..4 {
                    assert!(
                        actual[i + channel].abs_diff(expected[channel]) <= 1,
                        "{id} {x},{y} channel {channel}: {} vs {}",
                        actual[i + channel],
                        expected[channel]
                    );
                }
            }
        }
    }
}

struct NoSpill;
impl op_core::tiles::SpillIo for NoSpill {
    fn append(&self, _: &[u8]) -> std::io::Result<u64> {
        Err(std::io::Error::other("unexpected spill"))
    }
    fn read_exact_at(&self, _: u64, _: &mut [u8]) -> std::io::Result<()> {
        Err(std::io::Error::other("unexpected spill"))
    }
}

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
        if kernel == GpuKernel::Gaussian {
            assert_eq!(
                gpu.dispatch_count() - before,
                2,
                "{} dispatch count",
                spec.id
            );
        } else if matches!(kernel, GpuKernel::Glow | GpuKernel::SoftenPortrait) {
            assert_eq!(
                gpu.dispatch_count() - before,
                4,
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
        let blur = matches!(
            kernel,
            GpuKernel::Gaussian
                | GpuKernel::Bokeh
                | GpuKernel::Sketch
                | GpuKernel::Square
                | GpuKernel::Surface
                | GpuKernel::Unfocus
                | GpuKernel::Fragment
                | GpuKernel::Median
                | GpuKernel::Motion
                | GpuKernel::Radial
                | GpuKernel::Zoom
                | GpuKernel::EdgeDetect
                | GpuKernel::Emboss
                | GpuKernel::Relief
                | GpuKernel::OilPainting
                | GpuKernel::Outline
                | GpuKernel::InkSketch
                | GpuKernel::PencilSketch
                | GpuKernel::Pixelate
                | GpuKernel::Bulge
                | GpuKernel::Twist
                | GpuKernel::Vignette
                | GpuKernel::Sharpen
                | GpuKernel::ReduceNoise
                | GpuKernel::AddNoise
                | GpuKernel::Glow
                | GpuKernel::SoftenPortrait
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
                GpuKernel::BrightnessContrast
                | GpuKernel::TemperatureTint
                | GpuKernel::HighlightsShadows
                | GpuKernel::Curves
                | GpuKernel::Levels
                | GpuKernel::Exposure => {}
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
                GpuKernel::RedEyeRemoval => {
                    let hi = *pixel[..3].iter().max().unwrap();
                    let lo = *pixel[..3].iter().min().unwrap();
                    if hi > 0
                        && u32::from(hi - lo) * 255 / u32::from(hi) > 100
                        && i32::from(pixel[0]) - i32::from(pixel[1].max(pixel[2])) > 70
                    {
                        pixel[0] = ((299 * u32::from(pixel[0])
                            + 587 * u32::from(pixel[1])
                            + 114 * u32::from(pixel[2]))
                            * 90
                            / 100000) as u8;
                    }
                }
                GpuKernel::Gaussian => break,
                GpuKernel::Fragment
                | GpuKernel::Median
                | GpuKernel::Motion
                | GpuKernel::Radial
                | GpuKernel::Zoom
                | GpuKernel::Bokeh
                | GpuKernel::Sketch
                | GpuKernel::Square
                | GpuKernel::Surface
                | GpuKernel::Unfocus
                | GpuKernel::Pixelate
                | GpuKernel::Bulge
                | GpuKernel::Twist
                | GpuKernel::Vignette => break,
                GpuKernel::EdgeDetect
                | GpuKernel::Emboss
                | GpuKernel::Relief
                | GpuKernel::OilPainting
                | GpuKernel::Outline
                | GpuKernel::InkSketch
                | GpuKernel::PencilSketch => break,
                GpuKernel::Sharpen => break,
                GpuKernel::ReduceNoise => break,
                GpuKernel::AddNoise => break,
                GpuKernel::Glow | GpuKernel::SoftenPortrait => break,
            }
        }
        if blur {
            expected = if kernel == GpuKernel::AddNoise {
                op_ops::effects::add_noise::reference_rgba8(width, height, &bytes)
            } else if kernel == GpuKernel::Glow {
                op_ops::effects::glow::reference_rgba8(width, height, &bytes, 6, 10, 10)
            } else if kernel == GpuKernel::SoftenPortrait {
                op_ops::effects::soften_portrait::reference_rgba8(width, height, &bytes, 5, 0, 10)
            } else {
                op_ops::effect::reference_rgba8(spec.id, width, height, &bytes).unwrap()
            };
        }
        if blur {
            let tolerance = if matches!(
                kernel,
                GpuKernel::Gaussian
                    | GpuKernel::Motion
                    | GpuKernel::EdgeDetect
                    | GpuKernel::Emboss
                    | GpuKernel::Relief
                    | GpuKernel::Vignette
                    | GpuKernel::ReduceNoise
                    | GpuKernel::Glow
                    | GpuKernel::SoftenPortrait
            ) {
                1
            } else {
                0
            };
            for (a, b) in actual.iter().zip(&expected) {
                assert!(a.abs_diff(*b) <= tolerance, "{}: {} vs {}", spec.id, a, b);
            }
        } else if kernel == GpuKernel::Sepia {
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
fn m40_artistic_gpu_parameter_endpoints_match_cpu() {
    let mut gpu = GpuContext::new().expect("GPU adapter");
    let (width, height) = (5u32, 3u32);
    let bytes: Vec<u8> = (0..width * height)
        .flat_map(|i| {
            [
                (i * 53) as u8,
                (i * 71) as u8,
                (i * 19) as u8,
                ((i % 4) * 85) as u8,
            ]
        })
        .collect();
    for (id, kernel, key1, key2, endpoints) in [
        (
            "effect.artistic.ink_sketch",
            GpuKernel::InkSketch,
            "ink_outline",
            "coloring",
            [[0, 0], [99, 100]],
        ),
        (
            "effect.artistic.oil_painting",
            GpuKernel::OilPainting,
            "brush_size",
            "coarseness",
            [[1, 3], [8, 255]],
        ),
        (
            "effect.artistic.pencil_sketch",
            GpuKernel::PencilSketch,
            "pencil_tip_size",
            "range",
            [[1, -20], [20, 20]],
        ),
        (
            "effect.stylize.outline",
            GpuKernel::Outline,
            "thickness",
            "intensity",
            [[1, 0], [200, 100]],
        ),
    ] {
        let spec = effect::find(id).unwrap();
        for values in endpoints {
            let mut params = spec.default_params();
            params[key1] = serde_json::json!(values[0]);
            params[key2] = serde_json::json!(values[1]);
            let prep = effect::Prepared {
                width: width as i32,
                height: height as i32,
                ..effect::Prepared::default()
            };
            let sample = |x: i32, y: i32| -> [u8; 4] {
                if x < 0 || y < 0 || x >= width as i32 || y >= height as i32 {
                    return [0; 4];
                }
                let i = (y as usize * width as usize + x as usize) * 4;
                bytes[i..i + 4].try_into().unwrap()
            };
            let mut expected = Vec::new();
            for y in 0..height as i32 {
                for x in 0..width as i32 {
                    expected.extend_from_slice(&(spec.render_cpu)(&sample, x, y, &prep, &params));
                }
            }
            let actual = gpu
                .effect_kernel_with_params(kernel, width, height, &bytes, [values[0], values[1], 0])
                .unwrap();
            assert_eq!(actual, expected, "{id} {values:?}");
        }
    }
}

#[test]
fn m40_stylize_gpu_angle_endpoints_match_cpu() {
    let mut gpu = GpuContext::new().expect("GPU adapter");
    let (width, height) = (5u32, 3u32);
    let bytes: Vec<u8> = (0..width * height)
        .flat_map(|i| {
            [
                (i * 53) as u8,
                (i * 71) as u8,
                (i * 19) as u8,
                ((i % 4) * 85) as u8,
            ]
        })
        .collect();
    let sample = |x: i32, y: i32| -> [u8; 4] {
        if x < 0 || y < 0 || x >= width as i32 || y >= height as i32 {
            return [0; 4];
        }
        let i = (y as usize * width as usize + x as usize) * 4;
        bytes[i..i + 4].try_into().unwrap()
    };
    for (id, kernel) in [
        ("effect.stylize.edge_detect", GpuKernel::EdgeDetect),
        ("effect.stylize.emboss", GpuKernel::Emboss),
        ("effect.stylize.relief", GpuKernel::Relief),
    ] {
        let spec = effect::find(id).unwrap();
        for angle in [-180, 0, 45, 180] {
            let mut params = spec.default_params();
            params["angle"] = serde_json::json!(angle as f64);
            let prep = effect::Prepared {
                width: width as i32,
                height: height as i32,
                ..effect::Prepared::default()
            };
            let mut expected = Vec::new();
            for y in 0..height as i32 {
                for x in 0..width as i32 {
                    expected.extend_from_slice(&(spec.render_cpu)(&sample, x, y, &prep, &params));
                }
            }
            let actual = gpu
                .effect_kernel_with_params(kernel, width, height, &bytes, [angle, 0, 0])
                .unwrap();
            assert!(
                actual.iter().zip(expected).all(|(a, b)| a.abs_diff(b) <= 1),
                "{id} {angle}"
            );
        }
    }
}

#[test]
fn m41_gpu_photo_parameter_endpoints_match_cpu() {
    let mut gpu = GpuContext::new().expect("GPU adapter");
    let (width, height) = (3u32, 3u32);
    let bytes = vec![
        240, 30, 10, 117, 80, 60, 55, 0, 12, 90, 200, 255, 255, 26, 13, 0, 21, 40, 68, 128, 210,
        50, 8, 255, 99, 22, 10, 34, 40, 150, 30, 200, 4, 5, 6, 0,
    ];
    let sample = |x: i32, y: i32| {
        let i = (y as usize * width as usize + x as usize) * 4;
        bytes[i..i + 4].try_into().unwrap()
    };
    let prep = op_ops::effect::Prepared {
        width: width as i32,
        height: height as i32,
        ..Default::default()
    };
    for (kernel, id, values, params) in [
        (
            GpuKernel::RedEyeRemoval,
            "effect.photo.red_eye_removal",
            [0, 0, 0],
            serde_json::json!({"tolerance":0,"saturation":0}),
        ),
        (
            GpuKernel::RedEyeRemoval,
            "effect.photo.red_eye_removal",
            [100, 100, 0],
            serde_json::json!({"tolerance":100,"saturation":100}),
        ),
        (
            GpuKernel::Vignette,
            "effect.photo.vignette",
            [0, 0, 0],
            serde_json::json!({"strength":0,"radius":0}),
        ),
        (
            GpuKernel::Vignette,
            "effect.photo.vignette",
            [1000, -1000, (100 << 16) | 100],
            serde_json::json!({"center_x":1.0,"center_y":-1.0,"radius":100,"strength":100}),
        ),
        (
            GpuKernel::Sharpen,
            "effect.photo.sharpen",
            [1, 0, 0],
            serde_json::json!({"amount":1}),
        ),
        (
            GpuKernel::Sharpen,
            "effect.photo.sharpen",
            [20, 0, 0],
            serde_json::json!({"amount":20}),
        ),
        (
            GpuKernel::ReduceNoise,
            "effect.noise.reduce",
            [0, 0, 0],
            serde_json::json!({"radius":0,"strength":0.0}),
        ),
        (
            GpuKernel::ReduceNoise,
            "effect.noise.reduce",
            [200, 1000, 0],
            serde_json::json!({"radius":200,"strength":1.0}),
        ),
    ] {
        let spec = effect::find(id).unwrap();
        let params = spec.schema.validate(params).unwrap();
        let before = gpu.dispatch_count();
        let actual = gpu
            .effect_kernel_with_params(kernel, width, height, &bytes, values)
            .unwrap();
        assert!(gpu.dispatch_count() > before);
        for y in 0..height as i32 {
            for x in 0..width as i32 {
                let expected = (spec.render_cpu)(&sample, x, y, &prep, &params);
                let i = (y as usize * width as usize + x as usize) * 4;
                for channel in 0..4 {
                    assert!(
                        actual[i + channel].abs_diff(expected[channel]) <= 1,
                        "{id} {x},{y} channel {channel}"
                    );
                }
            }
        }
    }
}

#[test]
fn m41_gpu_seeded_noise_matches_cpu_for_endpoints_and_hidden_color() {
    let mut gpu = GpuContext::new().expect("GPU adapter");
    let (width, height) = (8u32, 8u32);
    let bytes: Vec<u8> = (0..width * height)
        .flat_map(|i| [i as u8 * 3, i as u8 * 2, 91, (i * 4) as u8])
        .collect();
    for (intensity, saturation, coverage, seed) in [
        (0, 0, 0.0, 0),
        (64, 100, 100.0, 0x51ed270b),
        (100, 400, 100.0, 17),
    ] {
        let before = gpu.dispatch_count();
        let actual = gpu
            .effect_add_noise(
                width,
                height,
                &bytes,
                NoiseParams {
                    intensity,
                    saturation,
                    coverage,
                    seed,
                },
            )
            .unwrap();
        assert!(gpu.dispatch_count() > before);
        let deltas = op_ops::effects::add_noise::delta_map(
            width, height, intensity, saturation, coverage, seed,
        );
        let mut expected = bytes.clone();
        for (pixel, delta) in expected.chunks_exact_mut(4).zip(deltas) {
            for channel in 0..3 {
                pixel[channel] = (i32::from(pixel[channel]) + delta[channel]).clamp(0, 255) as u8;
            }
        }
        assert_eq!(actual, expected);
    }
}

#[test]
fn m41_gpu_composed_photo_endpoints_match_cpu() {
    let mut gpu = GpuContext::new().expect("GPU adapter");
    let (width, height) = (3u32, 3u32);
    let bytes = vec![
        240, 30, 10, 117, 80, 60, 55, 0, 12, 90, 200, 255, 255, 26, 13, 0, 21, 40, 68, 128, 210,
        50, 8, 255, 99, 22, 10, 34, 40, 150, 30, 200, 4, 5, 6, 0,
    ];
    for (radius, brightness, contrast) in [(1, -100, -100), (20, 100, 100)] {
        let before = gpu.dispatch_count();
        let actual = gpu
            .effect_photo_glow(width, height, &bytes, radius, brightness, contrast)
            .unwrap();
        assert!(gpu.dispatch_count() > before);
        let expected = op_ops::effects::glow::reference_rgba8(
            width, height, &bytes, radius, brightness, contrast,
        );
        for (a, b) in actual.iter().zip(expected) {
            assert!(a.abs_diff(b) <= 1, "Glow {radius}: {a} vs {b}");
        }
    }
    for (softness, lighting, warmth) in [(0, -20, 0), (10, 20, 20)] {
        let before = gpu.dispatch_count();
        let actual = gpu
            .effect_photo_soften(width, height, &bytes, softness, lighting, warmth)
            .unwrap();
        assert!(gpu.dispatch_count() > before);
        let expected = op_ops::effects::soften_portrait::reference_rgba8(
            width, height, &bytes, softness, lighting, warmth,
        );
        for (a, b) in actual.iter().zip(expected) {
            assert!(a.abs_diff(b) <= 1, "Soften Portrait {softness}: {a} vs {b}");
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

#[test]
fn m26_lut_kernels_match_tables_in_both_curve_modes() {
    let mut gpu = GpuContext::new().expect("GPU adapter");
    let (width, height) = (257, 3);
    let bytes: Vec<u8> = (0..width * height)
        .flat_map(|i| [i as u8, (i * 3) as u8, (i * 7) as u8, (i * 11) as u8])
        .collect();
    let lut: [[u8; 256]; 3] = std::array::from_fn(|channel| {
        std::array::from_fn(|v| match channel {
            0 => 255 - v as u8,
            1 => ((v * 3) % 256) as u8,
            _ => v as u8,
        })
    });
    for kernel in [GpuKernel::Curves, GpuKernel::Levels, GpuKernel::Exposure] {
        for luminosity in [false, true] {
            let before = gpu.dispatch_count();
            let actual = gpu
                .effect_kernel_with_lut(kernel, width, height, &bytes, &lut, luminosity)
                .unwrap();
            assert_eq!(gpu.dispatch_count(), before + 1);
            let mut expected = bytes.clone();
            for pixel in expected.chunks_exact_mut(4) {
                if luminosity {
                    let old = ((7471 * u32::from(pixel[2])
                        + 38470 * u32::from(pixel[1])
                        + 19595 * u32::from(pixel[0]))
                        >> 16) as u8;
                    let diff = i16::from(lut[0][old as usize]) - i16::from(old);
                    for channel in &mut pixel[..3] {
                        *channel = (i16::from(*channel) + diff).clamp(0, 255) as u8;
                    }
                } else {
                    for channel in 0..3 {
                        pixel[channel] = lut[channel][pixel[channel] as usize];
                    }
                }
            }
            assert_eq!(actual, expected, "{kernel:?} luminosity={luminosity}");
        }
    }
}

#[test]
fn m26_highlights_shadows_gpu_matches_cpu() {
    let mut gpu = GpuContext::new().expect("GPU adapter");
    let spec = effect::find("effect.adjust.highlights_shadows").unwrap();
    let bytes: Vec<u8> = (0..513u32)
        .flat_map(|i| [i as u8, (i * 17) as u8, (i * 37) as u8, (i * 97) as u8])
        .collect();
    for (highlights, shadows) in [(-100, 100), (50, -75), (100, 100)] {
        let mut params = spec.default_params();
        params
            .as_object_mut()
            .unwrap()
            .insert("Highlights".into(), highlights.into());
        params
            .as_object_mut()
            .unwrap()
            .insert("Shadows".into(), shadows.into());
        let expected: Vec<u8> = bytes
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
        let actual = gpu
            .effect_kernel_with_params(
                GpuKernel::HighlightsShadows,
                513,
                1,
                &bytes,
                [highlights, shadows, 0],
            )
            .unwrap();
        assert_eq!(actual, expected);
    }
}

#[test]
fn m26_prepared_tables_match_cpu_on_metal() {
    use op_core::doc::{DocState, SelectionState};
    use op_core::tiles::{TILE_BYTES, TILE_SIZE, TileCoord, TileData, TileStore};
    use std::sync::Arc;
    let mut gpu = GpuContext::new().expect("GPU adapter");
    let width = 257u32;
    let bytes: Vec<u8> = (0..width)
        .flat_map(|i| [i as u8, (i * 7) as u8, (i * 19) as u8, (i * 71) as u8])
        .collect();
    let store = Arc::new(TileStore::with_io(4 * TILE_BYTES, Box::new(NoSpill)));
    let mut doc = DocState::new(store, width, 1, 96.0).unwrap();
    for coord in effect::tile_coords(effect::Rect::new(0, 0, width, 1)) {
        let mut tile = vec![0; TILE_BYTES];
        for x in 0..TILE_SIZE {
            let gx = coord.x as usize * TILE_SIZE + x;
            if gx >= width as usize {
                break;
            }
            tile[x * 4..x * 4 + 4].copy_from_slice(&bytes[gx * 4..gx * 4 + 4]);
        }
        doc = doc
            .write_tile(
                0,
                TileCoord { x: coord.x, y: 0 },
                TileData::try_from(tile).unwrap(),
            )
            .unwrap();
    }
    let cases = [
        (
            "effect.adjust.curves",
            GpuKernel::Curves,
            "Luminosity",
            vec![vec![[0, 0], [64, 130], [255, 255]]],
        ),
        (
            "effect.adjust.curves",
            GpuKernel::Curves,
            "Rgb",
            vec![
                vec![[0, 255], [255, 0]],
                vec![[0, 0], [255, 255]],
                vec![[0, 0], [255, 255]],
            ],
        ),
    ];
    for (id, kernel, mode, points) in cases {
        let spec = effect::find(id).unwrap();
        let mut params = spec.default_params();
        params
            .as_object_mut()
            .unwrap()
            .insert("ColorTransferMode".into(), mode.into());
        let points: Vec<Vec<Vec<i32>>> = points
            .into_iter()
            .map(|channel| channel.into_iter().map(|point| point.to_vec()).collect())
            .collect();
        params
            .as_object_mut()
            .unwrap()
            .insert("ControlPoints".into(), points.into());
        let prepared = (spec.prepare)(&doc, &SelectionState::None, &params).unwrap();
        let lut = prepared.channel_lut.as_ref().unwrap();
        let luminosity = params["ColorTransferMode"] == "Luminosity";
        let actual = gpu
            .effect_kernel_with_lut(kernel, width, 1, &bytes, lut, luminosity)
            .unwrap();
        let expected: Vec<u8> = bytes
            .chunks_exact(4)
            .flat_map(|pixel| {
                let source = [pixel[0], pixel[1], pixel[2], pixel[3]];
                (spec.render_cpu)(&|_, _| source, 0, 0, &prepared, &params)
            })
            .collect();
        assert_eq!(actual, expected, "{id} mode={luminosity}");
    }
    for (id, kernel, variant) in [
        ("effect.adjust.levels", GpuKernel::Levels, "gamma"),
        ("effect.adjust.levels", GpuKernel::Levels, "auto"),
        ("effect.adjust.exposure", GpuKernel::Exposure, "stops"),
    ] {
        let spec = effect::find(id).unwrap();
        let mut params = spec.default_params();
        match variant {
            "gamma" => {
                params
                    .as_object_mut()
                    .unwrap()
                    .insert("Gamma".into(), vec![0.5, 2.0, 5.0].into());
            }
            "auto" => {
                params
                    .as_object_mut()
                    .unwrap()
                    .insert("Auto".into(), true.into());
            }
            _ => {
                params
                    .as_object_mut()
                    .unwrap()
                    .insert("Exposure".into(), 2.0.into());
            }
        }
        let prepared = (spec.prepare)(&doc, &SelectionState::None, &params).unwrap();
        let lut = prepared.channel_lut.as_ref().unwrap();
        let actual = gpu
            .effect_kernel_with_lut(kernel, width, 1, &bytes, lut, false)
            .unwrap();
        let expected: Vec<u8> = bytes
            .chunks_exact(4)
            .flat_map(|pixel| {
                let source = [pixel[0], pixel[1], pixel[2], pixel[3]];
                (spec.render_cpu)(&|_, _| source, 0, 0, &prepared, &params)
            })
            .collect();
        assert_eq!(actual, expected, "{id} {variant}");
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

#[test]
fn pixelate_cell_size_endpoints_match_cpu_on_metal() {
    let mut gpu = GpuContext::new().expect("GPU adapter");
    #[cfg(target_os = "macos")]
    assert_eq!(format!("{:?}", gpu.info().backend), "Metal");
    let (width, height) = (257, 3);
    let bytes: Vec<u8> = (0..width * height)
        .flat_map(|i| {
            let x = i % width;
            [
                x as u8,
                (i / width) as u8,
                149,
                if x == 256 { 0 } else { 193 },
            ]
        })
        .collect();
    let spec = effect::find("effect.distort.pixelate").unwrap();
    for size in [1, 2, 100] {
        let mut params = spec.default_params();
        params["cell_size"] = size.into();
        let prep = effect::Prepared {
            width: width as i32,
            height: height as i32,
            ..Default::default()
        };
        let sample = |x: i32, y: i32| {
            let i = (y as usize * width as usize + x as usize) * 4;
            bytes[i..i + 4].try_into().unwrap()
        };
        let mut expected = Vec::with_capacity(bytes.len());
        for y in 0..height as i32 {
            for x in 0..width as i32 {
                expected.extend_from_slice(&(spec.render_cpu)(&sample, x, y, &prep, &params));
            }
        }
        let before = gpu.dispatch_count();
        let actual = gpu
            .effect_kernel_with_params(GpuKernel::Pixelate, width, height, &bytes, [size, 0, 0])
            .unwrap();
        assert_eq!(gpu.dispatch_count() - before, 1);
        assert_eq!(actual, expected, "cell size {size}");
    }
}
