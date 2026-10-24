use op_core::doc::LayerBlendMode;
use op_gpu::{GpuContext, LayerInput};
fn main() {
    let mut gpu = GpuContext::new().expect("GPU adapter");
    println!("GpuInfo: {:?}", gpu.info());
    let (width, height) = (6000u32, 4000u32);
    let colors = [
        [100, 30, 200, 255],
        [230, 90, 10, 128],
        [25, 240, 80, 64],
        [190, 20, 160, 200],
        [5, 150, 240, 96],
    ];
    let bytes: Vec<_> = colors
        .into_iter()
        .map(|color| color.repeat(width as usize * height as usize))
        .collect();
    let modes = [
        LayerBlendMode::Normal,
        LayerBlendMode::Multiply,
        LayerBlendMode::Overlay,
        LayerBlendMode::Screen,
        LayerBlendMode::ColorDodge,
    ];
    let layers: Vec<_> = bytes
        .iter()
        .zip(modes)
        .map(|(bytes, mode)| LayerInput {
            bytes,
            mode,
            opacity: 255,
        })
        .collect();
    let (out, timing) = gpu
        .composite_timed(width, height, &layers)
        .expect("GPU composite");
    assert_eq!(out.len(), width as usize * height as usize * 4);
    println!(
        "6000x4000, 5 layers: upload {:.2} ms; composite {:.2} ms; readback and GPU completion {:.2} ms; dispatches {}",
        timing.upload_ms,
        timing.composite_ms,
        timing.readback_ms,
        gpu.dispatch_count()
    );
}
