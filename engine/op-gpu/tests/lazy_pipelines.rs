//! M58: pipelines are made the first time they are used. A new canvas view
//! makes only its view pipeline; the mip builder waits for the first draw
//! below 100%, and an effect kernel waits for its first dispatch.
//!
//! The counts are process-wide, so this file holds a single test (its own
//! test binary) and compares counts before and after each step.
use op_gpu::GpuContext;
use op_gpu::pipeline_stats::{PipelineCounts, pipeline_counts};
use op_gpu::presenter::{CanvasParams, ViewParams, ViewRenderer};

fn delta(before: PipelineCounts) -> PipelineCounts {
    let now = pipeline_counts();
    PipelineCounts {
        effect: now.effect - before.effect,
        view: now.view - before.view,
        mip: now.mip - before.mip,
    }
}

fn draw(gpu: &GpuContext, renderer: &mut ViewRenderer, zoom: f32) {
    let target = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d {
            width: 32,
            height: 32,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Bgra8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let view = target.create_view(&Default::default());
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    let canvas = CanvasParams {
        view: ViewParams {
            zoom,
            origin_x: 0.0,
            origin_y: 0.0,
            background: [0.5, 0.5, 0.5, 1.0],
        },
        checker_px: 8,
        pixel_grid: false,
        use_mips: true,
    };
    renderer
        .encode_canvas(&gpu.device, &gpu.queue, &mut encoder, &view, &canvas)
        .unwrap();
    gpu.queue.submit(Some(encoder.finish()));
    gpu.device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("poll");
}

#[test]
fn pipelines_are_made_on_first_use() {
    if std::env::var("OP_GPU_TESTS").as_deref() == Ok("0") {
        println!("SKIP OP_GPU_TESTS=0");
        return;
    }
    let start = pipeline_counts();
    let mut gpu = GpuContext::new().expect("GPU adapter");
    assert_eq!(delta(start), PipelineCounts::default(), "device start");

    let mut renderer = ViewRenderer::new(&gpu.device, wgpu::TextureFormat::Bgra8Unorm);
    let doc: Vec<u8> = (0..64 * 64 * 4).map(|i| (i * 7) as u8).collect();
    renderer
        .set_document(&gpu.device, &gpu.queue, 64, 64, &doc)
        .unwrap();
    draw(&gpu, &mut renderer, 1.0);
    let after_view = PipelineCounts {
        effect: 0,
        view: 1,
        mip: 0,
    };
    assert_eq!(
        delta(start),
        after_view,
        "a view at 100% makes only its view pipeline"
    );

    draw(&gpu, &mut renderer, 0.25);
    draw(&gpu, &mut renderer, 0.5);
    assert_eq!(renderer.mip_builds(), 1);
    let after_mips = PipelineCounts {
        mip: 1,
        ..after_view
    };
    assert_eq!(
        delta(start),
        after_mips,
        "the first draw below 100% makes the mip builder once"
    );

    let src: Vec<u8> = (0..8 * 8 * 4).map(|i| i as u8).collect();
    gpu.resample_nearest(8, 8, 4, 4, &src).unwrap();
    gpu.resample_nearest(8, 8, 2, 2, &src).unwrap();
    assert_eq!(
        delta(start),
        PipelineCounts {
            effect: 1,
            ..after_mips
        },
        "the first kernel use compiles it once"
    );
}
