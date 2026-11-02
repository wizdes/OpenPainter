//! Retained Windows canvas shapes. Points stay in document coordinates; the
//! vertex shader applies the current view transform and expands strokes in
//! screen points. Only a new item list rebuilds the vertex buffer.
#![cfg(windows)]

use std::sync::Arc;
use wgpu::util::DeviceExt;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}
impl Point {
    pub fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

#[derive(Clone, Debug)]
pub enum Item {
    Path {
        points: Vec<Point>,
        closed: bool,
    },
    Rectangle {
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    },
    Ellipse {
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    },
    Handle(Point),
    Pivot(Point),
    Crop([Point; 4]),
    Caret {
        x: f32,
        y: f32,
        height: f32,
    },
    EditFrame {
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    },
    Underline {
        from: Point,
        to: Point,
    },
    FloodHandle(Point),
    ZoomRectangle {
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    },
}
impl Item {
    pub fn path(points: Vec<Point>, closed: bool) -> Self {
        Self::Path { points, closed }
    }
    pub fn handle(point: Point) -> Self {
        Self::Handle(point)
    }
    pub fn crop(points: [Point; 4]) -> Self {
        Self::Crop(points)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayError {
    BadPoint,
    TooFewPoints,
    TooManyPoints,
    BadRect,
    BadCrop,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    anchor: [f32; 2],
    offset: [f32; 2],
    distance: f32,
    kind: f32,
    edge: f32,
    width: f32,
}

pub struct Overlay {
    vertices: Vec<Vertex>,
    crop: Option<[Point; 4]>,
}
impl Overlay {
    pub fn new(items: Vec<Item>) -> Result<Self, OverlayError> {
        let mut out = Self {
            vertices: Vec::new(),
            crop: None,
        };
        for item in items {
            match item {
                Item::Path { points, closed } => {
                    if points.len() < 2 {
                        return Err(OverlayError::TooFewPoints);
                    }
                    if points.len() > 1_000_000 {
                        return Err(OverlayError::TooManyPoints);
                    }
                    if !points.iter().all(|p| p.x.is_finite() && p.y.is_finite()) {
                        return Err(OverlayError::BadPoint);
                    }
                    out.path(&points, closed, 0.0, 1.0);
                }
                Item::Rectangle {
                    x,
                    y,
                    width,
                    height,
                } => {
                    Self::valid_rect(x, y, width, height)?;
                    out.path(
                        &[
                            Point::new(x, y),
                            Point::new(x + width, y),
                            Point::new(x + width, y + height),
                            Point::new(x, y + height),
                        ],
                        true,
                        0.0,
                        1.0,
                    );
                }
                Item::EditFrame {
                    x,
                    y,
                    width,
                    height,
                } => {
                    Self::valid_rect(x, y, width, height)?;
                    let points = [
                        Point::new(x, y),
                        Point::new(x + width, y),
                        Point::new(x + width, y + height),
                        Point::new(x, y + height),
                    ];
                    out.path(&points, true, 6.0, 1.0);
                    out.path(&points, true, 7.0, 1.0);
                }
                Item::Ellipse {
                    x,
                    y,
                    width,
                    height,
                } => {
                    Self::valid_rect(x, y, width, height)?;
                    let count = ((width.max(height) * 0.8).ceil() as usize).clamp(32, 512);
                    let points: Vec<_> = (0..count)
                        .map(|i| {
                            let a = std::f32::consts::TAU * i as f32 / count as f32;
                            Point::new(
                                x + width * (0.5 + 0.5 * a.cos()),
                                y + height * (0.5 + 0.5 * a.sin()),
                            )
                        })
                        .collect();
                    out.path(&points, true, 0.0, 1.0);
                }
                Item::Handle(p) => {
                    Self::valid_point(p)?;
                    out.square(p, 7.0, 1.0);
                }
                Item::Pivot(p) => {
                    Self::valid_point(p)?;
                    let offsets: Vec<_> = (0..32)
                        .map(|i| {
                            let a = std::f32::consts::TAU * i as f32 / 32.0;
                            Point::new(6.0 * a.cos(), 6.0 * a.sin())
                        })
                        .collect();
                    for i in 0..32 {
                        out.screen_line(p, offsets[i], offsets[(i + 1) % 32], 1.0, 1.0);
                    }
                    out.screen_line(p, Point::new(-9.0, 0.0), Point::new(9.0, 0.0), 1.0, 1.0);
                    out.screen_line(p, Point::new(0.0, -9.0), Point::new(0.0, 9.0), 1.0, 1.0);
                }
                Item::Crop(points) => {
                    if out.crop.is_some() {
                        return Err(OverlayError::BadCrop);
                    }
                    if !points.iter().all(|p| p.x.is_finite() && p.y.is_finite()) {
                        return Err(OverlayError::BadPoint);
                    }
                    let not_collinear = (0..4).any(|i| {
                        let a = points[i];
                        let b = points[(i + 1) % 4];
                        let c = points[(i + 2) % 4];
                        ((b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x)).abs() > 0.001
                    });
                    if !not_collinear {
                        return Err(OverlayError::BadCrop);
                    }
                    out.crop = Some(points);
                    out.path(&points, true, 2.0, 1.5);
                    for f in [1.0 / 3.0, 2.0 / 3.0] {
                        let lerp = |a: Point, b: Point| {
                            Point::new(a.x + (b.x - a.x) * f, a.y + (b.y - a.y) * f)
                        };
                        out.line(
                            lerp(points[0], points[1]),
                            lerp(points[3], points[2]),
                            0.0,
                            3.0,
                            1.0,
                        );
                        out.line(
                            lerp(points[0], points[3]),
                            lerp(points[1], points[2]),
                            0.0,
                            3.0,
                            1.0,
                        );
                    }
                    for p in points {
                        out.square(p, 7.0, 1.0);
                    }
                }
                Item::Caret { x, y, height } => {
                    Self::valid_rect(x, y, 0.0, height)?;
                    out.line(Point::new(x, y), Point::new(x, y + height), 0.0, 2.0, 3.0);
                    out.line(Point::new(x, y), Point::new(x, y + height), 0.0, 5.0, 1.0);
                }
                Item::Underline { from, to } => {
                    Self::valid_point(from)?;
                    Self::valid_point(to)?;
                    out.line(from, to, 0.0, 2.0, 2.0);
                    out.line(from, to, 0.0, 5.0, 1.0);
                }
                Item::FloodHandle(p) => {
                    Self::valid_point(p)?;
                    out.quad(p, 17.0, 5.0);
                    out.quad(p, 15.0, 2.0);
                    for (dx, dy) in [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)] {
                        let tip = Point::new(dx * 5.5, dy * 5.5);
                        out.screen_line(p, Point::new(0.0, 0.0), tip, 5.0, 1.2);
                        out.screen_line(
                            p,
                            Point::new(tip.x - dx * 2.5 - dy * 2.5, tip.y - dy * 2.5 + dx * 2.5),
                            tip,
                            5.0,
                            1.2,
                        );
                        out.screen_line(
                            p,
                            Point::new(tip.x - dx * 2.5 + dy * 2.5, tip.y - dy * 2.5 - dx * 2.5),
                            tip,
                            5.0,
                            1.2,
                        );
                    }
                }
                Item::ZoomRectangle {
                    x,
                    y,
                    width,
                    height,
                } => {
                    Self::valid_rect(x, y, width, height)?;
                    out.path(
                        &[
                            Point::new(x, y),
                            Point::new(x + width, y),
                            Point::new(x + width, y + height),
                            Point::new(x, y + height),
                        ],
                        true,
                        8.0,
                        1.0,
                    );
                }
            }
        }
        Ok(out)
    }
    fn valid_point(p: Point) -> Result<(), OverlayError> {
        if p.x.is_finite() && p.y.is_finite() {
            Ok(())
        } else {
            Err(OverlayError::BadPoint)
        }
    }
    fn valid_rect(x: f32, y: f32, w: f32, h: f32) -> Result<(), OverlayError> {
        if ![x, y, w, h, x + w, y + h].iter().all(|v| v.is_finite()) || w < 0.0 || h < 0.0 {
            Err(OverlayError::BadRect)
        } else {
            Ok(())
        }
    }
    fn path(&mut self, points: &[Point], closed: bool, kind: f32, width: f32) {
        let mut distance = 0.0;
        let count = if closed {
            points.len()
        } else {
            points.len() - 1
        };
        for i in 0..count {
            let a = points[i];
            let b = points[(i + 1) % points.len()];
            self.line(a, b, distance, kind, width);
            distance += (b.x - a.x).hypot(b.y - a.y);
        }
    }
    fn line(&mut self, a: Point, b: Point, start: f32, kind: f32, width: f32) {
        let length = (b.x - a.x).hypot(b.y - a.y);
        if length < 0.00001 {
            return;
        }
        let nx = -(b.y - a.y) / length;
        let ny = (b.x - a.x) / length;
        let half = width * 0.5 + 1.0;
        let v = |p: Point, side: f32, d: f32| Vertex {
            anchor: [p.x, p.y],
            offset: [nx * half * side, ny * half * side],
            distance: d,
            kind,
            edge: side,
            width,
        };
        let (a0, a1, b0, b1) = (
            v(a, -1.0, start),
            v(a, 1.0, start),
            v(b, -1.0, start + length),
            v(b, 1.0, start + length),
        );
        self.vertices.extend_from_slice(&[a0, a1, b0, b0, a1, b1]);
    }
    fn square(&mut self, p: Point, size: f32, _kind: f32) {
        self.quad(p, size, 1.0);
        self.quad(p, size - 2.0, 2.0);
    }
    fn screen_line(&mut self, p: Point, a: Point, b: Point, kind: f32, width: f32) {
        let length = (b.x - a.x).hypot(b.y - a.y);
        if length < 0.00001 {
            return;
        }
        let nx = -(b.y - a.y) / length;
        let ny = (b.x - a.x) / length;
        let half = width * 0.5 + 1.0;
        let v = |q: Point, side: f32| Vertex {
            anchor: [p.x, p.y],
            offset: [q.x + nx * half * side, q.y + ny * half * side],
            distance: 0.0,
            kind,
            edge: side,
            width,
        };
        let (a0, a1, b0, b1) = (v(a, -1.0), v(a, 1.0), v(b, -1.0), v(b, 1.0));
        self.vertices.extend_from_slice(&[a0, a1, b0, b0, a1, b1]);
    }
    fn quad(&mut self, p: Point, size: f32, kind: f32) {
        let h = size / 2.0;
        let v = |x: f32, y: f32| Vertex {
            anchor: [p.x, p.y],
            offset: [x, y],
            distance: 0.0,
            kind,
            edge: 0.0,
            width: 0.0,
        };
        self.vertices.extend_from_slice(&[
            v(-h, -h),
            v(h, -h),
            v(-h, h),
            v(-h, h),
            v(h, -h),
            v(h, h),
        ]);
    }
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniform {
    view: [f32; 4],
    dash: [f32; 4],
    crop: [[f32; 4]; 4],
}

pub struct OverlayGpu {
    scene: Option<Arc<Overlay>>,
    vertices: Option<wgpu::Buffer>,
    count: u32,
    uniform: wgpu::Buffer,
    bind: wgpu::BindGroup,
    lines: wgpu::RenderPipeline,
    dim: wgpu::RenderPipeline,
    dash: f32,
    scale: f32,
    size: [f32; 2],
}
impl OverlayGpu {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("overlay shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("overlay.wgsl").into()),
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("overlay uniform"),
            size: std::mem::size_of::<Uniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("overlay layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("overlay bind"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("overlay pipeline layout"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let target = [Some(wgpu::ColorTargetState {
            format,
            blend: Some(wgpu::BlendState::ALPHA_BLENDING),
            write_mask: wgpu::ColorWrites::ALL,
        })];
        let lines = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("overlay lines"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_line"),
                compilation_options: Default::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: 32,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![
                        0 => Float32x2, 1 => Float32x2, 2 => Float32,
                        3 => Float32, 4 => Float32, 5 => Float32
                    ],
                }],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_line"),
                compilation_options: Default::default(),
                targets: &target,
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        let dim = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("overlay crop dim"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_dim"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_dim"),
                compilation_options: Default::default(),
                targets: &target,
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        Self {
            scene: None,
            vertices: None,
            count: 0,
            uniform,
            bind,
            lines,
            dim,
            dash: 0.0,
            scale: 1.0,
            size: [1.0; 2],
        }
    }
    pub fn set(
        &mut self,
        device: &wgpu::Device,
        scene: Option<Arc<Overlay>>,
        dash: f32,
        scale: f32,
        width: u32,
        height: u32,
    ) {
        let changed = match (&self.scene, &scene) {
            (Some(a), Some(b)) => !Arc::ptr_eq(a, b),
            (None, None) => false,
            _ => true,
        };
        if changed {
            self.count = scene.as_ref().map_or(0, |s| s.vertices.len() as u32);
            self.vertices = scene.as_ref().filter(|s| !s.vertices.is_empty()).map(|s| {
                device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("overlay geometry"),
                    contents: bytemuck::cast_slice(&s.vertices),
                    usage: wgpu::BufferUsages::VERTEX,
                })
            });
            self.scene = scene;
        }
        self.dash = dash;
        self.scale = scale;
        self.size = [width.max(1) as f32, height.max(1) as f32];
    }
    pub fn encode(
        &self,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        view: crate::presenter::ViewParams,
    ) {
        let Some(scene) = self.scene.as_ref() else {
            return;
        };
        let mut raw = Uniform {
            view: [view.origin_x, view.origin_y, view.zoom, self.scale],
            dash: [self.dash, 0.0, self.size[0], self.size[1]],
            crop: [[0.0; 4]; 4],
        };
        if let Some(crop) = scene.crop {
            for (i, p) in crop.iter().enumerate() {
                raw.crop[i] = [p.x, p.y, 0.0, 0.0];
            }
        }
        queue.write_buffer(&self.uniform, 0, bytemuck::bytes_of(&raw));
        if self.count == 0 && scene.crop.is_none() {
            return;
        }
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("overlay"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        pass.set_bind_group(0, &self.bind, &[]);
        if scene.crop.is_some() {
            pass.set_pipeline(&self.dim);
            pass.draw(0..3, 0..1);
        }
        if let Some(vertices) = self.vertices.as_ref() {
            pass.set_pipeline(&self.lines);
            pass.set_vertex_buffer(0, vertices.slice(..));
            pass.draw(0..self.count, 0..1);
        }
    }
}
