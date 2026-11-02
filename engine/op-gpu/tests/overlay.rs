#![cfg(windows)]

use op_gpu::GpuContext;
use op_gpu::overlay::{Item, Overlay, Point};
use op_gpu::presenter::{CanvasParams, ViewParams, ViewRenderer};
use std::sync::Arc;

fn pixels(items: Vec<Item>, zoom: f32, dash: f32) -> Vec<u8> {
    let gpu = GpuContext::new().expect("GPU adapter");
    let (device, queue) = (&gpu.device, &gpu.queue);
    let mut renderer = ViewRenderer::new(device, wgpu::TextureFormat::Bgra8Unorm);
    renderer
        .set_document(device, queue, 128, 128, &vec![255; 128 * 128 * 4])
        .unwrap();
    let overlay = Arc::new(Overlay::new(items).unwrap());
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("overlay readback target"),
        size: wgpu::Extent3d {
            width: 128,
            height: 128,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Bgra8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let row = 512;
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("overlay readback"),
        size: row * 128,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    renderer.set_overlay(device, Some(overlay), dash, 1.0, 128, 128);
    renderer
        .encode_canvas(
            device,
            queue,
            &mut encoder,
            &target.create_view(&Default::default()),
            &CanvasParams {
                view: ViewParams {
                    zoom,
                    origin_x: 0.0,
                    origin_y: 0.0,
                    background: [1.0; 4],
                },
                checker_px: 8,
                pixel_grid: false,
                use_mips: false,
            },
        )
        .unwrap();
    encoder.copy_texture_to_buffer(
        target.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(512),
                rows_per_image: Some(128),
            },
        },
        target.size(),
    );
    queue.submit(Some(encoder.finish()));
    readback.map_async(wgpu::MapMode::Read, .., |result| result.unwrap());
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    let bytes = readback.get_mapped_range(..).to_vec();
    bytes
}

fn rgb(p: &[u8], x: usize, y: usize) -> [u8; 3] {
    let i = (y * 128 + x) * 4;
    [p[i + 2], p[i + 1], p[i]]
}

#[test]
fn ants_move_and_keep_screen_width_at_three_zooms() {
    let path = Item::path(vec![Point::new(10.0, 20.0), Point::new(60.0, 20.0)], false);
    for zoom in [0.5, 1.0, 2.0] {
        let a = pixels(vec![path.clone()], zoom, 0.0);
        let b = pixels(vec![path.clone()], zoom, 4.0);
        let x = (14.0 * zoom) as usize;
        let y = (20.0 * zoom) as usize;
        assert_ne!(rgb(&a, x, y), rgb(&b, x, y), "dash at zoom {zoom}");
        if zoom == 1.0 {
            assert!(
                rgb(&a, x, y)[0] > rgb(&b, x, y)[0],
                "white turns black at zoom {zoom}"
            );
        } else {
            assert!(
                rgb(&a, x, y)[0] < rgb(&b, x, y)[0],
                "black turns white at zoom {zoom}"
            );
        }
        assert_eq!(rgb(&a, x, y + 3), [255; 3], "screen width at zoom {zoom}");
    }
}

#[test]
fn handles_are_fixed_size_and_crop_uses_even_odd() {
    for zoom in [0.5, 1.0, 2.0] {
        let c = (40.0 * zoom) as usize;
        let p = pixels(vec![Item::handle(Point::new(40.0, 40.0))], zoom, 0.0);
        assert_ne!(rgb(&p, c - 4, c), [255; 3]);
        assert_eq!(rgb(&p, c - 5, c), [255; 3]);
        let crop = Item::crop([
            Point::new(20.0, 10.0),
            Point::new(70.0, 20.0),
            Point::new(60.0, 70.0),
            Point::new(10.0, 60.0),
        ]);
        let p = pixels(vec![crop], zoom, 0.0);
        assert!(rgb(&p, 2, 2)[0] < 180, "outside dimmed at zoom {zoom}");
        assert_eq!(rgb(&p, c, c), [255; 3], "inside untouched at zoom {zoom}");
    }
}

