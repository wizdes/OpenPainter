use std::sync::mpsc;
use wgpu::*;

fn check(instance: &Instance, name: &str, fallback: bool) -> bool {
    let adapter = match pollster::block_on(instance.request_adapter(&RequestAdapterOptions {
        power_preference: PowerPreference::HighPerformance,
        compatible_surface: None,
        force_fallback_adapter: fallback,
    })) {
        Ok(adapter) => adapter,
        Err(error) => {
            println!("{name}: backend DX12 FAIL: adapter request: {error}");
            return false;
        }
    };
    let info = adapter.get_info();
    let label = format!("{name}: {} backend {:?}", info.name, info.backend);
    if info.backend != Backend::Dx12 || (fallback && info.device_type != DeviceType::Cpu) {
        println!(
            "{label} FAIL: expected DX12{}",
            if fallback { " WARP CPU" } else { "" }
        );
        return false;
    }
    match render(&adapter) {
        Ok(()) => {
            println!("{label} PASS");
            true
        }
        Err(error) => {
            println!("{label} FAIL: {error}");
            false
        }
    }
}

fn render(adapter: &Adapter) -> Result<(), String> {
    let (device, queue) = pollster::block_on(adapter.request_device(&DeviceDescriptor::default()))
        .map_err(|error| error.to_string())?;
    let texture = device.create_texture(&TextureDescriptor {
        label: Some("known color"),
        size: Extent3d {
            width: 4,
            height: 4,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format: TextureFormat::Rgba8Unorm,
        usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&TextureViewDescriptor::default());
    let buffer = device.create_buffer(&BufferDescriptor {
        label: Some("readback"),
        size: 256 * 4,
        usage: BufferUsages::COPY_DST | BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&CommandEncoderDescriptor { label: None });
    {
        let _pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: None,
            color_attachments: &[Some(RenderPassColorAttachment {
                view: &view,
                resolve_target: None,
                ops: Operations {
                    load: LoadOp::Clear(Color {
                        r: 1.0,
                        g: 0.0,
                        b: 0.0,
                        a: 1.0,
                    }),
                    store: StoreOp::Store,
                },
                depth_slice: None,
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
    }
    encoder.copy_texture_to_buffer(
        TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: Origin3d::ZERO,
            aspect: TextureAspect::All,
        },
        TexelCopyBufferInfo {
            buffer: &buffer,
            layout: TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(256),
                rows_per_image: Some(4),
            },
        },
        Extent3d {
            width: 4,
            height: 4,
            depth_or_array_layers: 1,
        },
    );
    queue.submit(Some(encoder.finish()));
    let (tx, rx) = mpsc::channel();
    buffer.slice(..).map_async(MapMode::Read, move |result| {
        let _ = tx.send(result);
    });
    device
        .poll(PollType::wait_indefinitely())
        .map_err(|error| error.to_string())?;
    rx.recv()
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())?;
    let data = buffer.slice(..).get_mapped_range();
    for y in 0..4 {
        for x in 0..4 {
            let i = y * 256 + x * 4;
            if data[i..i + 4] != [255, 0, 0, 255] {
                return Err(format!("pixel ({x},{y}) = {:?}", &data[i..i + 4]));
            }
        }
    }
    drop(data);
    buffer.unmap();
    Ok(())
}

fn main() {
    let all = Instance::new(&InstanceDescriptor::default());
    for adapter in all.enumerate_adapters(Backends::all()) {
        let info = adapter.get_info();
        println!("adapter: {} backend {:?}", info.name, info.backend);
    }
    let dx12 = Instance::new(&InstanceDescriptor {
        backends: Backends::DX12,
        ..Default::default()
    });
    let normal = check(&dx12, "default", false);
    let warp = check(&dx12, "WARP", true);
    if !normal || !warp {
        std::process::exit(1);
    }
}
