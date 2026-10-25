//! View renderer for presenters: draws the document into any color target at a zoom
//! and pan, with an optional document -> display lookup table (LUT).
//!
//! It is platform neutral: the Mac presenter spike points it at a CAMetalLayer
//! drawable (path A) or an IOSurface texture (path B), and the tests point it at an
//! offscreen texture.

use crate::{GpuError, Result};

/// A 3D color table. `data[r + g * n + b * n * n]` is the output for the encoded
/// input `(r, g, b) / (n - 1)`.
#[derive(Debug, Clone)]
pub struct Lut3d {
    pub n: u32,
    pub data: Vec<[f32; 4]>,
}

impl Lut3d {
    pub fn identity(n: u32) -> Self {
        Self::from_fn(n, |rgb| rgb)
    }
    pub fn from_fn(n: u32, mut f: impl FnMut([f32; 3]) -> [f32; 3]) -> Self {
        assert!(n >= 2);
        let step = 1.0 / (n - 1) as f32;
        let mut data = Vec::with_capacity((n * n * n) as usize);
        for b in 0..n {
            for g in 0..n {
                for r in 0..n {
                    let o = f([r as f32 * step, g as f32 * step, b as f32 * step]);
                    data.push([o[0], o[1], o[2], 1.0]);
                }
            }
        }
        Self { n, data }
    }
    /// The same trilinear lookup the shader does, for tests.
    pub fn apply(&self, rgb: [f32; 3]) -> [f32; 3] {
        let n = self.n;
        let at = |r: u32, g: u32, b: u32| self.data[(r + g * n + b * n * n) as usize];
        let mut i0 = [0u32; 3];
        let mut f = [0f32; 3];
        for k in 0..3 {
            let x = rgb[k].clamp(0.0, 1.0) * (n - 1) as f32;
            i0[k] = (x.floor() as u32).min(n - 2);
            f[k] = x - i0[k] as f32;
        }
        let mut out = [0f32; 3];
        for (k, o) in out.iter_mut().enumerate() {
            let l = |a: f32, b: f32, t: f32| a + (b - a) * t;
            let c = |dr: u32, dg: u32, db: u32| at(i0[0] + dr, i0[1] + dg, i0[2] + db)[k];
            let c00 = l(c(0, 0, 0), c(1, 0, 0), f[0]);
            let c10 = l(c(0, 1, 0), c(1, 1, 0), f[0]);
            let c01 = l(c(0, 0, 1), c(1, 0, 1), f[0]);
            let c11 = l(c(0, 1, 1), c(1, 1, 1), f[0]);
            *o = l(l(c00, c10, f[1]), l(c01, c11, f[1]), f[2]);
        }
        out
    }
}

/// Where the document sits in the target, in target pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewParams {
    /// Target pixels per document pixel.
    pub zoom: f32,
    /// Target pixel position of the document's top-left corner.
    pub origin_x: f32,
    pub origin_y: f32,
    /// Fill outside the document, already in target encoding (0..1).
    pub background: [f32; 4],
}

