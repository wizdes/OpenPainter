// M58 launch probe for scripts/launch-time.sh --first-effect.
//
// With OP_LAUNCH_PROBE=first-effect in the environment, the app runs one tiny
// Gaussian blur on the GPU right after OP_READY and prints
// "OP_FIRST_EFFECT <ms> gpu=<0|1>", where <ms> is milliseconds since the
// process started. The blur starts the effect GPU device and compiles the blur
// kernels, so this line times the first GPU effect a user can run. Without the
// variable nothing happens. A failure prints "OP_FIRST_EFFECT_FAILED <reason>".
import EngineBridge
import Foundation

public enum LaunchProbe {
    /// The environment value that turns the probe on.
    public static let firstEffect = "first-effect"

    /// True when the environment asks for the first-effect probe.
    public static func wantsFirstEffect(_ env: [String: String] = ProcessInfo.processInfo.environment) -> Bool {
        env["OP_LAUNCH_PROBE"] == firstEffect
    }

    /// Runs the probe off the main thread when the environment asks for it.
    public static func runIfAsked(engine: Engine, env: [String: String] = ProcessInfo.processInfo.environment) {
        guard wantsFirstEffect(env) else { return }
        Task.detached {
            let line: String
            do {
                let result = try await runFirstEffect(engine: engine)
                line = String(format: "OP_FIRST_EFFECT %.1f gpu=%d\n", result.ms, result.gpu ? 1 : 0)
            } catch {
                line = "OP_FIRST_EFFECT_FAILED \(error)\n"
            }
            FileHandle.standardOutput.write(Data(line.utf8))
        }
    }

    public enum ProbeError: Error, CustomStringConvertible {
        case status(String)
        case noResult
        public var description: String {
            switch self {
            case let .status(s): "the blur finished with status \(s)"
            case .noResult: "the blur did not finish within 30 s"
            }
        }
    }

    /// One Gaussian blur (radius 2) on a 64 x 64 image the user never sees.
    /// Returns the time since process start when it finished and whether the
    /// GPU ran it (doc.info gpu_effects.gpu_runs).
    public static func runFirstEffect(engine: Engine) async throws -> (ms: Double, gpu: Bool) {
        let side = 64
        var rgba = [UInt8](repeating: 255, count: side * side * 4)
        for i in 0..<(side * side) { rgba[i * 4] = UInt8(i & 255); rgba[i * 4 + 1] = UInt8((i >> 6) & 255) }
        let doc = try engine.importPixels(width: UInt32(side), height: UInt32(side), rgba: rgba)
        defer { try? doc.close() }
        let event = try await doc.run("effect.blur.gaussian", ["radius": 2], timeout: .seconds(30))
        let ms = LaunchClock.millisecondsSinceProcessStart()
        guard event.kind == .jobFinished else { throw ProbeError.noResult }
        guard event.status == "ok" else { throw ProbeError.status(event.status ?? "none") }
        let stats = try doc.info()["gpu_effects"] as? [String: Any] ?? [:]
        let runs = (stats["gpu_runs"] as? NSNumber)?.intValue ?? 0
        return (ms, runs > 0)
    }
}
