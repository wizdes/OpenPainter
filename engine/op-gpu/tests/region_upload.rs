//! M38: a partial document upload draws the same as a full upload, at 100% and
//! below 100% (the mip pyramid is refilled in place, not rebuilt).
use op_gpu::GpuContext;
use op_gpu::presenter::{CanvasParams, ViewParams, ViewRenderer};

fn gpu() -> Option<GpuContext> {
    if std::env::var("OP_GPU_TESTS").as_deref() == Ok("0") {
        println!("SKIP OP_GPU_TESTS=0");
        return None;
    }
    let gpu = GpuContext::new().expect("GPU adapter");
    println!("GpuInfo: {:?}", gpu.info());
    Some(gpu)
}

/// Renders into an offscreen BGRA8 target and returns its bytes as RGBA.
fn render(
    gpu: &GpuContext,
    renderer: &mut ViewRenderer,
    width: u32,
    height: u32,
    canvas: &CanvasParams,
) -> Vec<u8> {
    let (device, queue) = (&gpu.device, &gpu.queue);
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Bgra8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let row = (width * 4).div_ceil(256) * 256;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: (row * height) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    let view = target.create_view(&Default::default());
    renderer
        .encode_canvas(device, queue, &mut encoder, &view, canvas)
        .unwrap();
    encoder.copy_texture_to_buffer(
        target.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(row),
                rows_per_image: Some(height),
            },
        },
        target.size(),
    );
    queue.submit(Some(encoder.finish()));
    buffer.map_async(wgpu::MapMode::Read, .., |r| r.unwrap());
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("poll");
    let mapped = buffer.get_mapped_range(..);
    let mut out = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height as usize {
        for px in mapped[y * row as usize..y * row as usize + width as usize * 4].chunks(4) {
            out.extend_from_slice(&[px[2], px[1], px[0], px[3]]);
        }
    }
    out
}

fn canvas(zoom: f32, checker_px: u32, grid: bool, mips: bool) -> CanvasParams {
    CanvasParams {
        view: ViewParams {
            zoom,
            origin_x: 0.0,
            origin_y: 0.0,
            background: [64.0 / 255.0, 128.0 / 255.0, 191.0 / 255.0, 1.0],
        },
        checker_px,
        pixel_grid: grid,
        use_mips: mips,
    }
}

fn chart(width: u32, height: u32) -> Vec<u8> {
    (0..width * height)
        .flat_map(|i| {
            let (x, y) = (i % width, i / width);
            [
                (x * 37 + y * 11) as u8,
                (x * 5 + y * 71) as u8,
                (x ^ y) as u8,
                255,
            ]
        })
        .collect()
}

#[test]
fn region_upload_matches_full_upload_with_and_without_mips() {
    let Some(gpu) = gpu() else { return };
    let (device, queue) = (&gpu.device, &gpu.queue);
    let format = wgpu::TextureFormat::Bgra8Unorm;
    let (w, h) = (300u32, 200u32);
    let before = chart(w, h);
    // The edited copy: a 70 x 50 block at (100, 60) turns red and half transparent.
    let mut after = before.clone();
    let (rx, ry, rw, rh) = (100u32, 60u32, 70u32, 50u32);
    let mut block = Vec::new();
    for y in ry..ry + rh {
        for x in rx..rx + rw {
            let i = ((y * w + x) * 4) as usize;
            after[i..i + 4].copy_from_slice(&[250, 10, 20, 128]);
            block.extend_from_slice(&[250, 10, 20, 128]);
        }
    }
    let mut partial = ViewRenderer::new(device, format);
    partial.set_document(device, queue, w, h, &before).unwrap();
    // Build the pyramid first, so the region update must refill it.
    let _ = render(&gpu, &mut partial, 160, 100, &canvas(0.5, 8, false, true));
    assert_eq!(partial.mip_builds(), 1);
    partial
        .update_document_region(queue, rx, ry, rw, rh, &block)
        .unwrap();
    let mut full = ViewRenderer::new(device, format);
    full.set_document(device, queue, w, h, &after).unwrap();
    for (zoom, mips) in [(0.5, true), (0.25, true), (1.0, true), (2.0, false)] {
        let c = canvas(zoom, 8, false, mips);
        let (vw, vh) = ((w as f32 * zoom) as u32, (h as f32 * zoom) as u32);
        assert_eq!(
            render(&gpu, &mut partial, vw, vh, &c),
            render(&gpu, &mut full, vw, vh, &c),
            "zoom {zoom}"
        );
    }
    assert_eq!(partial.mip_builds(), 1, "the pyramid is kept");
    assert_eq!(partial.mip_refreshes(), 1, "and refilled once");
    // Rectangles outside the document and wrong byte counts are refused.
    assert!(
        partial
            .update_document_region(queue, 250, 0, 60, 1, &[0; 240])
            .is_err()
    );
    assert!(
        partial
            .update_document_region(queue, 0, 0, 2, 2, &[0; 12])
            .is_err()
    );
    assert!(
        partial
            .update_document_region(queue, 0, 0, 0, 1, &[])
            .is_err()
    );
    let mut empty = ViewRenderer::new(device, format);
    assert!(
        empty
            .update_document_region(queue, 0, 0, 1, 1, &[0; 4])
            .is_err()
    );
}