impl ViewParams {
    /// Box filter width per axis below 100%. Capped at 4 (the spike has no mips).
    pub fn taps(&self) -> u32 {
        if self.zoom >= 1.0 {
            1
        } else {
            ((1.0 / self.zoom).ceil() as u32).clamp(1, 4)
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct ParamsRaw {
    origin: [f32; 2],
    zoom: f32,
    taps: u32,
    doc_size: [u32; 2],
    use_lut: u32,
    lut_n: u32,
    background: [f32; 4],
    level: u32,
    checker_px: u32,
    grid: u32,
    _pad: u32,
}

/// Canvas options on top of [`ViewParams`] (M12).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CanvasParams {
    pub view: ViewParams,
    /// Checkerboard square side in target pixels (8 points times the backing scale).
    pub checker_px: u32,
    /// Draw a one-target-pixel grid on document pixel edges, after the table.
    pub pixel_grid: bool,
    /// Below 100%, read a premultiplied 2x2 box mip pyramid (built on first use)
    /// instead of a capped box filter on the full-size document.
    pub use_mips: bool,
}

impl CanvasParams {
    /// The pyramid level and box width per axis this view reads. With mips, the
    /// level is the deepest one that is still at least as detailed as the target
    /// (`zoom * 2^level` in (0.5, 1]), and the box is 1 or 2 texels wide.
    pub fn level_and_taps(&self, levels: u32) -> (u32, u32) {
        let zoom = self.view.zoom;
        if !self.use_mips || zoom >= 1.0 || levels <= 1 {
            return (0, self.view.taps());
        }
        let mut level = 0;
        let mut z = zoom;
        while z * 2.0 <= 1.0 && level + 1 < levels {
            z *= 2.0;
            level += 1;
        }
        let taps = ((1.0 / z).ceil() as u32).clamp(1, 4);
        (level, taps)
    }
}

/// Number of pyramid levels (including level 0) for a document size, stopping at 1x1.
pub fn pyramid_levels(width: u32, height: u32) -> u32 {
    32 - width.max(height).max(1).leading_zeros()
}

struct DocTexture {
    texture: wgpu::Texture,
    width: u32,
    height: u32,
}

pub struct ViewRenderer {
    format: wgpu::TextureFormat,
    layout: wgpu::BindGroupLayout,
    pipeline: wgpu::RenderPipeline,
    params: wgpu::Buffer,
    lut: wgpu::Buffer,
    lut_n: u32,
    doc: Option<DocTexture>,
    bind_group: Option<wgpu::BindGroup>,
    /// Pyramid levels 1.. (premultiplied), built on first use below 100%.
    mips: Option<wgpu::Texture>,
    dummy_mips: wgpu::Texture,
    mip_pipeline: wgpu::RenderPipeline,
    mip_layout: wgpu::BindGroupLayout,
    /// Uniforms for the mip pass: [source straight, source premultiplied].
    mip_params: [wgpu::Buffer; 2],
    mip_builds: u64,
    /// The document changed after the pyramid was built (a partial upload):
    /// the next draw below 100% refills the existing pyramid.
    mips_stale: bool,
    mip_refreshes: u64,
}

impl ViewRenderer {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("view renderer"),
            source: wgpu::ShaderSource::Wgsl(include_str!("presenter.wgsl").into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("view renderer bindings"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Uint,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Uint,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("view renderer layout"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("view renderer"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        let params = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("view params"),
            size: std::mem::size_of::<ParamsRaw>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let lut = Self::lut_buffer(device, 16);
        let (mip_pipeline, mip_layout) = Self::mip_pipeline(device);
        let mip_params = [0u32, 1].map(|premul| {
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("mip params"),
                size: 16,
                usage: wgpu::BufferUsages::UNIFORM,
                mapped_at_creation: true,
            });
            buffer.slice(..).get_mapped_range_mut()[..4].copy_from_slice(&premul.to_le_bytes());
            buffer.unmap();
            buffer
        });
        let dummy_mips = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("no mips"),
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Uint,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        Self {
            format,
            layout,
            pipeline,
            params,
            lut,
            lut_n: 0,
            doc: None,
            bind_group: None,
            mips: None,
            dummy_mips,
            mip_pipeline,
            mip_layout,
            mip_params,
            mip_builds: 0,
            mips_stale: false,
            mip_refreshes: 0,
        }
    }

    fn mip_pipeline(device: &wgpu::Device) -> (wgpu::RenderPipeline, wgpu::BindGroupLayout) {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("mip builder"),
            source: wgpu::ShaderSource::Wgsl(include_str!("presenter_mip.wgsl").into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("mip builder bindings"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Uint,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("mip builder layout"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("mip builder"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("mip_fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba8Uint,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        (pipeline, layout)
    }

    /// How many times a mip pyramid was built (for tests and stats).
    pub fn mip_builds(&self) -> u64 {
        self.mip_builds
    }

    /// Builds pyramid levels 1.. for the current document, once per document.
    fn ensure_mips(&mut self, device: &wgpu::Device, encoder: &mut wgpu::CommandEncoder) {
        let Some(doc) = self.doc.as_ref() else { return };
        if self.mips.is_some() && !self.mips_stale {
            return;
        }
        let levels = pyramid_levels(doc.width, doc.height);
        if levels <= 1 {
            return;
        }
        let refresh = self.mips.take();
        let fresh = refresh.is_none();
        let mips = refresh.unwrap_or_else(|| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some("document mips"),
                size: wgpu::Extent3d {
                    width: (doc.width / 2).max(1),
                    height: (doc.height / 2).max(1),
                    depth_or_array_layers: 1,
                },
                mip_level_count: levels - 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Uint,
                usage: wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            })
        });
        let single = |t: &wgpu::Texture, level: u32| {
            t.create_view(&wgpu::TextureViewDescriptor {
                base_mip_level: level,
                mip_level_count: Some(1),
                ..Default::default()
            })
        };
        for k in 0..levels - 1 {
            let src = if k == 0 {
                single(&doc.texture, 0)
            } else {
                single(&mips, k - 1)
            };
            let dst = single(&mips, k);
            let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("mip builder"),
                layout: &self.mip_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&src),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: self.mip_params[(k > 0) as usize].as_entire_binding(),
                    },
                ],
            });
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("mip level"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &dst,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&self.mip_pipeline);
            pass.set_bind_group(0, &bind, &[]);
            pass.draw(0..3, 0..1);
        }
        self.mips = Some(mips);
        self.mips_stale = false;
        if fresh {
            self.mip_builds += 1;
            self.bind_group = None;
        } else {
            self.mip_refreshes += 1;
        }
    }

    /// How many times an existing pyramid was refilled after a partial upload.
    pub fn mip_refreshes(&self) -> u64 {
        self.mip_refreshes
    }

