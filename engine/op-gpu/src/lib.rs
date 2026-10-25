//! Integer-addressed GPU image operations. RGBA8 stays encoded and straight in storage.
pub mod gaussian;
#[cfg(target_os = "macos")]
pub mod layer;
pub mod presenter;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use thiserror::Error;
use wgpu::util::DeviceExt;

#[derive(Debug, Error)]
pub enum GpuError {
    #[error("no usable GPU adapter: {0}")]
    NoAdapter(String),
    #[error("GPU device was lost")]
    DeviceLost,
    #[error("invalid image dimensions or byte count")]
    InvalidImage,
    #[error("GPU buffer mapping failed: {0}")]
    Map(String),
}
pub type Result<T> = std::result::Result<T, GpuError>;

#[derive(Debug, Clone)]
pub struct GpuInfo {
    pub name: String,
    pub backend: wgpu::Backend,
    pub driver: String,
    pub limits: wgpu::Limits,
    pub features: wgpu::Features,
    pub enabled_features: wgpu::Features,
}

#[derive(Debug, Clone, Copy)]
pub struct CompositeTimings {
    pub upload_ms: f64,
    pub composite_ms: f64,
    pub readback_ms: f64,
}

#[derive(Debug, Clone, Copy)]
pub struct NoiseParams {
    pub intensity: i32,
    pub saturation: i32,
    pub coverage: f64,
    pub seed: i32,
}