#[test]
fn long_path_and_empty_list() {
    let points = (0..10_000)
        .map(|i| Point::new(i as f32 / 100.0, 30.0))
        .collect();
    assert_ne!(
        rgb(&pixels(vec![Item::path(points, false)], 1.0, 0.0), 16, 30),
        [255; 3]
    );
    assert_eq!(rgb(&pixels(vec![], 1.0, 0.0), 20, 30), [255; 3]);
}

#[test]
fn crossing_crop_uses_even_odd_parity() {
    let crop = Item::crop([
        Point::new(10.0, 10.0),
        Point::new(70.0, 70.0),
        Point::new(10.0, 70.0),
        Point::new(70.0, 10.0),
    ]);
    let p = pixels(vec![crop], 1.0, 0.0);
    assert_eq!(
        rgb(&p, 40, 20),
        [255; 3],
        "one crossing leaves the upper lobe clear"
    );
    assert!(rgb(&p, 15, 40)[0] < 180, "two crossings dim the outside");
}

#[test]
fn pivot_circle_stays_six_screen_points_from_center() {
    for zoom in [0.5, 1.0, 2.0] {
        let c = (40.0 * zoom) as usize;
        let p = pixels(vec![Item::Pivot(Point::new(40.0, 40.0))], zoom, 0.0);
        assert_ne!(rgb(&p, c + 4, c + 3), [255; 3], "pivot at zoom {zoom}");
        assert_eq!(
            rgb(&p, c + 10, c + 10),
            [255; 3],
            "outside pivot at zoom {zoom}"
        );
    }
}

#[test]
fn text_caret_frame_and_underline_have_black_marks() {
    let p = pixels(
        vec![
            Item::Caret {
                x: 20.0,
                y: 20.0,
                height: 20.0,
            },
            Item::EditFrame {
                x: 30.0,
                y: 20.0,
                width: 40.0,
                height: 30.0,
            },
            Item::Underline {
                from: Point::new(30.0, 60.0),
                to: Point::new(60.0, 60.0),
            },
        ],
        1.0,
        0.0,
    );
    assert!(
        rgb(&p, 20, 30).iter().all(|&c| c < 190),
        "caret center is black"
    );
    assert!(
        rgb(&p, 32, 20).iter().all(|&c| c < 190),
        "frame has black dashes"
    );
    assert!(
        rgb(&p, 40, 60).iter().all(|&c| c < 190),
        "underline is black"
    );
}

#[test]
fn flood_handle_and_zoom_rectangle_match_screen_shapes() {
    for zoom in [0.5, 1.0, 2.0] {
        let c = (40.0 * zoom) as usize;
        let p = pixels(vec![Item::FloodHandle(Point::new(40.0, 40.0))], zoom, 0.0);
        assert_ne!(rgb(&p, c + 7, c), [255; 3], "flood border at zoom {zoom}");
        assert_eq!(rgb(&p, c + 10, c), [255; 3], "flood size at zoom {zoom}");
    }
    let rect = Item::ZoomRectangle {
        x: 10.0,
        y: 20.0,
        width: 40.0,
        height: 30.0,
    };
    assert_eq!(
        rgb(&pixels(vec![rect.clone()], 1.0, 0.0), 14, 20),
        rgb(&pixels(vec![rect], 1.0, 4.0), 14, 20),
        "zoom box dashes do not animate"
    );
}

#[test]
fn one_point_line_blends_boundary_pixels() {
    let p = pixels(
        vec![Item::path(
            vec![Point::new(10.0, 20.0), Point::new(60.0, 20.0)],
            false,
        )],
        1.0,
        0.0,
    );
    for y in [19, 20] {
        let c = rgb(&p, 12, y);
        assert!(
            c.iter().all(|&v| (80..190).contains(&v)),
            "antialias at row {y}: {c:?}"
        );
    }
    assert_eq!(rgb(&p, 12, 18), [255; 3]);
}