    /// Replaces a rectangle of the current document with straight, encoded RGBA8
    /// rows (`width * 4` bytes each). The texture is kept; mips refill on the
    /// next draw below 100%. Fails when no document is set or the rectangle is
    /// outside it.
    pub fn update_document_region(
        &mut self,
        queue: &wgpu::Queue,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
        rgba: &[u8],
    ) -> Result<()> {
        let doc = self.doc.as_ref().ok_or(GpuError::InvalidImage)?;
        if width == 0
            || height == 0
            || x.checked_add(width).is_none_or(|r| r > doc.width)
            || y.checked_add(height).is_none_or(|b| b > doc.height)
            || (width as u64) * (height as u64) * 4 != rgba.len() as u64
        {
            return Err(GpuError::InvalidImage);
        }
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &doc.texture,
                mip_level: 0,
                origin: wgpu::Origin3d { x, y, z: 0 },
                aspect: wgpu::TextureAspect::All,
            },
            rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * 4),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        if self.mips.is_some() {
            self.mips_stale = true;
        }
        Ok(())
    }

    fn lut_buffer(device: &wgpu::Device, size: u64) -> wgpu::Buffer {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("display LUT"),
            size,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }

    pub fn format(&self) -> wgpu::TextureFormat {
        self.format
    }

    pub fn document_size(&self) -> Option<(u32, u32)> {
        self.doc.as_ref().map(|d| (d.width, d.height))
    }

    /// Uploads straight, encoded RGBA8 document pixels.
    pub fn set_document(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        width: u32,
        height: u32,
        rgba: &[u8],
    ) -> Result<()> {
        let max = device.limits().max_texture_dimension_2d;
        if width == 0
            || height == 0
            || width > max
            || height > max
            || (width as u64) * (height as u64) * 4 != rgba.len() as u64
        {
            return Err(GpuError::InvalidImage);
        }
        let size = wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("document"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Uint,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            texture.as_image_copy(),
            rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * 4),
                rows_per_image: Some(height),
            },
            size,
        );
        self.doc = Some(DocTexture {
            texture,
            width,
            height,
        });
        self.mips = None;
        self.mips_stale = false;
        self.bind_group = None;
        Ok(())
    }

    /// Sets or clears the document -> display table.
    pub fn set_lut(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, lut: Option<&Lut3d>) {
        match lut {
            Some(lut) => {
                let bytes: &[u8] = bytemuck::cast_slice(&lut.data);
                if self.lut.size() < bytes.len() as u64 {
                    self.lut = Self::lut_buffer(device, bytes.len() as u64);
                    self.bind_group = None;
                }
                queue.write_buffer(&self.lut, 0, bytes);
                self.lut_n = lut.n;
            }
            None => self.lut_n = 0,
        }
    }

    pub fn has_lut(&self) -> bool {
        self.lut_n != 0
    }

    /// Records one full-target draw into `encoder`. Fails if no document is set.
    /// This is the M09 behavior: 8-pixel checkerboard, no grid, capped box filter.
    pub fn encode(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        params: &ViewParams,
    ) -> Result<()> {
        let canvas = CanvasParams {
            view: *params,
            checker_px: 8,
            pixel_grid: false,
            use_mips: false,
        };
        self.encode_canvas(device, queue, encoder, target, &canvas)
    }

    /// Records one full-target canvas draw into `encoder`: mips below 100% when
    /// asked, a checkerboard of `checker_px` squares, and the optional pixel grid.
    pub fn encode_canvas(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        canvas: &CanvasParams,
    ) -> Result<()> {
        let params = &canvas.view;
        let (width, height) = self.document_size().ok_or(GpuError::InvalidImage)?;
        if !(params.zoom.is_finite() && params.zoom > 0.0) || canvas.checker_px == 0 {
            return Err(GpuError::InvalidImage);
        }
        let mut levels = 1;
        if canvas.use_mips && params.zoom < 1.0 {
            self.ensure_mips(device, encoder);
            levels = pyramid_levels(width, height);
        }
        let (level, taps) = canvas.level_and_taps(levels);
        let raw = ParamsRaw {
            origin: [params.origin_x, params.origin_y],
            zoom: params.zoom,
            taps,
            doc_size: [width, height],
            use_lut: (self.lut_n != 0) as u32,
            lut_n: self.lut_n.max(2),
            background: params.background,
            level,
            checker_px: canvas.checker_px,
            grid: canvas.pixel_grid as u32,
            _pad: 0,
        };
        queue.write_buffer(&self.params, 0, bytemuck::bytes_of(&raw));
        if self.bind_group.is_none() {
            let doc = self.doc.as_ref().ok_or(GpuError::InvalidImage)?;
            let view = doc
                .texture
                .create_view(&wgpu::TextureViewDescriptor::default());
            let mips = self
                .mips
                .as_ref()
                .unwrap_or(&self.dummy_mips)
                .create_view(&wgpu::TextureViewDescriptor::default());
            self.bind_group = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("view renderer"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: self.lut.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: self.params.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::TextureView(&mips),
                    },
                ],
            }));
        }
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("view"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, self.bind_group.as_ref(), &[]);
        pass.draw(0..3, 0..1);
        Ok(())
    }
}
