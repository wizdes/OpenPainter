import COpPresentSpike
import Foundation

public struct SpikeError: Error, CustomStringConvertible {
    public let description: String
    public init(_ d: String) { description = d }
}

/// Throws with the Rust side's last error message when a call fails.
@discardableResult
func check(_ status: OpPsStatus, _ what: String) throws -> OpPsStatus {
    if status == 0 { return status }
    var buf = [CChar](repeating: 0, count: 512)
    _ = op_ps_last_error(&buf, buf.count)
    throw SpikeError("\(what) failed with status \(status): \(String(cString: buf))")
}

func cString<T>(_ tuple: T) -> String {
    withUnsafeBytes(of: tuple) { raw in
        String(cString: raw.bindMemory(to: CChar.self).baseAddress!)
    }
}

/// One shared GPU context (instance, device, queue, error handler) for all presenters.
public final class EngineContext {
    public let raw: OpaquePointer

    public init() throws {
        var out: OpaquePointer?
        try check(op_ps_context_new(&out), "op_ps_context_new")
        raw = out!
    }

    deinit { op_ps_context_free(raw) }

    public func maintain() throws { try check(op_ps_context_maintain(raw), "maintain") }

    public func report() throws -> OpPsReport {
        var r = OpPsReport()
        try check(op_ps_context_report(raw, &r), "report")
        return r
    }
}

extension OpPsReport: @retroactive CustomStringConvertible {
    public var description: String {
        "presenters \(live_presenters), ring textures \(live_ring_textures), surfaces \(live_surfaces), "
            + "wgpu textures \(wgpu_textures), views \(wgpu_texture_views), buffers \(wgpu_buffers), "
            + "bind groups \(wgpu_bind_groups), render pipelines \(wgpu_render_pipelines), "
            + "texture bytes \(wgpu_texture_memory), validation errors \(validation_errors)"
    }

    /// The counts that must return to baseline after presenters are destroyed.
    public var liveCounts: [UInt64] {
        [live_presenters, live_ring_textures, live_surfaces, wgpu_textures, wgpu_texture_views,
         wgpu_buffers, wgpu_bind_groups, wgpu_render_pipelines, wgpu_texture_memory]
    }
}
