//! Fixed-fixture GPU parity runner. Each operation must submit compute work.
use crate::{GpuContext, GpuError, LayerInput, MODES};
use op_core::doc::{LayerBlendMode, ToolCompositeMode};
use op_core::pixel::Rgba8;
use op_ops::composite::{composite_pixel, tool_composite};
use op_ops::effect::{GpuKernel, Prepared, find as find_effect, registered as registered_effects};

pub struct Inputs {
    pub width: u32,
    pub height: u32,
    pub bottom: Vec<u8>,
    pub top: Vec<u8>,
}
pub trait GpuOp {
    fn id(&self) -> String;
    fn cpu(&self, inputs: &Inputs) -> Vec<u8>;
    fn gpu(&self, ctx: &mut GpuContext, inputs: &Inputs) -> crate::Result<Vec<u8>>;
    fn compare_hidden_color(&self) -> bool {
        false
    }
}
pub struct EffectOp {
    pub kernel: GpuKernel,
    pub effect_id: &'static str,
}
impl GpuOp for EffectOp {
    fn id(&self) -> String {
        self.effect_id.into()
    }
    fn cpu(&self, inputs: &Inputs) -> Vec<u8> {
        if self.kernel == GpuKernel::Gaussian {
            return gaussian_reference(inputs.width, inputs.height, &inputs.bottom);
        }
        if matches!(
            self.kernel,
            GpuKernel::Fragment
                | GpuKernel::Median
                | GpuKernel::Motion
                | GpuKernel::Radial
                | GpuKernel::Zoom
        ) {
            return op_ops::effect::reference_rgba8(
                self.effect_id,
                inputs.width,
                inputs.height,
                &inputs.bottom,
            )
            .unwrap();
        }
        let spec = find_effect(self.effect_id).expect("registered effect");
        let params = spec.default_params();
        let mut out = inputs.bottom.clone();
        for pixel in out.chunks_exact_mut(4) {
            let original = [pixel[0], pixel[1], pixel[2], pixel[3]];
            pixel.copy_from_slice(&(spec.render_cpu)(
                &|_, _| original,
                0,
                0,
                &Prepared::default(),
                &params,
            ));
        }
        out
    }
    fn gpu(&self, ctx: &mut GpuContext, inputs: &Inputs) -> crate::Result<Vec<u8>> {
        ctx.effect_kernel(self.kernel, inputs.width, inputs.height, &inputs.bottom)
    }
    fn compare_hidden_color(&self) -> bool {
        true
    }
}
pub fn gaussian_reference(width: u32, height: u32, bytes: &[u8]) -> Vec<u8> {
    op_ops::effects::gaussian_blur::reference_rgba8(width, height, bytes)
}
pub struct CompositeOp {
    pub mode: LayerBlendMode,
    pub opacity: u8,
}
impl GpuOp for CompositeOp {
    fn id(&self) -> String {
        format!("composite.{:?}", self.mode)
    }
    fn cpu(&self, inputs: &Inputs) -> Vec<u8> {
        inputs
            .bottom
            .chunks_exact(4)
            .zip(inputs.top.chunks_exact(4))
            .flat_map(|(b, s)| {
                <[u8; 4]>::from(composite_pixel(
                    Rgba8::from([b[0], b[1], b[2], b[3]]),
                    Rgba8::from([s[0], s[1], s[2], s[3]]),
                    self.mode,
                    self.opacity,
                ))
            })
            .collect()
    }
    fn gpu(&self, ctx: &mut GpuContext, inputs: &Inputs) -> crate::Result<Vec<u8>> {
        ctx.composite(
            inputs.width,
            inputs.height,
            &[
                LayerInput {
                    bytes: &inputs.bottom,
                    mode: LayerBlendMode::Normal,
                    opacity: 255,
                },
                LayerInput {
                    bytes: &inputs.top,
                    mode: self.mode,
                    opacity: self.opacity,
                },
            ],
        )
    }
}
pub struct ToolOp {
    pub mode: ToolCompositeMode,
    pub coverage: u8,
}
impl GpuOp for ToolOp {
    fn id(&self) -> String {
        format!("tool.{:?}", self.mode)
    }
    fn cpu(&self, inputs: &Inputs) -> Vec<u8> {
        inputs
            .bottom
            .chunks_exact(4)
            .zip(inputs.top.chunks_exact(4))
            .flat_map(|(b, s)| {
                <[u8; 4]>::from(tool_composite(
                    Rgba8::from([b[0], b[1], b[2], b[3]]),
                    Rgba8::from([s[0], s[1], s[2], s[3]]),
                    self.mode,
                    self.coverage,
                ))
            })
            .collect()
    }
    fn gpu(&self, ctx: &mut GpuContext, inputs: &Inputs) -> crate::Result<Vec<u8>> {
        ctx.tool_composite(
            inputs.width,
            inputs.height,
            &inputs.bottom,
            &inputs.top,
            self.mode,
            self.coverage,
        )
    }
}
pub struct BoxBlurOp;
pub struct ResampleNearestOp;
pub struct ResampleBicubicOp;
pub struct ResampleBilinearOp;
pub struct ResampleSuperSamplingOp;
impl GpuOp for ResampleSuperSamplingOp {
    fn id(&self) -> String {
        "image.resize.super_sampling".into()
    }
    fn cpu(&self, inputs: &Inputs) -> Vec<u8> {
        op_ops::image::legacy::resize(
            &inputs.bottom,
            inputs.width,
            inputs.height,
            inputs.width,
            inputs.height,
            op_ops::image::Resampling::SuperSampling,
        )
    }
    fn gpu(&self, ctx: &mut GpuContext, inputs: &Inputs) -> crate::Result<Vec<u8>> {
        ctx.resample_super_sampling(
            inputs.width,
            inputs.height,
            inputs.width,
            inputs.height,
            &inputs.bottom,
        )
    }
    fn compare_hidden_color(&self) -> bool {
        true
    }
}
impl GpuOp for ResampleBilinearOp {
    fn id(&self) -> String {
        "image.resize.bilinear".into()
    }
    fn cpu(&self, inputs: &Inputs) -> Vec<u8> {
        op_ops::image::legacy::resize(
            &inputs.bottom,
            inputs.width,
            inputs.height,
            inputs.width,
            inputs.height,
            op_ops::image::Resampling::Bilinear,
        )
    }
    fn gpu(&self, ctx: &mut GpuContext, inputs: &Inputs) -> crate::Result<Vec<u8>> {
        ctx.resample_bilinear(
            inputs.width,
            inputs.height,
            inputs.width,
            inputs.height,
            &inputs.bottom,
        )
    }
}
impl GpuOp for ResampleBicubicOp {
    fn id(&self) -> String {
        "image.resize.bicubic".into()
    }
    fn cpu(&self, inputs: &Inputs) -> Vec<u8> {
        op_ops::image::legacy::resize(
            &inputs.bottom,
            inputs.width,
            inputs.height,
            inputs.width,
            inputs.height,
            op_ops::image::Resampling::Bicubic,
        )
    }
    fn gpu(&self, ctx: &mut GpuContext, inputs: &Inputs) -> crate::Result<Vec<u8>> {
        ctx.resample_bicubic(
            inputs.width,
            inputs.height,
            inputs.width,
            inputs.height,
            &inputs.bottom,
        )
    }
}
impl GpuOp for ResampleNearestOp {
    fn id(&self) -> String {
        "image.resize.nearest".into()
    }
    fn cpu(&self, inputs: &Inputs) -> Vec<u8> {
        op_ops::image::legacy::resize(
            &inputs.bottom,
            inputs.width,
            inputs.height,
            inputs.width,
            inputs.height,
            op_ops::image::Resampling::Nearest,
        )
    }
    fn gpu(&self, ctx: &mut GpuContext, inputs: &Inputs) -> crate::Result<Vec<u8>> {
        ctx.resample_nearest(
            inputs.width,
            inputs.height,
            inputs.width,
            inputs.height,
            &inputs.bottom,
        )
    }
    fn compare_hidden_color(&self) -> bool {
        true
    }
}
impl GpuOp for BoxBlurOp {
    fn id(&self) -> String {
        "box_blur".into()
    }
    fn cpu(&self, inputs: &Inputs) -> Vec<u8> {
        let mut out = vec![0; inputs.bottom.len()];
        for y in 0..inputs.height as i32 {
            for x in 0..inputs.width as i32 {
                for channel in 0..4 {
                    let mut sum = 0u32;
                    for dy in -1..=1 {
                        for dx in -1..=1 {
                            let xx = (x + dx).clamp(0, inputs.width as i32 - 1) as usize;
                            let yy = (y + dy).clamp(0, inputs.height as i32 - 1) as usize;
                            sum += inputs.bottom[(yy * inputs.width as usize + xx) * 4 + channel]
                                as u32;
                        }
                    }
                    out[(y as usize * inputs.width as usize + x as usize) * 4 + channel] =
                        ((sum + 4) / 9) as u8;
                }
            }
        }
        out
    }
    fn gpu(&self, ctx: &mut GpuContext, inputs: &Inputs) -> crate::Result<Vec<u8>> {
        ctx.box_blur(inputs.width, inputs.height, &inputs.bottom)
    }
}
pub fn registered() -> Vec<Box<dyn GpuOp>> {
    let mut ops: Vec<Box<dyn GpuOp>> = MODES
        .into_iter()
        .map(|mode| Box::new(CompositeOp { mode, opacity: 192 }) as Box<dyn GpuOp>)
        .collect();
    ops.push(Box::new(ToolOp {
        mode: ToolCompositeMode::Normal,
        coverage: 128,
    }));
    ops.push(Box::new(ToolOp {
        mode: ToolCompositeMode::Overwrite,
        coverage: 128,
    }));
    ops.push(Box::new(BoxBlurOp));
    ops.push(Box::new(ResampleNearestOp));
    ops.push(Box::new(ResampleBicubicOp));
    ops.push(Box::new(ResampleBilinearOp));
    ops.push(Box::new(ResampleSuperSamplingOp));
    for effect in registered_effects() {
        if let Some(kernel) = effect.gpu {
            ops.push(Box::new(EffectOp {
                kernel,
                effect_id: effect.id,
            }));
        }
    }
    ops
}
pub fn run(ctx: &mut GpuContext, inputs: &Inputs) -> crate::Result<Vec<(String, u8, u64)>> {
    let mut results = Vec::new();
    for op in registered() {
        let before = ctx.dispatch_count();
        let expected = op.cpu(inputs);
        let actual = op.gpu(ctx, inputs)?;
        if actual.len() != expected.len() {
            return Err(GpuError::Map(format!(
                "{} returned {} bytes, expected {}",
                op.id(),
                actual.len(),
                expected.len()
            )));
        }
        let dispatches = ctx.dispatch_count() - before;
        if dispatches == 0 {
            return Err(GpuError::Map(format!("{} submitted no dispatch", op.id())));
        }
        let mut worst = 0;
        for (a, b) in expected.chunks_exact(4).zip(actual.chunks_exact(4)) {
            if !op.compare_hidden_color() && a[3] == 0 && b[3] == 0 {
                continue;
            }
            for i in 0..4 {
                worst = worst.max(a[i].abs_diff(b[i]));
            }
        }
        println!(
            "{} adapter={} backend={:?} dispatches={} worst={}",
            op.id(),
            ctx.info().name,
            ctx.info().backend,
            dispatches,
            worst
        );
        results.push((op.id(), worst, dispatches));
    }
    Ok(results)
}
