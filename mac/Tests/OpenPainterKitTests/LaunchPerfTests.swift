// M58: launch stays lazy. The built app is started in a fresh process (so no
// earlier test has touched its pipeline counts) with one document; its
// OP_READY line must show the canvas view pipeline and no effect pipeline,
// and the first effect after it must run on the GPU. scripts/launch-time.sh
// checks the Release app the same way.
import AppKit
import EngineBridge
@testable import OpenPainterKit
import XCTest

/// Reads a child process's stdout line by line off the main thread.
final class LineReader: @unchecked Sendable {
    private let lock = NSLock()
    private var buffer = Data()
    private var lines: [String] = []

    init(_ handle: FileHandle) {
        handle.readabilityHandler = { [weak self] h in
            let chunk = h.availableData
            guard let self else { return }
            if chunk.isEmpty { h.readabilityHandler = nil; return }
            self.lock.lock()
            self.buffer.append(chunk)
            while let nl = self.buffer.firstIndex(of: 10) {
                self.lines.append(String(decoding: self.buffer[..<nl], as: UTF8.self))
                self.buffer.removeSubrange(...nl)
            }
            self.lock.unlock()
        }
    }

    var all: [String] {
        lock.lock()
        defer { lock.unlock() }
        return lines
    }
}

@MainActor
final class LaunchPerfTests: XCTestCase {
    static let macDir = URL(fileURLWithPath: #filePath).deletingLastPathComponent()
        .appendingPathComponent("../..").standardized
    static let app = macDir.appendingPathComponent("build/DerivedData/Build/Products/Debug/OpenPainter.app")
    static let document = macDir.appendingPathComponent("../tests/fixtures/plasma_512x384.png").standardized

    /// Parses "OP_READY <ms> effect=<n> view=<n> mip=<n>".
    static func readyCounts(_ line: String) -> PipelineCounts? {
        var c = PipelineCounts()
        var seen = 0
        for part in line.split(separator: " ").dropFirst(2) {
            let kv = part.split(separator: "=")
            guard kv.count == 2, let n = UInt64(kv[1]) else { return nil }
            switch kv[0] {
            case "effect": c.effect = n; seen += 1
            case "view": c.view = n; seen += 1
            case "mip": c.mip = n; seen += 1
            default: return nil
            }
        }
        return seen == 3 ? c : nil
    }

    func testFreshLaunchMakesNoEffectPipelineBeforeTheCanvasIsReady() throws {
        let binary = Self.app.appendingPathComponent("Contents/MacOS/OpenPainter")
        XCTAssertTrue(FileManager.default.isExecutableFile(atPath: binary.path), "build the app first: \(binary.path)")
        let p = Process()
        p.executableURL = binary
        p.arguments = [Self.document.path, "-ApplePersistenceIgnoreState", "YES"]
        var env = ProcessInfo.processInfo.environment
        env["OP_LAUNCH_PROBE"] = LaunchProbe.firstEffect
        // The test runner's own injection variables must not reach the app.
        for key in env.keys where key.hasPrefix("DYLD_INSERT") || key.hasPrefix("XCInject") { env[key] = nil }
        p.environment = env
        let out = Pipe()
        p.standardOutput = out
        p.standardError = FileHandle.nullDevice
        let reader = LineReader(out.fileHandleForReading)
        try p.run()
        defer {
            // Stop only the process this test started, by its PID.
            if p.isRunning { kill(p.processIdentifier, SIGKILL) }
            p.waitUntilExit()
        }
        let done = waitUntil(60) {
            reader.all.contains { $0.hasPrefix("OP_FIRST_EFFECT") } || !p.isRunning
        }
        let lines = reader.all
        XCTAssertTrue(done, "the app printed its launch lines: \(lines)")
        let ready = try XCTUnwrap(lines.first { $0.hasPrefix("OP_READY ") }, "\(lines)")
        let counts = try XCTUnwrap(Self.readyCounts(ready), ready)
        XCTAssertEqual(counts.effect, 0, "no effect kernel before the canvas is ready: \(ready)")
        XCTAssertEqual(counts.view, 1, "one canvas view pipeline (a document, not the start window): \(ready)")
        XCTAssertEqual(counts.mip, 0, "no mip builder at 100% or fit: \(ready)")
        let effect = try XCTUnwrap(lines.first { $0.hasPrefix("OP_FIRST_EFFECT") }, "\(lines)")
        XCTAssertTrue(effect.hasSuffix(" gpu=1"), "the first effect ran on the GPU: \(effect)")
    }

    func testReadyLineParsing() {
        XCTAssertEqual(Self.readyCounts("OP_READY 12.5 effect=0 view=1 mip=0"), PipelineCounts(effect: 0, view: 1, mip: 0))
        XCTAssertNil(Self.readyCounts("OP_READY 12.5"))
        XCTAssertNil(Self.readyCounts("OP_READY 12.5 pipelines=unknown(busy)"))
    }

    func testProbeRunsOnlyWhenAsked() {
        XCTAssertFalse(LaunchProbe.wantsFirstEffect([:]))
        XCTAssertFalse(LaunchProbe.wantsFirstEffect(["OP_LAUNCH_PROBE": "1"]))
        XCTAssertTrue(LaunchProbe.wantsFirstEffect(["OP_LAUNCH_PROBE": "first-effect"]))
    }

    /// The probe itself, in process: it reports the GPU path on this Mac.
    func testProbeBlurRunsOnTheGPU() async throws {
        let engine = try Engine()
        defer { engine.close() }
        let result = try await LaunchProbe.runFirstEffect(engine: engine)
        XCTAssertTrue(result.gpu, "the probe blur ran on the GPU")
        XCTAssertGreaterThan(result.ms, 0)
    }
}
