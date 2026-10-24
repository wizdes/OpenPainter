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
        let kernel = spec.gpu.expect("registered effect needs GPU kernel");
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
            }
        }
        assert_eq!(actual, expected, "{}", spec.id);
    }
}
