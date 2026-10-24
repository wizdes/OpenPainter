// Ported from OpenPDN 3.36 at cca476b (MIT), ColorBgra.GetIntensityByte and
// UnaryPixelOps.Desaturate. Copyright dotPDN LLC and contributors.
use crate::effect::*;
use crate::effects::intensity;

pub fn spec() -> EffectSpec {
    EffectSpec {
        id: "effect.adjust.black_and_white",
        category: "Adjustments",
        label_key: "effect.black_and_white",
        schema: ParamSchema::default(),
        contract: ArithmeticContract {
            alpha_form: AlphaForm::Straight,
            working_space: WorkingSpace::Encoded,
            quantization_points: &["intensity truncation"],
            rounding: RoundingRule::NearestHalfUp,
            border: BorderRule::Clamp,
            target: "legacy-3.36",
        },
        prepare: prepare_noop,
        required_input: pointwise_input,
        passes: &[PassKind::Pointwise],
        render_cpu: |sample, x, y, _, _| {
            let pixel = sample(x, y);
            let value = intensity(pixel);
            [value, value, value, pixel[3]]
        },
        gpu: Some(GpuKernel::BlackAndWhite),
    }
}
