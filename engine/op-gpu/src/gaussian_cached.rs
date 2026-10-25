//! The two-pass Gaussian from `gaussian.rs` with its pipelines built once per
//! context. The engine calls it once per chunk, so rebuilding both shaders on
//! every call (about 140 ms on an M2) would cost more than the blur itself.
//! Same WGSL, same dispatches, so the bytes equal `gaussian_timed`.
use crate::{GpuContext, GpuError, Result};
use wgpu::util::DeviceExt;

const VERTICAL: &str = "gaussian cached vertical";
const HORIZONTAL: &str = "gaussian cached horizontal";

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
fn bind(
    gpu: &GpuContext,
    pipeline: &wgpu::ComputePipeline,
    views: [&wgpu::TextureView; 3],
    params: &wgpu::Buffer,
) -> wgpu::BindGroup {
    gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("Gaussian cached bindings"),
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(views[0]),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(views[1]),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::TextureView(views[2]),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: params.as_entire_binding(),
            },
        ],
    })
}
impl GpuContext {
    fn cached_gaussian_pipeline(
        &mut self,
        label: &'static str,
        source: &'static str,
    ) -> wgpu::ComputePipeline {
        if let Some(pipeline) = self.pipelines.get(label) {
            return pipeline.clone();
        }
        let module = self
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(label),
                source: wgpu::ShaderSource::Wgsl(source.into()),
            });
        let pipeline = self
            .device
            .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(label),
                layout: None,
                module: &module,
                entry_point: Some("main"),
                compilation_options: Default::default(),
                cache: None,
            });
        self.pipelines.insert(label, pipeline.clone());
        pipeline
    }
    /// Legacy Gaussian blur (radius 0..=200) with cached pipelines.
    pub fn gaussian_cached(
        &mut self,
        width: u32,
        height: u32,
        bytes: &[u8],
        radius: u32,
    ) -> Result<Vec<u8>> {
        Self::checked_len(width, height, bytes)?;
        if radius > 200 {
            return Err(GpuError::InvalidImage);
        }
        self.check()?;
        let source = self.upload(width, height, bytes)?;
        if radius == 0 {
            return self.readback(&source, width, height);
        }
        let vertical =
            self.cached_gaussian_pipeline(VERTICAL, include_str!("gaussian_vertical.wgsl"));
        let horizontal =
            self.cached_gaussian_pipeline(HORIZONTAL, include_str!("gaussian_horizontal.wgsl"));
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
        let source_view = source.create_view(&Default::default());
        let sums_view = sums.create_view(&Default::default());
        let wc_view = wc.create_view(&Default::default());
        let output_view = output.create_view(&Default::default());
        let first = bind(
            self,
            &vertical,
            [&source_view, &sums_view, &wc_view],
            &params,
        );
        let second = bind(
            self,
            &horizontal,
            [&sums_view, &wc_view, &output_view],
            &params,
        );
        self.dispatch(&vertical, &first, width, height)?;
        self.dispatch(&horizontal, &second, width, height)?;
        self.readback(&output, width, height)
    }
}
