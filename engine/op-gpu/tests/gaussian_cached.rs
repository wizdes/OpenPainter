//! The cached-pipeline Gaussian must give `gaussian_timed`'s bytes.
use op_gpu::GpuContext;

#[test]
fn cached_gaussian_equals_the_timed_gaussian() {
    if std::env::var("OP_GPU_TESTS").as_deref() == Ok("0") {
        println!("SKIP OP_GPU_TESTS=0");
        return;
    }
    let mut gpu = GpuContext::new().expect("GPU adapter");
    let mut seed = 0x9e37_79b9u32;
    for (width, height, radius) in [(257, 19, 2), (64, 300, 0), (300, 64, 13), (130, 90, 50)] {
        let bytes: Vec<u8> = (0..width * height * 4)
            .map(|_| {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                (seed >> 24) as u8
            })
            .collect();
        let expected = gpu.gaussian_timed(width, height, &bytes, radius).unwrap().0;
        for _ in 0..2 {
            let before = gpu.dispatch_count();
            let actual = gpu.gaussian_cached(width, height, &bytes, radius).unwrap();
            assert!(actual == expected, "{width}x{height} r={radius}");
            let dispatches = if radius == 0 { 0 } else { 2 };
            assert_eq!(gpu.dispatch_count() - before, dispatches);
        }
    }
    assert!(gpu.gaussian_cached(4, 4, &[0; 64], 201).is_err());
}
