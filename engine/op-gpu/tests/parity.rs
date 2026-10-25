use op_core::doc::{LayerBlendMode, ToolCompositeMode};
use op_gpu::{GpuContext, LayerInput, MODES};
use op_ops::composite::{composite_pixel, tool_composite};
use op_testkit::{RgbaImage, Tolerance, compare};

fn gpu() -> Option<GpuContext> {
    if std::env::var("OP_GPU_TESTS").as_deref() == Ok("0") {
        println!("SKIP OP_GPU_TESTS=0");
        return None;
    }
    let gpu = GpuContext::new().expect("GPU adapter");
    println!("GpuInfo: {:?}", gpu.info());
    Some(gpu)
}
fn check(width: u32, height: u32, expected: Vec<u8>, actual: Vec<u8>) -> u8 {
    let comparison = compare(
        &RgbaImage {
            width,
            height,
            data: expected.clone(),
        },
        &RgbaImage {
            width,
            height,
            data: actual.clone(),
        },
        Tolerance::default(),
    );
    if let Some(&(x, y)) = comparison.first_failures.first() {
        let i = (y * width + x) as usize * 4;
        eprintln!(
            "first mismatch at {x},{y}: expected={:?} actual={:?}",
            &expected[i..i + 4],
            &actual[i..i + 4]
        );
    }
    assert!(comparison.passed, "{comparison}");
    comparison.max_delta
}
#[test]
fn composite_small() {
    let Some(mut gpu) = gpu() else {
        return;
    };
    let bottom = [17, 91, 212, 100].repeat(64);
    let top = [225, 31, 78, 145].repeat(64);
    let before = gpu.dispatch_count();
    let actual = gpu
        .composite(
            8,
            8,
            &[
                LayerInput {
                    bytes: &bottom,
                    mode: LayerBlendMode::Normal,
                    opacity: 255,
                },
                LayerInput {
                    bytes: &top,
                    mode: LayerBlendMode::Overlay,
                    opacity: 128,
                },
            ],
        )
        .unwrap();
    let expected: Vec<u8> = std::iter::repeat_n(
        <[u8; 4]>::from(composite_pixel(
            [17, 91, 212, 100].into(),
            [225, 31, 78, 145].into(),
            LayerBlendMode::Overlay,
            128,
        )),
        64,
    )
    .flatten()
    .collect();
    check(8, 8, expected, actual);
    assert!(gpu.dispatch_count() > before);
}
#[test]
fn tool_modes() {
    let Some(mut gpu) = gpu() else {
        return;
    };
    let dst = [17, 91, 212, 100].repeat(64);
    let src = [225, 31, 78, 145].repeat(64);
    for mode in [ToolCompositeMode::Normal, ToolCompositeMode::Overwrite] {
        let actual = gpu.tool_composite(8, 8, &dst, &src, mode, 128).unwrap();
        let expected = <[u8; 4]>::from(tool_composite(
            [17, 91, 212, 100].into(),
            [225, 31, 78, 145].into(),
            mode,
            128,
        ))
        .repeat(64);
        check(8, 8, expected, actual);
    }
}

#[test]
fn registered_ops() {
    let Some(mut gpu) = gpu() else {
        return;
    };
    let width = 64;
    let height = 64;
    let mut seed = 0x1827a31bu32;
    let mut make = || -> Vec<u8> {
        (0..width * height * 4)
            .map(|_| {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                (seed >> 24) as u8
            })
            .collect()
    };
    let inputs = op_gpu::parity::Inputs {
        width,
        height,
        bottom: make(),
        top: make(),
    };
    let results = op_gpu::parity::run(&mut gpu, &inputs).unwrap();
    // 49 composite, tool, resample, adjustment, artistic, distort, noise, photo and stylize ops, plus 6 blur kernels.
    assert_eq!(
        results
            .iter()
            .filter(|(id, _, _)| !id.starts_with("effect.blur."))
            .count(),
        49
    );
    assert_eq!(results.len(), 55);
    assert!(
        results
            .iter()
            .all(|(_, worst, dispatches)| *worst <= 1 && *dispatches > 0),
        "{results:?}"
    );
    for op in op_gpu::parity::registered() {
        let before = gpu.dispatch_count();
        let expected = op.cpu(&inputs);
        let actual = op.gpu(&mut gpu, &inputs).unwrap();
        check(width, height, expected, actual);
        assert!(
            gpu.dispatch_count() > before,
            "{} did not dispatch",
            op.id()
        );
    }
}

