use crate::effect::*;

pub fn spec() -> EffectSpec {
    EffectSpec {
        id: "effect.adjust.invert_alpha",
        category: "Adjustments",
        label_key: "effect.invert_alpha",
        schema: ParamSchema::default(),
        contract: ArithmeticContract {
            alpha_form: AlphaForm::Straight,
            working_space: WorkingSpace::Encoded,
            quantization_points: &["output byte"],
            rounding: RoundingRule::NearestHalfUp,
            border: BorderRule::Clamp,
            target: "own",
        },
        prepare: prepare_noop,
        required_input: pointwise_input,
        passes: &[PassKind::Pointwise],
        render_cpu: |sample, x, y, _, _| {
            let [r, g, b, a] = sample(x, y);
            [r, g, b, 255 - a]
        },
        gpu: Some(GpuKernel::InvertAlpha),
    }
}
