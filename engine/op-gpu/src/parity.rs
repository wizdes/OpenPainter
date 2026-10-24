//! Fixed-fixture GPU parity runner. Each operation must submit compute work.
use crate::{GpuContext, GpuError, LayerInput, MODES};
use op_core::doc::{LayerBlendMode, ToolCompositeMode};
use op_core::pixel::Rgba8;
use op_ops::composite::{composite_pixel, tool_composite};
use op_ops::effect::{GpuKernel, registered as registered_effects};

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
        if !matches!(
            self.kernel,
            GpuKernel::InvertColors | GpuKernel::InvertAlpha
        ) {
            return op_ops::effect::reference_rgba8(
                self.effect_id,
                inputs.width,
                inputs.height,
                &inputs.bottom,
            )
            .unwrap();
        }
        let mut out = inputs.bottom.clone();
        for pixel in out.chunks_exact_mut(4) {
            match self.kernel {
                GpuKernel::InvertColors => {
                    pixel[..3].iter_mut().for_each(|byte| *byte = 255 - *byte)
                }
                GpuKernel::InvertAlpha => pixel[3] = 255 - pixel[3],
                GpuKernel::Gaussian => unreachable!(),
                GpuKernel::Fragment
                | GpuKernel::Median
                | GpuKernel::Motion
                | GpuKernel::Radial
                | GpuKernel::Zoom => unreachable!(),
            }
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