const PAIRS: [([u8; 3], [u8; 3]); 16] = [
    ([0, 17, 231], [255, 199, 3]),
    ([255, 0, 37], [0, 255, 218]),
    ([1, 254, 89], [237, 1, 166]),
    ([127, 128, 129], [130, 126, 4]),
    ([19, 45, 71], [91, 123, 157]),
    ([240, 200, 160], [120, 80, 40]),
    ([31, 63, 95], [127, 159, 191]),
    ([2, 4, 8], [16, 32, 64]),
    ([3, 105, 207], [251, 149, 47]),
    ([222, 11, 88], [77, 233, 144]),
    ([54, 98, 201], [188, 42, 73]),
    ([255, 255, 0], [0, 1, 255]),
    ([0, 0, 255], [255, 255, 0]),
    ([5, 155, 250], [245, 100, 10]),
    ([100, 200, 50], [200, 50, 100]),
    ([254, 127, 1], [1, 128, 254]),
];
#[test]
fn exhaustive_alpha_grid() {
    let Some(mut gpu) = gpu() else {
        return;
    };
    let (width, height) = (1024u32, 1024u32);
    let mut bottom = Vec::with_capacity(width as usize * height as usize * 4);
    let mut top = Vec::with_capacity(bottom.capacity());
    for (bc, sc) in PAIRS {
        for ba in 0..=255u8 {
            for sa in 0..=255u8 {
                bottom.extend_from_slice(&[bc[0], bc[1], bc[2], ba]);
                top.extend_from_slice(&[sc[0], sc[1], sc[2], sa]);
            }
        }
    }
    for mode in MODES {
        let mut worst = 0;
        for opacity in [0, 1, 64, 128, 192, 255] {
            let before = gpu.dispatch_count();
            let actual = gpu
                .composite(
                    width,
                    height,
                    &[
                        LayerInput {
                            bytes: &bottom,
                            mode: LayerBlendMode::Normal,
                            opacity: 255,
                        },
                        LayerInput {
                            bytes: &top,
                            mode,
                            opacity,
                        },
                    ],
                )
                .unwrap();
            assert!(gpu.dispatch_count() > before);
            let expected: Vec<u8> = bottom
                .chunks_exact(4)
                .zip(top.chunks_exact(4))
                .flat_map(|(b, s)| {
                    <[u8; 4]>::from(composite_pixel(
                        [b[0], b[1], b[2], b[3]].into(),
                        [s[0], s[1], s[2], s[3]].into(),
                        mode,
                        opacity,
                    ))
                })
                .collect();
            worst = worst.max(check(width, height, expected, actual));
        }
        println!("mode={mode:?} worst={worst}");
    }
}

#[test]
fn hundred_layer_random_stack() {
    let Some(mut gpu) = gpu() else {
        return;
    };
    let mut seed = 0x8123_7281u32;
    let mut layers = Vec::new();
    for i in 0..100 {
        let mut pixels = Vec::new();
        for _ in 0..64 * 64 * 4 {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            pixels.push((seed >> 24) as u8);
        }
        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        layers.push((pixels, MODES[i % 14], seed as u8));
    }
    let input: Vec<_> = layers
        .iter()
        .map(|(bytes, mode, opacity)| LayerInput {
            bytes,
            mode: *mode,
            opacity: *opacity,
        })
        .collect();
    let actual = gpu.composite(64, 64, &input).unwrap();
    let mut expected = Vec::with_capacity(actual.len());
    for pixel in 0..64 * 64 {
        let mut acc = op_ops::composite::spec::Acc::default();
        for (bytes, mode, opacity) in &layers {
            let p = &bytes[pixel * 4..pixel * 4 + 4];
            acc.over([p[0], p[1], p[2], p[3]].into(), *mode, *opacity);
        }
        expected.extend_from_slice(&<[u8; 4]>::from(acc.pixel()));
    }
    check(64, 64, expected, actual);
    assert!(gpu.dispatch_count() >= 101);
}

#[test]
fn four_tile_seam_box_blur() {
    let Some(mut gpu) = gpu() else {
        return;
    };
    let (width, height) = (512u32, 512u32);
    let mut bytes = vec![0u8; width as usize * height as usize * 4];
    for y in 0..height as usize {
        for x in 0..width as usize {
            let tile = (x / 256 + 2 * (y / 256)) as u8;
            let i = (y * width as usize + x) * 4;
            bytes[i..i + 4].copy_from_slice(&[
                tile.wrapping_mul(77),
                (x % 256) as u8,
                (y % 256) as u8,
                255,
            ]);
        }
    }
    let inputs = op_gpu::parity::Inputs {
        width,
        height,
        bottom: bytes.clone(),
        top: vec![0; bytes.len()],
    };
    let expected = op_gpu::parity::GpuOp::cpu(&op_gpu::parity::BoxBlurOp, &inputs);
    let before = gpu.dispatch_count();
    let actual = gpu.box_blur(width, height, &bytes).unwrap();
    assert_eq!(actual, expected);
    assert!(gpu.dispatch_count() > before);
}

