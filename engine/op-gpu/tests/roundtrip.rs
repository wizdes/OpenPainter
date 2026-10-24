use op_gpu::GpuContext;

#[test]
fn odd_size_upload_readback_is_byte_exact() {
    if std::env::var("OP_GPU_TESTS").as_deref() == Ok("0") {
        println!("SKIP OP_GPU_TESTS=0");
        return;
    }
    let mut gpu = GpuContext::new().expect("GPU adapter");
    println!("GpuInfo: {:?}", gpu.info());
    for (width, height) in [(1u32, 1u32), (255, 7), (257, 3), (1000, 7)] {
        let bytes: Vec<u8> = (0..width * height * 4)
            .map(|i| (i.wrapping_mul(73) >> 2) as u8)
            .collect();
        assert_eq!(gpu.roundtrip(width, height, &bytes).unwrap(), bytes);
    }
}

#[test]
fn cache_evicts_lru_and_reuploads() {
    let Some(mut gpu) = (if std::env::var("OP_GPU_TESTS").as_deref() == Ok("0") {
        println!("SKIP OP_GPU_TESTS=0");
        None
    } else {
        Some(GpuContext::new().unwrap())
    }) else {
        return;
    };
    println!("GpuInfo: {:?}", gpu.info());
    use op_core::tiles::TileId;
    use op_gpu::{GpuTileCache, Representation};
    let mut cache = GpuTileCache::new(2 * 256 * 256 * 4);
    let tile = vec![73; 256 * 256 * 4];
    cache
        .get_or_upload(&mut gpu, TileId(1), Representation::Rgba8, &tile)
        .unwrap();
    cache
        .get_or_upload(&mut gpu, TileId(2), Representation::Rgba8, &tile)
        .unwrap();
    cache
        .get_or_upload(&mut gpu, TileId(1), Representation::Rgba8, &tile)
        .unwrap();
    cache
        .get_or_upload(&mut gpu, TileId(3), Representation::Rgba8, &tile)
        .unwrap();
    assert!(!cache.contains(TileId(2), Representation::Rgba8));
    assert_eq!(cache.bytes(), 2 * 256 * 256 * 4);
    cache
        .get_or_upload(&mut gpu, TileId(2), Representation::Rgba8, &tile)
        .unwrap();
    assert!(cache.contains(TileId(2), Representation::Rgba8));
    assert_eq!(cache.len(), 2);
    assert_eq!(gpu.roundtrip(256, 256, &tile).unwrap(), tile);
}
#[test]
fn loss_and_recreate() {
    if std::env::var("OP_GPU_TESTS").as_deref() == Ok("0") {
        println!("SKIP OP_GPU_TESTS=0");
        return;
    }
    let mut gpu = GpuContext::new().unwrap();
    println!("GpuInfo: {:?}", gpu.info());
    gpu.simulate_device_loss();
    assert!(matches!(
        gpu.roundtrip(1, 1, &[1, 2, 3, 4]),
        Err(op_gpu::GpuError::DeviceLost)
    ));
    gpu.recreate().unwrap();
    assert_eq!(gpu.roundtrip(1, 1, &[1, 2, 3, 4]).unwrap(), [1, 2, 3, 4]);
}

#[test]
fn delayed_readback_reverse_order() {
    if std::env::var("OP_GPU_TESTS").as_deref() == Ok("0") {
        println!("SKIP OP_GPU_TESTS=0");
        return;
    }
    let mut gpu = GpuContext::new().unwrap();
    println!("GpuInfo: {:?}", gpu.info());
    let images: Vec<_> = [(1u32, 7u32), (255, 3), (257, 5), (1000, 7)]
        .into_iter()
        .enumerate()
        .map(|(n, (w, h))| {
            (
                w,
                h,
                (0..w * h * 4)
                    .map(|i| (i as usize + n * 53) as u8)
                    .collect::<Vec<_>>(),
            )
        })
        .collect();
    let out = gpu.roundtrip_delayed(&images).unwrap();
    for (image, actual) in images.iter().zip(out) {
        assert_eq!(image.2, actual);
    }
    let reused = gpu.roundtrip_delayed(&images).unwrap();
    for (image, actual) in images.iter().zip(reused) {
        assert_eq!(image.2, actual);
    }
}
