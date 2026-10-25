// GPU pipelines the process has made (M58 lazy-pipeline checks): see
// op_gpu_pipeline_counts in openpainter.h.
import COpenPainter

public struct PipelineCounts: Equatable, Sendable, CustomStringConvertible {
    /// Compute pipelines for effect and resampling kernels.
    public var effect: UInt64
    /// Canvas view render pipelines (one per attached view).
    public var view: UInt64
    /// Mip builder render pipelines (made at the first draw below 100%).
    public var mip: UInt64

    public init(effect: UInt64 = 0, view: UInt64 = 0, mip: UInt64 = 0) {
        self.effect = effect
        self.view = view
        self.mip = mip
    }

    /// The process-wide counts now. Needs no engine.
    public static func now() throws -> PipelineCounts {
        var c = OpPipelineCounts()
        if let error = EngineError(op_gpu_pipeline_counts(&c)) { throw error }
        return PipelineCounts(effect: c.effect, view: c.view, mip: c.mip)
    }

    /// The counts as the app prints them on its `OP_READY` line (scripts/launch-time.sh).
    public var description: String { "effect=\(effect) view=\(view) mip=\(mip)" }
}