use proptest::prelude::*;
use std::sync::{Mutex, OnceLock};
static PROP_GPU: OnceLock<Mutex<GpuContext>> = OnceLock::new();
proptest! {
    #[test]
    fn random_pixels_from_proptest(bottom in any::<[u8;4]>(), top in any::<[u8;4]>(), mode in 0usize..14, opacity in any::<u8>()) {
        if std::env::var("OP_GPU_TESTS").as_deref() == Ok("0") { println!("SKIP OP_GPU_TESTS=0"); return Ok(()); }
        let mut gpu = PROP_GPU.get_or_init(|| { let gpu = GpuContext::new().expect("GPU adapter"); println!("GpuInfo: {:?}", gpu.info()); Mutex::new(gpu) }).lock().unwrap();
        let actual = gpu.composite(1,1,&[LayerInput { bytes: &bottom, mode: LayerBlendMode::Normal, opacity: 255 },LayerInput { bytes: &top, mode: MODES[mode], opacity }]).unwrap();
        let expected = <[u8;4]>::from(composite_pixel(bottom.into(),top.into(),MODES[mode],opacity));
        if expected[3] != 0 || actual[3] != 0 {
            prop_assert!((0..4).all(|i| expected[i].abs_diff(actual[i]) <= 1), "mode={:?} opacity={} bottom={:?} top={:?} expected={:?} actual={:?}",MODES[mode],opacity,bottom,top,expected,actual);
        }
        prop_assert!(gpu.dispatch_count()>0);
    }
}

#[test]
fn per_pixel_tool_coverage() {
    let Some(mut gpu) = gpu() else {
        return;
    };
    let width = 257;
    let height = 3;
    let dst: Vec<u8> = [18, 200, 44, 90].repeat(width as usize * height as usize);
    let src: Vec<u8> = [230, 40, 190, 170].repeat(width as usize * height as usize);
    let mask: Vec<u8> = (0..width * height).map(|i| i as u8).collect();
    for mode in [ToolCompositeMode::Normal, ToolCompositeMode::Overwrite] {
        let actual = gpu
            .tool_composite_mask(width, height, &dst, &src, &mask, mode)
            .unwrap();
        let expected: Vec<u8> = mask
            .iter()
            .flat_map(|&coverage| {
                <[u8; 4]>::from(tool_composite(
                    [18, 200, 44, 90].into(),
                    [230, 40, 190, 170].into(),
                    mode,
                    coverage,
                ))
            })
            .collect();
        check(width, height, expected, actual);
    }
}

struct MemorySpill(std::sync::Mutex<Vec<u8>>);
impl op_core::tiles::SpillIo for MemorySpill {
    fn append(&self, bytes: &[u8]) -> std::io::Result<u64> {
        let mut data = self.0.lock().unwrap();
        let offset = data.len() as u64;
        data.extend_from_slice(bytes);
        Ok(offset)
    }
    fn read_exact_at(&self, offset: u64, bytes: &mut [u8]) -> std::io::Result<()> {
        let data = self.0.lock().unwrap();
        let end = offset as usize + bytes.len();
        bytes.copy_from_slice(&data[offset as usize..end]);
        Ok(())
    }
}
#[test]
fn hundred_layer_cpu_f32_tile() {
    use op_core::doc::DocState;
    use op_core::tiles::{TILE_BYTES, TileCoord, TileData, TileStore};
    use std::sync::Arc;
    let Some(mut gpu) = gpu() else {
        return;
    };
    let store = Arc::new(TileStore::with_io(
        128 * TILE_BYTES,
        Box::new(MemorySpill(std::sync::Mutex::new(Vec::new()))),
    ));
    let mut doc = DocState::new(store, 256, 256, 96.0).unwrap();
    let coord = TileCoord { x: 0, y: 0 };
    let mut seed = 0x9182_7721u32;
    let mut layers = Vec::new();
    for i in 0..100 {
        if i > 0 {
            doc = doc.add_layer(i, format!("layer {i}")).unwrap();
        }
        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        let color = [
            seed as u8,
            (seed >> 8) as u8,
            (seed >> 16) as u8,
            (seed >> 24) as u8,
        ];
        let bytes = color.repeat(TILE_BYTES / 4);
        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        let opacity = seed as u8;
        let mode = MODES[((seed >> 8) as usize) % MODES.len()];
        doc = doc
            .write_tile(i, coord, TileData::try_from(bytes.clone()).unwrap())
            .unwrap();
        doc = doc.set_layer_opacity(i, opacity).unwrap();
        doc = doc.set_layer_blend(i, mode).unwrap();
        layers.push((bytes, mode, opacity));
    }
    let gpu_layers: Vec<_> = layers
        .iter()
        .map(|(bytes, mode, opacity)| LayerInput {
            bytes,
            mode: *mode,
            opacity: *opacity,
        })
        .collect();
    let actual = gpu.composite(256, 256, &gpu_layers).unwrap();
    let expected = op_ops::composite::composite_tile(&doc, coord).unwrap();
    check(256, 256, expected.as_bytes().to_vec(), actual);
    assert!(gpu.dispatch_count() >= 101);
}