pub struct GpuContext {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    info: GpuInfo,
    lost: Arc<AtomicBool>,
    dispatches: u64,
    pipelines: HashMap<&'static str, wgpu::ComputePipeline>,
    upload_ring: UploadRing,
    readback_ring: ReadbackRing,
}
impl GpuContext {
    /// Exact nearest-neighbor FitSurface mapping, including RGB under zero alpha.
    pub fn resample_nearest(
        &mut self,
        sw: u32,
        sh: u32,
        dw: u32,
        dh: u32,
        bytes: &[u8],
    ) -> Result<Vec<u8>> {
        if dw == 0 || dh == 0 {
            return Err(GpuError::InvalidImage);
        }
        let input = self.upload(sw, sh, bytes)?;
        let output = self.texture(dw, dh);
        let acc = self.accumulator(dw, dh);
        let params = self.params(dw, dh, 0, 255, false);
        let module = self
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("resample nearest"),
                source: wgpu::ShaderSource::Wgsl(include_str!("kernels.wgsl").into()),
            });
        let pipeline = self.pipeline(&module, "resample_nearest");
        let bind = self.bind(&pipeline, &input, &acc, &params, &output, None);
        self.dispatch(&pipeline, &bind, dw, dh)?;
        self.readback(&output, dw, dh)
    }
    /// GPU twin of the legacy bicubic fit; one dispatch per destination image.
    pub fn resample_bicubic(
        &mut self,
        sw: u32,
        sh: u32,
        dw: u32,
        dh: u32,
        bytes: &[u8],
    ) -> Result<Vec<u8>> {
        if sw < 2 || sh < 2 || dw < 2 || dh < 2 {
            return self.resample_nearest(sw, sh, dw, dh, bytes);
        }
        let input = self.upload(sw, sh, bytes)?;
        let output = self.texture(dw, dh);
        let acc = self.accumulator(dw, dh);
        let params = self.params(dw, dh, 0, 255, false);
        let module = self
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("resample bicubic"),
                source: wgpu::ShaderSource::Wgsl(include_str!("kernels.wgsl").into()),
            });
        let pipeline = self.pipeline(&module, "resample_bicubic");
        let bind = self.bind(&pipeline, &input, &acc, &params, &output, None);
        self.dispatch(&pipeline, &bind, dw, dh)?;
        self.readback(&output, dw, dh)
    }
    /// GPU twin of the legacy bilinear fit, with one-byte RGB tolerance.
    pub fn resample_bilinear(
        &mut self,
        sw: u32,
        sh: u32,
        dw: u32,
        dh: u32,
        bytes: &[u8],
    ) -> Result<Vec<u8>> {
        if sw < 2 || sh < 2 || dw < 2 || dh < 2 {
            return self.resample_nearest(sw, sh, dw, dh, bytes);
        }
        let input = self.upload(sw, sh, bytes)?;
        let output = self.texture(dw, dh);
        let acc = self.accumulator(dw, dh);
        let params = self.params(dw, dh, 0, 255, false);
        let module = self
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("resample bilinear"),
                source: wgpu::ShaderSource::Wgsl(include_str!("kernels.wgsl").into()),
            });
        let pipeline = self.pipeline(&module, "resample_bilinear");
        let bind = self.bind(&pipeline, &input, &acc, &params, &output, None);
        self.dispatch(&pipeline, &bind, dw, dh)?;
        self.readback(&output, dw, dh)
    }
    /// Area averaging on the GPU for the legacy Super Sampling reduction path.
    pub fn resample_super_sampling(
        &mut self,
        sw: u32,
        sh: u32,
        dw: u32,
        dh: u32,
        bytes: &[u8],
    ) -> Result<Vec<u8>> {
        if sw == dw && sh == dh {
            return self.resample_nearest(sw, sh, dw, dh, bytes);
        }
        if sw <= dw || sh <= dh {
            return self.resample_bicubic(sw, sh, dw, dh, bytes);
        }
        let input = self.upload(sw, sh, bytes)?;
        let output = self.texture(dw, dh);
        let acc = self.accumulator(dw, dh);
        let params = self.params(dw, dh, 0, 255, false);
        let module = self
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("resample super sampling"),
                source: wgpu::ShaderSource::Wgsl(include_str!("kernels.wgsl").into()),
            });
        let pipeline = self.pipeline(&module, "resample_super_sampling");
        let bind = self.bind(&pipeline, &input, &acc, &params, &output, None);
        self.dispatch(&pipeline, &bind, dw, dh)?;
        self.readback(&output, dw, dh)
    }
    /// Runs a registered effect with its legacy default parameters on straight RGBA8 bytes.
    pub fn effect_kernel(
        &mut self,
        kernel: op_ops::effect::GpuKernel,
        width: u32,
        height: u32,
        bytes: &[u8],
    ) -> Result<Vec<u8>> {
        if kernel == op_ops::effect::GpuKernel::AddNoise {
            return self.effect_add_noise(
                width,
                height,
                bytes,
                NoiseParams {
                    intensity: 64,
                    saturation: 100,
                    coverage: 100.0,
                    seed: 0x51ed270b,
                },
            );
        }
        if kernel == op_ops::effect::GpuKernel::Turbulence {
            return self.effect_turbulence(width, height, bytes, 0);
        }
        if kernel == op_ops::effect::GpuKernel::Clouds {
            return self.effect_clouds_seeded(width, height, bytes, 0);
        }
        if kernel == op_ops::effect::GpuKernel::Glow {
            return self.effect_photo_glow(width, height, bytes, 6, 10, 10);
        }
        if kernel == op_ops::effect::GpuKernel::SoftenPortrait {
            return self.effect_photo_soften(width, height, bytes, 5, 0, 10);
        }
        if kernel == op_ops::effect::GpuKernel::Gaussian {
            return self
                .gaussian_timed(width, height, bytes, 2)
                .map(|(pixels, _)| pixels);
        }
        if matches!(
            kernel,
            op_ops::effect::GpuKernel::Curves
                | op_ops::effect::GpuKernel::Levels
                | op_ops::effect::GpuKernel::Exposure
        ) {
            let lut = std::array::from_fn(|_| std::array::from_fn(|value| value as u8));
            return self.effect_kernel_with_lut(kernel, width, height, bytes, &lut, false);
        }
        let values = match kernel {
            op_ops::effect::GpuKernel::Posterize => [16, 16, 16],
            op_ops::effect::GpuKernel::Sepia => [50, 0, 0],
            op_ops::effect::GpuKernel::OilPainting => [3, 50, 0],
            op_ops::effect::GpuKernel::Outline => [3, 50, 0],
            op_ops::effect::GpuKernel::InkSketch => [50, 50, 0],
            op_ops::effect::GpuKernel::PencilSketch => [2, 0, 0],
            op_ops::effect::GpuKernel::EdgeDetect | op_ops::effect::GpuKernel::Relief => [45, 0, 0],
            op_ops::effect::GpuKernel::Pixelate => [2, 0, 0],
            op_ops::effect::GpuKernel::Bulge => [45, 0, 0],
            op_ops::effect::GpuKernel::RedEyeRemoval => [70, 90, 0],
            op_ops::effect::GpuKernel::Vignette => [0, 0, (50 << 16) | 50],
            op_ops::effect::GpuKernel::Sharpen => [2, 0, 0],
            op_ops::effect::GpuKernel::ReduceNoise => [10, 400, 0],
            _ => [0, 0, 0],
        };
        self.effect_kernel_with_params(kernel, width, height, bytes, values)
    }

    /// Runs the provisional integer Turbulence noise with a fixed seed.
    pub fn effect_turbulence(
        &mut self,
        width: u32,
        height: u32,
        bytes: &[u8],
        seed: u32,
    ) -> Result<Vec<u8>> {
        if seed > i32::MAX as u32 {
            return Err(GpuError::InvalidImage);
        }
        let packed = 4 | (128 << 4) | (50 << 14) | (1 << 21);
        self.effect_kernel_with_params(
            op_ops::effect::GpuKernel::Turbulence,
            width,
            height,
            bytes,
            [packed, seed as i32, 0],
        )
    }

    /// Uses the legacy f64 noise field prepared on the CPU, then writes its
    /// default black-to-white palette with integer RGBA8 math on the GPU.
    pub fn effect_clouds_seeded(
        &mut self,
        width: u32,
        height: u32,
        bytes: &[u8],
        seed: u8,
    ) -> Result<Vec<u8>> {
        let input = self.upload(width, height, bytes)?;
        let output = self.texture(width, height);
        let field = op_ops::effects::clouds::reference_rgba8(width, height, seed);
        let values: Vec<[f32; 4]> = field
            .chunks_exact(4)
            .map(|pixel| [f32::from(pixel[0]), 0.0, 0.0, 0.0])
            .collect();
        let table = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("seeded clouds field"),
                contents: bytemuck::cast_slice(&values),
                usage: wgpu::BufferUsages::STORAGE,
            });
        let params = self.params(width, height, 0, 0, false);
        let module = self
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("effect kernels"),
                source: wgpu::ShaderSource::Wgsl(include_str!("kernels.wgsl").into()),
            });
        let pipeline = self.pipeline(&module, "effect_clouds");
        let bind = self.bind(&pipeline, &input, &table, &params, &output, None);
        self.dispatch(&pipeline, &bind, width, height)?;
        self.readback(&output, width, height)
    }

    /// Applies OpenPDN's seeded noise offsets in an rgba8uint compute kernel.
    pub fn effect_add_noise(
        &mut self,
        width: u32,
        height: u32,
        bytes: &[u8],
        params: NoiseParams,
    ) -> Result<Vec<u8>> {
        if !(0..=100).contains(&params.intensity)
            || !(0..=400).contains(&params.saturation)
            || !(0.0..=100.0).contains(&params.coverage)
            || params.seed < 0
        {
            return Err(GpuError::InvalidImage);
        }
        let input = self.upload(width, height, bytes)?;
        let output = self.texture(width, height);
        let deltas = op_ops::effects::add_noise::delta_map(
            width,
            height,
            params.intensity,
            params.saturation,
            params.coverage,
            params.seed,
        );
        let mut values = Vec::with_capacity(deltas.len() * 4);
        for delta in deltas {
            values.extend([delta[0] as f32, delta[1] as f32, delta[2] as f32, 0.0]);
        }
        let table = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("seeded noise offsets"),
                contents: bytemuck::cast_slice(&values),
                usage: wgpu::BufferUsages::STORAGE,
            });
        let params = self.params(width, height, 0, 0, false);
        let module = self
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("effect kernels"),
                source: wgpu::ShaderSource::Wgsl(include_str!("kernels.wgsl").into()),
            });
        let pipeline = self.pipeline(&module, "effect_add_noise");
        let bind = self.bind(&pipeline, &input, &table, &params, &output, None);
        self.dispatch(&pipeline, &bind, width, height)?;
        self.readback(&output, width, height)
    }

    fn photo_blend(
        &mut self,
        width: u32,
        height: u32,
        adjusted: &[u8],
        original: &[u8],
        soften: bool,
        warmth: i32,
    ) -> Result<Vec<u8>> {
        let input = self.upload(width, height, adjusted)?;
        Self::checked_len(width, height, original)?;
        let output = self.texture(width, height);
        let mut source = Vec::with_capacity(original.len());
        for pixel in original.chunks_exact(4) {
            source.extend(pixel.iter().map(|value| f32::from(*value)));
        }
        let table = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("photo source pixels"),
                contents: bytemuck::cast_slice(&source),
                usage: wgpu::BufferUsages::STORAGE,
            });
        let params = self.params(width, height, warmth as u32, 0, false);
        let module = self
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("photo effect kernels"),
                source: wgpu::ShaderSource::Wgsl(include_str!("kernels.wgsl").into()),
            });
        let pipeline = self.pipeline(
            &module,
            if soften {
                "effect_photo_soften_blend"
            } else {
                "effect_photo_glow_blend"
            },
        );
        let bind = self.bind(&pipeline, &input, &table, &params, &output, None);
        self.dispatch(&pipeline, &bind, width, height)?;
        self.readback(&output, width, height)
    }
    pub fn effect_photo_glow(
        &mut self,
        width: u32,
        height: u32,
        bytes: &[u8],
        radius: u32,
        brightness: i32,
        contrast: i32,
    ) -> Result<Vec<u8>> {
        if !(1..=20).contains(&radius)
            || !(-100..=100).contains(&brightness)
            || !(-100..=100).contains(&contrast)
        {
            return Err(GpuError::InvalidImage);
        }
        let blur = self.gaussian_timed(width, height, bytes, radius)?.0;
        let adjusted = self.effect_kernel_with_params(
            op_ops::effect::GpuKernel::BrightnessContrast,
            width,
            height,
            &blur,
            [brightness, contrast, 0],
        )?;
        self.photo_blend(width, height, &adjusted, bytes, false, 0)
    }
    pub fn effect_photo_soften(
        &mut self,
        width: u32,
        height: u32,
        bytes: &[u8],
        softness: u32,
        lighting: i32,
        warmth: i32,
    ) -> Result<Vec<u8>> {
        if softness > 10 || !(-20..=20).contains(&lighting) || !(0..=20).contains(&warmth) {
            return Err(GpuError::InvalidImage);
        }
        let blur = self.gaussian_timed(width, height, bytes, softness * 3)?.0;
        let adjusted = self.effect_kernel_with_params(
            op_ops::effect::GpuKernel::BrightnessContrast,
            width,
            height,
            &blur,
            [lighting, -lighting / 2, 0],
        )?;
        self.photo_blend(width, height, &adjusted, bytes, true, warmth)
    }

    /// Runs a pointwise effect with its three validated integer parameters.
    /// Signed values are passed as two's-complement bits to the WGSL uniform.
    pub fn effect_kernel_with_params(
        &mut self,
        kernel: op_ops::effect::GpuKernel,
        width: u32,
        height: u32,
        bytes: &[u8],
        values: [i32; 3],
    ) -> Result<Vec<u8>> {
        let valid = match kernel {
            op_ops::effect::GpuKernel::BrightnessContrast
            | op_ops::effect::GpuKernel::TemperatureTint
            | op_ops::effect::GpuKernel::HighlightsShadows => {
                (-100..=100).contains(&values[0]) && (-100..=100).contains(&values[1])
            }
            op_ops::effect::GpuKernel::Posterize => {
                values.iter().all(|value| (2..=64).contains(value))
            }
            op_ops::effect::GpuKernel::Sepia => (0..=100).contains(&values[0]),
            op_ops::effect::GpuKernel::OilPainting => {
                (1..=8).contains(&values[0]) && (3..=255).contains(&values[1])
            }
            op_ops::effect::GpuKernel::Outline => {
                (1..=200).contains(&values[0]) && (0..=100).contains(&values[1])
            }
            op_ops::effect::GpuKernel::InkSketch => {
                (0..=99).contains(&values[0]) && (0..=100).contains(&values[1])
            }
            op_ops::effect::GpuKernel::PencilSketch => {
                (1..=20).contains(&values[0]) && (-20..=20).contains(&values[1])
            }
            op_ops::effect::GpuKernel::EdgeDetect
            | op_ops::effect::GpuKernel::Emboss
            | op_ops::effect::GpuKernel::Relief => (-180..=180).contains(&values[0]),
            op_ops::effect::GpuKernel::Pixelate => (1..=100).contains(&values[0]),
            op_ops::effect::GpuKernel::Bulge => (-200..=100).contains(&values[0]),
            op_ops::effect::GpuKernel::RedEyeRemoval => {
                (0..=100).contains(&values[0]) && (0..=100).contains(&values[1])
            }
            op_ops::effect::GpuKernel::Vignette => {
                (-1000..=1000).contains(&values[0])
                    && (-1000..=1000).contains(&values[1])
                    && (values[2] & 0xffff) <= 100
                    && ((values[2] >> 16) & 0xffff) <= 100
            }
            op_ops::effect::GpuKernel::Sharpen => (1..=20).contains(&values[0]),
            op_ops::effect::GpuKernel::ReduceNoise => {
                (0..=200).contains(&values[0]) && (0..=1000).contains(&values[1])
            }
            op_ops::effect::GpuKernel::Turbulence => {
                let packed = values[0] as u32;
                (1..=10).contains(&(packed & 15))
                    && (1..=1000).contains(&((packed >> 4) & 1023))
                    && ((packed >> 14) & 127) <= 100
                    && values[1] >= 0
            }
            op_ops::effect::GpuKernel::Clouds => false,
            _ => true,
        };
        if !valid {
            return Err(GpuError::InvalidImage);
        }
        let input = self.upload(width, height, bytes)?;
        let output = self.texture(width, height);
        let acc = if matches!(
            kernel,
            op_ops::effect::GpuKernel::EdgeDetect
                | op_ops::effect::GpuKernel::Emboss
                | op_ops::effect::GpuKernel::Relief
        ) {
            let angle = (values[0] as f64).to_radians();
            let delta = std::f64::consts::FRAC_PI_4;
            let center = if kernel == op_ops::effect::GpuKernel::Relief {
                1.0
            } else {
                0.0
            };
            let weights = [
                [
                    (angle + delta).cos(),
                    (angle + 2.0 * delta).cos(),
                    (angle + 3.0 * delta).cos(),
                ],
                [angle.cos(), center, (angle + 4.0 * delta).cos()],
                [
                    (angle - delta).cos(),
                    (angle - 2.0 * delta).cos(),
                    (angle - 3.0 * delta).cos(),
                ],
            ];
            let values: Vec<[f32; 4]> = weights
                .into_iter()
                .flatten()
                .map(|weight| [weight as f32, 0.0, 0.0, 0.0])
                .collect();
            self.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("stylize precomputed weights"),
                    contents: bytemuck::cast_slice(&values),
                    usage: wgpu::BufferUsages::STORAGE,
                })
        } else {
            self.accumulator(width, height)
        };
        let params = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("effect params"),
                contents: bytemuck::cast_slice(&[
                    width,
                    height,
                    values[0] as u32,
                    values[1] as u32,
                    values[2] as u32,
                ]),
                usage: wgpu::BufferUsages::UNIFORM,
            });
        let module = self
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("effect kernels"),
                source: wgpu::ShaderSource::Wgsl(include_str!("kernels.wgsl").into()),
            });
        let entry = match kernel {
            op_ops::effect::GpuKernel::InvertColors => "effect_invert_colors",
            op_ops::effect::GpuKernel::InvertAlpha => "effect_invert_alpha",
            op_ops::effect::GpuKernel::BlackAndWhite => "effect_black_and_white",
            op_ops::effect::GpuKernel::BrightnessContrast => "effect_brightness_contrast",
            op_ops::effect::GpuKernel::Posterize => "effect_posterize",
            op_ops::effect::GpuKernel::Sepia => "effect_sepia",
            op_ops::effect::GpuKernel::TemperatureTint => "effect_temperature_tint",
            op_ops::effect::GpuKernel::HighlightsShadows => "effect_highlights_shadows",
            op_ops::effect::GpuKernel::Curves
            | op_ops::effect::GpuKernel::Levels
            | op_ops::effect::GpuKernel::Exposure => return Err(GpuError::InvalidImage),
            op_ops::effect::GpuKernel::Gaussian => "effect_gaussian",
            op_ops::effect::GpuKernel::Fragment => "effect_fragment",
            op_ops::effect::GpuKernel::Median => "effect_median",
            op_ops::effect::GpuKernel::Motion => "effect_motion",
            op_ops::effect::GpuKernel::Radial => "effect_radial",
            op_ops::effect::GpuKernel::Zoom => "effect_zoom",
            op_ops::effect::GpuKernel::EdgeDetect => "effect_edge_detect",
            op_ops::effect::GpuKernel::Emboss => "effect_emboss",
            op_ops::effect::GpuKernel::Relief => "effect_relief",
            op_ops::effect::GpuKernel::OilPainting => "effect_oil_painting",
            op_ops::effect::GpuKernel::Outline => "effect_outline",
            op_ops::effect::GpuKernel::InkSketch => "effect_ink_sketch",
            op_ops::effect::GpuKernel::PencilSketch => "effect_pencil_sketch",
            op_ops::effect::GpuKernel::Pixelate => "effect_pixelate",
            op_ops::effect::GpuKernel::Bulge => "effect_bulge",
            op_ops::effect::GpuKernel::Twist => "effect_twist",
            op_ops::effect::GpuKernel::RedEyeRemoval => "effect_red_eye_removal",
            op_ops::effect::GpuKernel::Vignette => "effect_vignette",
            op_ops::effect::GpuKernel::Sharpen => "effect_sharpen",
            op_ops::effect::GpuKernel::ReduceNoise => "effect_reduce_noise",
            op_ops::effect::GpuKernel::Turbulence => "effect_turbulence",
            op_ops::effect::GpuKernel::Clouds => return Err(GpuError::InvalidImage),
            op_ops::effect::GpuKernel::AddNoise => return Err(GpuError::InvalidImage),
            op_ops::effect::GpuKernel::Glow | op_ops::effect::GpuKernel::SoftenPortrait => {
                return Err(GpuError::InvalidImage);
            }
        };
        let pipeline = self.pipeline(&module, entry);
        let bind = self.bind(&pipeline, &input, &acc, &params, &output, None);
        self.dispatch(&pipeline, &bind, width, height)?;
        self.readback(&output, width, height)
    }

    /// Uploads three 256-entry byte tables through the storage buffer. For
    /// luminosity Curves, the first table maps intensity to a common RGB shift.
    pub fn effect_kernel_with_lut(
        &mut self,
        kernel: op_ops::effect::GpuKernel,
        width: u32,
        height: u32,
        bytes: &[u8],
        lut: &[[u8; 256]; 3],
        luminosity: bool,
    ) -> Result<Vec<u8>> {
        if !matches!(
            kernel,
            op_ops::effect::GpuKernel::Curves
                | op_ops::effect::GpuKernel::Levels
                | op_ops::effect::GpuKernel::Exposure
        ) {
            return Err(GpuError::InvalidImage);
        }
        let input = self.upload(width, height, bytes)?;
        let output = self.texture(width, height);
        let mut values = [0f32; 256 * 4];
        for value in 0..256 {
            for channel in 0..3 {
                values[value * 4 + channel] = f32::from(lut[channel][value]);
            }
        }
        let table = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("effect channel tables"),
                contents: bytemuck::cast_slice(&values),
                usage: wgpu::BufferUsages::STORAGE,
            });
        let params = self.params(width, height, 0, 0, false);
        let module = self
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("effect kernels"),
                source: wgpu::ShaderSource::Wgsl(include_str!("kernels.wgsl").into()),
            });
        let entry = if luminosity {
            "effect_lut_luminosity"
        } else {
            "effect_lut_rgb"
        };
        let pipeline = self.pipeline(&module, entry);
        let bind = self.bind(&pipeline, &input, &table, &params, &output, None);
        self.dispatch(&pipeline, &bind, width, height)?;
        self.readback(&output, width, height)
    }
    pub fn new() -> Result<Self> {
        let instance = wgpu::Instance::default();
        let adapter =
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
                .map_err(|e| GpuError::NoAdapter(e.to_string()))?;
        let info = adapter.get_info();
        let limits = adapter.limits();
        let features = adapter.features();
        let enabled_features = wgpu::Features::empty();
        let desc = wgpu::DeviceDescriptor {
            required_limits: limits.clone(),
            required_features: enabled_features,
            ..Default::default()
        };
        let (device, queue) = pollster::block_on(adapter.request_device(&desc))
            .map_err(|e| GpuError::NoAdapter(e.to_string()))?;
        let lost = Arc::new(AtomicBool::new(false));
        let signal = lost.clone();
        device.set_device_lost_callback(move |_, _| {
            signal.store(true, Ordering::SeqCst);
        });
        Ok(Self {
            device,
            queue,
            info: GpuInfo {
                name: info.name,
                backend: info.backend,
                driver: info.driver,
                limits,
                features,
                enabled_features,
            },
            lost,
            dispatches: 0,
            pipelines: HashMap::new(),
            upload_ring: UploadRing::new(),
            readback_ring: ReadbackRing::new(),
        })
    }
    pub fn info(&self) -> &GpuInfo {
        &self.info
    }
    pub fn dispatch_count(&self) -> u64 {
        self.dispatches
    }
    pub fn recreate(&mut self) -> Result<()> {
        *self = Self::new()?;
        Ok(())
    }
    fn check(&self) -> Result<()> {
        if self.lost.load(Ordering::SeqCst) {
            Err(GpuError::DeviceLost)
        } else {
            Ok(())
        }
    }
    pub fn simulate_device_loss(&self) {
        self.lost.store(true, Ordering::SeqCst);
    }
    fn texture(&self, width: u32, height: u32) -> wgpu::Texture {
        self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("RGBA8 integer tile"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Uint,
            usage: wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::COPY_DST
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::STORAGE_BINDING,
            view_formats: &[],
        })
    }
    fn checked_len(width: u32, height: u32, bytes: &[u8]) -> Result<()> {
        if width == 0 || height == 0 || (width as u64) * (height as u64) * 4 != bytes.len() as u64 {
            Err(GpuError::InvalidImage)
        } else {
            Ok(())
        }
    }
    fn padded_row(width: u32) -> u32 {
        (width * 4).div_ceil(256) * 256
    }
    fn upload(&mut self, width: u32, height: u32, bytes: &[u8]) -> Result<wgpu::Texture> {
        self.check()?;
        Self::checked_len(width, height, bytes)?;
        let texture = self.texture(width, height);
        let row = Self::padded_row(width) as usize;
        let mut padded = vec![0; row * height as usize];
        for y in 0..height as usize {
            padded[y * row..y * row + width as usize * 4]
                .copy_from_slice(&bytes[y * width as usize * 4..(y + 1) * width as usize * 4]);
        }
        let stage = self.upload_ring.next(&self.device, &self.queue, &padded)?;
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        encoder.copy_buffer_to_texture(
            wgpu::TexelCopyBufferInfo {
                buffer: stage,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row as u32),
                    rows_per_image: Some(height),
                },
            },
            texture.as_image_copy(),
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit(Some(encoder.finish()));
        Ok(texture)
    }
    fn readback_pending(
        &mut self,
        texture: &wgpu::Texture,
        width: u32,
        height: u32,
    ) -> Result<PendingReadback> {
        self.check()?;
        let row = Self::padded_row(width) as u64;
        let stage = self
            .readback_ring
            .acquire(&self.device, row * height as u64);
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        encoder.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &stage,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row as u32),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit(Some(encoder.finish()));
        let (sender, receiver) = mpsc::channel();
        stage.map_async(wgpu::MapMode::Read, .., move |result| {
            let _ = sender.send(result);
        });
        Ok(PendingReadback {
            stage,
            receiver,
            width,
            height,
            row: row as usize,
        })
    }
    fn readback(&mut self, texture: &wgpu::Texture, width: u32, height: u32) -> Result<Vec<u8>> {
        let ticket = self.readback_pending(texture, width, height)?;
        let (pixels, stage) = ticket.finish(self)?;
        self.readback_ring.release(stage);
        Ok(pixels)
    }
    pub fn roundtrip_delayed(&mut self, images: &[(u32, u32, Vec<u8>)]) -> Result<Vec<Vec<u8>>> {
        let mut pending = Vec::new();
        for (width, height, bytes) in images {
            let texture = self.upload(*width, *height, bytes)?;
            pending.push(self.readback_pending(&texture, *width, *height)?);
        }
        let mut out = vec![Vec::new(); pending.len()];
        for (index, ticket) in pending.into_iter().enumerate().rev() {
            let (pixels, stage) = ticket.finish(self)?;
            out[index] = pixels;
            self.readback_ring.release(stage);
        }
        Ok(out)
    }
    pub fn roundtrip(&mut self, width: u32, height: u32, bytes: &[u8]) -> Result<Vec<u8>> {
        let texture = self.upload(width, height, bytes)?;
        self.readback(&texture, width, height)
    }
}

