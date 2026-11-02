#![cfg(windows)]

#[test]
fn requested_windows_adapter_is_exact() {
    let gpu = op_gpu::GpuContext::new().expect("requested Windows adapter");
    let info = gpu.info();
    assert_eq!(info.backend, wgpu::Backend::Dx12, "adapter: {}", info.name);
    match std::env::var("OP_GPU_ADAPTER").as_deref() {
        Ok("warp") => assert!(
            info.name.to_ascii_lowercase().contains("basic render"),
            "WARP was requested but {} ran",
            info.name
        ),
        Ok("hardware") | Err(_) => assert!(
            !info.name.to_ascii_lowercase().contains("basic render"),
            "hardware was requested but {} ran",
            info.name
        ),
        Ok(other) => panic!("unknown OP_GPU_ADAPTER={other}"),
    }
}
