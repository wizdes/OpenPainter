use crate::effect::*;

pub fn spec() -> EffectSpec {
    EffectSpec {
        id: "effect.adjust.invert_colors",
        category: "Adjustments",
        label_key: "effect.invert_colors",
        schema: ParamSchema::default(),
        contract: ArithmeticContract {
            alpha_form: AlphaForm::Straight,
            working_space: WorkingSpace::Encoded,
            quantization_points: &["output byte"],
            rounding: RoundingRule::NearestHalfUp,
            border: BorderRule::Clamp,
            target: "legacy-3.36",
        },
        prepare: prepare_noop,
        required_input: pointwise_input,
        passes: &[PassKind::Pointwise],
        render_cpu: |sample, x, y, _, _| {
            let [r, g, b, a] = sample(x, y);
            [255 - r, 255 - g, 255 - b, a]
        },
        gpu: Some(GpuKernel::InvertColors),
    }
}