struct UploadRing {
    slots: Vec<Option<wgpu::Buffer>>,
    cursor: usize,
    submitted: usize,
}
impl UploadRing {
    fn new() -> Self {
        Self {
            slots: vec![None, None, None],
            cursor: 0,
            submitted: 0,
        }
    }
    fn next<'a>(
        &'a mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        bytes: &[u8],
    ) -> Result<&'a wgpu::Buffer> {
        if self.submitted >= self.slots.len() {
            device
                .poll(wgpu::PollType::wait_indefinitely())
                .map_err(|_| GpuError::DeviceLost)?;
            self.submitted = 0;
        }
        let count = self.slots.len();
        let slot = &mut self.slots[self.cursor];
        if slot
            .as_ref()
            .is_none_or(|buffer| buffer.size() < bytes.len() as u64)
        {
            *slot = Some(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("upload ring slot"),
                size: bytes.len() as u64,
                usage: wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }
        let buffer = slot.as_ref().unwrap();
        queue.write_buffer(buffer, 0, bytes);
        self.cursor = (self.cursor + 1) % count;
        self.submitted += 1;
        Ok(buffer)
    }
}

struct ReadbackRing {
    available: std::collections::VecDeque<wgpu::Buffer>,
}
impl ReadbackRing {
    fn new() -> Self {
        Self {
            available: std::collections::VecDeque::new(),
        }
    }
    fn acquire(&mut self, device: &wgpu::Device, size: u64) -> wgpu::Buffer {
        if let Some(position) = self
            .available
            .iter()
            .position(|buffer| buffer.size() >= size)
        {
            return self.available.remove(position).unwrap();
        }
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback ring slot"),
            size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        })
    }
    fn release(&mut self, buffer: wgpu::Buffer) {
        self.available.push_back(buffer);
    }
}

