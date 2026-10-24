//! M12 canvas options on the view renderer: mip pyramid below 100%, checkerboard
//! square size, premultiplied filtering (no color bleed from zero-alpha pixels),
//! and the pixel grid.
use op_gpu::GpuContext;
use op_gpu::presenter::{CanvasParams, ViewParams, ViewRenderer, pyramid_levels};

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
fn level_and_taps_choose_the_deepest_detailed_level() {
    let c = |zoom: f32| canvas(zoom, 8, false, true);
    assert_eq!(c(1.0).level_and_taps(10), (0, 1));
    assert_eq!(c(4.0).level_and_taps(10), (0, 1));
    assert_eq!(c(0.5).level_and_taps(10), (1, 1));
    assert_eq!(c(0.25).level_and_taps(10), (2, 1));
    // 0.3 -> level 1 at 0.6 target px per texel -> a 2x2 box.
    assert_eq!(c(0.3).level_and_taps(10), (1, 2));
    assert_eq!(c(0.7).level_and_taps(10), (0, 2));
    // Too few levels: the deepest one, with a box of at most 4.
    assert_eq!(c(0.01).level_and_taps(4), (3, 4));
    // Without mips: the M09 capped box on level 0.
    assert_eq!(canvas(0.25, 8, false, false).level_and_taps(10), (0, 4));
    assert_eq!(pyramid_levels(1, 1), 1);
    assert_eq!(pyramid_levels(6000, 4000), 13);
    assert_eq!(pyramid_levels(64, 32), 7);
}

#[test]
fn quarter_zoom_with_mips_matches_two_rounded_box_levels() {
    let Some(gpu) = gpu() else { return };
    let mut renderer = ViewRenderer::new(&gpu.device, wgpu::TextureFormat::Bgra8Unorm);
    let (w, h) = (64u32, 32u32);
    let doc = chart(w, h);
    renderer
        .set_document(&gpu.device, &gpu.queue, w, h, &doc)
        .unwrap();
    let out = render(&gpu, &mut renderer, 16, 8, &canvas(0.25, 8, false, true));
    assert_eq!(renderer.mip_builds(), 1);
    let half = |src: &[u8], sw: u32, sh: u32| -> Vec<u8> {
        let (dw, dh) = (sw / 2, sh / 2);
        let mut out = vec![0u8; (dw * dh * 4) as usize];
        for y in 0..dh {
            for x in 0..dw {
                for c in 0..4 {
                    let mut sum = 0u32;
                    for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                        sum += src[(((y * 2 + dy) * sw + x * 2 + dx) * 4 + c) as usize] as u32;
                    }
                    out[((y * dw + x) * 4 + c) as usize] = ((sum as f32 / 4.0) + 0.5) as u8;
                }
            }
        }
        out
    };
    let l2 = half(&half(&doc, w, h), w / 2, h / 2);
    let mut worst = 0i32;
    for i in 0..(16 * 8) as usize {
        for c in 0..3 {
            worst = worst.max((out[i * 4 + c] as i32 - l2[i * 4 + c] as i32).abs());
        }
    }
    assert!(worst <= 1, "worst {worst}");
    // A second frame reuses the pyramid.
    render(&gpu, &mut renderer, 16, 8, &canvas(0.25, 8, false, true));
    assert_eq!(renderer.mip_builds(), 1);
}

#[test]
fn checkerboard_square_size_follows_the_parameter() {
    let Some(gpu) = gpu() else { return };
    let mut renderer = ViewRenderer::new(&gpu.device, wgpu::TextureFormat::Bgra8Unorm);
    let doc = vec![0u8; 32 * 32 * 4];
    renderer
        .set_document(&gpu.device, &gpu.queue, 32, 32, &doc)
        .unwrap();
    for size in [4u32, 16] {
        let out = render(&gpu, &mut renderer, 32, 32, &canvas(1.0, size, false, true));
        for y in 0..32u32 {
            for x in 0..32u32 {
                let dark = (x / size + y / size) % 2 == 1;
                let want = if dark { 204 } else { 255 };
                let got = &out[((y * 32 + x) * 4) as usize..][..3];
                assert_eq!(got, &[want; 3], "size {size} at {x},{y}");
            }
        }
    }
}

#[test]
fn zoomed_out_filtering_is_premultiplied() {
    let Some(gpu) = gpu() else { return };
    let mut renderer = ViewRenderer::new(&gpu.device, wgpu::TextureFormat::Bgra8Unorm);
    // Even columns: opaque red. Odd columns: alpha 0 with bright green under it.
    let (w, h) = (64u32, 64u32);
    let doc: Vec<u8> = (0..w * h)
        .flat_map(|i| {
            if i % 2 == 0 {
                [255, 0, 0, 255]
            } else {
                [0, 255, 0, 0]
            }
        })
        .collect();
    renderer
        .set_document(&gpu.device, &gpu.queue, w, h, &doc)
        .unwrap();
    // Checker squares larger than the view: everything sits on white.
    for (zoom, mips) in [(0.5f32, true), (0.25, true), (0.5, false)] {
        let out = render(&gpu, &mut renderer, 16, 16, &canvas(zoom, 64, false, mips));
        for px in out.chunks(4) {
            // Half red, half white: (255, 127.5, 127.5).
            assert_eq!(px[0], 255, "zoom {zoom} mips {mips}: {px:?}");
            assert!(
                (px[1] as i32 - 128).abs() <= 1 && (px[2] as i32 - 128).abs() <= 1,
                "zoom {zoom} mips {mips}: green under zero alpha leaked: {px:?}"
            );
        }
    }
}

#[test]
fn pixel_grid_marks_document_pixel_edges() {
    let Some(gpu) = gpu() else { return };
    let mut renderer = ViewRenderer::new(&gpu.device, wgpu::TextureFormat::Bgra8Unorm);
    let doc = vec![255u8; 8 * 8 * 4];
    renderer
        .set_document(&gpu.device, &gpu.queue, 8, 8, &doc)
        .unwrap();
    let plain = render(&gpu, &mut renderer, 32, 32, &canvas(4.0, 8, false, true));
    assert!(plain.chunks(4).all(|p| p[..3] == [255, 255, 255]));
    let grid = render(&gpu, &mut renderer, 32, 32, &canvas(4.0, 8, true, true));
    for y in 0..32u32 {
        for x in 0..32u32 {
            let p = &grid[((y * 32 + x) * 4) as usize..][..3];
            if x % 4 == 0 || y % 4 == 0 {
                assert!(p[0] < 200, "grid line expected at {x},{y}: {p:?}");
            } else {
                assert_eq!(p, &[255, 255, 255], "no line at {x},{y}");
            }
        }
    }
}
