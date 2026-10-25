//! Two-pass Metal/WGSL Gaussian blur with the OpenPDN 3.36 tent weights.
use crate::{GpuContext, GpuError, Result};
use std::time::Instant;
use wgpu::util::DeviceExt;

#[derive(Clone, Copy, Debug)]
pub struct GaussianTimings {
    pub full_ms: f64,
    pub upload_ms: f64,
    pub kernel_ms: f64,
    pub readback_ms: f64,
}

fn intermediate(
    gpu: &GpuContext,
    width: u32,
    height: u32,
    format: wgpu::TextureFormat,
) -> wgpu::Texture {
    gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Gaussian vertical sums"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    })
}
fn pipeline(gpu: &GpuContext, label: &'static str, source: &'static str) -> wgpu::ComputePipeline {
    let module = gpu
        .device
        .create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some(label),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });
    gpu.device
        .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some(label),
            layout: None,
            module: &module,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        })
}
impl GpuContext {
    /// Full time includes upload and readback. Kernel time includes dispatch and GPU completion.
    pub fn gaussian_timed(
        &mut self,
        width: u32,
        height: u32,
        bytes: &[u8],
        radius: u32,
    ) -> Result<(Vec<u8>, GaussianTimings)> {
        Self::checked_len(width, height, bytes)?;
        if radius > 200 {
            return Err(GpuError::InvalidImage);
        }
        self.check()?;
        let full_start = Instant::now();
        if radius == 0 {
            let upload_start = Instant::now();
            let source = self.upload(width, height, bytes)?;
            let upload_ms = upload_start.elapsed().as_secs_f64() * 1000.0;
            let readback_start = Instant::now();
            let output = self.readback(&source, width, height)?;
            let readback_ms = readback_start.elapsed().as_secs_f64() * 1000.0;
            return Ok((
                output,
                GaussianTimings {
                    full_ms: full_start.elapsed().as_secs_f64() * 1000.0,
                    upload_ms,
                    kernel_ms: 0.0,
                    readback_ms,
                },
            ));
        }
        let upload_start = Instant::now();
        let source = self.upload(width, height, bytes)?;
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|_| GpuError::DeviceLost)?;
        let upload_ms = upload_start.elapsed().as_secs_f64() * 1000.0;
        let sums = intermediate(self, width, height, wgpu::TextureFormat::Rgba32Uint);
        let wc = intermediate(self, width, height, wgpu::TextureFormat::R32Uint);
        let output = self.texture(width, height);
        let params = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Gaussian params"),
                contents: bytemuck::cast_slice(&[width, height, radius, 0u32]),
                usage: wgpu::BufferUsages::UNIFORM,
            });
        let vertical = pipeline(
            self,
            "Gaussian vertical",
            include_str!("gaussian_vertical.wgsl"),
        );
        let horizontal = pipeline(
            self,
            "Gaussian horizontal",
            include_str!("gaussian_horizontal.wgsl"),
        );
        let first = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Gaussian vertical bindings"),
            layout: &vertical.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(
                        &source.create_view(&Default::default()),
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(
                        &sums.create_view(&Default::default()),
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(
                        &wc.create_view(&Default::default()),
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: params.as_entire_binding(),
                },
            ],
        });
        let second = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Gaussian horizontal bindings"),
            layout: &horizontal.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(
                        &sums.create_view(&Default::default()),
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(
                        &wc.create_view(&Default::default()),
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(
                        &output.create_view(&Default::default()),
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: params.as_entire_binding(),
                },
            ],
        });
        let kernel_start = Instant::now();
        self.dispatch(&vertical, &first, width, height)?;
        self.dispatch(&horizontal, &second, width, height)?;
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|_| GpuError::DeviceLost)?;
        let kernel_ms = kernel_start.elapsed().as_secs_f64() * 1000.0;
        let readback_start = Instant::now();
        let pixels = self.readback(&output, width, height)?;
        let readback_ms = readback_start.elapsed().as_secs_f64() * 1000.0;
        Ok((
            pixels,
            GaussianTimings {
                full_ms: full_start.elapsed().as_secs_f64() * 1000.0,
                upload_ms,
                kernel_ms,
                readback_ms,
            },
        ))
    }
}