struct PendingReadback {
    stage: wgpu::Buffer,
    receiver: mpsc::Receiver<std::result::Result<(), wgpu::BufferAsyncError>>,
    width: u32,
    height: u32,
    row: usize,
}
impl PendingReadback {
    fn finish(self, ctx: &GpuContext) -> Result<(Vec<u8>, wgpu::Buffer)> {
        ctx.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|_| GpuError::DeviceLost)?;
        self.receiver
            .recv()
            .map_err(|e| GpuError::Map(e.to_string()))?
            .map_err(|e| GpuError::Map(e.to_string()))?;
        let mapped = self.stage.get_mapped_range(..);
        let width = self.width as usize;
        let height = self.height as usize;
        let mut out = vec![0; width * height * 4];
        for y in 0..height {
            out[y * width * 4..(y + 1) * width * 4]
                .copy_from_slice(&mapped[y * self.row..y * self.row + width * 4]);
        }
        drop(mapped);
        self.stage.unmap();
        ctx.check()?;
        Ok((out, self.stage))
    }
}

use op_core::doc::{LayerBlendMode, ToolCompositeMode};
use op_core::tiles::TileId;
use std::collections::HashMap;

pub const MODES: [LayerBlendMode; 14] = [
    LayerBlendMode::Normal,
    LayerBlendMode::Multiply,
    LayerBlendMode::Additive,
    LayerBlendMode::ColorBurn,
    LayerBlendMode::ColorDodge,
    LayerBlendMode::Reflect,
    LayerBlendMode::Glow,
    LayerBlendMode::Overlay,
    LayerBlendMode::Difference,
    LayerBlendMode::Negation,
    LayerBlendMode::Lighten,
    LayerBlendMode::Darken,
    LayerBlendMode::Screen,
    LayerBlendMode::Xor,
];
fn mode_index(mode: LayerBlendMode) -> u32 {
    MODES.iter().position(|&m| m == mode).unwrap() as u32
}
#[derive(Clone, Copy)]
pub struct LayerInput<'a> {
    pub bytes: &'a [u8],
    pub mode: LayerBlendMode,
    pub opacity: u8,
}

