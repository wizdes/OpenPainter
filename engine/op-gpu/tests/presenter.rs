use op_gpu::GpuContext;
use op_gpu::presenter::{Lut3d, ViewParams, ViewRenderer};

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
    params: &ViewParams,
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
        .encode(device, queue, &mut encoder, &view, params)
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

fn params(zoom: f32, x: f32, y: f32) -> ViewParams {
    ViewParams {
        zoom,
        origin_x: x,
        origin_y: y,
        background: [64.0 / 255.0, 128.0 / 255.0, 191.0 / 255.0, 1.0],
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
fn hundred_percent_is_byte_exact_with_and_without_identity_lut() {
    let Some(gpu) = gpu() else { return };
    let mut renderer = ViewRenderer::new(&gpu.device, wgpu::TextureFormat::Bgra8Unorm);
    let (w, h) = (67, 41);
    let doc = chart(w, h);
    renderer
        .set_document(&gpu.device, &gpu.queue, w, h, &doc)
        .unwrap();
    for lut in [None, Some(Lut3d::identity(33))] {
        renderer.set_lut(&gpu.device, &gpu.queue, lut.as_ref());
        // Target larger than the document: document at (3, 5), background elsewhere.
        let out = render(&gpu, &mut renderer, 80, 50, &params(1.0, 3.0, 5.0));
        for y in 0..50u32 {
            for x in 0..80u32 {
                let got = &out[((y * 80 + x) * 4) as usize..][..4];
                if (3..3 + w).contains(&x) && (5..5 + h).contains(&y) {
                    let i = (((y - 5) * w + (x - 3)) * 4) as usize;
                    assert_eq!(got, &doc[i..i + 4], "pixel {x},{y} lut {}", lut.is_some());
                } else {
                    assert_eq!(got, &[64, 128, 191, 255], "background {x},{y}");
                }
            }
        }
    }
}

#[test]
fn quarter_zoom_averages_four_by_four() {
    let Some(gpu) = gpu() else { return };
    let mut renderer = ViewRenderer::new(&gpu.device, wgpu::TextureFormat::Bgra8Unorm);
    let (w, h) = (64, 32);
    let doc = chart(w, h);
    renderer
        .set_document(&gpu.device, &gpu.queue, w, h, &doc)
        .unwrap();
    let out = render(&gpu, &mut renderer, 16, 8, &params(0.25, 0.0, 0.0));
    for y in 0..8u32 {
        for x in 0..16u32 {
            for c in 0..3 {
                let mut sum = 0u32;
                for dy in 0..4 {
                    for dx in 0..4 {
                        sum += doc[(((y * 4 + dy) * w + x * 4 + dx) * 4 + c) as usize] as u32;
                    }
                }
                let want = sum as f32 / 16.0;
                let got = out[((y * 16 + x) * 4 + c) as usize] as f32;
                assert!((got - want).abs() <= 0.51, "{x},{y},{c}: {got} vs {want}");
            }
        }
    }
}

#[test]
fn lut_on_gpu_matches_cpu_trilinear_within_one_level() {
    let Some(gpu) = gpu() else { return };
    let mut renderer = ViewRenderer::new(&gpu.device, wgpu::TextureFormat::Bgra8Unorm);
    let (w, h) = (256, 16);
    let doc = chart(w, h);
    renderer
        .set_document(&gpu.device, &gpu.queue, w, h, &doc)
        .unwrap();
    // A curved table: gamma 2.2 to 1.8 per channel plus a little channel mixing.
    let lut = Lut3d::from_fn(33, |[r, g, b]| {
        let f = |v: f32| v.powf(2.2 / 1.8);
        [f(0.9 * r + 0.1 * g), f(g), f(0.8 * b + 0.2 * r).min(1.0)]
    });
    renderer.set_lut(&gpu.device, &gpu.queue, Some(&lut));
    let out = render(&gpu, &mut renderer, w, h, &params(1.0, 0.0, 0.0));
    let mut worst = 0f32;
    for (i, px) in doc.chunks(4).enumerate() {
        let want = lut.apply([
            px[0] as f32 / 255.0,
            px[1] as f32 / 255.0,
            px[2] as f32 / 255.0,
        ]);
        for c in 0..3 {
            let d = (out[i * 4 + c] as f32 - want[c] * 255.0).abs();
            worst = worst.max(d);
        }
    }
    println!("worst GPU vs CPU LUT difference: {worst:.3} levels");
    assert!(worst <= 0.51 + 1e-3, "worst {worst}");
}

#[test]
fn rejects_bad_documents_and_encode_without_document() {
    let Some(gpu) = gpu() else { return };
    let mut renderer = ViewRenderer::new(&gpu.device, wgpu::TextureFormat::Bgra8Unorm);
    assert!(
        renderer
            .set_document(&gpu.device, &gpu.queue, 0, 1, &[])
            .is_err()
    );
    assert!(
        renderer
            .set_document(&gpu.device, &gpu.queue, 2, 2, &[0; 15])
            .is_err()
    );
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    let target = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d {
            width: 1,
            height: 1,
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
    assert!(
        renderer
            .encode(
                &gpu.device,
                &gpu.queue,
                &mut encoder,
                &view,
                &params(1.0, 0.0, 0.0)
            )
            .is_err()
    );
}