impl GpuContext {
    fn pipeline(
        &mut self,
        module: &wgpu::ShaderModule,
        entry: &'static str,
    ) -> wgpu::ComputePipeline {
        if let Some(pipeline) = self.pipelines.get(entry) {
            return pipeline.clone();
        }
        let layout = self
            .device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("kernel bindings"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Uint,
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: false },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 3,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::StorageTexture {
                            access: wgpu::StorageTextureAccess::WriteOnly,
                            format: wgpu::TextureFormat::Rgba8Uint,
                            view_dimension: wgpu::TextureViewDimension::D2,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 4,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Uint,
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                ],
            });
        let pipeline_layout = self
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("kernel layout"),
                bind_group_layouts: &[&layout],
                push_constant_ranges: &[],
            });
        let pipeline = self
            .device
            .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(entry),
                layout: Some(&pipeline_layout),
                module,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                cache: None,
            });
        self.pipelines.insert(entry, pipeline.clone());
        pipeline
    }
    fn bind(
        &self,
        pipeline: &wgpu::ComputePipeline,
        input: &wgpu::Texture,
        acc: &wgpu::Buffer,
        params: &wgpu::Buffer,
        output: &wgpu::Texture,
        mask: Option<&wgpu::Texture>,
    ) -> wgpu::BindGroup {
        self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("op-gpu bind"),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(
                        &input.create_view(&Default::default()),
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: acc.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: params.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(
                        &output.create_view(&Default::default()),
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::TextureView(
                        &mask.unwrap_or(input).create_view(&Default::default()),
                    ),
                },
            ],
        })
    }
    fn dispatch(
        &mut self,
        pipeline: &wgpu::ComputePipeline,
        bind: &wgpu::BindGroup,
        width: u32,
        height: u32,
    ) -> Result<()> {
        self.check()?;
        let mut encoder = self.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, bind, &[]);
            pass.dispatch_workgroups(width.div_ceil(16), height.div_ceil(16), 1);
        }
        self.queue.submit(Some(encoder.finish()));
        self.dispatches += 1;
        Ok(())
    }
    fn params(
        &self,
        width: u32,
        height: u32,
        mode: u32,
        opacity: u8,
        masked: bool,
    ) -> wgpu::Buffer {
        self.device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("kernel params"),
                contents: bytemuck::cast_slice(&[
                    width,
                    height,
                    mode,
                    opacity as u32,
                    u32::from(masked),
                ]),
                usage: wgpu::BufferUsages::UNIFORM,
            })
    }
    fn accumulator(&self, width: u32, height: u32) -> wgpu::Buffer {
        self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("premultiplied f32 accumulator"),
            size: width as u64 * height as u64 * 16,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }
    pub fn composite(
        &mut self,
        width: u32,
        height: u32,
        layers: &[LayerInput<'_>],
    ) -> Result<Vec<u8>> {
        self.composite_timed(width, height, layers)
            .map(|(pixels, _)| pixels)
    }
    pub fn composite_timed(
        &mut self,
        width: u32,
        height: u32,
        layers: &[LayerInput<'_>],
    ) -> Result<(Vec<u8>, CompositeTimings)> {
        self.composite_timed_inner(width, height, layers, None)
    }
    fn composite_timed_inner(
        &mut self,
        width: u32,
        height: u32,
        layers: &[LayerInput<'_>],
        coverage: Option<&[u8]>,
    ) -> Result<(Vec<u8>, CompositeTimings)> {
        self.check()?;
        if coverage.is_some_and(|bytes| bytes.len() != width as usize * height as usize) {
            return Err(GpuError::InvalidImage);
        }
        let mut timing = CompositeTimings {
            upload_ms: 0.0,
            composite_ms: 0.0,
            readback_ms: 0.0,
        };
        if width == 0 || height == 0 {
            return Err(GpuError::InvalidImage);
        }
        let zero = vec![0; width as usize * height as usize * 4];
        let module = self
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("compositor"),
                source: wgpu::ShaderSource::Wgsl(include_str!("kernels.wgsl").into()),
            });
        let layer_pipeline = self.pipeline(&module, "layer");
        let finish_pipeline = self.pipeline(&module, "finish");
        let acc = self.accumulator(width, height);
        let output = self.texture(width, height);
        let mut last_input = None;
        for (layer_index, layer) in layers.iter().enumerate() {
            let start = std::time::Instant::now();
            let input = self.upload(width, height, layer.bytes)?;
            self.device
                .poll(wgpu::PollType::wait_indefinitely())
                .map_err(|_| GpuError::DeviceLost)?;
            timing.upload_ms += start.elapsed().as_secs_f64() * 1000.0;
            let mask_texture = if layer_index + 1 == layers.len() {
                coverage
                    .map(|bytes| self.upload_mask(width, height, bytes))
                    .transpose()?
            } else {
                None
            };
            let params = self.params(
                width,
                height,
                mode_index(layer.mode),
                layer.opacity,
                mask_texture.is_some(),
            );
            let bind = self.bind(
                &layer_pipeline,
                &input,
                &acc,
                &params,
                &output,
                mask_texture.as_ref(),
            );
            let start = std::time::Instant::now();
            self.dispatch(&layer_pipeline, &bind, width, height)?;
            self.device
                .poll(wgpu::PollType::wait_indefinitely())
                .map_err(|_| GpuError::DeviceLost)?;
            timing.composite_ms += start.elapsed().as_secs_f64() * 1000.0;
            last_input = Some(input);
        }
        let input = match last_input {
            Some(input) => input,
            None => self.upload(width, height, &zero)?,
        };
        let params = self.params(width, height, 0, 255, false);
        let bind = self.bind(&finish_pipeline, &input, &acc, &params, &output, None);
        let start = std::time::Instant::now();
        self.dispatch(&finish_pipeline, &bind, width, height)?;
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|_| GpuError::DeviceLost)?;
        timing.composite_ms += start.elapsed().as_secs_f64() * 1000.0;
        let start = std::time::Instant::now();
        let pixels = self.readback(&output, width, height)?;
        timing.readback_ms = start.elapsed().as_secs_f64() * 1000.0;
        Ok((pixels, timing))
    }
    fn upload_mask(&mut self, width: u32, height: u32, mask: &[u8]) -> Result<wgpu::Texture> {
        if mask.len() != width as usize * height as usize {
            return Err(GpuError::InvalidImage);
        }
        let rgba: Vec<u8> = mask.iter().flat_map(|&value| [value, 0, 0, 0]).collect();
        self.upload(width, height, &rgba)
    }
    pub fn tool_composite_mask(
        &mut self,
        width: u32,
        height: u32,
        dst: &[u8],
        src: &[u8],
        mask: &[u8],
        mode: ToolCompositeMode,
    ) -> Result<Vec<u8>> {
        Self::checked_len(width, height, dst)?;
        Self::checked_len(width, height, src)?;
        if mask.len() != width as usize * height as usize {
            return Err(GpuError::InvalidImage);
        }
        match mode {
            ToolCompositeMode::Normal => self
                .composite_timed_inner(
                    width,
                    height,
                    &[
                        LayerInput {
                            bytes: dst,
                            mode: LayerBlendMode::Normal,
                            opacity: 255,
                        },
                        LayerInput {
                            bytes: src,
                            mode: LayerBlendMode::Normal,
                            opacity: 255,
                        },
                    ],
                    Some(mask),
                )
                .map(|(pixels, _)| pixels),
            ToolCompositeMode::Overwrite => {
                let module = self
                    .device
                    .create_shader_module(wgpu::ShaderModuleDescriptor {
                        label: Some("overwrite"),
                        source: wgpu::ShaderSource::Wgsl(include_str!("kernels.wgsl").into()),
                    });
                let pipeline = self.pipeline(&module, "overwrite");
                let input = self.upload(width, height, src)?;
                let mask_texture = self.upload_mask(width, height, mask)?;
                let output = self.texture(width, height);
                let acc = self.accumulator(width, height);
                let floats: Vec<f32> = dst.iter().map(|&x| x as f32).collect();
                self.queue
                    .write_buffer(&acc, 0, bytemuck::cast_slice(&floats));
                let params = self.params(width, height, 0, 255, true);
                let bind = self.bind(
                    &pipeline,
                    &input,
                    &acc,
                    &params,
                    &output,
                    Some(&mask_texture),
                );
                self.dispatch(&pipeline, &bind, width, height)?;
                self.readback(&output, width, height)
            }
        }
    }
    pub fn tool_composite(
        &mut self,
        width: u32,
        height: u32,
        dst: &[u8],
        src: &[u8],
        mode: ToolCompositeMode,
        coverage: u8,
    ) -> Result<Vec<u8>> {
        Self::checked_len(width, height, dst)?;
        Self::checked_len(width, height, src)?;
        match mode {
            ToolCompositeMode::Normal => self.composite(
                width,
                height,
                &[
                    LayerInput {
                        bytes: dst,
                        mode: LayerBlendMode::Normal,
                        opacity: 255,
                    },
                    LayerInput {
                        bytes: src,
                        mode: LayerBlendMode::Normal,
                        opacity: coverage,
                    },
                ],
            ),
            ToolCompositeMode::Overwrite => {
                let module = self
                    .device
                    .create_shader_module(wgpu::ShaderModuleDescriptor {
                        label: Some("overwrite"),
                        source: wgpu::ShaderSource::Wgsl(include_str!("kernels.wgsl").into()),
                    });
                let pipeline = self.pipeline(&module, "overwrite");
                let input = self.upload(width, height, src)?;
                let output = self.texture(width, height);
                let acc = self.accumulator(width, height);
                let floats: Vec<f32> = dst
                    .chunks_exact(4)
                    .flat_map(|p| p.iter().map(|&x| x as f32))
                    .collect();
                self.queue
                    .write_buffer(&acc, 0, bytemuck::cast_slice(&floats));
                let params = self.params(width, height, 0, coverage, false);
                let bind = self.bind(&pipeline, &input, &acc, &params, &output, None);
                self.dispatch(&pipeline, &bind, width, height)?;
                self.readback(&output, width, height)
            }
        }
    }
    pub fn box_blur(&mut self, width: u32, height: u32, bytes: &[u8]) -> Result<Vec<u8>> {
        let input = self.upload(width, height, bytes)?;
        let output = self.texture(width, height);
        let acc = self.accumulator(width, height);
        let params = self.params(width, height, 0, 255, false);
        let module = self
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("box blur"),
                source: wgpu::ShaderSource::Wgsl(include_str!("kernels.wgsl").into()),
            });
        let pipeline = self.pipeline(&module, "box_blur");
        let bind = self.bind(&pipeline, &input, &acc, &params, &output, None);
        self.dispatch(&pipeline, &bind, width, height)?;
        self.readback(&output, width, height)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Representation {
    Rgba8,
}
pub struct GpuTileCache {
    budget: usize,
    used: usize,
    tick: u64,
    tiles: HashMap<(TileId, Representation), (wgpu::Texture, u64)>,
}
impl GpuTileCache {
    pub fn new(budget: usize) -> Self {
        Self {
            budget,
            used: 0,
            tick: 0,
            tiles: HashMap::new(),
        }
    }
    pub fn bytes(&self) -> usize {
        self.used
    }
    pub fn len(&self) -> usize {
        self.tiles.len()
    }
    pub fn is_empty(&self) -> bool {
        self.tiles.is_empty()
    }
    pub fn contains(&self, id: TileId, repr: Representation) -> bool {
        self.tiles.contains_key(&(id, repr))
    }
    pub fn get_or_upload(
        &mut self,
        gpu: &mut GpuContext,
        id: TileId,
        repr: Representation,
        bytes: &[u8],
    ) -> Result<&wgpu::Texture> {
        const SIZE: usize = 256 * 256 * 4;
        if self.budget < SIZE {
            return Err(GpuError::InvalidImage);
        }
        self.tick += 1;
        let key = (id, repr);
        if let Some((_, stamp)) = self.tiles.get_mut(&key) {
            *stamp = self.tick;
        } else {
            while self.used + SIZE > self.budget {
                let oldest = self
                    .tiles
                    .iter()
                    .min_by_key(|(_, (_, stamp))| stamp)
                    .map(|(key, _)| *key)
                    .unwrap();
                self.tiles.remove(&oldest);
                self.used -= SIZE;
            }
            let texture = gpu.upload(256, 256, bytes)?;
            self.tiles.insert(key, (texture, self.tick));
            self.used += SIZE;
        }
        Ok(&self.tiles.get(&key).unwrap().0)
    }
}
pub mod parity;
